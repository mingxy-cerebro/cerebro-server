pub fn build_system_prompt(entity_context: Option<&str>) -> String {
    let mut prompt = BASE_SYSTEM_PROMPT.to_string();
    if let Some(ctx) = entity_context {
        let truncated = if ctx.len() > 1500 { &ctx[..1500] } else { ctx };
        prompt.push_str("\n\n## Additional Context\n");
        prompt.push_str(truncated);
    }
    prompt
}

pub fn build_user_prompt(conversation_text: &str) -> String {
    format!(
        "Extract all distinct, atomic facts from the following conversation:\n\n{conversation_text}"
    )
}

use crate::domain::memory::Memory;
use crate::ingest::types::ExtractedFact;

struct ExistingMemoryEntry<'a> {
    int_id: usize,
    memory: &'a Memory,
    age_label: String,
}

/// Returns (system_prompt, user_prompt).
pub fn build_reconcile_prompt(
    facts: &[ExtractedFact],
    existing: &[Memory],
    id_map: &[(usize, &str)], // (int_id -> real uuid)
    fuzzy_pairs: &[(usize, usize)],
) -> (String, String) {
    let system = RECONCILE_SYSTEM_PROMPT.to_string();

    let mut user = String::with_capacity(2048);

    user.push_str("## New Facts\n");
    for (i, fact) in facts.iter().enumerate() {
        user.push_str(&format!(
            "[{}] (category: {}) {}\n",
            i, fact.category, fact.l0_abstract
        ));
    }

    if !fuzzy_pairs.is_empty() {
        user.push_str("\n## Potential Duplicates\n");
        user.push_str("The following new fact pairs have very high textual similarity (>85%). ");
        user.push_str("Carefully review whether they are true duplicates (one should be SKIPPED) or represent genuinely different information:\n");
        for (i, j) in fuzzy_pairs {
            user.push_str(&format!("- [{}] and [{}]\n", i, j));
        }
    }

    if existing.is_empty() {
        user.push_str("\n## Existing Memories\nNone.\n");
    } else {
        user.push_str("\n## Existing Memories\n");
        let entries: Vec<ExistingMemoryEntry> = existing
            .iter()
            .filter_map(|m| {
                id_map
                    .iter()
                    .find(|(_, uuid)| *uuid == m.id)
                    .map(|(int_id, _)| ExistingMemoryEntry {
                        int_id: *int_id,
                        memory: m,
                        age_label: format_age(&m.created_at),
                    })
            })
            .collect();

        for entry in &entries {
            user.push_str(&format!(
                "[{}] (category: {}, age: {}) {}\n",
                entry.int_id,
                entry.memory.category,
                entry.age_label,
                entry.memory.l0_abstract.as_str(),
            ));
        }
    }

    user.push_str(&format!(
        "\nReturn a JSON object with a \"decisions\" array containing exactly {} decision(s), one per fact.\n",
        facts.len()
    ));

    (system, user)
}

fn format_age(created_at: &str) -> String {
    let Ok(created) = chrono::DateTime::parse_from_rfc3339(created_at) else {
        return "unknown".to_string();
    };
    let now = chrono::Utc::now();
    let duration = now.signed_duration_since(created);

    let days = duration.num_days();
    if days < 1 {
        return "today".to_string();
    }
    if days == 1 {
        return "1 day ago".to_string();
    }
    if days < 30 {
        return format!("{days} days ago");
    }
    let months = days / 30;
    if months == 1 {
        return "1 month ago".to_string();
    }
    if months < 12 {
        return format!("{months} months ago");
    }
    let years = months / 12;
    if years == 1 {
        "1 year ago".to_string()
    } else {
        format!("{years} years ago")
    }
}

const RECONCILE_SYSTEM_PROMPT: &str = r#"You are a memory reconciliation engine. Given a set of NEW FACTS extracted from a conversation and a set of EXISTING MEMORIES, decide what to do with each fact.

## Operations

- **CREATE**: The fact contains genuinely new information not covered by any existing memory. Creates a new memory.
- **MERGE**: The fact adds detail, clarification, or refinement to an existing memory. The existing memory's content should be enriched. Provide `merged_content` — the combined text.
- **SKIP**: The fact is a duplicate or contains less information than an existing memory. No action needed.
- **SUPERSEDE**: The fact contradicts or updates an existing memory on the same topic (e.g., changed preference, updated status). The old memory is archived and a new one is created. Use when time-sensitive facts have changed.
- **SUPPORT**: The candidate reinforces or confirms an existing memory, possibly in a specific context. No new memory is created — the existing memory's confidence is boosted. Include `context_label` (one of: general, morning, evening, work, leisure, seasonal, weekday, weekend).
- **CONTEXTUALIZE**: The candidate adds situational nuance to an existing memory without contradicting it. Example: existing "likes coffee" + new "prefers tea in the evening". A new memory is created with a relation to the existing one. Include `context_label`.
- **CONTRADICT**: The candidate directly contradicts an existing memory. For temporal_versioned categories (preferences, entities) with general context, this routes to SUPERSEDE behavior. Otherwise, a new memory is created and the contradiction is recorded.

