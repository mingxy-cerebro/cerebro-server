// Shared utilities — ported from plugins/opencode/src/hooks.ts + client.ts
// + ingest cleaning ported from plugins/claude-code/hooks/common.mjs
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { homedir } from "node:os";

const BOUNDARY_SEARCH_RATIO = 0.6;

// ── Git root detection ────────────────────────────────────────────────
// Normalize cwd to git root so project_path stays stable regardless of
// where inside the repo the agent was launched. Falls back to cwd when
// not in a git repo or git is unavailable.
const gitRootCache = new Map();
export function detectProjectRoot(cwd) {
  const dir = cwd || process.cwd();
  if (!dir) return "";
  const cached = gitRootCache.get(dir);
  if (cached !== undefined) return cached;
  let root = dir;
  try {
    const result = spawnSync("git", ["rev-parse", "--show-toplevel"], {
      cwd: dir,
      encoding: "utf-8",
      timeout: 3000,
      windowsHide: true,
    });
    if (result.status === 0 && result.stdout) {
      root = result.stdout.trim();
    }
  } catch {}
  gitRootCache.set(dir, root);
  return root;
}

// ── Cross-platform project path canonicalization ──────────────────────
// Memory producers run in different worlds over the SAME repo:
//   WSL (opencode / claude-code) sees /mnt/c/dev/foo
//   Windows (zcode)                sees C:\dev\foo, C:/dev/foo, or /c/dev/foo
// The server filters memories by exact project_path string, so without
// normalization the two worlds never see each other's memories.
// toRemoteProjectPath() converts any Windows-flavored path to the WSL form
// (/mnt/<drive>/...) for everything we SEND to the server. Local file IO
// must keep using the native path — convert only at the API boundary.
//
// style: "auto" (default; Windows drive paths → /mnt form, POSIX stays as-is
//        so the same code is safe when running inside WSL/Linux),
//        "wsl" (force conversion), "native" (never convert).
const PATH_STYLE = (process.env.OMEM_PROJECT_PATH_STYLE || "auto").toLowerCase();

function windowsToMnt(p) {
  // Windows drive form: D:\foo or D:/foo (unambiguous on any platform)
  let m = p.match(/^([a-zA-Z]):[\\/](.*)$/);
  if (m) return `/mnt/${m[1].toLowerCase()}/${m[2].replace(/\\/g, "/")}`;
  // Git-bash / MSYS form: /d/foo — only meaningful on Windows; on real Linux
  // a single-letter root dir is a genuine path, so don't touch it there.
  if (process.platform === "win32") {
    m = p.match(/^\/([a-zA-Z])\/(.*)$/);
    if (m) return `/mnt/${m[1].toLowerCase()}/${m[2]}`;
  }
  return null;
}

export function toRemoteProjectPath(p) {
  if (!p) return p;
  if (PATH_STYLE === "native") return p;
  const converted = windowsToMnt(p);
  if (converted) return converted;
  if (PATH_STYLE === "wsl") {
    // POSIX path in force-wsl mode: /mnt/... is already canonical; anything
    // else (e.g. native Linux path) stays as-is — we never guess.
    return p;
  }
  return p; // auto + non-Windows path (WSL/Linux) — canonical already
}

// ── Content sanitization ──────────────────────────────────────────────
// Strip XML/HTML tags, compress whitespace, truncate (port of client.ts:4-10)
export function sanitizeContent(text, maxLen = 3000) {
  if (!text) return "";
  let clean = text.replace(/<[\w-]+[^>]*>[\s\S]*?<\/[\w-]+>/g, "");
  clean = clean.replace(/<[\w-]+[^>]*\/>/g, "");
  clean = clean.replace(/\s+/g, " ").trim();
  if (clean.length <= maxLen) return clean;
  return clean.slice(0, maxLen) + "…[truncated]";
}

// Truncate query to avoid HTTP 414 (port of client.ts:12-15)
export function truncateQuery(query, maxLen = 200) {
  if (!query) return "";
  if (query.length <= maxLen) return query;
  return query.slice(0, maxLen);
}

// Truncate at sentence boundary if possible (port of hooks.ts:200-219)
export function truncateAtBoundary(text, maxLength) {
  if (!text || text.length <= maxLength) return text || "";
  const boundaries = /[.!?。！？\n]/;
  const searchEnd = Math.min(maxLength, text.length);
  for (let i = searchEnd - 1; i >= Math.floor(searchEnd * BOUNDARY_SEARCH_RATIO); i--) {
    if (boundaries.test(text[i])) {
      return text.slice(0, i + 1).trimEnd() + "…";
    }
  }
  let truncated = text.slice(0, maxLength);
  const lastCode = truncated.charCodeAt(truncated.length - 1);
  if (lastCode >= 0xd800 && lastCode <= 0xdbff) truncated = truncated.slice(0, -1);
  return truncated + "…";
}

