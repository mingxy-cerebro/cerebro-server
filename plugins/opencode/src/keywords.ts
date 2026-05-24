const SAVE_KEYWORDS: readonly string[] = [
  "remember", "save this", "don't forget",
  "记住", "记一下", "保存", "记下来", "别忘了",
  "memory_store",
] as const;

const RECALL_KEYWORDS: readonly string[] = [
  // English — explicit past-conversation references
  "i remember", "i recall", "we discussed", "we talked about",
  "last time", "earlier we", "previously", "before we",
  "look up", "find that", "search for", "check what",
  "what did we", "do you remember", "from our previous", "as discussed",
  // Chinese — explicit memory recall cues
  "我记得", "之前说过", "之前聊过", "上次说的",
  "之前讨论", "我记得之前", "查一下", "搜一下", "找一下",
  "之前提到", "记得吗", "你还记得", "回忆一下",
  "上次那个", "之前那个", "上次讨论", "上次做的",
  "之前记录", "之前保存", "上次决定", "之前约定",
  // Direct tool references
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
  "[cerebro] The user references past conversations or stored information. Use `memory_search` with keywords from their message to retrieve relevant memories before responding.";
