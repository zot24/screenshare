use crate::protocol::{DiscoveryMessage, DISCOVERY_INTERVAL, DISCOVERY_PORT};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub struct Announcer {
    shutdown: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Announcer {
    pub fn start(hostname: String, ip: String, port: u16, sharing: String) -> Self {
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        let handle = thread::spawn(move || {
            let socket = match UdpSocket::bind("0.0.0.0:0") {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("announcer: failed to bind UDP socket: {e}");
                    return;
                }
            };
            if let Err(e) = socket.set_broadcast(true) {
                eprintln!("announcer: failed to set broadcast: {e}");
                return;
            }

            let msg = DiscoveryMessage {
                hostname,
                ip,
                port,
                sharing,
            };
            let payload = match serde_json::to_vec(&msg) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("announcer: failed to serialize: {e}");
                    return;
                }
            };

            let dest = format!("255.255.255.255:{DISCOVERY_PORT}");
            while !shutdown_clone.load(Ordering::Relaxed) {
                let _ = socket.send_to(&payload, &dest);
                thread::sleep(DISCOVERY_INTERVAL);
            }
        });

        Self {
            shutdown,
            handle: Some(handle),
        }
    }
}

impl Drop for Announcer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn start_listener(my_ip: String) -> Receiver<DiscoveryMessage> {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let socket = match UdpSocket::bind(format!("0.0.0.0:{DISCOVERY_PORT}")) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("discovery listener: failed to bind: {e}");
                return;
            }
        };
        let _ = socket.set_read_timeout(Some(Duration::from_secs(1)));

        let mut buf = [0u8; 4096];
        loop {
            match socket.recv_from(&mut buf) {
                Ok((len, _addr)) => {
                    if let Ok(msg) = serde_json::from_slice::<DiscoveryMessage>(&buf[..len]) {
                        if msg.ip != my_ip && tx.send(msg).is_err() {
                            return; // receiver dropped
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                Err(e) => {
                    eprintln!("discovery listener error: {e}");
                    thread::sleep(Duration::from_secs(1));
                }
            }
        }
    });

    rx
}