// ── Relative age formatter (port of hooks.ts:188-198) ─────────────────
export function formatRelativeAge(isoDate) {
  if (!isoDate) return "unknown";
  const diffMs = Date.now() - new Date(isoDate).getTime();
  if (isNaN(diffMs)) return "unknown";
  const minutes = Math.floor(diffMs / 60000);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;
  const months = Math.floor(days / 30);
  return `${months}mo ago`;
}

// ── Container tags — user + project isolation (port of tags.ts) ───────
import { createHash } from "node:crypto";
import { execSync } from "node:child_process";

// Canonical user identity, aligned with claude-code containerTags():
// env override > git config user.email > legacy env fallbacks. Keeping this
// chain identical across plugins is what makes the omem_user_<hash> tag —
// and therefore user-scoped memory isolation — match across WSL/Windows.
export function resolveUserEmail() {
  if (process.env.OMEM_USER_EMAIL) return process.env.OMEM_USER_EMAIL;
  try {
    const email = execSync("git config user.email", {
      encoding: "utf-8",
      timeout: 2000,
      stdio: ["pipe", "pipe", "pipe"],
    }).trim();
    if (email) return email;
  } catch {}
  return process.env.GIT_AUTHOR_EMAIL || process.env.USER || process.env.USERNAME || "";
}

export function getUserTag(emailOverride) {
  const id = emailOverride || resolveUserEmail() || "unknown";
  const hash = createHash("sha256").update(id).digest("hex").slice(0, 16);
  return `omem_user_${hash}`;
}

export function getProjectTag(directory) {
  // Hash the REMOTE (canonical) form so the tag matches what WSL-side
  // plugins compute for the same repo.
  const dir = toRemoteProjectPath(directory || process.cwd());
  const hash = createHash("sha256").update(dir).digest("hex").slice(0, 16);
  return `omem_project_${hash}`;
}

// ── Project name detection (port of hooks.ts:18-119) ──────────────────
const projectNameCache = new Map();
export async function detectProjectName(rootPath) {
  if (!rootPath) return undefined;
  const cached = projectNameCache.get(rootPath);
  if (cached !== undefined) return cached;

  let result;
  const tryRead = async (file) => {
    try {
      return await readFileSync(join(rootPath, file), "utf-8");
    } catch {
      return null;
    }
  };

  // 1. AGENTS.md heading
  let agents = await tryRead("AGENTS.md");
  if (agents) {
    const m = agents.match(/^#\s+(.+)/m);
    if (m) result = m[1].replace(/\s*\(.*?\)/g, "").trim() || undefined;
  }

  // 2. package.json
  if (!result) {
    const pkg = await tryRead("package.json");
    if (pkg) {
      const m = pkg.match(/"name"\s*:\s*"([^"]+)"/);
      if (m) result = m[1].trim();
    }
  }

  // 3. Cargo.toml
  if (!result) {
    const cargo = await tryRead("Cargo.toml");
    if (cargo) {
      const inPkg = cargo.replace(/\r\n/g, "\n").split("\n").reduce(
        (acc, line) => {
          if (/^\[package\]/.test(line.trim())) return { ...acc, in: true };
          if (/^\[/.test(line.trim())) return { ...acc, in: false };
          if (acc.in) {
            const m = line.match(/name\s*=\s*"([^"]+)"/);
            if (m) return { ...acc, name: m[1] };
          }
          return acc;
        },
        { in: false, name: undefined },
      );
      result = inPkg.name?.trim();
    }
  }

  // 4. go.mod
  if (!result) {
    const gomod = await tryRead("go.mod");
    if (gomod) {
      const m = gomod.match(/^module\s+(\S+)/m);
      if (m) result = m[1].split("/").pop()?.trim();
    }
  }

  // 5. pyproject.toml
  if (!result) {
    const pyproj = await tryRead("pyproject.toml");
    if (pyproj) {
      const inPrj = pyproj.replace(/\r\n/g, "\n").split("\n").reduce(
        (acc, line) => {
          if (/^\[project\]/.test(line.trim())) return { ...acc, in: true };
          if (/^\[/.test(line.trim())) return { ...acc, in: false };
          if (acc.in) {
            const m = line.match(/name\s*=\s*"([^"]+)"/);
            if (m) return { ...acc, name: m[1] };
          }
          return acc;
        },
        { in: false, name: undefined },
      );
      result = inPrj.name?.trim();
    }
  }

  // 6. composer.json
  if (!result) {
    const composer = await tryRead("composer.json");
    if (composer) {
      const m = composer.match(/"name"\s*:\s*"([^"]+)"/);
      if (m) result = m[1].trim();
    }
  }

  // 7. fallback: dirname
  if (!result) {
    result = rootPath.split("/").pop() || rootPath.split("\\").pop() || undefined;
  }

  if (result) result = result.trim() || undefined;
  projectNameCache.set(rootPath, result);
  return result;
}

