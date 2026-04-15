import * as dgram from "dgram";
import * as net from "net";
import { execFile } from "child_process";
import {
  DISCOVERY_PORT,
  DISCOVERY_INTERVAL_MS,
  STREAM_PORT,
  TAILSCALE_POLL_INTERVAL_MS,
  TAILSCALE_PROBE_TIMEOUT_MS,
  type DiscoveryMessage,
} from "./protocol.js";

export interface Sharer extends DiscoveryMessage {
  lastSeen: number;
}

type SharerCallback = (sharers: Map<string, Sharer>) => void;

export class Discovery {
  private sharers = new Map<string, Sharer>();
  private myIp: string;
  private callback: SharerCallback;
  private announceTimer?: ReturnType<typeof setInterval>;
  private staleTimer?: ReturnType<typeof setInterval>;
  private tailscaleTimer?: ReturnType<typeof setInterval>;
  private udpSocket?: dgram.Socket;
  private staleTimeoutMs: number;

  constructor(myIp: string, callback: SharerCallback, staleTimeoutMs = 6000) {
    this.myIp = myIp;
    this.callback = callback;
    this.staleTimeoutMs = staleTimeoutMs;
  }

  start() {
    this.startLanListener();
    this.startTailscaleProber();
    this.staleTimer = setInterval(() => this.pruneStale(), 1000);
  }

  stop() {
    if (this.announceTimer) clearInterval(this.announceTimer);
    if (this.staleTimer) clearInterval(this.staleTimer);
    if (this.tailscaleTimer) clearInterval(this.tailscaleTimer);
    this.udpSocket?.close();
  }

  startAnnouncing(hostname: string, sharing: string, mode: string) {
    const msg: DiscoveryMessage = {
      hostname,
      ip: this.myIp,
      port: STREAM_PORT,
      sharing,
      mode,
      source: "lan",
    };
    const payload = Buffer.from(JSON.stringify(msg));

    const socket = dgram.createSocket("udp4");
    socket.bind(0, () => {
      socket.setBroadcast(true);
      this.announceTimer = setInterval(() => {
        socket.send(payload, DISCOVERY_PORT, "255.255.255.255");
      }, DISCOVERY_INTERVAL_MS);
      // Send immediately
      socket.send(payload, DISCOVERY_PORT, "255.255.255.255");
    });
  }

  stopAnnouncing() {
    if (this.announceTimer) {
      clearInterval(this.announceTimer);
      this.announceTimer = undefined;
    }
  }

  private startLanListener() {
    this.udpSocket = dgram.createSocket({ type: "udp4", reuseAddr: true });

    this.udpSocket.on("message", (data) => {
      try {
        const msg: DiscoveryMessage = JSON.parse(data.toString());
        if (msg.ip === this.myIp) return;
        msg.source = msg.source || "lan";
        this.addSharer(msg);
      } catch {
        // ignore malformed messages
      }
    });

    this.udpSocket.bind(DISCOVERY_PORT);
  }

  private startTailscaleProber() {
    // Check if tailscale exists
    execFile("tailscale", ["version"], (err) => {
      if (err) return; // not installed, silently skip

      const probe = () => {
        execFile("tailscale", ["status", "--json"], (err, stdout) => {
          if (err) return;
          try {
            const status = JSON.parse(stdout);
            const peers = status.Peer || {};
            for (const peer of Object.values(peers) as TailscalePeer[]) {
              if (!peer.Online) continue;
              const ip = peer.TailscaleIPs?.[0];
              if (!ip || ip === this.myIp) continue;
              this.probePeer(ip, peer.HostName);
            }
          } catch {
            // ignore parse errors
          }
        });
      };

      probe();
      this.tailscaleTimer = setInterval(probe, TAILSCALE_POLL_INTERVAL_MS);
    });
  }

  private probePeer(ip: string, hostname: string) {
    const socket = new net.Socket();
    socket.setTimeout(TAILSCALE_PROBE_TIMEOUT_MS);

    socket.connect(STREAM_PORT, ip, () => {
      socket.destroy();
      this.addSharer({
        hostname,
        ip,
        port: STREAM_PORT,
        sharing: "",
        mode: "",
        source: "tailscale",
      });
    });

    socket.on("error", () => socket.destroy());
    socket.on("timeout", () => socket.destroy());
  }

  private addSharer(msg: DiscoveryMessage) {
    const existing = this.sharers.get(msg.ip);
    this.sharers.set(msg.ip, {
      ...msg,
      // Keep richer info from LAN broadcast over bare Tailscale probe
      sharing: msg.sharing || existing?.sharing || "",
      mode: msg.mode || existing?.mode || "",
      lastSeen: Date.now(),
    });
    this.callback(this.sharers);
  }

  private pruneStale() {
    const now = Date.now();
    let changed = false;
    for (const [ip, sharer] of this.sharers) {
      if (now - sharer.lastSeen > this.staleTimeoutMs) {
        this.sharers.delete(ip);
        changed = true;
      }
    }
    if (changed) this.callback(this.sharers);
  }
}

interface TailscalePeer {
  HostName: string;
  TailscaleIPs?: string[];
  Online: boolean;
}