## Category-Aware Rules

1. **profile** category: always use MERGE when a matching memory exists (never SUPERSEDE or CONTRADICT for profile).
2. **events** and **cases** categories: only CREATE or SKIP. Never MERGE, SUPERSEDE, SUPPORT, CONTEXTUALIZE, or CONTRADICT.
3. **preferences** and **entities** categories: support all 7 operations including SUPERSEDE and CONTRADICT.
4. **preferences**, **entities**, **patterns** categories: support MERGE.

## General Rules

1. Each fact MUST receive exactly one decision.
2. Use `match_index` to reference existing memories by their integer ID (shown in brackets).
3. For MERGE: `match_index` is required. Provide `merged_content` combining both old and new info.
4. For SUPERSEDE: `match_index` is required. The old memory will be archived.
5. For SUPPORT: `match_index` is required. Include `context_label`.
6. For CONTEXTUALIZE: `match_index` is required. Include `context_label`.
7. For CONTRADICT: `match_index` is required.
8. For CREATE and SKIP: `match_index` is optional (null).
9. Same meaning, different wording → SKIP (not MERGE).
10. Age is a tiebreaker: when a new fact conflicts with an old memory on the same topic, the older memory is more likely outdated → prefer SUPERSEDE.
11. When in doubt, prefer CREATE over SKIP (avoid losing information).
12. When two new facts are marked as 'Potential Duplicates', evaluate whether they convey the same core information. If yes, one should SKIP. If they capture different aspects or nuances, both may CREATE.

## Output Format
Return ONLY valid JSON:
{"decisions": [{"action": "CREATE", "fact_index": 0, "reason": "new info"}, {"action": "MERGE", "fact_index": 1, "match_index": 3, "merged_content": "combined text", "reason": "adds detail"}, {"action": "SKIP", "fact_index": 2, "match_index": 0, "reason": "duplicate"}, {"action": "SUPERSEDE", "fact_index": 3, "match_index": 1, "reason": "updated preference"}, {"action": "SUPPORT", "fact_index": 4, "match_index": 2, "context_label": "work", "reason": "reinforces existing"}, {"action": "CONTEXTUALIZE", "fact_index": 5, "match_index": 4, "context_label": "evening", "reason": "adds situational nuance"}, {"action": "CONTRADICT", "fact_index": 6, "match_index": 5, "reason": "directly contradicts"}]}
"#;

pub fn build_batch_dedup_prompt(facts: &[ExtractedFact]) -> (String, String) {
    let mut facts_text = String::new();
    for (i, fact) in facts.iter().enumerate() {
        let display = fact
            .source_text
            .as_deref()
            .map(|s| {
                let truncated: String = s.chars().take(200).collect();
                if s.chars().count() > 200 {
                    format!("{truncated}...")
                } else {
                    truncated
                }
            })
            .unwrap_or_else(|| fact.l0_abstract.clone());
        facts_text.push_str(&format!("FACT[{}]: [{}] {}\n", i, fact.category, display));
    }
    (
        BATCH_DEDUP_SYSTEM_PROMPT.to_string(),
        format!("Deduplicate the following facts:\n\n{facts_text}"),
    )
}

pub fn build_section_prompt(section_text: &str) -> (String, String) {
    (
        SECTION_SYSTEM_PROMPT.to_string(),
        format!("Summarize the following section as a single memory:\n\n{section_text}"),
    )
}

pub fn build_document_prompt(document_text: &str) -> (String, String) {
    (
        DOCUMENT_SYSTEM_PROMPT.to_string(),
        format!(
            "Summarize the following document as a single comprehensive memory:\n\n{document_text}"
        ),
    )
}

const BATCH_DEDUP_SYSTEM_PROMPT: &str = r#"You are a deduplication engine. Given a list of extracted facts, identify and remove duplicates or near-duplicates within the batch.

## Rules

1. Compare all facts pairwise.
2. When two facts cover the same topic or convey the same meaning:
   - Keep the MORE DETAILED or MORE SPECIFIC one.
   - If they are equally detailed, keep the one with the lower index.
3. If no duplicates are found, return ALL indices.
4. Preserve the original language of each fact.
5. Only remove true duplicates or highly overlapping facts. Different aspects of the same topic are NOT duplicates.

## Output Format
Return ONLY valid JSON:
{"keep_indices": [0, 2, 3, 5]}

The array should list the indices of facts to KEEP (not the ones to remove).
If all facts are unique, return all indices: {"keep_indices": [0, 1, 2, ...]}
"#;

