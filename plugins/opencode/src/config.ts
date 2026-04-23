import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

export interface OmemPluginConfig {
  // Connection
  apiUrl: string;
  apiKey: string;
  // Timeouts (milliseconds)
  requestTimeoutMs: number;
  // Content limits
  maxQueryLength: number;
  maxContentChars: number;
  maxContentLength: number;
  // Auto capture
  autoCaptureThreshold: number;
  ingestMode: "smart" | "raw";
  // Recall settings
  similarityThreshold: number;
  maxRecallResults: number;
  // UI settings
  toastDelayMs: number;
}

const DEFAULTS: OmemPluginConfig = {
  apiUrl: "https://www.mengxy.cc",
  apiKey: "",
  requestTimeoutMs: 15000,
  maxQueryLength: 200,
  maxContentChars: 30000,
  maxContentLength: 500,
  autoCaptureThreshold: 5,
  ingestMode: "smart",
  similarityThreshold: 0.4,
  maxRecallResults: 10,
  toastDelayMs: 7000,
};

export function loadPluginConfig(overrides?: Partial<OmemPluginConfig>): OmemPluginConfig {
  const config: Partial<OmemPluginConfig> = { ...DEFAULTS };

  // Try loading from config file
  try {
    const cfgPath = join(homedir(), ".config", "ourmem", "config.json");
    const cfg = JSON.parse(readFileSync(cfgPath, "utf-8"));

    if (cfg.apiUrl) config.apiUrl = cfg.apiUrl;
    if (cfg.apiKey) config.apiKey = cfg.apiKey;
    if (typeof cfg.requestTimeoutMs === "number") config.requestTimeoutMs = cfg.requestTimeoutMs;
    if (typeof cfg.maxQueryLength === "number") config.maxQueryLength = cfg.maxQueryLength;
    if (typeof cfg.maxContentChars === "number") config.maxContentChars = cfg.maxContentChars;
    if (typeof cfg.maxContentLength === "number") config.maxContentLength = cfg.maxContentLength;
    if (typeof cfg.autoCaptureThreshold === "number") config.autoCaptureThreshold = cfg.autoCaptureThreshold;
    if (cfg.ingestMode === "raw" || cfg.ingestMode === "smart") config.ingestMode = cfg.ingestMode;
    if (typeof cfg.similarityThreshold === "number") config.similarityThreshold = cfg.similarityThreshold;
    if (typeof cfg.maxRecallResults === "number") config.maxRecallResults = cfg.maxRecallResults;
    if (typeof cfg.toastDelayMs === "number") config.toastDelayMs = cfg.toastDelayMs;
  } catch {
    // Config file doesn't exist or is invalid, use defaults
  }

  // Apply environment variable overrides
  if (process.env.OMEM_API_URL) config.apiUrl = process.env.OMEM_API_URL;
  if (process.env.OMEM_API_KEY) config.apiKey = process.env.OMEM_API_KEY;
  if (process.env.OMEM_REQUEST_TIMEOUT_MS) {
    config.requestTimeoutMs = parseInt(process.env.OMEM_REQUEST_TIMEOUT_MS, 10) || DEFAULTS.requestTimeoutMs;
  }
  if (process.env.OMEM_AUTO_CAPTURE_THRESHOLD) {
    config.autoCaptureThreshold = parseInt(process.env.OMEM_AUTO_CAPTURE_THRESHOLD, 10) || DEFAULTS.autoCaptureThreshold;
  }
  if (process.env.OMEM_INGEST_MODE === "raw" || process.env.OMEM_INGEST_MODE === "smart") {
    config.ingestMode = process.env.OMEM_INGEST_MODE;
  }
  if (process.env.OMEM_SIMILARITY_THRESHOLD) {
    config.similarityThreshold = parseFloat(process.env.OMEM_SIMILARITY_THRESHOLD) || DEFAULTS.similarityThreshold;
  }
  if (process.env.OMEM_MAX_RECALL_RESULTS) {
    config.maxRecallResults = parseInt(process.env.OMEM_MAX_RECALL_RESULTS, 10) || DEFAULTS.maxRecallResults;
  }

  // Apply explicit overrides (from opencode.json)
  if (overrides) {
    Object.assign(config, overrides);
  }

  return config as OmemPluginConfig;
}

export { DEFAULTS };
