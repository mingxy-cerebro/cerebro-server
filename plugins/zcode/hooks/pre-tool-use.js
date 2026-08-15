#!/usr/bin/env node
// PreToolUse hook — auto-approve memory-recall skill/tool calls.
// Port of plugins/claude-code/hooks/recall-approve.mjs, adapted for ZCode
// (zcode skills are named memory-recall / memory-store, not memory-search).
//
// hooks.json matcher: "Skill|Bash"
// Input (stdin): { tool_name, tool_input }
// Output (stdout):
//   { "hookSpecificOutput": { "hookEventName": "PreToolUse", "permissionDecision": "allow" } }
// Non-matching calls emit {} (empty output is valid; no opinion).

import { logDebug } from "./lib/logger.js";

function readStdin() {
  return new Promise((resolve) => {
    let data = "";
    if (process.stdin.isTTY) return resolve({});
    process.stdin.setEncoding("utf-8");
    process.stdin.on("data", (chunk) => (data += chunk));
    process.stdin.on("end", () => {
      try {
        resolve(data.trim() ? JSON.parse(data) : {});
      } catch {
        resolve({});
      }
    });
    process.stdin.on("error", () => resolve({}));
    setTimeout(() => resolve({}), 2000);
  });
}

async function main() {
  const input = await readStdin();
  const toolName = String(input?.tool_name || input?.toolName || "").toLowerCase();
  const toolInput = input?.tool_input || input?.toolInput || {};
  const cmd = typeof toolInput.command === "string" ? toolInput.command : "";
  const name = typeof toolInput.name === "string" ? toolInput.name : "";

  let isRecall = false;
  if (toolName.includes("memory-recall")) {
    isRecall = true;
  } else if (toolName === "skill" && name.toLowerCase().includes("memory-recall")) {
    isRecall = true;
  } else if (toolName === "bash" && cmd.includes("memory")) {
    // Allow only clean single commands — reject anything with shell chaining
    // or substitution (prevents injection via a crafted recall command)
    isRecall = !/[;&|`]/.test(cmd) && !cmd.includes("$(");
  }

  if (isRecall) {
    logDebug("PreToolUse: auto-approving memory-recall call", { toolName });
    process.stdout.write(
      JSON.stringify({
        hookSpecificOutput: {
          hookEventName: "PreToolUse",
          permissionDecision: "allow",
        },
      }),
    );
  } else {
    process.stdout.write("{}");
  }
}

main().catch(() => {
  process.stdout.write("{}");
});
