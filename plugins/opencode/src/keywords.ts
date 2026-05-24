const SAVE_KEYWORDS: readonly string[] = [
  // English
  "remember", "save this", "don't forget", "keep in mind",
  "note that", "store this", "memorize",
  "make a note", "write this down", "jot this down",
  "for future reference", "bear in mind",
  "commit to memory", "take note",
  // Chinese
  "记住", "记一下", "保存", "记下来", "别忘了",
  "记好", "存一下", "记住了",
  "写下来", "记到", "存起来",
  // Tool-related
  "memory_store", "save memory", "store memory",
  "保存记忆", "存储记忆",
] as const;

export function detectSaveKeyword(text: string): boolean {
  const lower = text.toLowerCase();
  return SAVE_KEYWORDS.some((kw) => lower.includes(kw));
}

export const KEYWORD_NUDGE =
  "[cerebro] The user wants you to remember this. Use the `memory_store` tool to save it now.";
