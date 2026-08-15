// Cerebro plugin config — ported from plugins/opencode/src/config.ts
// Loads: DEFAULTS -> ~/.config/cerebro/config.json -> env vars (highest priority)
//
// Injection-content config (recall prompt, nudge keywords, sessionStart toggles)
// is separate: ~/.zcode/cerebro.json, three-level fallback mirroring the
// claude-code plugin (~/.claude/cerebro.json):
//   user file > auto-init from bundled config.default.json > bundled default
import { readFileSync, copyFileSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const DEFAULTS = {
  connection: {
    apiUrl: "https://www.mengxy.cc",
    apiKey: "",
    requestTimeoutMs: 15000,
  },
  content: {
    maxQueryLength: 200,
    maxContentChars: 30000,
    maxContentLength: 3000,
  },
  injection: {
    recentCount: 5,
    searchCount: 10,
    recentTruncateChars: 0, // 0 = no truncation
    searchTruncateChars: 0,
    recentTimeoutMs: 3000,
    searchTimeoutMs: 5000,
    profileTimeoutMs: 2000,
  },
  ingest: {
    autoCaptureThreshold: 5,
    ingestMode: "smart",
  },
  logging: {
    logEnabled: true,
    logLevel: "INFO",
    logDir: join(homedir(), ".config", "cerebro"),
  },
};

function deepMerge(base, overrides) {
  if (!overrides) return base;
  const out = { ...base };
  for (const k of Object.keys(overrides)) {
    if (
      base[k] &&
      typeof base[k] === "object" &&
      !Array.isArray(base[k]) &&
      overrides[k] &&
      typeof overrides[k] === "object" &&
      !Array.isArray(overrides[k])
    ) {
      out[k] = deepMerge(base[k], overrides[k]);
    } else if (overrides[k] !== undefined) {
      out[k] = overrides[k];
    }
  }
  return out;
}

// Flat -> nested auto-migration (legacy config compat)
function isFlatConfig(cfg) {
  return "apiUrl" in cfg && !("connection" in cfg);
}
function migrateFlatToNested(flat) {
  return {
    connection: {
      apiUrl: flat.apiUrl ?? DEFAULTS.connection.apiUrl,
      apiKey: flat.apiKey ?? DEFAULTS.connection.apiKey,
      requestTimeoutMs: flat.requestTimeoutMs ?? DEFAULTS.connection.requestTimeoutMs,
    },
    content: {
      maxQueryLength: flat.maxQueryLength ?? DEFAULTS.content.maxQueryLength,
      maxContentChars: flat.maxContentChars ?? DEFAULTS.content.maxContentChars,
      maxContentLength: flat.maxContentLength ?? DEFAULTS.content.maxContentLength,
    },
    injection: { ...DEFAULTS.injection },
    ingest: {
      autoCaptureThreshold: flat.autoCaptureThreshold ?? DEFAULTS.ingest.autoCaptureThreshold,
      ingestMode: flat.ingestMode ?? DEFAULTS.ingest.ingestMode,
    },
    logging: {
      logEnabled: flat.logEnabled ?? DEFAULTS.logging.logEnabled,
      logLevel: flat.logLevel ?? DEFAULTS.logging.logLevel,
      logDir: flat.logDir ?? DEFAULTS.logging.logDir,
    },
  };
}

const INGEST_MODES = new Set(["smart", "raw"]);

export function loadConfig() {
  let config = structuredClone(DEFAULTS);

  try {
    const cfgPath = join(homedir(), ".config", "cerebro", "config.json");
    const raw = JSON.parse(readFileSync(cfgPath, "utf-8"));
    const parsed = isFlatConfig(raw) ? migrateFlatToNested(raw) : raw;
    config = deepMerge(config, parsed);
  } catch {}

  // Env vars have highest priority
  if (process.env.OMEM_API_URL) config.connection.apiUrl = process.env.OMEM_API_URL;
  if (process.env.OMEM_API_KEY) config.connection.apiKey = process.env.OMEM_API_KEY;
  if (process.env.OMEM_REQUEST_TIMEOUT_MS) {
    config.connection.requestTimeoutMs =
      parseInt(process.env.OMEM_REQUEST_TIMEOUT_MS, 10) || DEFAULTS.connection.requestTimeoutMs;
  }
  if (process.env.OMEM_AUTO_CAPTURE_THRESHOLD) {
    config.ingest.autoCaptureThreshold =
      parseInt(process.env.OMEM_AUTO_CAPTURE_THRESHOLD, 10) || DEFAULTS.ingest.autoCaptureThreshold;
  }
  if (INGEST_MODES.has(process.env.OMEM_INGEST_MODE ?? "")) {
    config.ingest.ingestMode = process.env.OMEM_INGEST_MODE;
  }

  // Expand ~ in logDir
  if (config.logging?.logDir?.startsWith("~")) {
    config.logging.logDir = config.logging.logDir.replace(/^~/, homedir());
  }
  return config;
}

// ── Injection-content config (zcode-specific, independent from server config) ──
const PLUGIN_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const ZC_USER_CONFIG = join(homedir(), ".zcode", "cerebro.json");
const ZC_BUNDLED_CONFIG = join(PLUGIN_ROOT, "config.default.json");

const INJECTION_FALLBACK = {
  language: "en",
  recall: { enabled: true },
  nudge: { enabled: true },
  sessionStart: { profileEnabled: true, recentActivityEnabled: true, timeEnabled: true },
};

export function loadInjectionConfig() {
  // 1. User config exists → read it
  try {
    return JSON.parse(readFileSync(ZC_USER_CONFIG, "utf-8"));
  } catch {}
  // 2. First run → auto-initialize by copying bundled default
  try {
    mkdirSync(dirname(ZC_USER_CONFIG), { recursive: true });
    copyFileSync(ZC_BUNDLED_CONFIG, ZC_USER_CONFIG);
    return JSON.parse(readFileSync(ZC_USER_CONFIG, "utf-8"));
  } catch {}
  // 3. Fallback → read bundled default directly
  try {
    return JSON.parse(readFileSync(ZC_BUNDLED_CONFIG, "utf-8"));
  } catch {
    return structuredClone(INJECTION_FALLBACK);
  }
}
