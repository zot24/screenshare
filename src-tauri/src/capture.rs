use crate::protocol::{write_frame, JPEG_QUALITY, STREAM_PORT, TARGET_FPS};
use image::codecs::jpeg::JpegEncoder;
use image::RgbaImage;
use serde::Serialize;
use std::io::Cursor;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize)]
pub struct WindowInfo {
    pub id: u32,
    pub app_name: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize)]
pub enum CaptureSource {
    FullScreen,
    Window(WindowInfo),
}

impl std::fmt::Display for CaptureSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureSource::FullScreen => write!(f, "Full Screen"),
            CaptureSource::Window(w) => {
                if w.title.is_empty() {
                    write!(f, "{}", w.app_name)
                } else if w.title.len() > 50 {
                    write!(f, "{} - {}…", w.app_name, &w.title[..47])
                } else {
                    write!(f, "{} - {}", w.app_name, w.title)
                }
            }
        }
    }
}

pub fn list_windows() -> Vec<WindowInfo> {
    let windows = match xcap::Window::all() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("failed to list windows: {e}");
            return vec![];
        }
    };

    windows
        .into_iter()
        .filter_map(|w| {
            let minimized = w.is_minimized().unwrap_or(true);
            if minimized {
                return None;
            }
            let width = w.width().unwrap_or(0);
            let height = w.height().unwrap_or(0);
            if width == 0 || height == 0 {
                return None;
            }
            let id = w.id().ok()?;
            let app_name = w.app_name().unwrap_or_default();
            let title = w.title().unwrap_or_default();
            // Skip empty/system windows
            if app_name.is_empty() && title.is_empty() {
                return None;
            }
            Some(WindowInfo {
                id,
                app_name,
                title,
            })
        })
        .collect()
}

pub struct ShareHandle {
    shutdown: Arc<AtomicBool>,
    _capture_handle: thread::JoinHandle<()>,
    _server_handle: thread::JoinHandle<()>,
}

impl Drop for ShareHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

pub fn start_sharing(source: CaptureSource) -> anyhow::Result<ShareHandle> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let latest_frame: Arc<Mutex<Option<Arc<Vec<u8>>>>> = Arc::new(Mutex::new(None));

    // Capture thread
    let shutdown_cap = shutdown.clone();
    let frame_writer = latest_frame.clone();
    let capture_handle = thread::spawn(move || {
        let frame_interval = Duration::from_millis(1000 / TARGET_FPS as u64);

        // For window capture, we need to re-find the window each time since
        // xcap::Window doesn't implement Send. We cache the window ID.
        let window_id = match &source {
            CaptureSource::Window(info) => Some(info.id),
            CaptureSource::FullScreen => None,
        };

        // For full screen, grab the primary monitor once
        let monitor = if window_id.is_none() {
            match xcap::Monitor::all() {
                Ok(m) => m.into_iter().next(),
                Err(e) => {
                    eprintln!("capture: failed to list monitors: {e}");
                    return;
                }
            }
        } else {
            None
        };

        if window_id.is_none() && monitor.is_none() {
            eprintln!("capture: no monitors found");
            return;
        }

        while !shutdown_cap.load(Ordering::Relaxed) {
            let start = Instant::now();

            let img: Option<RgbaImage> = if let Some(wid) = window_id {
                // Re-find window by ID each frame (handles moved/resized windows)
                find_and_capture_window(wid)
            } else if let Some(ref mon) = monitor {
                mon.capture_image().ok()
            } else {
                None
            };

            if let Some(img) = img {
                let mut jpeg_buf = Cursor::new(Vec::new());
                let encoder = JpegEncoder::new_with_quality(&mut jpeg_buf, JPEG_QUALITY);
                if let Err(e) = img.write_with_encoder(encoder) {
                    eprintln!("capture: jpeg encode error: {e}");
                } else {
                    let jpeg_data = Arc::new(jpeg_buf.into_inner());
                    if let Ok(mut lock) = frame_writer.lock() {
                        *lock = Some(jpeg_data);
                    }
                }
            }

            let elapsed = start.elapsed();
            if elapsed < frame_interval {
                thread::sleep(frame_interval - elapsed);
            }
        }
    });

    // TCP server thread
    let shutdown_srv = shutdown.clone();
    let frame_reader = latest_frame;
    let server_handle = thread::spawn(move || {
        let listener = match TcpListener::bind(format!("0.0.0.0:{STREAM_PORT}")) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("capture server: failed to bind: {e}");
                return;
            }
        };
        let _ = listener.set_nonblocking(true);

        while !shutdown_srv.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, addr)) => {
                    eprintln!("viewer connected: {addr}");
                    let shutdown_viewer = shutdown_srv.clone();
                    let frames = frame_reader.clone();
                    thread::spawn(move || {
                        serve_viewer(stream, frames, shutdown_viewer);
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    eprintln!("capture server: accept error: {e}");
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    });

    Ok(ShareHandle {
        shutdown,
        _capture_handle: capture_handle,
        _server_handle: server_handle,
    })
}

fn find_and_capture_window(window_id: u32) -> Option<RgbaImage> {
    let windows = xcap::Window::all().ok()?;
    let window = windows
        .into_iter()
        .find(|w| w.id().ok() == Some(window_id))?;
    window.capture_image().ok()
}

fn serve_viewer(
    mut stream: TcpStream,
    frames: Arc<Mutex<Option<Arc<Vec<u8>>>>>,
    shutdown: Arc<AtomicBool>,
) {
    let frame_interval = Duration::from_millis(1000 / TARGET_FPS as u64);

    while !shutdown.load(Ordering::Relaxed) {
        let start = Instant::now();

        let frame = frames.lock().ok().and_then(|lock| lock.clone());
        if let Some(jpeg_data) = frame {
            if write_frame(&mut stream, &jpeg_data).is_err() {
                eprintln!("viewer disconnected");
                return;
            }
        }

        let elapsed = start.elapsed();
        if elapsed < frame_interval {
            thread::sleep(frame_interval - elapsed);
        }
    }
}
