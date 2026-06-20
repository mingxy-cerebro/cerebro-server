#!/usr/bin/env node
// Cerebro Web UI server — standalone process for ZCode.
// Serves the omem-web frontend on localhost:5212 (port = 我爱月儿).
// Reuses opencode plugin's pre-built web assets.
//
// Usage:
//   node web-server.js                    # default port 5212
//   OMEM_LOCAL_PORT=5314 node web-server.js
//   PORT=5314 node web-server.js
//
// The web assets are expected at <plugin-root>/web/. If missing, run:
//   bash ../../scripts/build-plugin-web.sh   (from opencode plugin)
//   cp -r ../../plugins/opencode/web/ ./web/

import { createServer } from "node:http";
import { existsSync, readFileSync, statSync } from "node:fs";
import { join, dirname, extname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const __dirname = dirname(fileURLToPath(import.meta.url));
const DEFAULT_PORT = 5212;

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
  ".webp": "image/webp",
};

// Resolve web dir: plugin's ./web/ → fallback to opencode plugin's ./web/
function findWebDir() {
  const candidates = [
    join(__dirname, "web"),
    join(__dirname, "..", "opencode", "web"),
    join(homedir(), ".zcode", "plugins", "cerebro", "web"),
  ];
  for (const d of candidates) {
    if (existsSync(join(d, "index.html"))) return resolve(d);
  }
  return null;
}

// Read API URL from cerebro config (shared with hooks)
function getApiUrl() {
  try {
    const cfg = JSON.parse(readFileSync(join(homedir(), ".config", "cerebro", "config.json"), "utf-8"));
    return cfg?.connection?.apiUrl || "https://www.mengxy.cc";
  } catch {
    return "https://www.mengxy.cc";
  }
}

function resolveSafe(baseDir, pathname) {
  const rel = pathname.startsWith("/") ? pathname.slice(1) : pathname;
  const resolved = resolve(baseDir, rel || ".");
  if (!resolved.startsWith(baseDir) && resolved !== baseDir) return null;
  return resolved;
}

function serveFile(res, filePath, apiUrl) {
  const ext = extname(filePath).toLowerCase();
  const contentType = MIME[ext] || "application/octet-stream";
  try {
    let data = readFileSync(filePath);
    // Inject API URL into index.html
    if (ext === ".html" && data.includes("__OMEM_API_URL__")) {
      data = Buffer.from(
        data.toString("utf-8").replace(
          /window\.__OMEM_API_URL__\s*=\s*["']__OMEM_API_URL__["']/,
          `window.__OMEM_API_URL__ = ${JSON.stringify(apiUrl)}`,
        ),
      );
    }
    res.writeHead(200, {
      "Content-Type": contentType,
      "Cache-Control": ext === ".html" ? "no-cache, no-store, must-revalidate" : "public, max-age=86400",
    });
    res.end(data);
  } catch {
    res.writeHead(500, { "Content-Type": "text/plain" });
    res.end("Internal Server Error");
  }
}

function main() {
  const port = parseInt(process.env.OMEM_LOCAL_PORT || process.env.PORT || "", 10) || DEFAULT_PORT;
  const webDir = findWebDir();

  if (!webDir) {
    console.error("[cerebro-web] Web assets not found.");
    console.error("  Expected: web/index.html in plugin dir");
    console.error("  Run build-plugin-web.sh or copy from plugins/opencode/web/");
    process.exit(1);
  }

  const apiUrl = getApiUrl();
  const indexPath = join(webDir, "index.html");

  // ── Idle self-shutdown ─────────────────────────────────────────────────
  // zcode spawns this as a detached daemon at SessionStart; zcode itself has
  // no Exit/SessionEnd hook, so nothing tears the daemon down on quit.
  // Instead we self-terminate after OMEM_WEB_IDLE_TIMEOUT_MS (default 30min)
  // with no incoming request. While zcode runs, stop.js pings /health at the
  // end of every turn → keeps the daemon alive. Once zcode closes, no pings
  // arrive → daemon exits on its own.
  const IDLE_TIMEOUT_MS = parseInt(process.env.OMEM_WEB_IDLE_TIMEOUT_MS || "", 10) || 20 * 1000;
  // Poll every timeout/4 (min 2s) so the watchdog fires reliably within the idle window.
  const IDLE_CHECK_MS = Math.max(2000, Math.floor(IDLE_TIMEOUT_MS / 4));
  let lastActivity = Date.now();

  const server = createServer((req, res) => {
    lastActivity = Date.now();

    // Health check — also serves as the keepalive ping target
    if (req.url === "/health" || req.url === "/health/") {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ status: "ok", service: "cerebro", port }));
      return;
    }

    if (req.method !== "GET" && req.method !== "HEAD") {
      res.writeHead(405, { "Content-Type": "text/plain" });
      res.end("Method Not Allowed");
      return;
    }

    const url = new URL(req.url || "/", `http://localhost:${port}`);
    const safePath = resolveSafe(webDir, url.pathname);

    if (!safePath) {
      res.writeHead(403, { "Content-Type": "text/plain" });
      res.end("Forbidden");
      return;
    }

    // Serve file if it exists, else SPA fallback to index.html
    try {
      if (existsSync(safePath) && statSync(safePath).isFile()) {
        serveFile(res, safePath, apiUrl);
        return;
      }
    } catch {}
    serveFile(res, indexPath, apiUrl);
  });

  server.listen(port, "127.0.0.1", () => {
    console.log(`[cerebro-web] ✓ Running at http://localhost:${port}`);
    console.log(`[cerebro-web]   API: ${apiUrl}`);
    console.log(`[cerebro-web]   Web dir: ${webDir}`);
    console.log(`[cerebro-web]   Idle auto-shutdown: ${IDLE_TIMEOUT_MS / 60000}min (stop hook pings /health to keep alive)`);
    console.log(`[cerebro-web]   Press Ctrl+C to stop.`);
  });

  // Idle watchdog — exit if no traffic for IDLE_TIMEOUT_MS.
  // NOTE: do NOT unref this timer — unref'd timers can be skipped by the event
  // loop on Windows, causing missed shutdowns. The server handle keeps the
  // loop alive anyway; this watchdog must fire reliably.
  const watchdog = setInterval(() => {
    const idle = Date.now() - lastActivity;
    if (idle >= IDLE_TIMEOUT_MS) {
      console.log(`[cerebro-web] Idle for ${Math.round(idle / 1000)}s, self-shutting down.`);
      clearInterval(watchdog);
      server.close(() => process.exit(0));
      setTimeout(() => process.exit(0), 1000);
    }
  }, IDLE_CHECK_MS);

  const shutdown = () => {
    clearInterval(watchdog);
    server.close(() => process.exit(0));
    setTimeout(() => process.exit(0), 1000);
  };
  process.on("SIGTERM", shutdown);
  process.on("SIGINT", shutdown);
}

main();
