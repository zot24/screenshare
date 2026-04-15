# screenshare

Zero-config LAN screen and terminal sharing. One binary, auto-discovery, works out of the box.

## What it does

- **Terminal sharing (TUI):** Share your terminal with others on your network. They see your tmux/zellij/shell rendered perfectly in their own terminal.
- **Screen sharing (GUI):** Share your full screen or a specific window as a live video stream.
- **Auto-discovery:** Peers appear automatically via LAN broadcast + Tailscale (if installed).
- **Zero config:** Launch it and it works. No accounts, no servers, no setup.

## Install

### Quick install (download binary)

**macOS (Apple Silicon):**
```bash
curl -L https://github.com/zot24/screenshare/releases/latest/download/screenshare-macOS-arm64.tar.gz | tar xz
sudo mv screenshare /usr/local/bin/
```

**macOS (Intel):**
```bash
curl -L https://github.com/zot24/screenshare/releases/latest/download/screenshare-macOS-x86_64.tar.gz | tar xz
sudo mv screenshare /usr/local/bin/
```

**Linux (x86_64):**
```bash
curl -L https://github.com/zot24/screenshare/releases/latest/download/screenshare-Linux-x86_64.tar.gz | tar xz
sudo mv screenshare /usr/local/bin/
```

**Linux (.deb):**
```bash
# Download the .deb from the latest release
curl -LO https://github.com/zot24/screenshare/releases/latest/download/screenshare_0.1.0_amd64.deb
sudo dpkg -i screenshare_0.1.0_amd64.deb
```

### macOS app

Download the `.dmg` from the [latest release](https://github.com/zot24/screenshare/releases/latest) and drag to Applications.

### Build from source

```bash
git clone https://github.com/zot24/screenshare.git
cd screenshare/src-tauri
cargo build --release
# Binary at: target/release/screenshare
```

#### Linux build dependencies

```bash
sudo apt install libxcb-randr0-dev libxcb-shm0-dev libxcb-xfixes0-dev \
  libxcb-shape0-dev libxcb-xkb-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libdbus-1-dev pkg-config libgtk-3-dev libwayland-dev \
  libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev \
  libpipewire-0.3-dev libgbm-dev libdrm-dev libegl-dev
```

#### macOS note

Screen Recording permission is required for screen capture. macOS will prompt you on first use.

## Usage

### Terminal mode (default)

```bash
screenshare
```

Opens a TUI with:
- **s** — Start/stop sharing your terminal
- **j/k** or arrow keys — Navigate the peer list
- **Enter** — View a peer's shared terminal
- **Esc** — Stop viewing / stop sharing
- **q** — Quit

### GUI mode

```bash
screenshare --gui
```

Opens a desktop window where you can:
- Pick a capture source (full screen or specific window)
- Share your screen
- View others' shared screens

### Both modes discover each other

Terminal sharers appear in the GUI's peer list and vice versa. They use the same discovery protocol (UDP broadcast on port 42069, TCP streaming on port 42070).

## Tailscale support

If [Tailscale](https://tailscale.com) is installed and connected, peers on your tailnet are automatically discovered alongside LAN peers. No configuration needed — it's auto-detected.

- LAN broadcast results appear instantly
- Tailscale peers appear within ~5 seconds
- Works across different physical networks (home, office, etc.)
- If Tailscale isn't installed, it's silently skipped

## Configuration (optional)

Everything works without a config file. Power users can create `~/.config/screenshare/config.toml`:

```toml
[discovery]
lan = true              # enable LAN broadcast (default: true)
tailscale = true        # enable Tailscale discovery (default: true)
discovery_port = 42069  # UDP discovery port
stream_port = 42070     # TCP streaming port

[sharing]
fps = 15                # screen capture FPS
jpeg_quality = 70       # JPEG quality (1-100)

[tailscale]
poll_interval = 5       # seconds between tailscale status polls
probe_timeout = 500     # ms timeout for TCP probe

[network]
stale_timeout = 6       # seconds before a peer is considered gone
announce_interval = 2   # seconds between announcements
```

## How it works

### Discovery
- **LAN:** UDP broadcast to `255.255.255.255:42069` every 2 seconds
- **Tailscale:** Runs `tailscale status --json`, probes each online peer's port 42070
- Both run in parallel; results merge into one peer list, deduplicated by IP

### Terminal sharing
- Spawns a PTY (pseudo-terminal) running your `$SHELL`
- Captures raw terminal output (ANSI escape codes, colors, cursor movement)
- Streams via length-prefixed TCP frames to connected viewers
- Viewers write raw bytes to their terminal — renders identically

### Screen sharing
- Captures screen/window via [xcap](https://github.com/nashaofu/xcap) at 15 FPS
- JPEG-encodes each frame (quality 70)
- Streams via length-prefixed TCP frames
- Viewers decode and render in a Tauri webview

## Development

```bash
# Build
cd src-tauri && cargo build

# Run TUI
cd src-tauri && cargo run

# Run GUI (requires npm install first for Tauri frontend)
npm install && npm run dev:gui

# Test
cd src-tauri && cargo test

# Lint
cd src-tauri && cargo clippy -- -D warnings

# Format
cd src-tauri && cargo fmt
```

## License

MIT
