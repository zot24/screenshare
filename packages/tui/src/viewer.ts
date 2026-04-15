import * as net from "net";
import { FrameReader } from "./protocol.js";

export interface ViewerHandle {
  stop: () => void;
}

export function startViewing(
  ip: string,
  port: number,
  onData: (data: Buffer) => void,
  onDisconnect: () => void
): ViewerHandle {
  const socket = new net.Socket();
  const reader = new FrameReader();

  socket.connect(port, ip, () => {
    // connected
  });

  socket.on("data", (chunk) => {
    const frames = reader.push(chunk);
    for (const frame of frames) {
      onData(frame);
    }
  });

  socket.on("close", onDisconnect);
  socket.on("error", () => {
    socket.destroy();
    onDisconnect();
  });

  return {
    stop: () => socket.destroy(),
  };
}
