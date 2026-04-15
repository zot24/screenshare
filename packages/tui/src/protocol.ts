import * as net from "net";

export const DISCOVERY_PORT = 42069;
export const STREAM_PORT = 42070;
export const DISCOVERY_INTERVAL_MS = 2000;
export const STALE_TIMEOUT_MS = 6000;
export const TAILSCALE_POLL_INTERVAL_MS = 5000;
export const TAILSCALE_PROBE_TIMEOUT_MS = 500;

export interface DiscoveryMessage {
  hostname: string;
  ip: string;
  port: number;
  sharing: string;
  mode: string; // "terminal" or "screen"
  source: string; // "lan" or "tailscale"
}

export function writeFrame(socket: net.Socket, data: Buffer): boolean {
  const header = Buffer.alloc(4);
  header.writeUInt32BE(data.length, 0);
  try {
    socket.write(header);
    socket.write(data);
    return true;
  } catch {
    return false;
  }
}

export class FrameReader {
  private buffer: Buffer = Buffer.alloc(0);
  private expectedLen: number | null = null;

  push(chunk: Buffer): Buffer[] {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    const frames: Buffer[] = [];

    while (true) {
      if (this.expectedLen === null) {
        if (this.buffer.length < 4) break;
        this.expectedLen = this.buffer.readUInt32BE(0);
        this.buffer = this.buffer.subarray(4);
      }

      if (this.buffer.length < this.expectedLen) break;

      frames.push(this.buffer.subarray(0, this.expectedLen));
      this.buffer = this.buffer.subarray(this.expectedLen);
      this.expectedLen = null;
    }

    return frames;
  }
}