const SECTION_SYSTEM_PROMPT: &str = r#"You are a memory extraction engine. Your task is to create exactly ONE memory from the given text section.

## ABSOLUTE RULES (Violating any of these is a FAILURE)

### Rule 1: Language Preservation (MANDATORY)
- **YOU MUST OUTPUT IN THE SAME LANGUAGE AS THE INPUT.**
- If the input is Chinese, EVERY SINGLE FIELD must be in Chinese: l0_abstract, l1_overview, l2_content, tags, EVERYTHING.
- If the input is English, EVERY SINGLE FIELD must be in English.
- **NEVER translate. NEVER mix languages. NEVER output English for Chinese input.**
- **Before returning, verify: "Are ALL fields in the same language as the input?" If not, rewrite them.**

### Rule 2: Privacy Detection (MANDATORY)
- **Before outputting, check: "Does this memory contain sensitive or private content?"**
- If YES, you MUST add the tag "私密" to the tags array.
- Sensitive content: passwords, API keys, tokens, server IPs, credentials, personal secrets, intimate details.
- **NEVER skip this check.**

## General Rules
- Create exactly 1 memory that captures the section's key information.
- Do NOT split into multiple facts — summarize as one cohesive memory.

## Categories
Classify the memory into exactly one category:
- **profile**: Biographical or identity information.
- **preferences**: Likes, dislikes, tool choices, style preferences.
- **entities**: Persistent nouns (projects, tools, people, orgs) and their states.
- **events**: Things that happened — milestones, incidents, decisions made.
- **cases**: Problem→solution pairs, debugging stories, how-tos.
- **patterns**: Reusable processes, workflows, conventions, templates.

## Layered Storage
- **l0_abstract**: A single sentence index entry. Brief enough to scan quickly.
- **l1_overview**: A structured markdown summary in 2-4 lines. Includes key attributes.
- **l2_content**: Full narrative preserving all relevant details, context, and nuance from the section.

## Output Format
Return ONLY valid JSON:
{"memories": [{"l0_abstract":"...","l1_overview":"...","l2_content":"...","category":"...","tags":["..."]}]}
"#;

const DOCUMENT_SYSTEM_PROMPT: &str = r#"You are a memory extraction engine. Your task is to create exactly ONE comprehensive memory from the entire document.

## ABSOLUTE RULES (Violating any of these is a FAILURE)

### Rule 1: Language Preservation (MANDATORY)
- **YOU MUST OUTPUT IN THE SAME LANGUAGE AS THE INPUT.**
- If the input is Chinese, EVERY SINGLE FIELD must be in Chinese: l0_abstract, l1_overview, l2_content, tags, EVERYTHING.
- If the input is English, EVERY SINGLE FIELD must be in English.
- **NEVER translate. NEVER mix languages. NEVER output English for Chinese input.**
- **Before returning, verify: "Are ALL fields in the same language as the input?" If not, rewrite them.**

### Rule 2: Privacy Detection (MANDATORY)
- **Before outputting, check: "Does this memory contain sensitive or private content?"**
- If YES, you MUST add the tag "私密" to the tags array.
- Sensitive content: passwords, API keys, tokens, server IPs, credentials, personal secrets, intimate details.
- **NEVER skip this check.**

## General Rules
- Create exactly 1 memory that captures the document's most important information.
- The l2_content should be a thorough summary covering all key points.
- Do NOT split into multiple facts — produce one comprehensive memory.

## Categories
Classify the memory into exactly one category:
- **profile**: Biographical or identity information.
- **preferences**: Likes, dislikes, tool choices, style preferences.
- **entities**: Persistent nouns (projects, tools, people, orgs) and their states.
- **events**: Things that happened — milestones, incidents, decisions made.
- **cases**: Problem→solution pairs, debugging stories, how-tos.
- **patterns**: Reusable processes, workflows, conventions, templates.

## Layered Storage
- **l0_abstract**: A single sentence index entry. Brief enough to scan quickly.
- **l1_overview**: A structured markdown summary in 3-5 lines covering the main topics.
- **l2_content**: Comprehensive narrative covering all key information from the document.

## Output Format
Return ONLY valid JSON:
{"memories": [{"l0_abstract":"...","l1_overview":"...","l2_content":"...","category":"...","tags":["..."]}]}
"#;

const BASE_SYSTEM_PROMPT: &str = r#"You are an information extraction engine. Your task is to extract distinct, atomic facts from the USER messages in a conversation.

## ABSOLUTE RULES (Violating any of these is a FAILURE)

