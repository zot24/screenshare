use crate::protocol::read_frame;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct ViewerHandle {
    shutdown: Arc<AtomicBool>,
    pub frame_rx: Receiver<DecodedFrame>,
    _handle: thread::JoinHandle<()>,
}

impl ViewerHandle {
    pub fn try_recv_frame(&self) -> Option<DecodedFrame> {
        let mut latest = None;
        loop {
            match self.frame_rx.try_recv() {
                Ok(frame) => latest = Some(frame),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        latest
    }
}

impl Drop for ViewerHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

pub fn start_viewing(ip: &str, port: u16) -> anyhow::Result<ViewerHandle> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();

    let shutdown_clone = shutdown.clone();
    let addr = format!("{ip}:{port}");

    let handle = thread::spawn(move || {
        let mut stream = match TcpStream::connect_timeout(
            &addr.parse().expect("valid addr"),
            Duration::from_secs(5),
        ) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("viewer: failed to connect to {addr}: {e}");
                return;
            }
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

        while !shutdown_clone.load(Ordering::Relaxed) {
            let jpeg_data = match read_frame(&mut stream) {
                Ok(data) => data,
                Err(e) => {
                    eprintln!("viewer: read error: {e}");
                    return;
                }
            };

            let img =
                match image::load_from_memory_with_format(&jpeg_data, image::ImageFormat::Jpeg) {
                    Ok(img) => img.to_rgba8(),
                    Err(e) => {
                        eprintln!("viewer: decode error: {e}");
                        continue;
                    }
                };

            let frame = DecodedFrame {
                width: img.width(),
                height: img.height(),
                rgba: img.into_raw(),
            };

            if tx.send(frame).is_err() {
                return; // receiver dropped
            }
        }
    });

    Ok(ViewerHandle {
        shutdown,
        frame_rx: rx,
        _handle: handle,
    })
}
