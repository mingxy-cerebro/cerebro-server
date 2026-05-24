import { readFileSync, appendFileSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

// ── Nested config interface ──────────────────────────────────────────

export interface OmemPluginConfig {
  connection: {
    apiUrl: string;
    apiKey: string;
    requestTimeoutMs: number;
  };
  content: {
    maxQueryLength: number;
    maxContentChars: number;
    maxContentLength: number;
  };
  ingest: {
    autoCaptureThreshold: number;
    ingestMode: "smart" | "raw";
  };
  recall: {
    similarityThreshold: number;
    maxRecallResults: number;
    fetchMultiplier: number;
    topkCapMultiplier: number;
    mmrJaccardThreshold: number;
    mmrPenaltyFactor: number;
    phase2Multiplier: number;
    llmMaxEval: number;
    refineStrategy: "strict" | "balanced" | "loose";
  };
  logging: {
    logEnabled: boolean;
    logLevel: "DEBUG" | "INFO" | "WARN" | "ERROR";
    logDir: string;
  };
  ui: {
    toastDelayMs: number;
  };
  web?: {
    enabled?: boolean;
    port?: number;
  };
  profile?: {
    ttlMs?: number;
  };
  agentMemoryPolicy?: Record<string, "none" | "readonly" | "readwrite">;
  defaultPolicy?: "none" | "readonly" | "readwrite";
}

// ── Defaults ─────────────────────────────────────────────────────────

const DEFAULTS: OmemPluginConfig = {
  connection: {
    apiUrl: "https://www.mengxy.cc",
    apiKey: "",
    requestTimeoutMs: 15000,
  },
  content: {
    maxQueryLength: 200,
    maxContentChars: 30000,
    maxContentLength: 500,
  },
  ingest: {
    autoCaptureThreshold: 5,
    ingestMode: "smart",
  },
  recall: {
    similarityThreshold: 0.4,
    maxRecallResults: 10,
    fetchMultiplier: 3,
    topkCapMultiplier: 2,
    mmrJaccardThreshold: 0.85,
    mmrPenaltyFactor: 0.5,
    phase2Multiplier: 2,
    llmMaxEval: 15,
    refineStrategy: "loose",
  },
  logging: {
    logEnabled: true,
    logLevel: "INFO",
    logDir: join(homedir(), ".config", "cerebro"),
  },
  ui: {
    toastDelayMs: 7000,
  },
  web: {
    enabled: true,
  },
  profile: {
    ttlMs: 300000,
  },
};

// ── Flat-to-nested migration ─────────────────────────────────────────

/** Shape of legacy flat config (pre-nesting). */
interface FlatConfig {
  apiUrl?: string;
  apiKey?: string;
  requestTimeoutMs?: number;
  maxQueryLength?: number;
  maxContentChars?: number;
  maxContentLength?: number;
  autoCaptureThreshold?: number;
  ingestMode?: "smart" | "raw";
  similarityThreshold?: number;
  maxRecallResults?: number;
  toastDelayMs?: number;
  logEnabled?: boolean;
  logLevel?: "DEBUG" | "INFO" | "WARN" | "ERROR";
  logDir?: string;
  // Nested fields that would indicate new format
  connection?: unknown;
}

function isFlatConfig(cfg: Record<string, unknown>): boolean {
  return "apiUrl" in cfg && !("connection" in cfg);
}

function migrateFlatToNested(flat: FlatConfig): OmemPluginConfig {
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
    ingest: {
      autoCaptureThreshold: flat.autoCaptureThreshold ?? DEFAULTS.ingest.autoCaptureThreshold,
      ingestMode: flat.ingestMode ?? DEFAULTS.ingest.ingestMode,
    },
    recall: {
      similarityThreshold: flat.similarityThreshold ?? DEFAULTS.recall.similarityThreshold,
      maxRecallResults: flat.maxRecallResults ?? DEFAULTS.recall.maxRecallResults,
      fetchMultiplier: DEFAULTS.recall.fetchMultiplier,
      topkCapMultiplier: DEFAULTS.recall.topkCapMultiplier,
      mmrJaccardThreshold: DEFAULTS.recall.mmrJaccardThreshold,
      mmrPenaltyFactor: DEFAULTS.recall.mmrPenaltyFactor,
      phase2Multiplier: DEFAULTS.recall.phase2Multiplier,
      llmMaxEval: DEFAULTS.recall.llmMaxEval,
      refineStrategy: DEFAULTS.recall.refineStrategy,
    },
    logging: {
      logEnabled: flat.logEnabled ?? DEFAULTS.logging.logEnabled,
      logLevel: flat.logLevel ?? DEFAULTS.logging.logLevel,
      logDir: flat.logDir ?? DEFAULTS.logging.logDir,
    },
    ui: {
      toastDelayMs: flat.toastDelayMs ?? DEFAULTS.ui.toastDelayMs,
    },
  };
}

