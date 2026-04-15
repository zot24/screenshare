mod capture;
mod discovery;
mod protocol;
mod viewer;

use capture::{CaptureSource, WindowInfo};
use discovery::Announcer;
use eframe::egui;
use protocol::{DiscoveryMessage, STALE_TIMEOUT, STREAM_PORT};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::time::Instant;

struct Sharer {
    hostname: String,
    ip: String,
    port: u16,
    sharing: String,
    last_seen: Instant,
}

enum Screen {
    Home,
    Viewing {
        sharer_name: String,
        handle: viewer::ViewerHandle,
        texture: Option<egui::TextureHandle>,
    },
}

struct ScreenShareApp {
    my_ip: String,
    my_hostname: String,
    discovery_rx: Receiver<DiscoveryMessage>,
    sharers: HashMap<String, Sharer>,
    is_sharing: bool,
    share_handle: Option<capture::ShareHandle>,
    announcer: Option<Announcer>,
    screen: Screen,
    // Window picker state
    selected_source: CaptureSource,
    available_windows: Vec<WindowInfo>,
    last_window_refresh: Instant,
}

impl ScreenShareApp {
    fn new(my_ip: String, my_hostname: String, discovery_rx: Receiver<DiscoveryMessage>) -> Self {
        let available_windows = capture::list_windows();
        Self {
            my_ip,
            my_hostname,
            discovery_rx,
            sharers: HashMap::new(),
            is_sharing: false,
            share_handle: None,
            announcer: None,
            screen: Screen::Home,
            selected_source: CaptureSource::FullScreen,
            available_windows,
            last_window_refresh: Instant::now(),
        }
    }

    fn update_discovery(&mut self) {
        while let Ok(msg) = self.discovery_rx.try_recv() {
            self.sharers.insert(
                msg.ip.clone(),
                Sharer {
                    hostname: msg.hostname,
                    ip: msg.ip,
                    port: msg.port,
                    sharing: msg.sharing,
                    last_seen: Instant::now(),
                },
            );
        }

        self.sharers
            .retain(|_, s| s.last_seen.elapsed() < STALE_TIMEOUT);
    }

    fn refresh_windows(&mut self) {
        // Refresh window list every 2 seconds (not while sharing)
        if !self.is_sharing && self.last_window_refresh.elapsed().as_secs() >= 2 {
            self.available_windows = capture::list_windows();
            self.last_window_refresh = Instant::now();
        }
    }

    fn start_sharing(&mut self) {
        let source = self.selected_source.clone();
        let label = source.to_string();
        match capture::start_sharing(source) {
            Ok(handle) => {
                self.announcer = Some(Announcer::start(
                    self.my_hostname.clone(),
                    self.my_ip.clone(),
                    STREAM_PORT,
                    label,
                ));
                self.share_handle = Some(handle);
                self.is_sharing = true;
            }
            Err(e) => {
                eprintln!("failed to start sharing: {e}");
            }
        }
    }

    fn stop_sharing(&mut self) {
        self.share_handle = None;
        self.announcer = None;
        self.is_sharing = false;
    }

