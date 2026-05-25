import * as http from "node:http";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { logInfo, logWarn, logError } from "./logger.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const DEFAULT_PORT = 5212;
const TAKEOVER_INTERVAL_MS = 5000;
const TAKEOVER_JITTER_MIN_MS = 500;
const TAKEOVER_JITTER_RANGE_MS = 1000;
const TAKEOVER_MAX_RETRIES = 60;

export interface WebServerConfig {
  apiUrl: string;
  port?: number;
}

export interface WebServerHandle {
  address(): { port: number; family: string; address: string } | string | null;
  isOwner(): boolean;
  stop(): Promise<void>;
}

const MIME_TYPES: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "application/javascript; charset=utf-8",
  ".mjs": "application/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".ico": "image/x-icon",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
  ".ttf": "font/ttf",
  ".webp": "image/webp",
  ".map": "application/json",
};

const COMMON_HEADERS: Record<string, string> = {
  "X-Content-Type-Options": "nosniff",
};

function resolveSafe(baseDir: string, pathname: string): string | null {
  const relative = pathname.startsWith("/") ? pathname.slice(1) : pathname;
  const resolved = path.resolve(baseDir, relative || ".");
  if (!resolved.startsWith(baseDir + path.sep) && resolved !== baseDir) {
    return null;
  }
  return resolved;
}

async function probeExistingServer(port: number): Promise<boolean> {
  try {
    const resp = await fetch(`http://127.0.0.1:${port}/health`);
    if (resp.ok) {
      const body = await resp.text();
      return body.includes("cerebro");
    }
  } catch {}
  return false;
}

let activeServerHandle: WebServerHandle | null = null;
let takeoverTimer: ReturnType<typeof setInterval> | null = null;
let takeoverRetries = 0;

export async function startWebServer(config: WebServerConfig): Promise<WebServerHandle | null> {
  const port = config.port || parseInt(process.env.OMEM_LOCAL_PORT || "", 10) || DEFAULT_PORT;
  const webDir = path.resolve(__dirname, "..", "web");

  if (!fs.existsSync(webDir)) {
    logWarn("web-server: web directory not found, skipping", { webDir });
    return null;
  }
  if (!fs.existsSync(path.join(webDir, "index.html"))) {
    logWarn("web-server: index.html not found, skipping", { webDir });
    return null;
  }

  if (await probeExistingServer(port)) {
    logInfo("web-server: reusing existing server", { port });
    startTakeoverWatch(port, webDir, config.apiUrl);
    return createHandle(port);
  }

  const handle = await tryBind(port, webDir, config.apiUrl);
  if (handle) {
    activeServerHandle = handle;
    return createHandle(port);
  }

  logInfo("web-server: port busy, starting takeover watch", { port });
  startTakeoverWatch(port, webDir, config.apiUrl);
  return createHandle(port);
}

function tryBind(port: number, webDir: string, apiUrl: string): Promise<WebServerHandle | null> {
  return new Promise((resolve) => {
    const indexPath = path.join(webDir, "index.html");

    const server = http.createServer((req: http.IncomingMessage, res: http.ServerResponse) => {
      if (req.url === "/health" || req.url === "/health/") {
        res.writeHead(200, { ...COMMON_HEADERS, "Content-Type": "application/json" });
        res.end(JSON.stringify({ status: "ok", service: "cerebro", port }));
        return;
      }

      if (req.method !== "GET" && req.method !== "HEAD") {
        res.writeHead(405, { ...COMMON_HEADERS, "Content-Type": "text/plain" });
        res.end("Method Not Allowed");
        return;
      }

      const url = new URL(req.url || "/", `http://localhost:${port}`);
      const safePath = resolveSafe(webDir, url.pathname);

      if (!safePath) {
        res.writeHead(403, { ...COMMON_HEADERS, "Content-Type": "text/plain" });
        res.end("Forbidden");
        return;
      }

      fs.stat(safePath, (statErr, stats) => {
        if (!statErr && stats.isFile()) {
          serveFile(res, safePath, apiUrl);
          return;
        }
        fs.stat(indexPath, (idxErr, idxStats) => {
          if (idxErr || !idxStats.isFile()) {
            res.writeHead(404, { ...COMMON_HEADERS, "Content-Type": "text/plain" });
            res.end("Not Found");
            return;
          }
          serveFile(res, indexPath, apiUrl);
        });
      });
    });

    server.on("error", (err: NodeJS.ErrnoException) => {
      if (err.code === "EADDRINUSE") {
        resolve(null);
      } else {
        logError("web-server: bind error", { error: err.message });
        resolve(null);
      }
    });

    server.listen(port, "127.0.0.1", () => {
      logInfo("web-server: server started", { port });
      resolve({
        address: () => ({ port, family: "IPv4", address: "127.0.0.1" }),
        isOwner: () => true,
        stop: () => new Promise<void>((r) => server.close(() => r())),
      });
    });
  });
}

