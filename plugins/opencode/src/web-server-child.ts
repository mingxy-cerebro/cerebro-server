/**
 * web-server-child.ts — Cerebro Web Server Child Process
 *
 * 独立 fork 入口。实际 HTTP server 运行在此进程中。
 * 通过 IPC 从父进程接收配置，启动后通知父进程 ready。
 * 通过心跳文件 mtime 探测 plugin 进程存活状态。
 */
import * as http from "node:http";
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

// ── Lightweight file logger for child process ──────────────────────────
// Cannot import logger.ts (no opencodeClient + circular dependency risk).
// Writes to cerebro-web-child.log in the configured log directory.

const CHILD_LOG_FILE = path.join(
  process.env.OMEM_LOG_DIR || path.join(os.homedir(), ".config", "cerebro", "logs"),
  "cerebro-web-child.log",
);

function childLog(level: string, message: string, fields?: Record<string, unknown>): void {
  try {
    const dir = path.dirname(CHILD_LOG_FILE);
    if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
    const now = new Date();
    const ts = now.toISOString().replace("T", " ").replace(/\.\d+Z$/, "");
    const parts = [`${level.padEnd(5)} ${ts} service=cerebro-web-child`];
    if (fields) {
      for (const [k, v] of Object.entries(fields)) {
        parts.push(`${k}=${typeof v === "string" ? v : JSON.stringify(v)}`);
      }
    }
    parts.push(message);
    fs.appendFileSync(CHILD_LOG_FILE, parts.join(" ") + "\n");
  } catch { /* best effort */ }
}

// ── Types ────────────────────────────────────────────────────────────────

interface ChildConfig {
  port: number;
  webDir: string;
  apiUrl: string;
}

// ── MIME map ─────────────────────────────────────────────────────────────

const MIME_TYPES: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "application/javascript; charset=utf-8",
  ".mjs": "application/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".gif": "image/gif",
  ".ico": "image/x-icon",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
  ".ttf": "font/ttf",
  ".eot": "application/vnd.ms-fontobject",
  ".webp": "image/webp",
  ".webmanifest": "application/manifest+json",
  ".map": "application/json",
  ".txt": "text/plain; charset=utf-8",
};

function getMimeType(ext: string): string {
  return MIME_TYPES[ext] || "application/octet-stream";
}

// ── Safe path resolver (prevent traversal) ───────────────────────────────

function resolveSafe(baseDir: string, pathname: string): string | null {
  const relative = pathname.startsWith("/") ? pathname.slice(1) : pathname;
  const resolved = path.resolve(baseDir, relative || ".");
  if (!resolved.startsWith(baseDir + path.sep) && resolved !== baseDir) {
    return null;
  }
  return resolved;
}

// ── File serving ─────────────────────────────────────────────────────────

