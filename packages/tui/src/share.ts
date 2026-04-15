import * as net from "net";
import * as pty from "node-pty";
import { STREAM_PORT, writeFrame } from "./protocol.js";

export interface ShareHandle {
  pty: pty.IPty;
  stop: () => void;
}

export function startSharing(shell?: string): ShareHandle {
  const shellCmd = shell || process.env.SHELL || "/bin/sh";
  const cols = process.stdout.columns || 80;
  const rows = process.stdout.rows || 24;

  const ptyProcess = pty.spawn(shellCmd, [], {
    name: "xterm-256color",
    cols,
    rows,
    cwd: process.env.HOME || "/",
    env: process.env as Record<string, string>,
  });

  const viewers = new Set<net.Socket>();

  // Mirror PTY output to all connected viewers
  ptyProcess.onData((data) => {
    const buf = Buffer.from(data);
    for (const viewer of viewers) {
      if (!writeFrame(viewer, buf)) {
        viewers.delete(viewer);
        viewer.destroy();
      }
    }
  });

  // TCP server for viewers
  const server = net.createServer((socket) => {
    viewers.add(socket);
    socket.on("close", () => viewers.delete(socket));
    socket.on("error", () => {
      viewers.delete(socket);
      socket.destroy();
    });
  });

  server.listen(STREAM_PORT, "0.0.0.0");

  // Handle terminal resize
  const onResize = () => {
    ptyProcess.resize(
      process.stdout.columns || 80,
      process.stdout.rows || 24
    );
  };
  process.stdout.on("resize", onResize);

  const stop = () => {
    process.stdout.off("resize", onResize);
    for (const viewer of viewers) {
      viewer.destroy();
    }
    viewers.clear();
    server.close();
    ptyProcess.kill();
  };

  return { pty: ptyProcess, stop };
}