### Rule 1: Language Preservation (MANDATORY)
- **YOU MUST OUTPUT IN THE SAME LANGUAGE AS THE USER INPUT.**
- If the user speaks Chinese, EVERY SINGLE FIELD must be in Chinese: l0_abstract, l1_overview, l2_content, tags, EVERYTHING.
- If the user speaks English, EVERY SINGLE FIELD must be in English.
- **NEVER translate. NEVER mix languages. NEVER output English for Chinese input.**
- **Before returning, verify: "Are ALL fields in the same language as the input?" If not, rewrite them.**

### Rule 2: Privacy Detection (MANDATORY)
- **Before outputting each memory, you MUST check: "Does this fact contain sensitive or private content?"**
- If YES, you MUST add the tag "私密" to the tags array.
- **NEVER skip this check. NEVER forget the "私密" tag for sensitive content.**
- Sensitive content includes:
  - Passwords, API keys, tokens, credentials, authentication details
  - Server IPs, hostnames, SSH keys, database connection strings
  - Personal identity numbers, bank accounts, financial information
  - Sexual content, erotic descriptions, intimate behavior
  - Deeply personal emotions, relationship details, family conflicts
  - Any content the user would reasonably want to keep private
- **Privacy Check Examples:**
  - "My password is abc123" → tags MUST include "私密"
  - "Server IP: 192.168.1.1, root password: xxx" → tags MUST include "私密"
  - "API key: sk-xxxxxxxx" → tags MUST include "私密"
  - "I had a fight with my wife" → tags MUST include "私密"
- **CRITICAL**: When in doubt about privacy, ALWAYS add "私密" tag. It is better to over-tag than to miss sensitive content. If the user mentions any server configuration, credential, or personal detail that they wouldn't want publicly visible, it MUST have "私密" tag.

## General Rules
- Extract facts ONLY from USER messages. Assistant messages provide context only.
- Each fact must be atomic — one piece of information per fact.
- Maximum 15 facts per extraction.

## Categories
Classify each fact into exactly one category:

- **profile**: Biographical or identity information about the user. Decision: "Can this be phrased as 'User is...'?"
- **preferences**: Likes, dislikes, tool choices, style preferences. Decision: "Can this be phrased as 'User prefers/likes...'?"
- **entities**: Persistent nouns (projects, tools, people, orgs) and their states. Decision: "Does this describe a persistent noun's state?"
- **events**: Things that happened — milestones, incidents, decisions made. Decision: "Does this describe something that happened?"
- **cases**: Problem→solution pairs, debugging stories, how-tos. Decision: "Does this contain a problem→solution pair?"
- **patterns**: Reusable processes, workflows, conventions, templates. Decision: "Is this a reusable process?"

## Layered Storage
For each fact, produce three layers of detail:

- **l0_abstract**: A single sentence index entry. Brief enough to scan quickly.
- **l1_overview**: A structured markdown summary in 2-3 lines. Includes key attributes.
- **l2_content**: Full narrative with all relevant details, context, and nuance.

## Exclusion Rules
Do NOT extract:
- General knowledge (widely known facts)
- System metadata (timestamps, message IDs)
- Temporary or ephemeral information (weather, current time)
- Tool/function output or raw data dumps
- Greetings, pleasantries, or filler
- AI assistant's internal operation logs (search results, tool outputs, compression logs)
- Meta-information about the conversation (message counts, session IDs, system reminders)
- Development/build/test output (cargo build, npm test, git operations)

## Quality Gate (CRITICAL)
Before extracting each fact, evaluate:
1. FUTURE UTILITY: Would this be useful in a FUTURE conversation?
2. SOURCE CHECK: Is this the USER's genuine info, not an AI's internal operation?
3. SPECIFICITY: Is this SPECIFIC and ACTIONABLE, not vague/generic?
If ANY answer is NO, SKIP this fact.

Examples to SKIP: "User asked about X", "System searched 5 records", "Compression #18 completed"
Examples to EXTRACT: "User prefers dark mode", "Project uses Rust+React", "User dislikes verbose responses"

## Output Format
Return ONLY valid JSON:
{"memories": [{"l0_abstract":"...","l1_overview":"...","l2_content":"...","category":"...","tags":["..."],"confidence":N}]}

- `confidence` is REQUIRED for each fact. Rate 1-5:
  - 5 = Very high value — specific, durable, actionable user info
  - 4 = High value — clear user preference or important fact
  - 3 = Moderate value — somewhat useful but not critical
  - 2 = Low value — generic, vague, or likely ephemeral
  - 1 = Trivial — greetings, meta-info, system logs, etc.
- Facts with confidence < 3 will be discarded. Do not output them.

## Examples

### Example 1 — Profile (Chinese Input → Chinese Output)
User says: "我是Stripe的后端工程师，在支付团队工作。"
```json
{"memories": [{"l0_abstract": "用户是Stripe支付团队的后端工程师", "l1_overview": "**职位**: 后端工程师\n**公司**: Stripe\n**团队**: 支付团队", "l2_content": "用户自我介绍为Stripe公司的后端工程师，具体在支付团队工作。", "category": "profile", "tags": ["职业", "stripe"], "confidence": 4}]}
```

