// MCP server protocol tests — spawn mcp/server.js over stdio JSON-RPC.
// Verifies the zero-dependency transport: initialize handshake, tools/list
// (17 tools), enum validation (fails BEFORE any network call), unknown tool
// error, and ping. No real API contact is needed for these paths.
import { test, describe, before, after } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { createInterface } from "node:readline";

const MCP_SERVER = join(dirname(fileURLToPath(import.meta.url)), "..", "mcp", "server.js");

let child;
let tempHome;
const pending = new Map(); // id → {resolve, reject}
let nextId = 1;

function rpc(method, params) {
  const id = nextId++;
  const msg = { jsonrpc: "2.0", id, method, ...(params !== undefined ? { params } : {}) };
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`timeout waiting for ${method} response`));
    }, 10000);
    pending.set(id, {
      resolve: (v) => { clearTimeout(timer); resolve(v); },
      reject: (e) => { clearTimeout(timer); reject(e); },
    });
    child.stdin.write(JSON.stringify(msg) + "\n");
  });
}

before(async () => {
  tempHome = mkdtempSync(join(tmpdir(), "cerebro-zcode-mcp-"));
  child = spawn(process.execPath, [MCP_SERVER], {
    stdio: ["pipe", "pipe", "pipe"],
    env: {
      ...process.env,
      HOME: tempHome,
      USERPROFILE: tempHome,
      OMEM_API_KEY: "",
      OMEM_API_URL: "http://127.0.0.1:1",
    },
  });
  const rl = createInterface({ input: child.stdout, terminal: false });
  rl.on("line", (line) => {
    const trimmed = line.trim();
    if (!trimmed) return;
    let msg;
    try { msg = JSON.parse(trimmed); } catch { return; }
    if (msg.id !== undefined && pending.has(msg.id)) {
      const p = pending.get(msg.id);
      pending.delete(msg.id);
      if (msg.error) p.reject(new Error(JSON.stringify(msg.error)));
      else p.resolve(msg.result);
    }
  });
});

after(() => {
  try { child.kill(); } catch {}
  try { rmSync(tempHome, { recursive: true, force: true }); } catch {}
});

describe("stdio JSON-RPC transport", () => {
  test("initialize handshake returns serverInfo + tools capability", async () => {
    const result = await rpc("initialize", {
      protocolVersion: "2024-11-05",
      capabilities: {},
      clientInfo: { name: "test-client", version: "1.0.0" },
    });
    assert.equal(result.serverInfo.name, "cerebro-zcode");
    assert.match(result.serverInfo.version, /^\d+\.\d+\.\d+$/);
    assert.ok(result.capabilities.tools, "must advertise tools capability");
    assert.equal(result.protocolVersion, "2024-11-05");
  });

  test("tools/list exposes 17 tools with JSON schemas", async () => {
    const result = await rpc("tools/list", {});
    const names = result.tools.map((t) => t.name);
    assert.equal(result.tools.length, 17);
    for (const expected of [
      "memory_store", "memory_search", "memory_get", "memory_update", "memory_delete",
      "memory_list", "memory_profile", "memory_profile_stats", "memory_ingest",
      "memory_stats", "memory_toggle",
      "space_create", "space_list", "space_add_member",
      "memory_share", "memory_pull", "memory_reshare",
    ]) {
      assert.ok(names.includes(expected), `missing tool: ${expected}`);
    }
    for (const t of result.tools) {
      assert.equal(t.inputSchema.type, "object", `${t.name} inputSchema must be an object schema`);
      assert.ok(typeof t.description === "string" && t.description.length > 0, `${t.name} needs a description`);
    }
  });

  test("enum validation rejects bad category BEFORE any network call", async () => {
    const result = await rpc("tools/call", {
      name: "memory_store",
      arguments: { content: "test content", category: "bogus-category" },
    });
    assert.equal(result.isError, true);
    assert.match(result.content[0].text, /category must be one of/);
  });

  test("enum validation rejects bad scope", async () => {
    const result = await rpc("tools/call", {
      name: "memory_search",
      arguments: { query: "q", scope: "everywhere" },
    });
    assert.equal(result.isError, true);
    assert.match(result.content[0].text, /scope must be one of/);
  });

  test("limit bounds enforced", async () => {
    const result = await rpc("tools/call", {
      name: "memory_search",
      arguments: { query: "q", limit: 500 },
    });
    assert.equal(result.isError, true);
    assert.match(result.content[0].text, /limit must be an integer between 1 and 50/);
  });

  test("memory_toggle works without network", async () => {
    const q = await rpc("tools/call", { name: "memory_toggle", arguments: { session_id: "mcp-test" } });
    assert.match(q.content[0].text, /Auto-store for session mcp-test: ON/);
    const off = await rpc("tools/call", { name: "memory_toggle", arguments: { session_id: "mcp-test", state: "off" } });
    assert.match(off.content[0].text, /set to OFF/);
    const on = await rpc("tools/call", { name: "memory_toggle", arguments: { session_id: "mcp-test", state: "on" } });
    assert.match(on.content[0].text, /set to ON/);
  });

  test("unknown tool returns JSON-RPC error -32602", async () => {
    await assert.rejects(
      () => rpc("tools/call", { name: "no_such_tool", arguments: {} }),
      (err) => err.message.includes("-32602"),
    );
  });

  test("ping responds with empty result", async () => {
    const result = await rpc("ping", {});
    assert.deepEqual(result, {});
  });

  test("unknown method returns -32601", async () => {
    await assert.rejects(
      () => rpc("bogus/method", {}),
      (err) => err.message.includes("-32601"),
    );
  });

  test("missing API key surfaces as tool error (not crash)", async () => {
    // Validation passes, then request() throws on missing key — must come
    // back as an isError tool result, never kill the server process.
    const result = await rpc("tools/call", {
      name: "memory_store",
      arguments: { content: "will fail at HTTP layer" },
    });
    assert.equal(result.isError, true);
    assert.match(result.content[0].text, /OMEM_API_KEY/);
    // Server still alive afterwards
    const pong = await rpc("ping", {});
    assert.deepEqual(pong, {});
  });
});