// ── User request extraction (port of hooks.ts:140-178) ────────────────
const SYSTEM_INJECTION_PATTERNS = [
  /<!--\s*OMO_INTERNAL_INITIATOR\s*-->/,
  /^\[SYSTEM DIRECTIVE:/,
  /^\[restore checkpointed/,
  /^\[session recovered/,
  /^<system-reminder>/,
  /^<EXTREMELY_IMPORTANT>/,
  /^\[CONTEXT\]/,
  /^\[GOAL\]/,
  /^## 任务[：:]/,
  /^## 改动/,
  /^Analyze the attached file/,
  /^Provide ONLY the extracted/,
  /^Called the Read tool/,
  /^MANDATORY delegate_task/,
  /^[▣▪]\s*DCP/,
];

const MODE_TAG_PATTERN = /^\[(?:search-mode|analyze-mode)\][\s\S]*?\n---\n?/;
const MODE_TAG_LINE = /^\[(?:search-mode|analyze-mode)\]\s*\n/;

export function extractUserRequest(content) {
  if (!content) return "";
  const match = content.match(/<user-request>([\s\S]*?)<\/user-request>/);
  let text = match ? match[1].trim() : content;

  const stripped = text.replace(MODE_TAG_PATTERN, "");
  if (stripped !== text && stripped.trim()) {
    text = stripped.trim();
  } else {
    text = text.replace(MODE_TAG_LINE, "").trim();
  }

  for (const pattern of SYSTEM_INJECTION_PATTERNS) {
    if (pattern.test(text)) return "";
  }
  return text;
}

// ── Ingest cleaning (port of claude-code common.mjs cleanText/blockText) ──
// Strips inject-echo blocks (<system-reminder>, <cerebro-*>, [CEREBRO-MEMORY])
// so hooks never re-ingest their own injections.
const INJECT_TAG_RE = /<(system-reminder|cerebro-[a-z0-9_-]+|supermemory-[a-z0-9_-]+)\b[^>]*>[\s\S]*?<\/\1>/gi;
const INJECT_SELF_RE = /<(system-reminder|cerebro-[a-z0-9_-]+|supermemory-[a-z0-9_-]+)\b[^>]*\/>/gi;
const MEMORY_BLOCK_RE = /\[CEREBRO-MEMORY\][\s\S]*?\[\/CEREBRO-MEMORY\]/g;
const WS_RE = /\s+/g;

export function cleanText(s) {
  if (typeof s !== "string") s = String(s);
  return s
    .replace(INJECT_TAG_RE, "")
    .replace(INJECT_SELF_RE, "")
    .replace(MEMORY_BLOCK_RE, "")
    .replace(WS_RE, " ")
    .trim();
}

// Content-block flattening: drop thinking, truncate tool_result (500) and
// serialized tool_use input (100) — matches claude-code flushSessionIngest.
export function blockText(b) {
  const t = b.type;
  if (t === "text") return b.text || "";
  if (t === "thinking") return null;
  if (t === "tool_result") {
    let c = b.content || "";
    if (Array.isArray(c)) {
      c = c
        .map((x) => (typeof x === "object" && x?.type === "text" ? x.text || "" : typeof x === "string" ? x : ""))
        .join("\n");
    } else if (typeof c !== "string") {
      try { c = JSON.stringify(c); } catch { c = String(c); }
    }
    return "tool_result: " + (c || "").slice(0, 500);
  }
  if (t === "tool_use") {
    let inp;
    try { inp = JSON.stringify(b.input); } catch { inp = String(b.input); }
    return `tool_use(${b.name || "?"}): ${inp.slice(0, 100)}`;
  }
  return null;
}

export function contentText(content) {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((b) => {
        if (typeof b === "string") return b;
        if (typeof b === "object" && b !== null) return blockText(b);
        return null;
      })
      .filter(Boolean)
      .join("\n");
  }
  return "";
}

// ── Auto-store toggle (shared by Stop hook + MCP memory_toggle tool) ─────
function stateDir() {
  if (process.env.ZCODE_PLUGIN_DATA) return process.env.ZCODE_PLUGIN_DATA;
  return join(homedir(), ".config", "cerebro", "zcode-state");
}

function autoStorePath(sessionId) {
  return join(stateDir(), `autostore-${sessionId || "default"}.json`);
}

export function getAutoStore(sessionId) {
  try {
    const data = JSON.parse(readFileSync(autoStorePath(sessionId), "utf-8"));
    return data.enabled ?? true;
  } catch {
    return true;
  }
}

export function setAutoStore(sessionId, enabled) {
  try {
    mkdirSync(stateDir(), { recursive: true });
    writeFileSync(autoStorePath(sessionId), JSON.stringify({ enabled, ts: Date.now() }));
  } catch {}
}
