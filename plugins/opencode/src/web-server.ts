import * as http from "node:http";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// ── Types ────────────────────────────────────────────────────────────────

export interface WebServerConfig {
  apiUrl: string;
  port?: number;
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
  // Strip leading slash so path.resolve treats it as relative
  const relative = pathname.startsWith("/") ? pathname.slice(1) : pathname;
  const resolved = path.resolve(baseDir, relative || ".");
  if (!resolved.startsWith(baseDir + path.sep) && resolved !== baseDir) {
    return null;
  }
  return resolved;
}

// ── Start / Stop ─────────────────────────────────────────────────────────

export function startWebServer(config: WebServerConfig): Promise<http.Server | null> {
  return new Promise((resolve) => {
    const webDir = path.resolve(__dirname, "..", "web");

    // Check web directory exists
    if (!fs.existsSync(webDir)) {
      console.warn(`[cerebro:web] Web directory not found: ${webDir}, skipping server start`);
      resolve(null);
      return;
    }

    const indexPath = path.join(webDir, "index.html");
    if (!fs.existsSync(indexPath)) {
      console.warn(`[cerebro:web] index.html not found in ${webDir}, skipping server start`);
      resolve(null);
      return;
    }

    const port = config.port || parseInt(process.env.OMEM_LOCAL_PORT || "", 10) || 5212;

    const server = http.createServer(
      (req: http.IncomingMessage, res: http.ServerResponse) => {
        // Only handle GET / HEAD
        if (req.method !== "GET" && req.method !== "HEAD") {
          res.writeHead(405, { "Content-Type": "text/plain" });
          res.end("Method Not Allowed");
          return;
        }

        // Parse URL, strip query string
        const url = new URL(req.url || "/", `http://localhost:${port}`);
        const pathname = decodeURIComponent(url.pathname);

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
            serveFile(res, safePath, config.apiUrl);
            return;
          }

          // SPA fallback: serve index.html for non-file paths
          fs.stat(indexPath, (idxErr, idxStats) => {
            if (idxErr || !idxStats.isFile()) {
              res.writeHead(404, { "Content-Type": "text/plain" });
              res.end("Not Found");
              return;
            }
            serveFile(res, indexPath, config.apiUrl);
          });
        });
      },
    );

    server.on("error", (err: NodeJS.ErrnoException) => {
      if (err.code === "EADDRINUSE") {
        console.warn(`[cerebro:web] Port ${port} already in use, web server not started`);
      } else {
        console.warn(`[cerebro:web] Server error: ${err.message}`);
      }
      resolve(null);
    });

    server.listen(port, "127.0.0.1", () => {
      const addr = server.address();
      const actualPort = typeof addr === "object" && addr ? addr.port : port;
      console.log(`[cerebro:web] Static server listening on http://localhost:${actualPort}`);
      resolve(server);
    });
  });
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
      body = data.toString("utf-8").replace(/window\.__OMEM_API_URL__\s*=\s*["']__OMEM_API_URL__["']/, `window.__OMEM_API_URL__ = "${apiUrl}"`);
    }

    res.writeHead(200, {
      "Content-Type": contentType,
      "Cache-Control": ext === ".html" ? "no-cache, no-store, must-revalidate" : "public, max-age=86400",
    });
    res.end(body);
  });
}

// ── Graceful shutdown ────────────────────────────────────────────────────

export function stopWebServer(server: http.Server): Promise<void> {
  return new Promise((resolve) => {
    server.closeAllConnections?.();
    const timer = setTimeout(resolve, 3000);
    server.close(() => {
      clearTimeout(timer);
      console.log("[cerebro:web] Server stopped");
      resolve();
    });
  });
}