function serveFile(res: http.ServerResponse, filePath: string, apiUrl: string): void {
  const ext = path.extname(filePath).toLowerCase();
  const contentType = MIME_TYPES[ext] || "application/octet-stream";

  fs.readFile(filePath, (err, data) => {
    if (err) {
      logError("web-server: file read failed", { filePath, error: err.message });
      res.writeHead(500, { ...COMMON_HEADERS, "Content-Type": "text/plain" });
      res.end("Internal Server Error");
      return;
    }

    let body: Buffer | string = data;
    if (ext === ".html" && data.includes("__OMEM_API_URL__")) {
      body = data.toString("utf-8").replace(
        /window\.__OMEM_API_URL__\s*=\s*["']__OMEM_API_URL__["']/,
        `window.__OMEM_API_URL__ = ${JSON.stringify(apiUrl)}`,
      );
    }

    res.writeHead(200, {
      ...COMMON_HEADERS,
      "Content-Type": contentType,
      "Cache-Control": ext === ".html" ? "no-cache, no-store, must-revalidate" : "public, max-age=86400",
    });
    res.end(body);
  });
}

function clearTakeoverTimer(): void {
  if (takeoverTimer) {
    clearInterval(takeoverTimer);
    takeoverTimer = null;
  }
}

function startTakeoverWatch(port: number, webDir: string, apiUrl: string): void {
  if (takeoverTimer) return;
  takeoverRetries = 0;
  takeoverTimer = setInterval(async () => {
    if (await probeExistingServer(port)) {
      takeoverRetries = 0;
      return;
    }

    takeoverRetries++;
    if (takeoverRetries > TAKEOVER_MAX_RETRIES) {
      logWarn("web-server: takeover abandoned after max retries", { port });
      clearTakeoverTimer();
      return;
    }

    const jitter = TAKEOVER_JITTER_MIN_MS + Math.random() * TAKEOVER_JITTER_RANGE_MS;
    await new Promise((r) => setTimeout(r, jitter));
    if (await probeExistingServer(port)) {
      takeoverRetries = 0;
      return;
    }

    const handle = await tryBind(port, webDir, apiUrl);
    if (handle) {
      activeServerHandle = handle;
      logInfo("web-server: takeover successful", { port });
      clearTakeoverTimer();
    }
  }, TAKEOVER_INTERVAL_MS);
  takeoverTimer.unref();
}

function createHandle(port: number): WebServerHandle {
  return {
    address: () => ({ port, family: "IPv4", address: "127.0.0.1" }),
    isOwner: () => activeServerHandle?.isOwner() ?? false,
    stop: () => {
      if (activeServerHandle?.isOwner()) {
        clearTakeoverTimer();
        const h = activeServerHandle;
        activeServerHandle = null;
        return h.stop();
      }
      clearTakeoverTimer();
      activeServerHandle = null;
      return Promise.resolve();
    },
  };
}

export function stopWebServer(handle: WebServerHandle): Promise<void> {
  return handle.stop();
}