function serveFile(
  res: http.ServerResponse,
  filePath: string,
  apiUrl: string,
): void {
  const ext = path.extname(filePath).toLowerCase();
  const contentType = getMimeType(ext);

  fs.readFile(filePath, (err, data) => {
    if (err) {
      res.writeHead(500, { "Content-Type": "text/plain" });
      res.end("Internal Server Error");
      return;
    }

    let body: Buffer | string = data;

    // Config injection: replace placeholder in index.html
    if (ext === ".html" && data.includes("__OMEM_API_URL__")) {
      body = data
        .toString("utf-8")
        .replace(
          /window\.__OMEM_API_URL__\s*=\s*["']__OMEM_API_URL__["']/,
          `window.__OMEM_API_URL__ = "${apiUrl}"`,
        );
    }

    res.writeHead(200, {
      "Content-Type": contentType,
      "Cache-Control":
        ext === ".html"
          ? "no-cache, no-store, must-revalidate"
          : "public, max-age=86400",
    });
    res.end(body);
  });
}

// ── Server lifecycle ─────────────────────────────────────────────────────

let server: http.Server | null = null;
let heartbeatTimer: ReturnType<typeof setInterval> | null = null;
let pidFilePath = "";
let heartbeatFilePath = "";

function cleanup(): void {
  if (heartbeatTimer) {
    clearInterval(heartbeatTimer);
    heartbeatTimer = null;
  }
  if (server) {
    server.closeAllConnections?.();
    const forceTimer = setTimeout(() => {
      try { fs.unlinkSync(pidFilePath); } catch { /* ignore */ }
      process.exit(0);
    }, 3000);
    server.close(() => {
      clearTimeout(forceTimer);
      try { fs.unlinkSync(pidFilePath); } catch { /* ignore */ }
      process.exit(0);
    });
  } else {
    try { fs.unlinkSync(pidFilePath); } catch { /* ignore */ }
    process.exit(0);
  }
}

function startServer(config: ChildConfig): void {
  const { port, webDir, apiUrl } = config;

  pidFilePath = path.join(os.tmpdir(), `cerebro-web-${port}.pid`);
  heartbeatFilePath = path.join(os.tmpdir(), `cerebro-web-${port}.heartbeat`);

  const indexPath = path.join(webDir, "index.html");

  // 写 PID 文件（子进程自己的 PID）
  try {
    fs.writeFileSync(pidFilePath, String(process.pid));
  } catch {
    childLog("ERROR", "Failed to write PID file");
  }

  // 初始 touch 心跳文件
  try {
    fs.writeFileSync(heartbeatFilePath, "");
  } catch {
    childLog("ERROR", "Failed to create heartbeat file");
  }

  server = http.createServer(
    (req: http.IncomingMessage, res: http.ServerResponse) => {
      // ── /health 端点 ──
      if (req.url === "/health" || req.url === "/health/") {
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ status: "ok", service: "cerebro", port }));
        return;
      }

      // Only handle GET / HEAD
      if (req.method !== "GET" && req.method !== "HEAD") {
        res.writeHead(405, { "Content-Type": "text/plain" });
        res.end("Method Not Allowed");
        return;
      }

      // Parse URL, strip query string
      const url = new URL(req.url || "/", `http://localhost:${port}`);
      // new URL() already decodes percent-encoding; no double-decode
      const pathname = url.pathname;

      // Resolve safe file path
      const safePath = resolveSafe(webDir, pathname);

      if (!safePath) {
        res.writeHead(403, { "Content-Type": "text/plain" });
        res.end("Forbidden");
        return;
      }

      // Try to serve the file directly
      fs.stat(safePath, (statErr, stats) => {
        if (!statErr && stats.isFile()) {
          serveFile(res, safePath, apiUrl);
          return;
        }

        // SPA fallback: serve index.html for non-file paths
        fs.stat(indexPath, (idxErr, idxStats) => {
          if (idxErr || !idxStats.isFile()) {
            res.writeHead(404, { "Content-Type": "text/plain" });
            res.end("Not Found");
            return;
          }
          serveFile(res, indexPath, apiUrl);
        });
      });
    },
  );

  server.on("error", (err: NodeJS.ErrnoException) => {
    childLog("ERROR", "Server error", { error: err.message });
    process.send?.({ type: "error", message: err.message });
    cleanup();
  });

  server.listen(port, "127.0.0.1", () => {
    childLog("INFO", "Server listening", { port });
    process.send?.({ type: "ready", port });
  });

  // ── 心跳检测：每 30 秒检查心跳文件 mtime ──
  heartbeatTimer = setInterval(() => {
    try {
      const stat = fs.statSync(heartbeatFilePath);
      if (Date.now() - stat.mtimeMs > 60_000) {
        childLog("INFO", "Heartbeat expired, shutting down");
        cleanup();
      }
    } catch {
      // 心跳文件不存在，可能被清理，继续等待不主动退出
    }
  }, 30_000);

  // 确保定时器不阻止进程退出
  if (heartbeatTimer) heartbeatTimer.unref();

  // ── 信号处理 ──
  process.on("SIGTERM", cleanup);
  process.on("SIGINT", cleanup);
}

// ── IPC 监听（只处理第一条消息） ─────────────────────────────────────────

let started = false;

process.on("message", (config: ChildConfig) => {
  if (started) return;
  started = true;
  startServer(config);
});
