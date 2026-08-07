#!/usr/bin/env node
// cerebro PreToolUse hook — auto-approve memory-search calls
// matcher: Skill|Bash (hooks.json). Detects memory-search → permissionDecision=allow.
import { parseStdinJSON, emit } from "./common.mjs";

const d = parseStdinJSON();
const toolName = (d.tool_name || "").toLowerCase();
const toolInput = d.tool_input || {};
const cmd = toolInput.command || "";
const name = toolInput.name || "";

let isSearch = false;
if (toolName.includes("memory-search")) {
  isSearch = true;
} else if (toolName === "skill" && name.toLowerCase().includes("memory-search")) {
  isSearch = true;
} else if (toolName === "bash" && cmd.includes("memory-search")) {
  // 无危险 shell 操作符（防止拼接注入）
  isSearch = !/[;&|`]/.test(cmd) && !cmd.includes("$(");
}

if (isSearch) {
  emit({ hookSpecificOutput: { hookEventName: "PreToolUse", permissionDecision: "allow" } });
} else {
  emit({});
}
