#!/usr/bin/env node
import { createCliRenderer, Box, Text } from "@opentui/core";
import { networkInterfaces, hostname as getHostname } from "os";
import { Discovery, type Sharer } from "./discovery.js";
import { startSharing, type ShareHandle } from "./share.js";
import { startViewing, type ViewerHandle } from "./viewer.js";
import { loadConfig } from "./config.js";

const config = loadConfig();

function getLocalIp(): string {
  const nets = networkInterfaces();
  for (const name of Object.keys(nets)) {
    for (const entry of nets[name] || []) {
      if (entry.family === "IPv4" && !entry.internal) {
        return entry.address;
      }
    }
  }
  return "127.0.0.1";
}

const myIp = getLocalIp();
const myHostname = getHostname();

let currentSharers = new Map<string, Sharer>();
let shareHandle: ShareHandle | null = null;
let viewerHandle: ViewerHandle | null = null;
let isSharing = false;
let isViewing = false;
let selectedIndex = 0;

const discovery = new Discovery(myIp, (sharers) => {
  currentSharers = sharers;
  if (!isViewing) renderHome();
}, config.network.stale_timeout * 1000);

async function main() {
  const renderer = await createCliRenderer({ exitOnCtrlC: false });

  discovery.start();

  // Handle keyboard input
  process.stdin.setRawMode?.(true);
  process.stdin.resume();
  process.stdin.on("data", (data) => {
    const key = data.toString();

    if (isViewing) {
      // Escape or 'q' to go back
      if (key === "\x1b" || key === "q") {
        stopViewingSession();
        return;
      }
      return;
    }

    switch (key) {
      case "s": // Toggle share
        if (isSharing) {
          stopSharingSession();
        } else {
          startSharingSession();
        }
        break;
      case "j": // Move down
      case "\x1b[B":
        selectedIndex = Math.min(selectedIndex + 1, currentSharers.size - 1);
        renderHome();
        break;
      case "k": // Move up
      case "\x1b[A":
        selectedIndex = Math.max(selectedIndex - 1, 0);
        renderHome();
        break;
      case "\r": // Enter - view selected
      case "\n":
        viewSelected();
        break;
      case "q": // Quit
      case "\x03": // Ctrl+C
        cleanup();
        process.exit(0);
        break;
    }
  });

  renderHome();

  function renderHome() {
    renderer.root.clear();

    const sharerList: ReturnType<typeof Box>[] = [];
    const sharerArray = Array.from(currentSharers.values()).filter(
      (s) => s.mode === "terminal" || s.mode === ""
    );

    if (sharerArray.length === 0) {
      sharerList.push(
        Text({ content: "  Scanning local network...", fg: "#666666" })
      );
    } else {
      sharerArray.forEach((s, i) => {
        const prefix = i === selectedIndex ? "> " : "  ";
        const sourceTag = s.source === "tailscale" ? " [ts]" : "";
        const sharingLabel = s.sharing ? ` - ${s.sharing}` : "";
        const fg = i === selectedIndex ? "#00FFFF" : "#CCCCCC";
        sharerList.push(
          Text({
            content: `${prefix}${s.hostname} (${s.ip})${sharingLabel}${sourceTag}`,
            fg,
          })
        );
      });
    }

    const statusText = isSharing
      ? "SHARING (press 's' to stop)"
      : "press 's' to share terminal";

    renderer.root.add(
      Box(
        {
          flexDirection: "column",
          padding: 1,
          gap: 1,
        },
        Box(
          { flexDirection: "column" },
          Text({ content: "LAN Screen Share", fg: "#FFFF00", bold: true }),
          Text({ content: `You: ${myHostname} (${myIp})`, fg: "#888888" }),
        ),
        Box(
          { flexDirection: "column" },
          Text({
            content: isSharing ? `● ${statusText}` : statusText,
            fg: isSharing ? "#00FF00" : "#888888",
          }),
        ),
        Box(
          { flexDirection: "column", borderStyle: "rounded", padding: 1 },
          Text({ content: "Terminal sharers on your network:", fg: "#AAAAAA" }),
          ...sharerList,
        ),
        Text({
          content: "j/k: navigate  enter: view  s: share  q: quit",
          fg: "#555555",
        }),
      )
    );
  }

  function startSharingSession() {
    shareHandle = startSharing();
    isSharing = true;
    discovery.startAnnouncing(myHostname, `Terminal - ${process.env.SHELL || "sh"}`, "terminal");

    // Attach PTY to stdin/stdout
    process.stdin.setRawMode?.(false);
    renderer.root.clear();

    shareHandle.pty.onData((data) => {
      process.stdout.write(data);
    });

    process.stdin.on("data", (data) => {
      if (isSharing && shareHandle) {
        shareHandle.pty.write(data.toString());
      }
    });

    process.stdin.setRawMode?.(true);
  }

  function stopSharingSession() {
    if (shareHandle) {
      shareHandle.stop();
      shareHandle = null;
    }
    isSharing = false;
    discovery.stopAnnouncing();
    renderHome();
  }

  function viewSelected() {
    const sharerArray = Array.from(currentSharers.values()).filter(
      (s) => s.mode === "terminal" || s.mode === ""
    );
    if (selectedIndex >= sharerArray.length) return;
    const sharer = sharerArray[selectedIndex];

    renderer.root.clear();
    isViewing = true;

    // Switch to raw terminal mode for viewing
    process.stdout.write(`\x1b[2J\x1b[H`); // Clear screen
    process.stdout.write(`Connecting to ${sharer.hostname}...\r\n`);

    viewerHandle = startViewing(
      sharer.ip,
      sharer.port,
      (data) => {
        process.stdout.write(data);
      },
      () => {
        process.stdout.write("\r\nDisconnected. Press 'q' to go back.\r\n");
      }
    );
  }

  function stopViewingSession() {
    if (viewerHandle) {
      viewerHandle.stop();
      viewerHandle = null;
    }
    isViewing = false;
    process.stdout.write("\x1b[2J\x1b[H"); // Clear screen
    renderHome();
  }

  function cleanup() {
    discovery.stop();
    if (shareHandle) shareHandle.stop();
    if (viewerHandle) viewerHandle.stop();
  }
}

main().catch(console.error);
