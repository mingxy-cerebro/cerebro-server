#!/usr/bin/env node
// cerebro UserPromptSubmit hook — reasoned recall instruction + 语义搜索注入
// 对标 opencode buildMemoryInjection 的 search 路：每轮用 prompt 做语义搜索。
// POST recall-event 让 web Sessions 页面看到每轮注入（对标 opencode chatMessageRecallHook createRecallEvent）。
import { readStdin, emit, searchMemories, detectProjectPath, formatRelativeAge, postRecallEvent } from "./common.mjs";

const input = JSON.parse(readStdin() || "{}");
const prompt = typeof input === "object" ? input.prompt || input.message || "" : "";
const sid = input.session_id || "";

// ─── 语义搜索（对标 opencode searchMemories 路）─────────────────────────────────
const pp = detectProjectPath();
const results = await searchMemories(prompt, undefined, pp);

let searchBlock = "";
if (results.length > 0) {
  const lines = ["## Relevant Memories"];
  for (const r of results) {
    const age = formatRelativeAge(r.memory?.created_at);
    lines.push(`- (${age}) ${r.memory?.content || ""}`);
  }
  searchBlock = lines.join("\n");
}

// ─── reasoned recall 指令（保留，补充主动搜索）──────────────────────────────────
const instruction = `<cerebro-recall>
在回复前默默判断：召回长期记忆对回答这条消息是否有实质帮助。先推理再决定——不要条件反射式搜索，也不要把判断过程说出来。

符合以下任一才召回（调用 memory-search skill）：
- 提及过往工作、决策或上下文（"之前的X"、"上次那个 bug"、"继续"、"接着做"）
- 触及可能已保存的偏好、约定、模式（编码风格、工具选择、协作习惯）
- 出现有歧义的历史名词 / 文件 / 概念，存储的记忆能消解

跳过这些情况：
- 消息自包含、琐碎、问候、或 meta 性提问
- 当前对话内已有足够信息作答
- 本 session 已就该话题召回过且上下文未变

连续多轮召回无妨，整个 session 一次不召回也无妨。判断它是否真正有用，而不是为了用而用。
</cerebro-recall>`;

// 关键词 nudge
const SAVE_KW = ["记住", "记着", "别忘了", "记一下", "remember", "don't forget", "note that"];
const RECALL_KW = ["记得", "之前", "上次", "刚才", "那个", "continue", "recall", "earlier", "the bug from before"];

const pLow = prompt.toLowerCase();
const nudges = [];
if (SAVE_KW.some((kw) => pLow.includes(kw))) {
  nudges.push("<cerebro-nudge>检测到你希望保存信息。请用 memory-save skill 记录（选准 category，敏感数据用 visibility=private）。</cerebro-nudge>");
}
if (RECALL_KW.some((kw) => pLow.includes(kw))) {
  nudges.push("<cerebro-nudge>这条消息可能需要历史上下文。若 reasoned recall 判断相关，果断用 memory-search skill 召回。</cerebro-nudge>");
}

// 组装：搜索结果在前，指令在后
const parts = [];
if (searchBlock) parts.push(searchBlock);
parts.push(instruction);
if (nudges.length) parts.push(nudges.join("\n"));

const injectionText = parts.join("\n\n");

// ─── POST recall-event（对标 opencode chatMessageRecallHook createRecallEvent）──────
// 让 web Sessions 页面显示用户实际 prompt + 注入内容，而非只有 SessionStart 一条。
await postRecallEvent({
  sessionId: sid,
  recallType: "auto",
  queryText: prompt,
  profileInjected: false,
  keptCount: results.length,
  injectedContent: injectionText,
  maxScore: results.length > 0 ? (results[0]?.score || 0) : 0,
});

emit({ hookSpecificOutput: { hookEventName: "UserPromptSubmit", additionalContext: injectionText } });
