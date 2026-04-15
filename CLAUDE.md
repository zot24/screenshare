# screenshare

## Project overview

Zero-config LAN screen and terminal sharing app. Single Rust binary with two modes:
- `screenshare` — ratatui TUI for terminal sharing (PTY capture, raw byte streaming)
- `screenshare --gui` — Tauri v2 GUI for screen/window sharing (JPEG capture)

Both modes share the same discovery protocol and see each other on the network.

## Architecture

```
src-tauri/src/
  main.rs       — Entry point, routes --gui to Tauri or default to ratatui TUI
  lib.rs        — Tauri commands, app state, GUI backend
  tui.rs        — Ratatui terminal UI (home screen, sharing, viewing)
  terminal.rs   — PTY capture via portable-pty, TCP server for terminal bytes
  protocol.rs   — Wire protocol: constants, DiscoveryMessage, frame read/write
  discovery.rs  — LAN broadcast + Tailscale auto-detection, unified peer channel
  capture.rs    — Screen/window capture via xcap, JPEG encoding, TCP server
  viewer.rs     — TCP client for JPEG streams (used by GUI mode)
```

Frontend (Tauri GUI only):
```
index.html      — Tauri webview entry point
src/main.js     — Frontend logic (discovery list, viewer, share controls)
src/style.css   — Dark theme styles
```

## Key technical decisions

- **Single binary:** Both TUI (ratatui) and GUI (Tauri) compile into one executable. No Node.js runtime needed for the TUI.
- **Discovery is parallel:** LAN broadcast (instant, UDP) and Tailscale probing (~5s, `tailscale status --json`) run simultaneously and feed the same mpsc channel.
- **Terminal sharing streams raw bytes:** No encoding/decoding. PTY output (ANSI escape codes) is length-prefixed and sent over TCP. Viewers write directly to their terminal.
- **Screen sharing streams JPEG:** xcap captures frames, JPEG-encodes at quality 70, streams via length-prefixed TCP. GUI frontend renders via base64 data URLs in an `<img>` tag.
- **Tailscale is optional:** Auto-detected by checking if `tailscale` CLI exists on PATH. Silently skipped if not installed.

## Build and test

```bash
# Rust backend (TUI + GUI backend)
cd src-tauri && cargo build
cd src-tauri && cargo test
cd src-tauri && cargo clippy -- -D warnings
cd src-tauri && cargo fmt --check

# Full Tauri GUI (needs npm deps for frontend)
npm install && npm run dev:gui
```

## Protocol

- **Discovery port:** UDP 42069 (broadcast `255.255.255.255`)
- **Stream port:** TCP 42070
- **Frame format:** 4-byte big-endian length prefix + payload (JPEG or raw terminal bytes)
- **Discovery message:** JSON with fields: hostname, ip, port, sharing, mode ("terminal"/"screen"), source ("lan"/"tailscale")

## Dependencies

Rust: tauri, ratatui, crossterm, portable-pty, xcap, image, serde, base64, anyhow
Frontend: @tauri-apps/api, vite (dev only)