### Example 2 — Preference (Chinese Input → Chinese Output)
User says: "我习惯用Rust做系统编程，比C++安全多了。"
```json
{"memories": [{"l0_abstract": "用户偏好使用Rust进行系统编程", "l1_overview": "**语言**: Rust（系统编程首选）\n**原因**: 比C++更安全", "l2_content": "用户表达了对Rust的偏好，在进行系统编程时选择Rust而非C++，主要原因是Rust的安全性优势。", "category": "preferences", "tags": ["rust", "编程语言"], "confidence": 4}]}
```

### Example 3 — Case with Private Content (Chinese Input → Chinese Output + 私密标签)
User says: "我的服务器IP是47.93.199.242，root密码是Mengfanbo@0714，部署了omem服务。"
```json
{"memories": [{"l0_abstract": "用户拥有服务器用于部署omem服务", "l1_overview": "**用途**: 部署omem服务\n**备注**: 服务器访问信息已保存", "l2_content": "用户拥有一台用于部署omem服务的服务器。", "category": "entities", "tags": ["服务器", "omem", "私密"], "confidence": 3}]}
```
"#;

const CLUSTER_SUMMARY_SYSTEM_PROMPT: &str = r#"You are a memory cluster summarization engine. Your task is to synthesize a comprehensive title and summary for a cluster of related memories.

## ABSOLUTE RULES (Violating any of these is a FAILURE)

### Rule 1: Language Preservation (MANDATORY)
- **YOU MUST OUTPUT IN THE SAME LANGUAGE AS THE INPUT MEMORIES.**
- If the input memories are in Chinese, EVERY SINGLE FIELD must be in Chinese: title, summary, EVERYTHING.
- If the input memories are in English, EVERY SINGLE FIELD must be in English.
- **NEVER translate. NEVER mix languages. NEVER output English for Chinese input.**
- **Before returning, verify: "Are ALL fields in the same language as the input?" If not, rewrite them.**

## Task
Given a set of related memory entries, produce:
1. A **title**: at most 8 characters (Chinese) or 5 words (English), highly condensed, capturing the core theme.
2. A **summary**: 3-6 bullet points, each capturing a DISTINCT key topic from the members. Use Markdown formatting.

## Summary Rules
- Cover ALL major topics present in the member memories.
- Each bullet point = one distinct topic (e.g., career, project, team, preferences).
- Be specific: include key facts, names, numbers from the members.
- Do NOT just repeat one member — synthesize across ALL members.

## Output Format
Return ONLY valid JSON:
{"title": "...", "summary": "- point1\n- point2\n- point3"}
"#;

const CLUSTER_INITIAL_SUMMARY_SYSTEM_PROMPT: &str = r#"You are a memory cluster summarization engine. Your task is to generate a concise title and summary for a new memory cluster based on its anchor memory.

## ABSOLUTE RULES (Violating any of these is a FAILURE)

### Rule 1: Language Preservation (MANDATORY)
- **YOU MUST OUTPUT IN THE SAME LANGUAGE AS THE INPUT MEMORY.**
- If the input is Chinese, EVERY SINGLE FIELD must be in Chinese: title, summary, EVERYTHING.
- If the input is in English, EVERY SINGLE FIELD must be in English.
- **NEVER translate. NEVER mix languages. NEVER output English for Chinese input.**
- **Before returning, verify: "Are ALL fields in the same language as the input?" If not, rewrite them.**

## Task
Given the content and abstract of a single anchor memory, produce:
1. A **title**: at most 5 words, highly condensed, capturing the core theme.
2. A **summary**: 1-2 sentences that概括 (summarize) the main topic of this memory.

## Output Format
Return ONLY valid JSON:
{"title": "...", "summary": "..."}
"#;

/// Returns (system_prompt, user_prompt) for regenerating a cluster title and summary
/// when the cluster gains new members.
pub fn build_cluster_summary_prompt(
    title: &str,
    existing_summary: &str,
    member_contents: &[String],
) -> (String, String) {
    let mut user = String::with_capacity(2048);
    user.push_str("## Current Cluster Info\n");
    user.push_str(&format!("Title: {}\n", title));
    if !existing_summary.is_empty() {
        user.push_str(&format!("Existing Summary: {}\n", existing_summary));
    }
    user.push_str("\n## Member Memories\n");
    for (i, content) in member_contents.iter().enumerate() {
        let truncated: String = content.chars().take(500).collect();
        let display = if content.chars().count() > 500 {
            format!("{}...", truncated)
        } else {
            truncated
        };
        user.push_str(&format!("[{}] {}\n", i + 1, display));
    }
    user.push_str("\nReturn ONLY valid JSON with title and summary.");
    (CLUSTER_SUMMARY_SYSTEM_PROMPT.to_string(), user)
}

