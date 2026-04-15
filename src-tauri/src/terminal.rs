use crate::protocol::{MAX_FRAME_SIZE, STREAM_PORT};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub struct TerminalShareHandle {
    shutdown: Arc<AtomicBool>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl TerminalShareHandle {
    /// Write user input to the PTY
    pub fn write_input(&self, data: &[u8]) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(data);
        }
    }
}

impl Drop for TerminalShareHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

pub fn start_terminal_sharing(cols: u16, rows: u16) -> anyhow::Result<TerminalShareHandle> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let pty_system = native_pty_system();

    let pair = pty_system.openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let mut cmd = CommandBuilder::new(&shell);
    cmd.cwd(std::env::var("HOME").unwrap_or_else(|_| "/".to_string()));

    pair.slave.spawn_command(cmd)?;

    let writer: Arc<Mutex<Box<dyn Write + Send>>> =
        Arc::new(Mutex::new(pair.master.take_writer()?));
    let mut reader = pair.master.try_clone_reader()?;

    // Shared buffer for latest terminal output chunk
    let latest_data: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));

    // PTY reader thread - reads terminal output and stores it
    let shutdown_reader = shutdown.clone();
    let data_writer = latest_data.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while !shutdown_reader.load(Ordering::Relaxed) {
            match reader.read(&mut buf) {
                Ok(0) => return, // EOF - shell exited
                Ok(n) => {
                    if let Ok(mut data) = data_writer.lock() {
                        data.extend_from_slice(&buf[..n]);
                    }
                }
                Err(_) => return,
            }
        }
    });

    // TCP server thread - serves terminal output to viewers
    let shutdown_srv = shutdown.clone();
    let data_reader = latest_data;
    thread::spawn(move || {
        let listener = match TcpListener::bind(format!("0.0.0.0:{STREAM_PORT}")) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("terminal server: failed to bind: {e}");
                return;
            }
        };
        let _ = listener.set_nonblocking(true);

        while !shutdown_srv.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, addr)) => {
                    eprintln!("terminal viewer connected: {addr}");
                    let shutdown_viewer = shutdown_srv.clone();
                    let data = data_reader.clone();
                    thread::spawn(move || {
                        serve_terminal_viewer(stream, data, shutdown_viewer);
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    eprintln!("terminal server: accept error: {e}");
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    });

    Ok(TerminalShareHandle { shutdown, writer })
}

fn serve_terminal_viewer(
    mut stream: TcpStream,
    data: Arc<Mutex<Vec<u8>>>,
    shutdown: Arc<AtomicBool>,
) {
    while !shutdown.load(Ordering::Relaxed) {
        let chunk = {
            let mut d = match data.lock() {
                Ok(d) => d,
                Err(_) => return,
            };
            if d.is_empty() {
                drop(d);
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            let chunk = d.clone();
            d.clear();
            chunk
        };

        // Write length-prefixed frame
        let len = chunk.len() as u32;
        if len > MAX_FRAME_SIZE {
            continue;
        }
        if stream.write_all(&len.to_be_bytes()).is_err() || stream.write_all(&chunk).is_err() {
            return; // viewer disconnected
        }
    }
}

/// Connect to a remote terminal sharer and return raw bytes via callback
pub struct TerminalViewerHandle {
    shutdown: Arc<AtomicBool>,
}

impl Drop for TerminalViewerHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

pub fn start_terminal_viewing(
    ip: &str,
    port: u16,
    on_data: std::sync::mpsc::Sender<Vec<u8>>,
) -> anyhow::Result<TerminalViewerHandle> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();
    let addr = format!("{ip}:{port}");

    thread::spawn(move || {
        let mut stream = match TcpStream::connect_timeout(
            &addr.parse().expect("valid addr"),
            Duration::from_secs(5),
        ) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("terminal viewer: failed to connect: {e}");
                return;
            }
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

        let mut len_buf = [0u8; 4];
        while !shutdown_clone.load(Ordering::Relaxed) {
            if stream.read_exact(&mut len_buf).is_err() {
                return;
            }
            let len = u32::from_be_bytes(len_buf);
            if len > MAX_FRAME_SIZE {
                return;
            }
            let mut buf = vec![0u8; len as usize];
            if stream.read_exact(&mut buf).is_err() {
                return;
            }
            if on_data.send(buf).is_err() {
                return;
            }
        }
    });

    Ok(TerminalViewerHandle { shutdown })
}
