/**
 * web-server.ts — Cerebro Web Server Manager (spawn mode)
 *
 * 不再直接创建 HTTP server，而是 fork 独立子进程 (web-server-child.ts)。
 * 多窗口共享一个 server，任一窗口关闭不影响其他窗口。
 * 子进程通过心跳文件 mtime 探测 plugin 存活状态，全部退出后自动关闭。
 */
import { fork } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// ── Types ────────────────────────────────────────────────────────────────

export interface WebServerConfig {
  apiUrl: string;
  port?: number;
}

export interface WebServerHandle {
  address(): { port: number; family: string; address: string } | string | null;
}

// ── Helpers ──────────────────────────────────────────────────────────────

/** Touch a file — update mtime or create if missing */
function touchFile(filePath: string): void {
  try {
    fs.utimesSync(filePath, new Date(), new Date());
  } catch {
    try {
      fs.writeFileSync(filePath, "");
    } catch { /* ignore */ }
  }
}

/** Check if a process with the given PID is still running */
function isProcessAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

/** Probe an existing server's /health endpoint */
async function probeExistingServer(port: number): Promise<boolean> {
  try {
    const resp = await fetch(`http://127.0.0.1:${port}/health`);
    if (resp.ok) {
      const body = await resp.text();
      return body.includes("cerebro");
    }
  } catch { /* connection refused → port free */ }
  return false;
}

// ── Start / Stop ─────────────────────────────────────────────────────────

export async function startWebServer(
  config: WebServerConfig,
): Promise<WebServerHandle | null> {
  const port =
    config.port || parseInt(process.env.OMEM_LOCAL_PORT || "", 10) || 5212;
  const pidFilePath = path.join(os.tmpdir(), `cerebro-web-${port}.pid`);
  const heartbeatFilePath = path.join(
    os.tmpdir(),
    `cerebro-web-${port}.heartbeat`,
  );

  // ── Step 1: 检查端口是否已有 cerebro server ──
  if (await probeExistingServer(port)) {
    console.log(`[cerebro:web] Reusing existing server on port ${port}`);
    return createHandle(port, heartbeatFilePath);
  }

  // ── Step 2: 检查 PID 文件（可能有其他进程正在 fork） ──
  try {
    const pidStr = fs.readFileSync(pidFilePath, "utf-8").trim();
    const pid = parseInt(pidStr, 10);
    if (pid > 0 && isProcessAlive(pid)) {
      // 有其他进程正在 fork 或运行，等待 200ms 后重试
      await new Promise((r) => setTimeout(r, 200));
      if (await probeExistingServer(port)) {
        console.log(
          `[cerebro:web] Reusing server after PID check on port ${port}`,
        );
        return createHandle(port, heartbeatFilePath);
      }
    } else {
      // 进程已死亡，清理 PID 文件
      try { fs.unlinkSync(pidFilePath); } catch { /* ignore */ }
    }
  } catch { /* PID 文件不存在，继续 */ }

  // ── Step 3: 检查 web 目录 ──
  const webDir = path.resolve(__dirname, "..", "web");
  if (!fs.existsSync(webDir)) {
    console.warn(
      `[cerebro:web] Web directory not found: ${webDir}, skipping server start`,
    );
    return null;
  }
  if (!fs.existsSync(path.join(webDir, "index.html"))) {
    console.warn(
      `[cerebro:web] index.html not found in ${webDir}, skipping server start`,
    );
    return null;
  }

  // ── Step 4: 写 PID 文件（标记正在 fork） ──
  try {
    fs.writeFileSync(pidFilePath, String(process.pid));
  } catch { /* ignore */ }

  // ── Step 5: Fork 子进程 ──
  const childPath = path.resolve(__dirname, "web-server-child.js");

  const child = fork(childPath, [], {
    detached: true,
    stdio: ["pipe", "pipe", "pipe", "ipc"],
  });

  // Drain stdout/stderr to prevent pipe buffer from blocking the child
  child.stdout?.on("data", () => {});
  child.stderr?.on("data", () => {});

  child.unref();

  // 发送配置给子进程
  child.send({ port, webDir, apiUrl: config.apiUrl });

  // ── Step 6: 等待 ready 或超时 5s ──
  const ready = await new Promise<boolean>((resolve) => {
    const timeout = setTimeout(() => resolve(false), 5000);

    child.on("message", (msg: { type: string }) => {
      if (msg.type === "ready") {
        clearTimeout(timeout);
        resolve(true);
      } else if (msg.type === "error") {
        clearTimeout(timeout);
        resolve(false);
      }
    });

    child.on("error", () => {
      clearTimeout(timeout);
      resolve(false);
    });

    child.on("exit", () => {
      clearTimeout(timeout);
      resolve(false);
    });
  });

  if (!ready) {
    console.warn(
      `[cerebro:web] Failed to start web server child process on port ${port}`,
    );
    try { child.kill(); } catch { /* ignore */ }
    try { fs.unlinkSync(pidFilePath); } catch { /* ignore */ }
    return null;
  }

  console.log(
    `[cerebro:web] Web server child process started on port ${port}`,
  );
  return createHandle(port, heartbeatFilePath);
}

/** Create a handle with address() compat + heartbeat keep-alive */
function createHandle(
  port: number,
  heartbeatFilePath: string,
): WebServerHandle {
  // 初始 touch
  touchFile(heartbeatFilePath);

  // 每 30 秒 touch 心跳文件，保持子进程存活
  const timer = setInterval(() => {
    touchFile(heartbeatFilePath);
  }, 30_000);

  // 确保定时器不阻止进程退出
  timer.unref();

  return {
    address() {
      return { port, family: "IPv4", address: "127.0.0.1" };
    },
  };
}

export function stopWebServer(_handle: WebServerHandle): Promise<void> {
  // Intentionally do NOT touch heartbeat: parent exits → timer stops →
  // heartbeat ages out → child detects mtime > 60s → self-terminate.
  return Promise.resolve();
}