// ── Helpers ──────────────────────────────────────────────────────────

type IngestMode = "smart" | "raw";
const INGEST_MODES: ReadonlySet<string> = new Set<IngestMode>(["smart", "raw"]);

function deepMerge(base: OmemPluginConfig, overrides: Partial<OmemPluginConfig>): OmemPluginConfig {
  const result: OmemPluginConfig = {
    connection: { ...base.connection, ...overrides.connection },
    content: { ...base.content, ...overrides.content },
    ingest: { ...base.ingest, ...overrides.ingest },
    recall: { ...base.recall, ...overrides.recall },
    logging: { ...base.logging, ...overrides.logging },
    ui: { ...base.ui, ...overrides.ui },
  };
  result.web = { ...base.web!, ...overrides.web };
  result.profile = { ...base.profile!, ...overrides.profile };
  if (overrides.agentMemoryPolicy) result.agentMemoryPolicy = overrides.agentMemoryPolicy;
  if (overrides.defaultPolicy) result.defaultPolicy = overrides.defaultPolicy;
  return result;
}

// ── Load config ──────────────────────────────────────────────────────

const LEVEL_MAP: Record<string, number> = { DEBUG: 0, INFO: 1, WARN: 2, ERROR: 3 };

function readConfiguredLogLevel(): number {
  try {
    const cfgPath = join(homedir(), ".config", "cerebro", "config.json");
    const raw = JSON.parse(readFileSync(cfgPath, "utf-8")) as Record<string, unknown>;
    const nested = (raw?.logging as Record<string, unknown>)?.logLevel as string | undefined;
    const flat = raw?.logLevel as string | undefined;
    const level = nested ?? flat ?? "INFO";
    return LEVEL_MAP[level] ?? LEVEL_MAP.INFO;
  } catch {
    return LEVEL_MAP.INFO;
  }
}

const CONFIGURED_MIN_LEVEL = readConfiguredLogLevel();

/** File-only logger for config.ts (cannot import logger.ts due to circular dependency). */
function configLog(message: string, fields?: Record<string, unknown>, level: string = "WARN"): void {
  const lvl = LEVEL_MAP[level] ?? 0;
  if (lvl < CONFIGURED_MIN_LEVEL) return;
  try {
    const logDir = join(homedir(), ".config", "cerebro", "logs");
    const logPath = join(logDir, "plugin.log");
    const ts = new Date().toISOString().replace("T", " ").replace(/\.\d+Z$/, "");
    const parts = [`${level.padEnd(5)} ${ts} service=cerebro ${message}`];
    if (fields) {
      for (const [k, v] of Object.entries(fields)) {
        parts.push(`${k}=${typeof v === "string" ? v : JSON.stringify(v)}`);
      }
    }
    mkdirSync(logDir, { recursive: true });
    appendFileSync(logPath, parts.join(" ") + "\n");
  } catch (writeErr) {
    process.stderr.write(`[cerebro] configLog write failed: ${writeErr instanceof Error ? writeErr.message : String(writeErr)}\n`);
  }
}

