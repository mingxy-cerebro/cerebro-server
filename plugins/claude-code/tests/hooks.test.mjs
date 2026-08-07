import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HOOKS_DIR = join(dirname(fileURLToPath(import.meta.url)), "..", "hooks");

/**
 * Run a hook script with given stdin JSON, return parsed stdout JSON.
 * Points OMEM_API_URL to a dead port so fetch calls fail fast
 * (postRecallEvent / buildMemoryInjection swallow errors via try/catch).
 */
function runHook(script, stdinObj, opts = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [join(HOOKS_DIR, script)], {
      stdio: ["pipe", "pipe", "pipe"],
      env: {
        ...process.env,
        OMEM_API_KEY: opts.apiKey ?? "test-key-12345",
        OMEM_API_URL: opts.apiUrl ?? "http://127.0.0.1:1",
      },
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d) => (stdout += d));
    child.stderr.on("data", (d) => (stderr += d));
    child.on("close", (code) => resolve({ stdout, stderr, code }));
    child.on("error", reject);
    child.stdin.write(JSON.stringify(stdinObj));
    child.stdin.end();
  });
}

describe("user-prompt-submit.mjs", () => {
  test("injects cerebro-recall instruction for normal message", async () => {
    const { stdout } = await runHook("user-prompt-submit.mjs", {
      session_id: "test-sid",
      prompt: "hello world",
    });
    const out = JSON.parse(stdout);
    const ctx = out.hookSpecificOutput.additionalContext;
    assert.ok(ctx.includes("<cerebro-recall>"), "should contain cerebro-recall instruction");
    assert.ok(!ctx.includes("cerebro-nudge"), "should NOT contain nudge for normal message");
  });

  test("injects save nudge when prompt contains save keyword", async () => {
    const { stdout } = await runHook("user-prompt-submit.mjs", {
      session_id: "test-sid",
      prompt: "记住这个配置",
    });
    const ctx = JSON.parse(stdout).hookSpecificOutput.additionalContext;
    assert.ok(ctx.includes("<cerebro-nudge>"), "should contain nudge");
    assert.ok(ctx.includes("memory-save"), "should mention memory-save skill");
  });

  test("injects recall nudge when prompt contains recall keyword", async () => {
    const { stdout } = await runHook("user-prompt-submit.mjs", {
      session_id: "test-sid",
      prompt: "之前那个bug怎么修的",
    });
    const ctx = JSON.parse(stdout).hookSpecificOutput.additionalContext;
    assert.ok(ctx.includes("<cerebro-nudge>"), "should contain nudge");
    assert.ok(ctx.includes("memory-search"), "should mention memory-search skill");
  });

  test("handles English keywords", async () => {
    const { stdout } = await runHook("user-prompt-submit.mjs", {
      session_id: "test-sid",
      prompt: "remember to update the docs",
    });
    const ctx = JSON.parse(stdout).hookSpecificOutput.additionalContext;
    assert.ok(ctx.includes("cerebro-nudge"), "should detect English save keyword");
  });
});

describe("pre-compact.mjs", () => {
  test("outputs empty JSON (no hookSpecificOutput)", async () => {
    const { stdout } = await runHook("pre-compact.mjs", {
      session_id: "test-sid",
      transcript_path: "/nonexistent/path.jsonl",
    });
    const out = JSON.parse(stdout);
    assert.deepEqual(out, {});
  });

  test("outputs empty JSON when missing session_id", async () => {
    const { stdout } = await runHook("pre-compact.mjs", {
      transcript_path: "/nonexistent/path.jsonl",
    });
    const out = JSON.parse(stdout);
    assert.deepEqual(out, {});
  });
});

describe("session-end.mjs", () => {
  test("outputs empty JSON for nonexistent transcript", async () => {
    const { stdout } = await runHook("session-end.mjs", {
      session_id: "test-sid",
      transcript_path: "/nonexistent/path.jsonl",
    });
    const out = JSON.parse(stdout);
    assert.deepEqual(out, {});
  });

  test("outputs empty JSON when missing fields", async () => {
    const { stdout } = await runHook("session-end.mjs", {});
    const out = JSON.parse(stdout);
    assert.deepEqual(out, {});
  });
});
