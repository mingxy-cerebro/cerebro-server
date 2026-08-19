#!/usr/bin/env node
// apply-dream: merge a dream output archive into MEMORY_DIR under the merge contract:
//   join key = name (the only anchor; server prompt forces verbatim copy for kept)
//   kept   → old archive wins verbatim; every LLM-provided field except name is ignored
//   unknown kept name → surfaced as `unknown`, NEVER silently dropped (that is memory evaporation)
//   dropped → removal candidate, listed for review; applied only with --apply
//   merged/updated/added → LLM version wins (content is allowed to change there)
//   stats.total still counts kept entries — the ledger is the server's, not ours to re-derive
//
// Read-only review by default (prints the diff); `--apply` writes after user approval.
// Usage:
//   node apply-dream.mjs            # review diff for the unconsumed output
//   node apply-dream.mjs --apply    # write files + rebuild MEMORY.md index lines
import { existsSync, readFileSync, writeFileSync, readdirSync, rmSync, mkdirSync, renameSync } from "node:fs";
import { join } from "node:path";

const HOME = process.env.HOME || "/home/dongx";
const DREAM_DIR = process.env.OMEM_DREAM_DIR || join(HOME, ".cache", "cerebro", "dream");
const OUT_DIR = join(DREAM_DIR, "output");
const STATE = join(DREAM_DIR, "state.json");
const MEMORY_DIR = process.env.OMEM_DREAM_MEMORY_DIR || join(HOME, ".claude", "projects", "-home-dongx", "memory");
const INDEX = join(MEMORY_DIR, "MEMORY.md");
const APPLY = process.argv.includes("--apply");

