// Hook contract tests — stdin JSON → stdout JSON (strict zcode schema).
// OMEM_API_URL points at a dead port so fetch calls fail fast; HOME/USERPROFILE
// are redirected to a temp dir so the real user config never influences results.
import { test, describe, before, after } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HOOKS_DIR = join(dirname(fileURLToPath(import.meta.url)), "..", "hooks");

let tempHome;
before(() => {
  tempHome = mkdtempSync(join(tmpdir(), "cerebro-zcode-test-"));
});
after(() => {
  try { rmSync(tempHome, { recursive: true, force: true }); } catch {}
});

function runHook(script, stdinObj, opts = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [join(HOOKS_DIR, script)], {
      stdio: ["pipe", "pipe", "pipe"],
      env: {
        ...process.env,
        HOME: tempHome,
        USERPROFILE: tempHome,
        OMEM_API_KEY: opts.apiKey ?? "test-key-12345",
        OMEM_API_URL: opts.apiUrl ?? "http://127.0.0.1:1",
        OMEM_LOG_ENABLED: "0",
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

describe("user-prompt-submit.js", () => {
  test("emits valid zcode schema with cerebro-recall instruction", async () => {
    const { stdout } = await runHook("user-prompt-submit.js", {
      session_id: "test-sid",
      prompt: "hello world",
    });
    const out = JSON.parse(stdout);
    assert.equal(out.hookSpecificOutput.hookEventName, "UserPromptSubmit");
    const ctx = out.hookSpecificOutput.additionalContext;
    assert.ok(ctx.includes("<cerebro-recall>"), "should contain cerebro-recall instruction");
    assert.ok(!ctx.includes("cerebro-nudge"), "should NOT contain nudge for normal message");
  });

  test("injects save nudge when prompt contains save keyword", async () => {
    const { stdout } = await runHook("user-prompt-submit.js", {
      session_id: "test-sid",
      prompt: "记住这个配置",
    });
    const ctx = JSON.parse(stdout).hookSpecificOutput.additionalContext;
    assert.ok(ctx.includes("<cerebro-nudge>"), "should contain nudge");
    assert.ok(ctx.includes("memory_store"), "should mention memory_store tool");
  });

  test("injects recall nudge when prompt contains recall keyword", async () => {
    const { stdout } = await runHook("user-prompt-submit.js", {
      session_id: "test-sid",
      prompt: "之前那个bug怎么修的",
    });
    const ctx = JSON.parse(stdout).hookSpecificOutput.additionalContext;
    assert.ok(ctx.includes("<cerebro-nudge>"), "should contain nudge");
    assert.ok(ctx.includes("memory_search"), "should mention memory_search tool");
  });

  test("handles English keywords", async () => {
    const { stdout } = await runHook("user-prompt-submit.js", {
      session_id: "test-sid",
      prompt: "remember to update the docs",
    });
    const ctx = JSON.parse(stdout).hookSpecificOutput.additionalContext;
    assert.ok(ctx.includes("cerebro-nudge"), "should detect English save keyword");
  });
});

describe("pre-tool-use.js", () => {
  test("auto-approves memory-recall skill call", async () => {
    const { stdout } = await runHook("pre-tool-use.js", {
      tool_name: "Skill",
      tool_input: { name: "cerebro:memory-recall", args: "query" },
    });
    const out = JSON.parse(stdout);
    assert.equal(out.hookSpecificOutput.hookEventName, "PreToolUse");
    assert.equal(out.hookSpecificOutput.permissionDecision, "allow");
  });

  test("auto-approves clean bash memory command", async () => {
    const { stdout } = await runHook("pre-tool-use.js", {
      tool_name: "Bash",
      tool_input: { command: "node memory-recall.js query" },
    });
    const out = JSON.parse(stdout);
    assert.equal(out.hookSpecificOutput.permissionDecision, "allow");
  });

  test("rejects bash with shell chaining (injection guard)", async () => {
    const { stdout } = await runHook("pre-tool-use.js", {
      tool_name: "Bash",
      tool_input: { command: "node memory-recall.js q; rm -rf /" },
    });
    const out = JSON.parse(stdout);
    assert.deepEqual(out, {});
  });

  test("empty output for unrelated tools", async () => {
    const { stdout } = await runHook("pre-tool-use.js", {
      tool_name: "Read",
      tool_input: { file_path: "/tmp/x" },
    });
    assert.deepEqual(JSON.parse(stdout), {});
  });
});

describe("stop.js", () => {
  test("empty JSON when no sessionId", async () => {
    const { stdout } = await runHook("stop.js", { cwd: process.cwd() });
    assert.deepEqual(JSON.parse(stdout), {});
  });

  test("empty JSON when rollout does not exist (isolated HOME)", async () => {
    const { stdout } = await runHook("stop.js", {
      sessionId: "nonexistent-sess-xyz",
      turnId: "t1",
      cwd: process.cwd(),
    });
    assert.deepEqual(JSON.parse(stdout), {});
  });

  test("empty JSON without API key", async () => {
    const { stdout } = await runHook("stop.js", { sessionId: "s1" }, { apiKey: "" });
    assert.deepEqual(JSON.parse(stdout), {});
  });
});

describe("session-start.js", () => {
  test("emits API-key guidance when no key configured", async () => {
    const { stdout } = await runHook("session-start.js", { cwd: process.cwd() }, { apiKey: "" });
    const out = JSON.parse(stdout);
    assert.equal(out.hookSpecificOutput.hookEventName, "SessionStart");
    assert.ok(out.hookSpecificOutput.additionalContext.includes("OMEM_API_KEY"));
  });

  test("degrades gracefully when API unreachable (dead port)", async () => {
    const { stdout, code } = await runHook("session-start.js", {
      cwd: process.cwd(),
      source: "startup",
    });
    const out = JSON.parse(stdout);
    assert.equal(code, 0, "must exit 0 (never block session start)");
    assert.ok(
      out.hookSpecificOutput === undefined || typeof out.hookSpecificOutput.additionalContext === "string",
      "output must stay schema-valid on degraded path",
    );
  });
});
