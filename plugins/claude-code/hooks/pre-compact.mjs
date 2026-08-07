#!/usr/bin/env node
// cerebro PreCompact hook — flush delta + compaction guidance
import { config, flushSessionIngest, parseStdinJSON, emit, logWarn, logError } from "./common.mjs";

if (!config.apiKey) { emit({}); process.exit(0); }

const input = parseStdinJSON();
const tp = input.transcript_path || "";
const sid = input.session_id || input.sessionId || "";

if (!tp || !sid) {
  logWarn("pre-compact: missing transcript_path/session_id");
  emit({});
  process.exit(0);
}

const ok = await flushSessionIngest(tp, sid).catch(() => false);
if (!ok) logError(`pre-compact: flush_session_ingest failed for sid=${sid} (will retry next hook)`);

const guidance = `<cerebro-compaction>
压缩这段对话时遵循以下原则：

保留：
- 架构决策与理由（为什么选 X 而非 Y）
- 已确认的偏好、约定、协作模式
- 当前进度、未完成事项、下一步计划
- 关键 bug / 修复、踩过的坑、重要发现

丢弃：
- 未被采纳的建议、被否决的方案
- 临时调试输出、重复的工具调用结果
- 已被新信息取代的旧结论

FETCH_POLICY：压缩后若提到历史记忆但细节不足，优先用 memory-search skill 取全量，不要凭印象补全。
</cerebro-compaction>`;

emit({ hookSpecificOutput: { hookEventName: "PreCompact", additionalContext: guidance } });
