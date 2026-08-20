#!/usr/bin/env node
// cerebro web-server — CC 插件独立进程（移植自 plugins/opencode/src/web-server.ts）
// 由 session-start.sh 经 setsid detach 拉起，多 CC session 共享端口 5212。
// 与 opencode 版的区别：去掉 takeover 定时器（SessionStart probe 已够），纯静态 serve。
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { writeFileSync, unlinkSync } from "node:fs";

const PID_FILE = path.join(process.env.HOME || process.env.USERPROFILE || "", ".config/cerebro/web-server.pid");

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PORT = parseInt(process.env.OMEM_LOCAL_PORT || "", 10) || 5212;
const API_URL = process.env.OMEM_API_URL || "https://www.mengxy.cc";

// web 目录查找：env > 本地 web/ > 兄弟 opencode/web/
function findWebDir() {
  const candidates = [
    process.env.CEREBRO_WEB_DIR,
    path.resolve(__dirname, "../web"),
    path.resolve(__dirname, "../../opencode/web"),
  ].filter(Boolean);
  for (const d of candidates) {
    if (fs.existsSync(path.join(d, "index.html"))) return d;
  }
  return null;
}

const WEB_DIR = findWebDir();
if (!WEB_DIR) {
  console.error("[cerebro web-server] no web directory found, exiting");
  process.exit(0); // exit 0 — not an error, just nothing to serve
}

const COMMON = { "X-Content-Type-Options": "nosniff" };
const MIME = {
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

function resolveSafe(baseDir, pathname) {
  const rel = pathname.startsWith("/") ? pathname.slice(1) : pathname;
  const resolved = path.resolve(baseDir, rel || ".");
  if (!resolved.startsWith(baseDir + path.sep) && resolved !== baseDir) return null;
  return resolved;
}

function serveFile(res, filePath) {
  const ext = path.extname(filePath).toLowerCase();
  fs.readFile(filePath, (err, data) => {
    if (err) {
      res.writeHead(500, { ...COMMON, "Content-Type": "text/plain" });
      res.end("Internal Server Error");
      return;
    }
    let body = data;
    if (ext === ".html" && data.includes("__OMEM_API_URL__")) {
      body = data.toString("utf-8").replace(
        /window\.__OMEM_API_URL__\s*=\s*["']__OMEM_API_URL__["']/,
        `window.__OMEM_API_URL__ = ${JSON.stringify(API_URL)}`,
      );
    }
    res.writeHead(200, {
      ...COMMON,
      "Content-Type": MIME[ext] || "application/octet-stream",
      "Cache-Control": ext === ".html" ? "no-cache, no-store, must-revalidate" : "public, max-age=86400",
    });
    res.end(body);
  });
}

const indexPath = path.join(WEB_DIR, "index.html");
const server = http.createServer((req, res) => {
  if (req.url === "/health" || req.url === "/health/") {
    res.writeHead(200, { ...COMMON, "Content-Type": "application/json" });
    res.end(JSON.stringify({ status: "ok", service: "cerebro", port: PORT }));
    return;
  }
  if (req.method !== "GET" && req.method !== "HEAD") {
    res.writeHead(405, { ...COMMON, "Content-Type": "text/plain" });
    res.end("Method Not Allowed");
    return;
  }
  const url = new URL(req.url || "/", `http://localhost:${PORT}`);
  const safePath = resolveSafe(WEB_DIR, url.pathname);
  if (!safePath) {
    res.writeHead(403, { ...COMMON, "Content-Type": "text/plain" });
    res.end("Forbidden");
    return;
  }
  fs.stat(safePath, (statErr, stats) => {
    if (!statErr && stats.isFile()) { serveFile(res, safePath); return; }
    fs.stat(indexPath, (idxErr, idxStats) => {
      if (idxErr || !idxStats.isFile()) {
        res.writeHead(404, { ...COMMON, "Content-Type": "text/plain" });
        res.end("Not Found");
        return;
      }
      serveFile(res, indexPath);
    });
  });
});

server.on("error", (err) => {
  if (err.code === "EADDRINUSE") {
    // 另一个 CC session / opencode 已占端口 — 正常，静默退出
    process.exit(0);
  }
  console.error(`[cerebro web-server] error: ${err.message}`);
  process.exit(1);
});

server.listen(PORT, "127.0.0.1", () => {
  try { writeFileSync(PID_FILE, String(process.pid)); } catch {}
  console.log(`[cerebro web-server] serving ${WEB_DIR} at http://localhost:${PORT} (pid=${process.pid})`);
});

// ── CC-liveness watchdog ─────────────────────────────────────────────────────
// Truth = OS process table: sessions/<sid>.live maps each CC session to its
// CC main pid. Sweep on an interval, kill(pid, 0) each, sweep stale ones.
// Zero CC sessions alive → count down the grace window → self-exit. Survives
// killed terminals (no SessionEnd needed) — refcount bookkeeping is gone.
const SESSIONS_DIR = path.join(process.env.HOME || process.env.USERPROFILE || "", ".config/cerebro/sessions");
const GRACE_MS = parseInt(process.env.OMEM_WEB_GRACE_MS || "", 10) || 60_000;
const WATCH_MS = parseInt(process.env.OMEM_WEB_WATCH_MS || "", 10) || 60_000;
let zeroSince = 0; // 0 = CC alive (or unknown)

setInterval(() => {
  let alive = 0;
  try {
    for (const f of fs.readdirSync(SESSIONS_DIR)) {
      if (!f.endsWith(".live")) continue;
      const fp = path.join(SESSIONS_DIR, f);
      try {
        process.kill(parseInt(fs.readFileSync(fp, "utf-8").trim(), 10), 0);
        alive++;
      } catch (e) {
        if (e && e.code === "EPERM") alive++; // exists, root-owned — alive
        else { try { fs.unlinkSync(fp); } catch {} }
      }
    }
  } catch {} // dir missing = no CC session ever
  if (alive > 0) { zeroSince = 0; return; }
  if (!zeroSince) zeroSince = Date.now();
  if (Date.now() - zeroSince >= GRACE_MS) {
    console.log(`[cerebro web-server] no CC sessions alive for ${GRACE_MS / 1000}s, self-exiting`);
    try { fs.unlinkSync(PID_FILE); } catch {}
    process.exit(0);
  }
}, WATCH_MS);

const shutdown = () => {
  try { server.closeAllConnections(); } catch {} // Node ≥18.2; old node degrades
  server.close(() => { try { unlinkSync(PID_FILE); } catch {} process.exit(0); });
  setTimeout(() => process.exit(0), 1000).unref();
};
process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);