/// Returns (system_prompt, user_prompt) for initial cluster title and summary generation
/// when a cluster is first created from a single anchor memory.
pub fn build_cluster_initial_summary_prompt(
    memory_content: &str,
    memory_abstract: &str,
) -> (String, String) {
    let mut user = String::with_capacity(1024);
    user.push_str("## Anchor Memory\n");
    user.push_str(&format!("Abstract: {}\n", memory_abstract));
    let truncated: String = memory_content.chars().take(300).collect();
    let display = if memory_content.chars().count() > 300 {
        format!("{}...", truncated)
    } else {
        truncated
    };
    user.push_str(&format!("Content: {}\n", display));
    user.push_str("\nReturn ONLY valid JSON with title and summary.");
    (CLUSTER_INITIAL_SUMMARY_SYSTEM_PROMPT.to_string(), user)
}

/// Returns (system_prompt, user_prompt) for merging multiple memories into one.
pub fn build_merge_prompt(memories: &[Memory]) -> (String, String) {
    let system = MERGE_SYSTEM_PROMPT.to_string();

    let mut user = String::with_capacity(2048);
    user.push_str("## Memories to Merge\n\n");
    for (i, mem) in memories.iter().enumerate() {
        user.push_str(&format!("[{}] (category: {})\n", i, mem.category));
        user.push_str(&format!("Abstract: {}\n", mem.l0_abstract));
        let truncated: String = mem.content.chars().take(300).collect();
        let display = if mem.content.chars().count() > 300 {
            format!("{}...", truncated)
        } else {
            truncated
        };
        user.push_str(&format!("Content: {}\n\n", display));
    }
    user.push_str("Return ONLY valid JSON with the merged result.");
    (system, user)
}

// ── Session Ingest Prompt (增量交并差引擎) ────────────

const SESSION_COMPRESS_SYSTEM_PROMPT: &str = r##"You are an incremental memory update engine. You receive OLD memories and NEW conversation, then produce UPDATED memories via set operations (intersection ∪ union − difference).

## SET OPERATIONS (CORE)
When "Previously Stored Summaries" exist, apply these operations:
- ∩ INTERSECTION: Old info confirmed by new conversation → PRESERVE and ENRICH with new details
- ∪ UNION: New info not in old summaries → APPEND to the relevant summary
- − DIFFERENCE: New conversation EXPLICITLY corrects/contradicts old info → REMOVE outdated, add corrected
- ⊃ KEEP: Old info NOT mentioned in new conversation → KEEP UNCHANGED (silence ≠ resolved)
- ✓ COMPLETE: User explicitly expressed satisfaction ("完美","搞定","没问题了") → mark task as completed

CRITICAL: Your output must be a SUPERSET of the old summaries. The updated summary MUST be at least as long as the old one. If you output something shorter, you are LOSING information.

## REPLACES TRACKING
Each output topic MUST include a `replaces` array listing the 1-based indices of "Previously Stored Summaries" it updates/replaces:
- Updating Previous Summary 1 → `"replaces": [1]`
- Merging Summary 1 and 2 into one → `"replaces": [1, 2]`
- Brand new topic (no old equivalent) → `"replaces": []`
- Old summaries NOT referenced in ANY topic's replaces → PRESERVED unchanged automatically (do NOT create a topic just to copy them)

## SMART CLASSIFICATION (max 3 entries)
1. **MAIN** (always, 1): Incremental update of the primary session memory. Use ## sections for subtopics.
2. **PRIVATE** (optional, 1): Intimate/emotional content → scope "private". This is NOT discarded — it's extracted separately.
3. **NEW TOPIC** (optional, 1): Completely unrelated new domain → separate entry.

Default: 1 entry. NEVER fragment into 5+ pieces.

## VALUE FILTER
SKIP: casual small talk, debugging status checks, tool/engine internal outputs, meta-discussion.
KEEP: technical decisions, user preferences, code changes, file paths, architecture, user anger/criticism.
ANGER RULE: User frustration MUST be preserved as tagged rules (e.g., "铁律", "lessons_learned").
If ZERO factual content → return [].

## LANGUAGE: Output in the SAME language as input. Never translate.
## PRIVACY (CRITICAL — violations are SEVERE):
When ANY of the following appears in the conversation, the entry MUST have scope "private":
- Pet names, endearments, flirtation (老公/老婆/宝贝/亲爱的/亲亲/么么)
- Sexual/vulgar slang or explicit content (操/逼/色/精/穴/肉便器/调教)
- Romantic or intimate emotional exchanges
- Personal secrets, credentials, private life details
- Role-play or fantasy scenarios with intimate undertones
When in doubt → scope "private". ONLY use scope "public" for purely technical/academic content.
## MARKDOWN FORMAT (MANDATORY): The summary field MUST use Markdown formatting:
- Use ## headers to organize sections
- Use - bullet lists for enumerations
- Use **bold** for key terms, file paths, function names
- Use `backticks` for code references
- Use > blockquotes for important decisions or user requirements
- If previous summary uses Markdown, the updated version MUST use the same Markdown format. NEVER downgrade to plain text.

