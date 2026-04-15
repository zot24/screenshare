use crate::discovery::{self, Announcer};
use crate::protocol::{DiscoveryMessage, STALE_TIMEOUT, STREAM_PORT};
use crate::terminal;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use std::collections::HashMap;
use std::io::{self, stdout, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

struct Sharer {
    hostname: String,
    ip: String,
    port: u16,
    sharing: String,
    mode: String,
    source: String,
    last_seen: Instant,
}

enum AppScreen {
    Home,
    Sharing,
    Viewing { _name: String },
}

pub fn run_tui() -> anyhow::Result<()> {
    let my_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string());

    let my_hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let discovery_rx = discovery::start_listener(my_ip.clone());

    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut sharers: HashMap<String, Sharer> = HashMap::new();
    let mut list_state = ListState::default();
    let mut screen = AppScreen::Home;
    let mut is_sharing = false;
    let mut share_handle: Option<terminal::TerminalShareHandle> = None;
    let mut announcer: Option<Announcer> = None;
    let mut viewer_handle: Option<terminal::TerminalViewerHandle> = None;
    let (viewer_tx, viewer_rx) = mpsc::channel::<Vec<u8>>();

    let result = run_loop(
        &mut terminal,
        &discovery_rx,
        &mut sharers,
        &mut list_state,
        &mut screen,
        &mut is_sharing,
        &mut share_handle,
        &mut announcer,
        &mut viewer_handle,
        &viewer_tx,
        &viewer_rx,
        &my_ip,
        &my_hostname,
    );

    // Cleanup
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

#[allow(clippy::too_many_arguments)]
fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    discovery_rx: &mpsc::Receiver<DiscoveryMessage>,
    sharers: &mut HashMap<String, Sharer>,
    list_state: &mut ListState,
    screen: &mut AppScreen,
    is_sharing: &mut bool,
    share_handle: &mut Option<terminal::TerminalShareHandle>,
    announcer: &mut Option<Announcer>,
    viewer_handle: &mut Option<terminal::TerminalViewerHandle>,
    viewer_tx: &mpsc::Sender<Vec<u8>>,
    viewer_rx: &mpsc::Receiver<Vec<u8>>,
    my_ip: &str,
    my_hostname: &str,
) -> anyhow::Result<()> {
    loop {
        // Drain discovery
        while let Ok(msg) = discovery_rx.try_recv() {
            sharers.insert(
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
        sharers.retain(|_, s| s.last_seen.elapsed() < STALE_TIMEOUT);

        match screen {
            AppScreen::Home => {
                let sharer_list: Vec<_> = sharers
                    .values()
                    .filter(|s| s.mode == "terminal" || s.mode.is_empty())
                    .collect();

                terminal.draw(|f| {
                    draw_home(f, my_hostname, my_ip, &sharer_list, list_state, *is_sharing)
                })?;

                if event::poll(Duration::from_millis(100))? {
                    if let Event::Key(key) = event::read()? {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Char('c')
                                if key.modifiers.contains(KeyModifiers::CONTROL)
                                    || key.code == KeyCode::Char('q') =>
                            {
                                return Ok(());
                            }
                            KeyCode::Char('s') => {
                                if *is_sharing {
                                    *share_handle = None;
                                    *announcer = None;
                                    *is_sharing = false;
                                } else {
                                    let size = terminal.size()?;
                                    match terminal::start_terminal_sharing(size.width, size.height)
                                    {
                                        Ok(handle) => {
                                            *announcer = Some(Announcer::start(
                                                my_hostname.to_string(),
                                                my_ip.to_string(),
                                                STREAM_PORT,
                                                format!(
                                                    "Terminal - {}",
                                                    std::env::var("SHELL")
                                                        .unwrap_or_else(|_| "sh".into())
                                                ),
                                                "terminal".into(),
                                            ));
                                            *share_handle = Some(handle);
                                            *is_sharing = true;
                                            *screen = AppScreen::Sharing;
                                        }
                                        Err(e) => eprintln!("Failed to share: {e}"),
                                    }
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                let len = sharer_list.len();
                                if len > 0 {
                                    let i =
                                        list_state.selected().map(|i| (i + 1) % len).unwrap_or(0);
                                    list_state.select(Some(i));
                                }
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                let len = sharer_list.len();
                                if len > 0 {
                                    let i = list_state
                                        .selected()
                                        .map(|i| if i == 0 { len - 1 } else { i - 1 })
                                        .unwrap_or(0);
                                    list_state.select(Some(i));
                                }
                            }
                            KeyCode::Enter => {
                                let sharer_vec: Vec<_> = sharers
                                    .values()
                                    .filter(|s| s.mode == "terminal" || s.mode.is_empty())
                                    .collect();
                                if let Some(idx) = list_state.selected() {
                                    if let Some(sharer) = sharer_vec.get(idx) {
                                        let name = sharer.hostname.clone();
                                        match terminal::start_terminal_viewing(
                                            &sharer.ip,
                                            sharer.port,
                                            viewer_tx.clone(),
                                        ) {
                                            Ok(handle) => {
                                                *viewer_handle = Some(handle);
                                                *screen = AppScreen::Viewing { _name: name };
                                                // Leave alternate screen for raw terminal output
                                                disable_raw_mode()?;
                                                execute!(
                                                    terminal.backend_mut(),
                                                    LeaveAlternateScreen
                                                )?;
                                                print!("\x1b[2J\x1b[H"); // Clear screen
                                            }
                                            Err(e) => eprintln!("Failed to connect: {e}"),
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            AppScreen::Sharing => {
                // In sharing mode, forward keypresses to the PTY
                // and also render PTY output
                if let Some(ref handle) = share_handle {
                    // Check for incoming terminal data from the PTY
                    // (The PTY output goes to viewers; we need to show it locally too)
                    // For now, show a status screen
                    terminal.draw(|f| draw_sharing(f, my_hostname))?;

                    if event::poll(Duration::from_millis(50))? {
                        if let Event::Key(KeyEvent {
                            code: KeyCode::Esc, ..
                        }) = event::read()?
                        {
                            *share_handle = None;
                            *announcer = None;
                            *is_sharing = false;
                            *screen = AppScreen::Home;
                        } else if let Event::Key(key) = event::read()? {
                            // Forward keypress to PTY
                            let bytes = key_to_bytes(key);
                            if !bytes.is_empty() {
                                handle.write_input(&bytes);
                            }
                        }
                    }
                } else {
                    *screen = AppScreen::Home;
                }
            }
            AppScreen::Viewing { .. } => {
                // Pipe raw terminal bytes to stdout
                while let Ok(data) = viewer_rx.try_recv() {
                    io::stdout().write_all(&data)?;
                    io::stdout().flush()?;
                }

                // Check for Escape key (in raw-ish mode)
                // We need to read stdin without raw mode interfering with terminal output
                // Use a short poll
                if event::poll(Duration::from_millis(50))? {
                    if let Event::Key(key) = event::read()? {
                        if key.code == KeyCode::Esc {
                            *viewer_handle = None;
                            *screen = AppScreen::Home;
                            // Re-enter alternate screen
                            enable_raw_mode()?;
                            execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                            terminal.clear()?;
                        }
                    }
                }
            }
        }
    }
}

fn draw_home(
    f: &mut Frame,
    hostname: &str,
    ip: &str,
    sharers: &[&Sharer],
    list_state: &mut ListState,
    is_sharing: bool,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Length(3), // Identity + status
            Constraint::Min(5),    // Sharers list
            Constraint::Length(1), // Help
        ])
        .split(f.area());

    // Title
    let title = Paragraph::new("LAN Screen Share")
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(title, chunks[0]);

    // Identity + sharing status
    let status = if is_sharing {
        format!("You: {hostname} ({ip})  ● SHARING (press 's' to stop)")
    } else {
        format!("You: {hostname} ({ip})  press 's' to share terminal")
    };
    let status_style = if is_sharing {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Gray)
    };
    let status_widget = Paragraph::new(status).style(status_style);
    f.render_widget(status_widget, chunks[1]);

    // Sharers list
    let items: Vec<ListItem> = if sharers.is_empty() {
        vec![ListItem::new("  Scanning local network...")
            .style(Style::default().fg(Color::DarkGray))]
    } else {
        sharers
            .iter()
            .map(|s| {
                let source_tag = if s.source == "tailscale" { " [ts]" } else { "" };
                let sharing = if s.sharing.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", s.sharing)
                };
                ListItem::new(format!(
                    "  {} ({}){}{}",
                    s.hostname, s.ip, sharing, source_tag
                ))
            })
            .collect()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Terminal sharers on your network "),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    f.render_stateful_widget(list, chunks[2], list_state);

    // Help
    let help = Paragraph::new("j/k: navigate  enter: view  s: share  q: quit")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, chunks[3]);
}

fn draw_sharing(f: &mut Frame, hostname: &str) {
    let area = f.area();
    let text = vec![
        Line::from(Span::styled(
            "Sharing Terminal",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(format!("Host: {hostname}")),
        Line::from("Others on your network can now view your terminal."),
        Line::from(""),
        Line::from(Span::styled(
            "Press ESC to stop sharing",
            Style::default().fg(Color::Yellow),
        )),
    ];
    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Sharing Active "),
        )
        .alignment(Alignment::Center);
    f.render_widget(paragraph, area);
}

fn key_to_bytes(key: KeyEvent) -> Vec<u8> {
    match key.code {
        KeyCode::Char(c) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                vec![(c as u8) & 0x1f]
            } else {
                c.to_string().into_bytes()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        _ => vec![],
    }
}
