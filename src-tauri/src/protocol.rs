use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub const DISCOVERY_PORT: u16 = 42069;
pub const STREAM_PORT: u16 = 42070;
pub const DISCOVERY_INTERVAL: Duration = Duration::from_secs(2);
pub const STALE_TIMEOUT: Duration = Duration::from_secs(6);
pub const TARGET_FPS: u32 = 15;
pub const JPEG_QUALITY: u8 = 70;
pub const MAX_FRAME_SIZE: u32 = 10_000_000; // 10MB sanity limit

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryMessage {
    pub hostname: String,
    pub ip: String,
    pub port: u16,
    #[serde(default)]
    pub sharing: String, // e.g. "Full Screen" or "Terminal - tmux"
    #[serde(default)]
    pub mode: String, // "screen" or "terminal"
    #[serde(default)]
    pub source: String, // "lan" or "tailscale" (UI hint only)
}

pub fn write_frame(stream: &mut TcpStream, jpeg_data: &[u8]) -> Result<()> {
    let len = jpeg_data.len() as u32;
    stream
        .write_all(&len.to_be_bytes())
        .context("write frame length")?;
    stream.write_all(jpeg_data).context("write frame data")?;
    Ok(())
}

pub fn read_frame(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .context("read frame length")?;
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_FRAME_SIZE {
        anyhow::bail!("frame too large: {} bytes", len);
    }
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf).context("read frame data")?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn test_frame_roundtrip() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let data = vec![1, 2, 3, 4, 5, 255, 0, 128];
        let send_data = data.clone();

        let handle = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).unwrap();
            write_frame(&mut stream, &send_data).unwrap();
        });

        let (mut stream, _) = listener.accept().unwrap();
        let received = read_frame(&mut stream).unwrap();
        assert_eq!(received, data);
        handle.join().unwrap();
    }

    #[test]
    fn test_discovery_message_serde() {
        let msg = DiscoveryMessage {
            hostname: "test-host".into(),
            ip: "192.168.1.42".into(),
            port: 42070,
            sharing: "Terminal - tmux".into(),
            mode: "terminal".into(),
            source: "lan".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: DiscoveryMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.hostname, "test-host");
        assert_eq!(decoded.ip, "192.168.1.42");
        assert_eq!(decoded.port, 42070);
        assert_eq!(decoded.sharing, "Terminal - tmux");
        assert_eq!(decoded.mode, "terminal");
        assert_eq!(decoded.source, "lan");
    }
}