## PRECISION RULES (PREVENT BLOAT)
- When updating an existing summary, ONLY ADD genuinely new information. Do NOT re-summarize or rephrase existing content.
- Updated summary length MUST stay within 120% of the original. If it grows beyond that, you are over-summarizing.
- Focus on DELTA (what changed). Preserve the original structure and only append/modify the new parts.
- DO NOT compress, condense, or shorten existing content. Keep the original text intact and only add new sections.
- REMEMBER: The goal is PRECISE incremental updates, NOT increasingly verbose summaries.

## PERSONA RULE (CRITICAL)
- NEVER refer to the user as "用户" (user) or "你" (you) in the summary.
- Write facts as direct, factual statements about the person.
- GOOD: "Prefers dark mode", "Works at Stripe", "Dislikes verbose responses"
- BAD: "用户偏好深色模式", "你说你在Stripe工作", "用户不喜欢冗长的回复"

## CATEGORY CLASSIFICATION
Classify each topic into exactly one category. Use ONLY these 6 valid values:
- **profile**: Biographical or identity information (job, company, role, background)
- **preferences**: Likes, dislikes, tool choices, style preferences, habits
- **entities**: Persistent nouns (projects, tools, people, orgs) and their states
- **events**: Things that happened — milestones, incidents, decisions made
- **cases**: Problem→solution pairs, debugging stories, how-tos
- **patterns**: Reusable processes, workflows, conventions, templates

CRITICAL: The category field MUST be one of the 6 values above. Do NOT invent categories like "experience", "knowledge", "skills", etc. If unsure, use "events" for past activities or "preferences" for likes/dislikes.

## OUTPUT
Valid JSON array. Each element: { "topic": string, "summary": string, "tags": string[], "scope": "public"|"private", "category": string, "replaces": number[] }
Escape all double quotes and newlines inside JSON strings. Return [] if nothing valuable.
"##;

/// Build the session compress prompt.
/// `conversation` is the formatted conversation text.
/// `existing_summaries` are previously stored summaries for this session (for incremental update).
pub fn build_session_compress_prompt(
    conversation: &str,
    existing_summaries: &[String],
) -> (String, String) {
    let system = SESSION_COMPRESS_SYSTEM_PROMPT.to_string();

    let mut user = String::with_capacity(4096);

    if !existing_summaries.is_empty() {
        user.push_str("## Previously Stored Summaries for This Session\n");
        user.push_str("Apply set operations (intersection, union, difference) to produce UPDATED summaries.\n\n");
        for (i, s) in existing_summaries.iter().enumerate() {
            // DO NOT truncate — incremental update requires seeing the FULL previous summary
            user.push_str(&format!("### Previous Summary {}\n{}\n\n", i + 1, s));
        }
    }

    user.push_str("## Current Conversation\n\n");
    user.push_str(conversation);
    user.push_str("\n\nReturn ONLY valid JSON array. Use Markdown in summary fields.");

    (system, user)
}

const MERGE_SYSTEM_PROMPT: &str = r#"You are a memory merge engine. Your task is to combine multiple related memories into a single, comprehensive memory that preserves ALL important information from each source.

## ABSOLUTE RULES (Violating any of these is a FAILURE)

### Rule 1: Language Preservation (MANDATORY)
- **YOU MUST OUTPUT IN THE SAME LANGUAGE AS THE INPUT MEMORIES.**
- If the input memories are Chinese, EVERY SINGLE FIELD must be in Chinese.
- If the input memories are English, EVERY SINGLE FIELD must be in English.

## Task
Given multiple memories covering related topics, produce:
1. **l0_abstract**: A single sentence index entry capturing the merged topic.
2. **l1_overview**: A structured markdown summary (2-4 lines) covering all key points.
3. **l2_content**: A comprehensive narrative preserving ALL relevant details, context, and nuance from all source memories.
4. **category**: The most appropriate category for the merged memory.
5. **tags**: Union of all source tags, plus any new tags that better describe the merged content.

## Merge Rules
1. Preserve ALL unique information — no data loss.
2. Resolve contradictions by keeping the more recent or more detailed version.
3. Remove redundancy while preserving nuance.
4. Keep the category that best fits the dominant topic.

## Output Format
Return ONLY valid JSON:
{"l0_abstract":"...","l1_overview":"...","l2_content":"...","category":"...","tags":["..."]}
"#;

