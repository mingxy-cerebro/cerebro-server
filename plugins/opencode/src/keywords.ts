const SAVE_KEYWORDS: readonly string[] = [
  "remember", "save this", "don't forget",
  "记住", "记一下", "保存", "记下来", "别忘了",
  "memory_store",
] as const;

const RECALL_KEYWORDS: readonly string[] = [
  "i remember", "i recall", "we discussed", "we talked about",
  "last time", "previously", "before we", "earlier we",
  "look up", "find that", "search for", "check what",
  "我记得", "之前说过", "之前聊过", "上次说的", "查一下",
  "搜一下", "之前讨论", "我记得之前", "找一下",
  "memory_search", "memory_get",
] as const;

export function detectSaveKeyword(text: string): boolean {
  const lower = text.toLowerCase();
  return SAVE_KEYWORDS.some((kw) => lower.includes(kw));
}

export function detectRecallKeyword(text: string): boolean {
  const lower = text.toLowerCase();
  return RECALL_KEYWORDS.some((kw) => lower.includes(kw));
}

export const KEYWORD_NUDGE =
  "[cerebro] The user wants you to remember this. Use the `memory_store` tool to save it now.";

export const RECALL_NUDGE =
  "[cerebro] The user is referencing past context. Use the `memory_search` tool to find relevant memories before responding.";
