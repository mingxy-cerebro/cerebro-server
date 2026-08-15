// Unit tests for hooks/lib — ingest cleaning + config fallbacks.
import { test, describe, after } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { createHash } from "node:crypto";

// Redirect homedir BEFORE importing lib modules (config.js resolves paths at
// module load for injection config, and util.js autostore functions at call).
const tempHome = mkdtempSync(join(tmpdir(), "cerebro-zcode-lib-"));
process.env.HOME = tempHome;
process.env.USERPROFILE = tempHome;
process.env.OMEM_LOG_ENABLED = "0";

const libUrl = (p) => pathToFileURL(join(process.cwd(), "hooks", "lib", p)).href;

const { cleanText, contentText, blockText, getAutoStore, setAutoStore, toRemoteProjectPath } =
  await import(libUrl("util.js"));
const { loadInjectionConfig } = await import(libUrl("config.js"));

after(() => {
  try { rmSync(tempHome, { recursive: true, force: true }); } catch {}
});

describe("cleanText", () => {
  test("strips system-reminder blocks", () => {
    const out = cleanText("before <system-reminder>noise inside</system-reminder> after");
    assert.equal(out, "before after");
  });

  test("strips cerebro inject-echo tags", () => {
    const out = cleanText('text <cerebro-recall>recall instruction</cerebro-recall> tail');
    assert.equal(out, "text tail");
  });

  test("strips self-closing inject tags", () => {
    const out = cleanText("a <cerebro-nudge/> b");
    assert.equal(out, "a b");
  });

  test("strips [CEREBRO-MEMORY] injection blocks", () => {
    const out = cleanText("keep this [CEREBRO-MEMORY] profile stuff [/CEREBRO-MEMORY] and this");
    assert.equal(out, "keep this and this");
  });

  test("compresses whitespace", () => {
    assert.equal(cleanText("a\n\n   b\t\tc"), "a b c");
  });
});

describe("blockText / contentText", () => {
  test("text blocks pass through", () => {
    assert.equal(contentText([{ type: "text", text: "hello" }]), "hello");
  });

  test("thinking blocks are dropped", () => {
    assert.equal(contentText([{ type: "thinking", thinking: "secret" }, { type: "text", text: "kept" }]), "kept");
  });

  test("tool_result truncated to 500 chars", () => {
    const out = contentText([{ type: "tool_result", content: "x".repeat(1000) }]);
    assert.ok(out.startsWith("tool_result: "));
    assert.equal(out.length, "tool_result: ".length + 500);
  });

  test("tool_use input serialized and capped at 100 chars", () => {
    const out = contentText([{ type: "tool_use", name: "Bash", input: { cmd: "y".repeat(500) } }]);
    assert.ok(out.startsWith("tool_use(Bash): "));
    assert.ok(out.length <= "tool_use(Bash): ".length + 100);
  });

  test("string array elements kept, mixed with blocks", () => {
    assert.equal(contentText(["plain", { type: "text", text: "block" }]), "plain\nblock");
  });

  test("plain string content passes through", () => {
    assert.equal(contentText("direct"), "direct");
  });
});

describe("auto-store toggle", () => {
  test("defaults to ON", () => {
    assert.equal(getAutoStore("lib-test-session"), true);
  });

  test("set + read roundtrip", () => {
    setAutoStore("lib-test-session", false);
    assert.equal(getAutoStore("lib-test-session"), false);
    setAutoStore("lib-test-session", true);
    assert.equal(getAutoStore("lib-test-session"), true);
  });
});

describe("toRemoteProjectPath (WSL/Windows cross-world canonicalization)", () => {
  test("Windows backslash drive path → /mnt form", () => {
    assert.equal(toRemoteProjectPath("D:\\dev\\github\\foo"), "/mnt/d/dev/github/foo");
  });

  test("Windows forward-slash drive path (git for windows output) → /mnt form", () => {
    assert.equal(toRemoteProjectPath("C:/dev/foo"), "/mnt/c/dev/foo");
  });

  test("drive letter lowercased for tag stability", () => {
    assert.equal(toRemoteProjectPath("D:\\X\\Y"), "/mnt/d/X/Y");
    assert.equal(toRemoteProjectPath("d:/x/y"), "/mnt/d/x/y");
  });

  test("already-canonical /mnt path passes through", () => {
    assert.equal(toRemoteProjectPath("/mnt/c/dev/foo"), "/mnt/c/dev/foo");
  });

  test("native POSIX path untouched (WSL-side safety)", () => {
    assert.equal(toRemoteProjectPath("/home/user/project"), "/home/user/project");
  });

  test("empty/invalid input passes through", () => {
    assert.equal(toRemoteProjectPath(""), "");
    assert.equal(toRemoteProjectPath(undefined), undefined);
  });

  test("same repo maps to same hash across path styles", () => {
    const hash = (s) => createHash("sha256").update(s).digest("hex").slice(0, 16);
    assert.equal(hash(toRemoteProjectPath("D:\\dev\\p")), hash("/mnt/d/dev/p"));
    assert.equal(hash(toRemoteProjectPath("D:/dev/p")), hash("/mnt/d/dev/p"));
  });
});

describe("loadInjectionConfig", () => {
  test("first run auto-initializes ~/.zcode/cerebro.json from bundled default", () => {
    // Call FIRST — the auto-init copy happens inside loadInjectionConfig()
    const cfg = loadInjectionConfig();
    const userCfg = join(tempHome, ".zcode", "cerebro.json");
    assert.ok(existsSync(userCfg), "user config should be auto-created");
    assert.equal(cfg.recall.enabled, true);
    assert.equal(cfg.nudge.enabled, true);
    assert.equal(cfg.sessionStart.profileEnabled, true);
    assert.ok(Array.isArray(cfg.nudge.saveKeywords) && cfg.nudge.saveKeywords.includes("记住"));
  });

  test("user file wins over bundled default", () => {
    const userCfg = join(tempHome, ".zcode", "cerebro.json");
    writeFileSync(userCfg, JSON.stringify({ recall: { enabled: false }, language: "zh" }));
    const cfg = loadInjectionConfig();
    assert.equal(cfg.recall.enabled, false);
    assert.equal(cfg.language, "zh");
  });
});
