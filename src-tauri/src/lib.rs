mod capture;
mod discovery;
mod protocol;
pub mod terminal;
pub mod tui;
mod viewer;

use capture::{CaptureSource, WindowInfo};
use discovery::Announcer;
use protocol::{DiscoveryMessage, STALE_TIMEOUT, STREAM_PORT};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::Mutex;
use std::time::Instant;
use tauri::{Emitter, Manager};

#[derive(Clone, Serialize)]
struct SharerInfo {
    hostname: String,
    ip: String,
    port: u16,
    sharing: String,
    mode: String,
    source: String,
}

struct Sharer {
    hostname: String,
    ip: String,
    port: u16,
    sharing: String,
    mode: String,
    source: String,
    last_seen: Instant,
}

struct AppState {
    my_ip: String,
    my_hostname: String,
    discovery_rx: Receiver<DiscoveryMessage>,
    sharers: HashMap<String, Sharer>,
    is_sharing: bool,
    share_handle: Option<capture::ShareHandle>,
    announcer: Option<Announcer>,
    selected_source: CaptureSource,
    viewer_handle: Option<viewer::JpegViewerHandle>,
}

#[tauri::command]
fn get_identity(state: tauri::State<'_, Mutex<AppState>>) -> (String, String) {
    let s = state.lock().unwrap();
    (s.my_hostname.clone(), s.my_ip.clone())
}

#[tauri::command]
fn list_windows() -> Vec<WindowInfo> {
    capture::list_windows()
}

#[tauri::command]
fn start_sharing(
    source_type: String,
    window_id: Option<u32>,
    window_app: Option<String>,
    window_title: Option<String>,
    state: tauri::State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let mut s = state.lock().unwrap();
    let source = if source_type == "window" {
        CaptureSource::Window(WindowInfo {
            id: window_id.unwrap_or(0),
            app_name: window_app.unwrap_or_default(),
            title: window_title.unwrap_or_default(),
        })
    } else {
        CaptureSource::FullScreen
    };

    let label = source.to_string();
    match capture::start_sharing(source.clone()) {
        Ok(handle) => {
            s.announcer = Some(Announcer::start(
                s.my_hostname.clone(),
                s.my_ip.clone(),
                STREAM_PORT,
                label,
                "screen".into(),
            ));
            s.share_handle = Some(handle);
            s.selected_source = source;
            s.is_sharing = true;
            Ok(())
        }
        Err(e) => Err(format!("Failed to start sharing: {e}")),
    }
}

#[tauri::command]
fn stop_sharing(state: tauri::State<'_, Mutex<AppState>>) {
    let mut s = state.lock().unwrap();
    s.share_handle = None;
    s.announcer = None;
    s.is_sharing = false;
}

#[tauri::command]
fn get_sharing_status(state: tauri::State<'_, Mutex<AppState>>) -> (bool, String) {
    let s = state.lock().unwrap();
    (s.is_sharing, s.selected_source.to_string())
}

#[tauri::command]
fn start_viewing(
    ip: String,
    port: u16,
    state: tauri::State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let mut s = state.lock().unwrap();
    s.viewer_handle = None;
    match viewer::start_viewing_jpeg(&ip, port) {
        Ok(handle) => {
            s.viewer_handle = Some(handle);
            Ok(())
        }
        Err(e) => Err(format!("Failed to connect: {e}")),
    }
}

#[tauri::command]
fn stop_viewing(state: tauri::State<'_, Mutex<AppState>>) {
    let mut s = state.lock().unwrap();
    s.viewer_handle = None;
}

#[tauri::command]
fn poll_frame(state: tauri::State<'_, Mutex<AppState>>) -> Option<String> {
    let s = state.lock().unwrap();
    if let Some(ref handle) = s.viewer_handle {
        if let Some(jpeg_data) = handle.try_recv_jpeg() {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&jpeg_data);
            return Some(format!("data:image/jpeg;base64,{b64}"));
        }
    }
    None
}

#[tauri::command]
fn get_sharers(state: tauri::State<'_, Mutex<AppState>>) -> Vec<SharerInfo> {
    let s = state.lock().unwrap();
    s.sharers
        .values()
        .map(|v| SharerInfo {
            hostname: v.hostname.clone(),
            ip: v.ip.clone(),
            port: v.port,
            sharing: v.sharing.clone(),
            mode: v.mode.clone(),
            source: v.source.clone(),
        })
        .collect()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let my_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string());

    let my_hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let discovery_rx = discovery::start_listener(my_ip.clone());

    let app_state = AppState {
        my_ip,
        my_hostname,
        discovery_rx,
        sharers: HashMap::new(),
        is_sharing: false,
        share_handle: None,
        announcer: None,
        selected_source: CaptureSource::FullScreen,
        viewer_handle: None,
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(app_state))
        .invoke_handler(tauri::generate_handler![
            get_identity,
            list_windows,
            start_sharing,
            stop_sharing,
            get_sharing_status,
            start_viewing,
            stop_viewing,
            poll_frame,
            get_sharers,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let state = handle.state::<Mutex<AppState>>();
                let sharers: Vec<SharerInfo> = {
                    let mut s = state.lock().unwrap();
                    while let Ok(msg) = s.discovery_rx.try_recv() {
                        s.sharers.insert(
                            msg.ip.clone(),
                            Sharer {
                                hostname: msg.hostname,
                                ip: msg.ip,
                                port: msg.port,
                                sharing: msg.sharing,
                                mode: msg.mode,
                                source: msg.source,
                                last_seen: Instant::now(),
                            },
                        );
                    }
                    s.sharers
                        .retain(|_, v| v.last_seen.elapsed() < STALE_TIMEOUT);
                    s.sharers
                        .values()
                        .map(|v| SharerInfo {
                            hostname: v.hostname.clone(),
                            ip: v.ip.clone(),
                            port: v.port,
                            sharing: v.sharing.clone(),
                            mode: v.mode.clone(),
                            source: v.source.clone(),
                        })
                        .collect()
                };
                let _ = handle.emit("sharers-updated", &sharers);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error running tauri app");
}