// ─── old archive: parse every memory file into {name, description, type, body, raw} ──
function parseMemoryFile(path) {
  const raw = readFileSync(path, "utf8");
  const m = raw.match(/^---\n([\s\S]*?)\n---\n([\s\S]*)$/);
  if (!m) return { name: null, raw };
  const fm = {};
  for (const line of m[1].split("\n")) {
    const kv = line.match(/^(\w[\w-]*):\s*(.*)$/);
    if (kv) fm[kv[1]] = kv[2].replace(/^["']|["']$/g, "");
  }
  return { name: fm.name || null, description: fm.description || "", type: (fm.metadata || "").replace(/.*type:\s*/, "").trim() || "", body: m[2], fmText: m[1], raw };
}

const oldFiles = new Map(); // name → {path, parsed}
const byBase = new Map();   // basename → same objects, second-chance join by file
const alias = new Map();    // MEMORY.md link text (often Chinese) → name
const orphanFiles = [];     // unparseable / no name — never touched, surfaced
for (const f of readdirSync(MEMORY_DIR)) {
  if (!f.endsWith(".md") || f === "MEMORY.md") continue;
  const p = { path: join(MEMORY_DIR, f), parsed: parseMemoryFile(join(MEMORY_DIR, f)) };
  byBase.set(f, p);
  if (p.parsed.name) oldFiles.set(p.parsed.name, p); else orphanFiles.push(p.path);
}
// the dream LLM sees MEMORY.md too and sometimes echoes the Chinese link text
// instead of the frontmatter slug — join those via the index line
{
  const idxRaw = existsSync(INDEX) ? readFileSync(INDEX, "utf8") : "";
  for (const m of idxRaw.matchAll(/\[([^\]]+)\]\(([^)]+\.md)\)/g)) {
    const hit = byBase.get(m[2].replace(/.*\//, ""));
    if (hit?.parsed.name) alias.set(m[1], hit.parsed.name);
  }
}

// ─── pick the archive: unconsumed output from state, else newest output file ──
let archivePath = null;
try {
  const st = JSON.parse(readFileSync(STATE, "utf8"));
  if (st.output && existsSync(st.output)) archivePath = st.output;
} catch {}
if (!archivePath && existsSync(OUT_DIR)) {
  const files = readdirSync(OUT_DIR).filter((f) => f.endsWith(".json")).sort();
  if (files.length) archivePath = join(OUT_DIR, files[files.length - 1]);
}
if (!archivePath) { console.error("apply-dream: no dream output found"); process.exit(1); }
let entries = JSON.parse(readFileSync(archivePath, "utf8")).entries || [];

// ─── dedup: the LLM occasionally emits the same name twice — once with full
// content, once as a bare stub (source=kept, empty body/description; the
// deepseek rewrite tic, cf. cc-dream-truncation-fix). The write phase is
// last-write-wins, so a trailing stub would clobber the real entry into a
// 60-byte frontmatter shell. Collapse by name BEFORE merging: longest
// non-empty body wins; on a tie kept wins (conservative — kept writes nothing). ──
{
  const seen = new Map();
  for (const e of entries) {
    const cur = seen.get(e.name);
    if (!cur || (e.body || "").length > (cur.body || "").length
      || ((e.body || "").length === (cur.body || "").length && e.source === "kept" && cur.source !== "kept"))
      seen.set(e.name, e);
  }
  entries = [...seen.values()];
}

// ─── merge ────────────────────────────────────────────────────────────────────
const unknown = [];   // kept names missing from the old archive — LLM renamed, memory at risk
const empty = [];     // content actions with empty body+description — never written, surfaced
const report = { keep: 0, write: [], drop: [], dropCount: 0 };
for (let e of entries) {
  // normalize BEFORE any branch: an LLM-emitted Chinese name that the MEMORY.md
  // index can resolve must land on the old file, or updated entries fork duplicates
  if (!oldFiles.has(e.name) && alias.has(e.name)) e = { ...e, name: alias.get(e.name) };
  const act = e.source || e.action || "";
  if (e.source === "kept" || act === "kept") {
    if (!oldFiles.has(e.name)) { unknown.push(e.name); continue; }
    report.keep++; // old archive wins; nothing to do, not even a rewrite
  } else if (act === "dropped") {
    report.drop.push(e.name);
    report.dropCount++;
  } else if (act === "added" || act === "merged" || act === "updated") {
    // an empty stub must never clobber a real file: writing it would leave a
    // 60-byte frontmatter shell. merged/updated skip = old content survives;
    // added skip = surfaced below, not silently dropped.
    if (!(e.body || "").trim() && !(e.description || "").trim()) { empty.push(`${e.name} (${act})`); continue; }
    report.write.push(e); // LLM content is authoritative for these actions
  } else {
    unknown.push(`${e.name} (action=${act || "?"})`);
  }
}

// ─── review output ────────────────────────────────────────────────────────────
const lines = [
  `apply-dream review · archive ${archivePath}`,
  `kept ${report.keep} / write ${report.write.length} / drop ${report.dropCount} / unknown ${unknown.length}`,
];
for (const e of report.write) lines.push(`  ${e.source || e.action}  ${e.name} — ${(e.description || "").slice(0, 60)}`);
for (const n of report.drop) lines.push(`  drop  ${n}`);
for (const n of unknown) lines.push(`  ?     ${n}  ← surfaced, not dropped`);
for (const n of empty) lines.push(`  ~     ${n}  ← empty stub skipped, not written`);
if (orphanFiles.length) lines.push(`  (unparsed old files left untouched: ${orphanFiles.length})`);
console.log(lines.join("\n"));
if (unknown.length) console.log("\n⚠ URGENT: unknown kept names above — surface to the user BEFORE applying; they may be renames the LLM invented.");
if (!APPLY) { console.log("\ndry run — pass --apply to write"); process.exit(0); }

// ─── write phase (--apply) ─────────────────────────────────────────────────────
mkdirSync(MEMORY_DIR, { recursive: true });
const slug = (s) => s.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "memory";
for (const e of report.write) {
  const old = oldFiles.get(e.name);
  const path = old?.path || join(MEMORY_DIR, `${slug(e.name)}.md`);
  const type = e.type || old?.parsed.type || "project";
  const out = [
    "---",
    `name: ${e.name}`,
    ...(e.description ? [`description: ${e.description.replace(/\n/g, " ")}`] : []),
    "metadata:",
    `  type: ${type}`,
    "---",
    "",
    (e.body || "").trim(),
    "",
  ].join("\n");
  const tmp = path + ".tmp";
  writeFileSync(tmp, out); renameSync(tmp, path); // atomic, same convention as state.json
}
for (const n of report.drop) {
  const old = oldFiles.get(n);
  if (old) rmSync(old.path);
}

// MEMORY.md index: keep human-written lines by name, append new, drop removed
const idx = existsSync(INDEX) ? readFileSync(INDEX, "utf8").split("\n") : ["# MEMORY.md — 本地记忆索引", ""];
const keptNames = new Set([...entries.filter((e) => e.source !== "kept" || e.action !== "dropped" && oldFiles.has(e.name) || true).map((e) => e.name)]);
const outLines = idx.filter((l) => {
  const m = l.match(/\]\(([^)]+\.md)\)/);
  if (!m) return true; // headers, blanks, non-link lines stay
  const base = m[1].replace(/.*\//, "");
  const hit = report.write.find((e) => (oldFiles.get(e.name)?.path || "").endsWith(base));
  if (hit) return keptNames.has(hit.name);
  const drop = report.drop.find((n) => (oldFiles.get(n)?.path || "").endsWith(base));
  return !drop;
});
for (const e of report.write) {
  if (outLines.some((l) => l.includes(`(${slug(e.name)}.md)`))) continue;
  outLines.push(`- [${e.name}](${slug(e.name)}.md) — ${(e.description || "").slice(0, 40)}`);
}
writeFileSync(INDEX, outLines.join("\n").replace(/\n{3,}/g, "\n\n") + "\n");
console.log(`\napplied: ${report.write.length} written, ${report.dropCount} dropped, index rebuilt`);