export function loadPluginConfig(overrides?: Partial<OmemPluginConfig>): OmemPluginConfig {
  let config: OmemPluginConfig = structuredClone(DEFAULTS);

  // Try loading from config file
  try {
    const cfgPath = join(homedir(), ".config", "cerebro", "config.json");
    const raw = JSON.parse(readFileSync(cfgPath, "utf-8")) as Record<string, unknown>;

    // Auto-migrate flat format
    const parsed: OmemPluginConfig = isFlatConfig(raw) ? migrateFlatToNested(raw as FlatConfig) : raw as unknown as OmemPluginConfig;

    // Merge nested groups with defaults for safety
    config = deepMerge(config, parsed);
  } catch (e) {
    configLog("config.json load failed, using defaults", { error: String(e) });
  }

  // Apply explicit overrides (from opencode.json)
  if (overrides) {
    config = deepMerge(config, overrides);
  }

  // Apply environment variable overrides last — env vars have highest priority
  if (process.env.OMEM_API_URL) config.connection.apiUrl = process.env.OMEM_API_URL;
  if (process.env.OMEM_API_KEY) config.connection.apiKey = process.env.OMEM_API_KEY;
  if (process.env.OMEM_REQUEST_TIMEOUT_MS) {
    config.connection.requestTimeoutMs = parseInt(process.env.OMEM_REQUEST_TIMEOUT_MS, 10) || DEFAULTS.connection.requestTimeoutMs;
  }
  if (process.env.OMEM_AUTO_CAPTURE_THRESHOLD) {
    config.ingest.autoCaptureThreshold = parseInt(process.env.OMEM_AUTO_CAPTURE_THRESHOLD, 10) || DEFAULTS.ingest.autoCaptureThreshold;
  }
  if (INGEST_MODES.has(process.env.OMEM_INGEST_MODE ?? "")) {
    config.ingest.ingestMode = process.env.OMEM_INGEST_MODE as IngestMode;
  }
  if (process.env.OMEM_SIMILARITY_THRESHOLD) {
    config.recall.similarityThreshold = parseFloat(process.env.OMEM_SIMILARITY_THRESHOLD) || DEFAULTS.recall.similarityThreshold;
  }
  if (process.env.OMEM_MAX_RECALL_RESULTS) {
    config.recall.maxRecallResults = parseInt(process.env.OMEM_MAX_RECALL_RESULTS, 10) || DEFAULTS.recall.maxRecallResults;
  }

  if (process.env.OMEM_WEB_ENABLED === "false" || process.env.OMEM_WEB_ENABLED === "0") {
    config.web = { ...config.web!, enabled: false };
  }
  if (process.env.OMEM_LOCAL_PORT) {
    config.web = { ...config.web!, port: parseInt(process.env.OMEM_LOCAL_PORT, 10) || DEFAULTS.web!.port };
  }

  // Expand ~ to home directory in logDir
  if (config.logging.logDir?.startsWith("~")) {
    config.logging.logDir = config.logging.logDir.replace(/^~/, homedir());
  }

  return config;
}

// ── Agent policy resolver ────────────────────────────────────────────

export type AgentPolicy = "none" | "readonly" | "readwrite";

export function resolveAgentPolicy(
  agentName: string,
  config: Partial<OmemPluginConfig>,
): AgentPolicy {
  const policies = config.agentMemoryPolicy;
  if (policies) {
    const exact = policies[agentName];
    if (exact) return exact;
    const lower = agentName.toLowerCase();
    for (const [key, policy] of Object.entries(policies)) {
      if (lower.startsWith(key.toLowerCase()) || key.toLowerCase().startsWith(lower)) {
        return policy;
      }
    }
  }
  if (config.defaultPolicy) return config.defaultPolicy;
  configLog("resolveAgentPolicy: defaulting to readwrite", { agentName }, "DEBUG");
  return "readwrite";
}

export { DEFAULTS };
