import { readFileSync } from "fs";
import { join } from "path";
import { homedir } from "os";

export interface Config {
  discovery: {
    lan: boolean;
    tailscale: boolean;
    discovery_port: number;
    stream_port: number;
  };
  sharing: {
    default_mode: "terminal" | "screen";
  };
  tailscale: {
    poll_interval: number;
    probe_timeout: number;
  };
  network: {
    stale_timeout: number;
    announce_interval: number;
  };
}

const DEFAULTS: Config = {
  discovery: {
    lan: true,
    tailscale: true,
    discovery_port: 42069,
    stream_port: 42070,
  },
  sharing: {
    default_mode: "terminal",
  },
  tailscale: {
    poll_interval: 5,
    probe_timeout: 500,
  },
  network: {
    stale_timeout: 6,
    announce_interval: 2,
  },
};

export function loadConfig(): Config {
  const configPath = join(
    homedir(),
    ".config",
    "screenshare",
    "config.toml"
  );

  try {
    const raw = readFileSync(configPath, "utf-8");
    const overrides = parseSimpleToml(raw);
    return deepMerge(DEFAULTS, overrides) as Config;
  } catch {
    return DEFAULTS;
  }
}

// Minimal TOML parser for flat key-value sections (no nested tables, no arrays)
function parseSimpleToml(content: string): Record<string, Record<string, unknown>> {
  const result: Record<string, Record<string, unknown>> = {};
  let currentSection = "";

  for (const line of content.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;

    const sectionMatch = trimmed.match(/^\[(.+)]$/);
    if (sectionMatch) {
      currentSection = sectionMatch[1];
      if (!result[currentSection]) result[currentSection] = {};
      continue;
    }

    const kvMatch = trimmed.match(/^(\w+)\s*=\s*(.+)$/);
    if (kvMatch && currentSection) {
      const [, key, rawValue] = kvMatch;
      let value: unknown = rawValue.trim();
      if (value === "true") value = true;
      else if (value === "false") value = false;
      else if (/^\d+$/.test(value as string)) value = parseInt(value as string, 10);
      else if (/^\d+\.\d+$/.test(value as string)) value = parseFloat(value as string);
      else if ((value as string).startsWith('"') && (value as string).endsWith('"'))
        value = (value as string).slice(1, -1);
      result[currentSection][key] = value;
    }
  }

  return result;
}

function deepMerge(target: unknown, source: unknown): unknown {
  if (typeof target !== "object" || target === null) return source ?? target;
  if (typeof source !== "object" || source === null) return target;

  const result = { ...(target as Record<string, unknown>) };
  for (const key of Object.keys(source as Record<string, unknown>)) {
    result[key] = deepMerge(
      result[key],
      (source as Record<string, unknown>)[key]
    );
  }
  return result;
}