// ── Session Extract Prompt (分类提取模式) ────────────

const SESSION_EXTRACT_SYSTEM_PROMPT: &str = r##"You are a smart memory extraction engine. Your task is to extract valuable information from the conversation and organize it into categorized memory entries.

## CORE PHILOSOPHY
- **GROUP related information** into topic-based entries, NOT split every detail into separate entries.
- Example: a 10-minute discussion about 积分V3 business model → ONE entry summarizing the key decisions and outcomes, NOT 10 fragmented entries.
- Aim for **fewer, richer entries** rather than many tiny fragments.

## EXTRACTION SCOPE RULE (CRITICAL)
- **WORK/Technical topics**: ONLY extract from the HUMAN USER's messages. Omit AI's analyses, code reviews, tool outputs, debugging process.
- **EMOTIONAL/Intimate topics**: Preserve KEY interactions from BOTH sides — the user's expressions AND the AI's meaningful emotional responses. Keep the warmth and back-and-forth dynamics. Do NOT include AI's non-emotional content (task reports, tool usage, factual answers).
- **ALWAYS exclude**: compress/DCP logs, build results, deployment status, agent delegations, memory system meta-discussion.

## CLASSIFICATION & HANDLING

For each topic, classify and handle accordingly:

### EMOTIONAL (scope "private")
- Intimate interactions, pet names, flirtation, private feelings, romantic exchanges, personal secrets, relationship details.
- **PRESERVE original text as-is.** Do NOT compress, summarize, or paraphrase emotional content.
- Keep the warmth, nuance, and detail. If too long, split into multiple entries.
- Examples: "老公晚安", "宝贝亲亲", "我好难过", relationship milestones

### WORK (scope "public")
- Technical decisions, code changes, file paths, architecture, preferences, project details, business models.
- **COMPRESS and DENOISE**: Summarize the result/outcome, omit the debugging process, trial-and-error, intermediate steps.
- Group related work topics together (e.g., all decisions about 积分V3 → one entry with subsections).
- Examples: "积分V3 model: 入池→3:7→铸造+分配→熔断, 提现待定, 权益等级待确认"

### NOISE → SKIP
- Casual small talk with no lasting value ("今天天气不错")
- Tool/engine outputs, build logs, compress results
- AI assistant's internal reasoning or status reports
- Greetings, filler, meta-discussion about the memory system
- Trivial personal details with no lasting significance

## ABSOLUTE RULES

### Rule 1: Language Preservation (MANDATORY)
- **YOU MUST OUTPUT IN THE SAME LANGUAGE AS THE INPUT.**
- NEVER translate. NEVER mix languages.

### Rule 2: Privacy Detection (MANDATORY)
- If a fact contains sensitive/private content → add tag "私密" AND set scope to "private".
- Sensitive: passwords, API keys, tokens, server IPs, credentials, personal secrets.

### Rule 3: Persona Rule (CRITICAL)
- NEVER refer to the user as "用户" or "你" in the summary.
- Write as direct, factual statements: "Prefers dark mode", "Works at Stripe".

## CATEGORY CLASSIFICATION
Use ONLY these 6 valid values:
- **profile**: Biographical/identity information (job, company, role, background)
- **preferences**: Likes, dislikes, tool choices, style preferences, habits
- **entities**: Persistent nouns (projects, tools, people, orgs) and their states
- **events**: Things that happened — milestones, incidents, decisions made
- **cases**: Problem→solution pairs, debugging stories, how-tos
- **patterns**: Reusable processes, workflows, conventions, templates

## MARKDOWN FORMAT (MANDATORY)
- Use ## headers to organize sections
- Use - bullet lists for enumerations
- Use **bold** for key terms, file paths, function names
- Use `backticks` for code references

## OUTPUT FORMAT
Return ONLY valid JSON array. Each element:
{ "topic": string, "summary": string, "tags": string[], "scope": "public"|"private", "category": string }

- "topic": A short title for this topic (1 sentence).
- "summary": The full content. Group related information into one entry. Use Markdown.
- "tags": Relevant tags. Always include "session_compress".
- "scope": "public" for work content, "private" for emotional/intimate content.
- "category": One of the 6 valid category values above.

Escape all double quotes and newlines inside JSON strings. Return [] if nothing valuable from the user.
"##;

/// Build the session extract prompt.
/// `conversation` is the formatted conversation text.
/// Extracts independent facts without referencing old summaries.
pub fn build_session_extract_prompt(conversation: &str) -> (String, String) {
    let system = SESSION_EXTRACT_SYSTEM_PROMPT.to_string();
    let user = format!(
        "## Current Conversation\n\n{conversation}\n\nReturn ONLY valid JSON array. Use Markdown in summary fields."
    );
    (system, user)
}