    fn draw_home(&mut self, ui: &mut egui::Ui) {
        ui.heading("LAN Screen Share");
        ui.separator();

        ui.horizontal(|ui| {
            ui.label(format!("You: {} ({})", self.my_hostname, self.my_ip));
        });

        ui.add_space(8.0);

        // Source picker (disabled while sharing)
        let mut action = None;

        ui.group(|ui| {
            ui.label("Share source:");
            ui.add_space(4.0);

            if self.is_sharing {
                ui.disable();
            }

            // Full screen option
            let is_fullscreen = matches!(self.selected_source, CaptureSource::FullScreen);
            if ui.radio(is_fullscreen, "Full Screen").clicked() {
                self.selected_source = CaptureSource::FullScreen;
            }

            // Window options
            if self.available_windows.is_empty() {
                ui.colored_label(egui::Color32::GRAY, "  No windows found");
            } else {
                for win in &self.available_windows {
                    let is_selected = matches!(
                        &self.selected_source,
                        CaptureSource::Window(w) if w.id == win.id
                    );
                    let label = if win.title.is_empty() {
                        win.app_name.clone()
                    } else if win.title.len() > 60 {
                        format!("{} - {}…", win.app_name, &win.title[..57])
                    } else {
                        format!("{} - {}", win.app_name, win.title)
                    };
                    if ui.radio(is_selected, label).clicked() {
                        self.selected_source = CaptureSource::Window(win.clone());
                    }
                }
            }
        });

        ui.add_space(8.0);

        ui.horizontal(|ui| {
            if self.is_sharing {
                if ui.button("Stop Sharing").clicked() {
                    action = Some(false);
                }
                ui.colored_label(
                    egui::Color32::from_rgb(0, 180, 0),
                    format!("● Sharing: {}", self.selected_source),
                );
            } else if ui.button("Share").clicked() {
                action = Some(true);
            }
        });

        match action {
            Some(true) => self.start_sharing(),
            Some(false) => self.stop_sharing(),
            None => {}
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(8.0);

        ui.label("Available screens on your network:");
        ui.add_space(4.0);

        if self.sharers.is_empty() {
            ui.colored_label(egui::Color32::GRAY, "Scanning local network...");
        } else {
            let mut connect_to: Option<(String, String, u16)> = None;
            for sharer in self.sharers.values() {
                ui.horizontal(|ui| {
                    let label = if sharer.sharing.is_empty() {
                        format!("{} ({})", sharer.hostname, sharer.ip)
                    } else {
                        format!("{} ({}) — {}", sharer.hostname, sharer.ip, sharer.sharing)
                    };
                    ui.label(label);
                    if ui.button("View").clicked() {
                        connect_to =
                            Some((sharer.hostname.clone(), sharer.ip.clone(), sharer.port));
                    }
                });
            }

            if let Some((name, ip, port)) = connect_to {
                match viewer::start_viewing(&ip, port) {
                    Ok(handle) => {
                        self.screen = Screen::Viewing {
                            sharer_name: name,
                            handle,
                            texture: None,
                        };
                    }
                    Err(e) => {
                        eprintln!("failed to connect: {e}");
                    }
                }
            }
        }
    }

    fn draw_viewer(&mut self, ui: &mut egui::Ui) {
        let mut go_back = false;

        if let Screen::Viewing {
            ref sharer_name,
            ref handle,
            ref mut texture,
        } = self.screen
        {
            if let Some(frame) = handle.try_recv_frame() {
                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                    [frame.width as usize, frame.height as usize],
                    &frame.rgba,
                );
                match texture {
                    Some(tex) => tex.set(color_image, egui::TextureOptions::LINEAR),
                    None => {
                        *texture = Some(ui.ctx().load_texture(
                            "screen",
                            color_image,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                }
            }

            ui.horizontal(|ui| {
                if ui.button("<< Back").clicked() {
                    go_back = true;
                }
                ui.label(format!("Viewing: {sharer_name}"));
            });
            ui.separator();

            if let Some(tex) = texture {
                let available = ui.available_size();
                let tex_size = tex.size_vec2();
                let scale = (available.x / tex_size.x).min(available.y / tex_size.y);
                let display_size = egui::vec2(tex_size.x * scale, tex_size.y * scale);

                ui.centered_and_justified(|ui| {
                    ui.image(egui::load::SizedTexture::new(tex.id(), display_size));
                });
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Connecting...");
                });
            }
        }

        if go_back {
            self.screen = Screen::Home;
        }
    }
}

impl eframe::App for ScreenShareApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update_discovery();
        self.refresh_windows();
        ctx.request_repaint();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        match self.screen {
            Screen::Home => self.draw_home(ui),
            Screen::Viewing { .. } => self.draw_viewer(ui),
        }
    }
}

fn main() -> eframe::Result<()> {
    let my_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string());

    let my_hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let discovery_rx = discovery::start_listener(my_ip.clone());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_title("LAN Screen Share"),
        ..Default::default()
    };

    eframe::run_native(
        "LAN Screen Share",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(ScreenShareApp::new(
                my_ip,
                my_hostname,
                discovery_rx,
            )))
        }),
    )
}
