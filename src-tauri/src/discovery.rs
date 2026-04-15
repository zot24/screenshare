use crate::protocol::{DiscoveryMessage, DISCOVERY_INTERVAL, DISCOVERY_PORT, STREAM_PORT};
use std::net::{TcpStream, UdpSocket};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub struct Announcer {
    shutdown: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Announcer {
    pub fn start(hostname: String, ip: String, port: u16, sharing: String, mode: String) -> Self {
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
                mode,
                source: "lan".into(),
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

/// Starts both LAN broadcast listener and Tailscale prober in parallel.
/// Both feed the same channel, so the caller gets a unified stream.
pub fn start_listener(my_ip: String) -> Receiver<DiscoveryMessage> {
    let (tx, rx) = mpsc::channel();

    // LAN broadcast listener
    start_lan_listener(my_ip.clone(), tx.clone());

    // Tailscale prober (silently skipped if tailscale not installed)
    start_tailscale_prober(my_ip, tx);

    rx
}

fn start_lan_listener(my_ip: String, tx: Sender<DiscoveryMessage>) {
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
                            return;
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
}

fn start_tailscale_prober(my_ip: String, tx: Sender<DiscoveryMessage>) {
    thread::spawn(move || {
        // Check if tailscale CLI exists
        if Command::new("tailscale").arg("version").output().is_err() {
            return; // tailscale not installed, silently skip
        }

        loop {
            if let Ok(peers) = get_tailscale_peers(&my_ip) {
                for peer in peers {
                    // Probe if peer is sharing on our stream port
                    if probe_peer(&peer.ip) && tx.send(peer).is_err() {
                        return;
                    }
                }
            }
            thread::sleep(Duration::from_secs(5));
        }
    });
}

#[derive(serde::Deserialize)]
struct TailscaleStatus {
    #[serde(rename = "Peer")]
    peer: Option<std::collections::HashMap<String, TailscalePeer>>,
}

#[derive(serde::Deserialize)]
struct TailscalePeer {
    #[serde(rename = "HostName")]
    host_name: String,
    #[serde(rename = "TailscaleIPs")]
    tailscale_ips: Option<Vec<String>>,
    #[serde(rename = "Online")]
    online: bool,
}

fn get_tailscale_peers(my_ip: &str) -> anyhow::Result<Vec<DiscoveryMessage>> {
    let output = Command::new("tailscale")
        .args(["status", "--json"])
        .output()?;

    if !output.status.success() {
        anyhow::bail!("tailscale status failed");
    }

    let status: TailscaleStatus = serde_json::from_slice(&output.stdout)?;
    let mut peers = Vec::new();

    if let Some(peer_map) = status.peer {
        for peer in peer_map.values() {
            if !peer.online {
                continue;
            }
            if let Some(ips) = &peer.tailscale_ips {
                if let Some(ip) = ips.first() {
                    if ip != my_ip {
                        peers.push(DiscoveryMessage {
                            hostname: peer.host_name.clone(),
                            ip: ip.clone(),
                            port: STREAM_PORT,
                            sharing: String::new(),
                            mode: String::new(),
                            source: "tailscale".into(),
                        });
                    }
                }
            }
        }
    }

    Ok(peers)
}

fn probe_peer(ip: &str) -> bool {
    let addr = format!("{ip}:{STREAM_PORT}");
    TcpStream::connect_timeout(
        &addr.parse().unwrap_or_else(|_| {
            format!("0.0.0.0:{STREAM_PORT}")
                .parse()
                .expect("fallback addr")
        }),
        Duration::from_millis(500),
    )
    .is_ok()
}
