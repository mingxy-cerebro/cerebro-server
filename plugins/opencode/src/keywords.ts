const SAVE_KEYWORDS: readonly string[] = [
  "remember", "save this", "don't forget",
  "记住", "记一下", "保存", "记下来", "别忘了",
  "memory_store",
] as const;

export function detectSaveKeyword(text: string): boolean {
  const lower = text.toLowerCase();
  return SAVE_KEYWORDS.some((kw) => lower.includes(kw));
}

export const KEYWORD_NUDGE =
  "[cerebro] The user wants you to remember this. Use the `memory_store` tool to save it now.";
