// Shared utilities — ported from plugins/opencode/src/hooks.ts + client.ts
import { readFileSync } from "node:fs";
import { join } from "node:path";

const BOUNDARY_SEARCH_RATIO = 0.6;

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

export function getUserTag(email) {
  const id = email || process.env.USER || process.env.USERNAME || "unknown";
  const hash = createHash("sha256").update(id).digest("hex").slice(0, 16);
  return `omem_user_${hash}`;
}

export function getProjectTag(directory) {
  const dir = directory || process.cwd();
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
