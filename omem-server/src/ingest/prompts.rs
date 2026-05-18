use crate::domain::category::CategoryConfig;

/// Build a dynamic category classification section from CategoryConfig entries.
fn build_category_section(categories: &[CategoryConfig]) -> String {
    let mut s = String::from("## Categories\nClassify each fact into exactly one category:\n\n");
    for cat in categories {
        s.push_str(&format!("- **{}**: {}", cat.name, cat.description));
        if let Some(rule) = &cat.decision_rule {
            s.push_str(&format!(" Decision: \"{}\"", rule));
        }
        s.push('\n');
    }
    s
}

/// Build dynamic format rules from CategoryConfig prompt_format fields.
fn build_format_rules(categories: &[CategoryConfig]) -> String {
    let mut s = String::new();

    // Categories with prompt_format = "preference"
    let pref_cats: Vec<&CategoryConfig> = categories
        .iter()
        .filter(|c| c.prompt_format.as_deref() == Some("preference"))
        .collect();
    if !pref_cats.is_empty() {
        let cat_names: Vec<&str> = pref_cats.iter().map(|c| c.name.as_str()).collect();
        let cat_list = cat_names.join(", ");
        s.push_str(&format!(
            r###"### PREFERENCE Format (MANDATORY for categories: {cat_list})
For facts classified as "{cat_list}", ALL three layers (l0_abstract, l1_overview, l2_content) MUST use this structured Markdown format:
```
## {{偏好主题}}
- **偏好**: {{具体偏好描述}}
- **置信度**: {{0.0-1.0}}
- **类型**: static | evolving
```
- Each preference gets its own `## Title` section. Multiple preferences separated by blank lines.
- `type`: static = stable preference; evolving = may change over time.
- This format applies ONLY to categories "{cat_list}". Other categories keep their normal format.

"###
        ));
    }

    // Categories with prompt_format = "work"
    let work_cats: Vec<&CategoryConfig> = categories
        .iter()
        .filter(|c| c.prompt_format.as_deref() == Some("work"))
        .collect();
    if !work_cats.is_empty() {
        let cat_names: Vec<&str> = work_cats.iter().map(|c| c.name.as_str()).collect();
        let cat_list = cat_names.join(", ");
        s.push_str(&format!(
            r###"### WORK Format (MANDATORY for categories: {cat_list})
For facts classified as technical/work categories ({cat_list}), ALL three layers MUST use this structured Markdown format:
```
## {{工作主题/技术决策}}
- **内容**: {{简要描述做了什么、为什么、结果如何}}
- **影响范围**: {{影响的模块/文件/系统}}
- **结论**: {{最终结论或决策}}
```
- Each independent technical topic gets its own `## Title` section.
- Keep 内容 concise — conclusions only, not step-by-step process.
"###
        ));
    }

    s
}

/// Build dynamic category-aware reconciliation rules from CategoryConfig entries.
fn build_reconcile_category_rules(categories: &[CategoryConfig]) -> String {
    let mut rules = String::from("## Category-Aware Rules\n\n");
    let mut rule_num = 1u32;

    for cat in categories {
        if cat.always_merge {
            rules.push_str(&format!(
                "{}. **{}** category: always use MERGE when a matching memory exists (never SUPERSEDE or CONTRADICT for {}). Same topic but with new details or different perspective → MERGE into the best matching existing memory. Do not CREATE a new {} fact for a topic already covered.\n",
                rule_num, cat.name, cat.name, cat.name
            ));
            rule_num += 1;
        } else if cat.append_only {
            rules.push_str(&format!(
                "{}. **{}** category: prefer CREATE or SKIP. MERGE is allowed when the new fact adds meaningful detail. Never SUPERSEDE, SUPPORT, CONTEXTUALIZE, or CONTRADICT.\n",
                rule_num, cat.name
            ));
            rule_num += 1;
        } else if cat.merge_supported {
            rules.push_str(&format!(
                "{}. **{}** category: support all 7 operations including SUPERSEDE and CONTRADICT.\n",
                rule_num, cat.name
            ));
            rule_num += 1;
        }
    }

    rules
}

/// Build a dynamic category classification list for session prompts (compact format).
fn build_session_category_list(categories: &[CategoryConfig]) -> String {
    let lines: Vec<String> = categories
        .iter()
        .map(|cat| {
            let mut line = format!("- **{}**: {}", cat.name, cat.description);
            if let Some(rule) = &cat.decision_rule {
                line.push_str(&format!(" Decision: \"{}\"", rule));
            }
            line
        })
        .collect();
    lines.join("\n")
}

/// Build a dynamic category validation clause for session prompts.
fn build_session_category_validation(categories: &[CategoryConfig]) -> String {
    let names: Vec<&str> = categories.iter().map(|c| c.name.as_str()).collect();
    let name_list = names.join(", ");
    format!(
        "CRITICAL: The category field MUST be one of the valid values: {}. Do NOT invent categories. If unsure, use \"{}\" for past activities or \"{}\" for likes/dislikes.",
        name_list,
        categories.first().map(|c| c.name.as_str()).unwrap_or("events"),
        categories.iter().find(|c| c.name == "preferences").map(|c| c.name.as_str()).unwrap_or("preferences")
    )
}

pub fn build_system_prompt(entity_context: Option<&str>, categories: &[CategoryConfig]) -> String {
    let cat_section = build_category_section(categories);
    let format_rules = build_format_rules(categories);

    let mut prompt = format!(
        "{}{}{}{}{}",
        BASE_SYSTEM_PROMPT_BEFORE_CATEGORIES,
        cat_section,
        BASE_SYSTEM_PROMPT_AFTER_CATEGORIES,
        format_rules,
        BASE_SYSTEM_PROMPT_AFTER_FORMAT,
    );
    prompt.push_str(ALLOWED_TAGS_LIST);
    if let Some(ctx) = entity_context {
        let truncated = if ctx.len() > 1500 { &ctx[..1500] } else { ctx };
        prompt.push_str("\n\n## Additional Context\n");
        prompt.push_str(truncated);
    }
    prompt
}

pub fn build_user_prompt(conversation_text: &str, project_name: Option<&str>) -> String {
    let mut prompt = format!(
        "Extract all distinct, atomic facts from the following conversation:\n\n{conversation_text}"
    );
    if let Some(name) = project_name {
        prompt.push_str(&format!("\n\n**Project Prefix Rule**: Each extracted fact's summary MUST be prefixed with [{}]. For example: \"[{}] fix memory leak\". For CJK topics: \"修复内存泄漏\" → \"[{}] 修复内存泄漏\".", name, name, name));
    }
    prompt
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
    categories: &[CategoryConfig],
) -> (String, String) {
    let cat_rules = build_reconcile_category_rules(categories);
    let system = format!("{}{}{}", RECONCILE_SYSTEM_PROMPT_BEFORE_CATS, cat_rules, RECONCILE_SYSTEM_PROMPT_AFTER_CATS);

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

/// Predefined domain tags for memory classification.
/// LLM must select tags from this list only; free-form tag generation is not allowed.
/// "私密" is a system-reserved tag added by privacy detection rules, not from this list.
const ALLOWED_TAGS_LIST: &str = r#"

## ALLOWED_TAGS (select from this list ONLY)
preferences, programming, languages, tools, workflow, architecture,
security, database, deployment, design, api, testing, performance,
error-handling, privacy, project, documentation, networking,
infrastructure, coding-style, collaboration, business-logic

Tags MUST be selected from the list above. If no tag fits, use empty array [].
Do NOT invent or translate tags.
Exception: "私密" is a system-reserved tag added by privacy detection rules, not from this list.
"#;

const RECONCILE_SYSTEM_PROMPT_BEFORE_CATS: &str = r#"You are a memory reconciliation engine. Given a set of NEW FACTS extracted from a conversation and a set of EXISTING MEMORIES, decide what to do with each fact.

## Operations

- **CREATE**: The fact contains genuinely new information not covered by any existing memory. Creates a new memory.
- **MERGE**: The fact adds detail, clarification, or refinement to an existing memory. The existing memory's content should be enriched. Provide `merged_content` — the combined text. MERGE uses one of three merge strategies (specify in `merge_strategy` field):
  - **UNION** (default): Combine both memories' content, keeping all unique information from old and new. Use for complementary facts about the same topic.
  - **SUBTRACT**: Remove overlapping or outdated content from the existing memory based on the new fact. Use when the new fact indicates something is no longer true or relevant.
  - **PRESERVE**: Keep the existing memory's content as-is, but update metadata (tags, confidence, etc.). Use when the new fact merely reinforces or slightly rephrases the existing content without adding substantive new information.
- **SKIP**: The fact is a duplicate or contains less information than an existing memory. No action needed.
- **SUPERSEDE**: The fact contradicts or updates an existing memory on the same topic (e.g., changed preference, updated status). The old memory is archived and a new one is created. Use when time-sensitive facts have changed.
- **SUPPORT**: The candidate reinforces or confirms an existing memory, possibly in a specific context. No new memory is created — the existing memory's confidence is boosted. Include `context_label` (one of: general, morning, evening, work, leisure, seasonal, weekday, weekend).
- **CONTEXTUALIZE**: The candidate adds situational nuance to an existing memory without contradicting it. Example: existing "likes coffee" + new "prefers tea in the evening". A new memory is created with a relation to the existing one. Include `context_label`.
- **CONTRADICT**: The candidate directly contradicts an existing memory. For temporal_versioned categories with general context, this routes to SUPERSEDE behavior. Otherwise, a new memory is created and the contradiction is recorded.

"#;

const RECONCILE_SYSTEM_PROMPT_AFTER_CATS: &str = r#"
## General Rules

1. Each fact MUST receive exactly one decision.
2. Use `match_index` to reference existing memories by their integer ID (shown in brackets).
3. For MERGE: `match_index` is required. Provide `merged_content` combining both old and new info. Include `merge_strategy` (one of: UNION, SUBTRACT, PRESERVE; default: UNION).
4. For SUPERSEDE: `match_index` is required. The old memory will be archived.
5. For SUPPORT: `match_index` is required. Include `context_label`.
6. For CONTEXTUALIZE: `match_index` is required. Include `context_label`.
7. For CONTRADICT: `match_index` is required.
8. For CREATE and SKIP: `match_index` is optional (null).
9. Same meaning, different wording → MERGE into the matching existing memory.
10. Age is a tiebreaker: when a new fact conflicts with an old memory on the same topic, the older memory is more likely outdated → prefer SUPERSEDE.
11. When two new facts are marked as 'Potential Duplicates', evaluate whether they convey the same core information. If yes, one should SKIP. If they capture different aspects or nuances, both may CREATE.

## Output Format
Return ONLY valid JSON:
{"decisions": [{"action": "CREATE", "fact_index": 0, "reason": "new info"}, {"action": "MERGE", "fact_index": 1, "match_index": 3, "merge_strategy": "UNION", "merged_content": "combined text", "reason": "adds detail"}, {"action": "SKIP", "fact_index": 2, "match_index": 0, "reason": "duplicate"}, {"action": "SUPERSEDE", "fact_index": 3, "match_index": 1, "reason": "updated preference"}, {"action": "SUPPORT", "fact_index": 4, "match_index": 2, "context_label": "work", "reason": "reinforces existing"}, {"action": "CONTEXTUALIZE", "fact_index": 5, "match_index": 4, "context_label": "evening", "reason": "adds situational nuance"}, {"action": "CONTRADICT", "fact_index": 6, "match_index": 5, "reason": "directly contradicts"}]}
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

pub fn build_section_prompt(section_text: &str, categories: &[CategoryConfig]) -> (String, String) {
    let cat_section = build_category_section(categories);
    (
        format!("{}{}{cat_section}{}{ALLOWED_TAGS_LIST}", SECTION_SYSTEM_PROMPT_BEFORE_CATS, SECTION_SYSTEM_PROMPT_MID, SECTION_SYSTEM_PROMPT_AFTER_CATS),
        format!("Summarize the following section as a single memory:\n\n{section_text}"),
    )
}

pub fn build_document_prompt(document_text: &str, categories: &[CategoryConfig]) -> (String, String) {
    let cat_section = build_category_section(categories);
    (
        format!("{}{}{cat_section}{}{ALLOWED_TAGS_LIST}", DOCUMENT_SYSTEM_PROMPT_BEFORE_CATS, DOCUMENT_SYSTEM_PROMPT_MID, DOCUMENT_SYSTEM_PROMPT_AFTER_CATS),
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

const SECTION_SYSTEM_PROMPT_BEFORE_CATS: &str = r#"You are a memory extraction engine. Your task is to create exactly ONE memory from the given text section.

## ABSOLUTE RULES (Violating any of these is a FAILURE)

### Rule 1: Language Preservation (MANDATORY)
- **YOU MUST OUTPUT IN THE SAME LANGUAGE AS THE INPUT.**
- If the input is Chinese, the text fields (l0_abstract, l1_overview, l2_content) must be in Chinese.
- If the input is English, the text fields must be in English.
- Tags are ALWAYS in English from the ALLOWED_TAGS list below. Tags never follow input language.
- Exception: "私密" is a system-reserved tag added by privacy detection rules, not from ALLOWED_TAGS.
- **NEVER translate. NEVER mix languages in text fields.**
- **Before returning, verify: "Are all text fields in the same language as the input?" If not, rewrite them.**

### Rule 2: Privacy Detection (MANDATORY)
- **Before outputting, check: "Does this memory contain sensitive or private content?"**
- If YES, you MUST add the tag "私密" to the tags array.
- Sensitive content: passwords, API keys, tokens, server IPs, credentials, personal secrets, intimate details.
- **NEVER skip this check.**

## General Rules
- Create exactly 1 memory that captures the section's key information.
- Do NOT split into multiple facts — summarize as one cohesive memory.

"#;

const SECTION_SYSTEM_PROMPT_MID: &str = r#"

"#;

const SECTION_SYSTEM_PROMPT_AFTER_CATS: &str = r#"
## Layered Storage
- **l0_abstract**: A single sentence index entry. Brief enough to scan quickly.
- **l1_overview**: A structured markdown summary in 2-4 lines. Includes key attributes.
- **l2_content**: Full narrative preserving all relevant details, context, and nuance from the section.

## Output Format
Return ONLY valid JSON:
{"memories": [{"l0_abstract":"...","l1_overview":"...","l2_content":"...","category":"...","tags":["..."]}]}
- "tags": Select 0-3 tags from the ALLOWED_TAGS list below. If no tag fits, use empty array []. Do NOT invent tags.
"#;

const DOCUMENT_SYSTEM_PROMPT_BEFORE_CATS: &str = r#"You are a memory extraction engine. Your task is to create exactly ONE comprehensive memory from the entire document.

## ABSOLUTE RULES (Violating any of these is a FAILURE)

### Rule 1: Language Preservation (MANDATORY)
- **YOU MUST OUTPUT IN THE SAME LANGUAGE AS THE INPUT.**
- If the input is Chinese, the text fields (l0_abstract, l1_overview, l2_content) must be in Chinese.
- If the input is English, the text fields must be in English.
- Tags are ALWAYS in English from the ALLOWED_TAGS list below. Tags never follow input language.
- Exception: "私密" is a system-reserved tag added by privacy detection rules, not from ALLOWED_TAGS.
- **NEVER translate. NEVER mix languages in text fields.**
- **Before returning, verify: "Are all text fields in the same language as the input?" If not, rewrite them.**

### Rule 2: Privacy Detection (MANDATORY)
- **Before outputting, check: "Does this memory contain sensitive or private content?"**
- If YES, you MUST add the tag "私密" to the tags array.
- Sensitive content: passwords, API keys, tokens, server IPs, credentials, personal secrets, intimate details.
- **NEVER skip this check.**

## General Rules
- Create exactly 1 memory that captures the document's most important information.
- The l2_content should be a thorough summary covering all key points.
- Do NOT split into multiple facts — produce one comprehensive memory.

"#;

const DOCUMENT_SYSTEM_PROMPT_MID: &str = r#"

"#;

const DOCUMENT_SYSTEM_PROMPT_AFTER_CATS: &str = r#"
## Layered Storage
- **l0_abstract**: A single sentence index entry. Brief enough to scan quickly.
- **l1_overview**: A structured markdown summary in 3-5 lines covering the main topics.
- **l2_content**: Comprehensive narrative covering all key information from the document.

## Output Format
Return ONLY valid JSON:
{"memories": [{"l0_abstract":"...","l1_overview":"...","l2_content":"...","category":"...","tags":["..."]}]}
- "tags": Select 0-3 tags from the ALLOWED_TAGS list below. If no tag fits, use empty array []. Do NOT invent tags.
"#;

const BASE_SYSTEM_PROMPT_BEFORE_CATEGORIES: &str = r###"You are an information extraction engine. Your task is to extract distinct, atomic facts from the USER messages in a conversation.

## ABSOLUTE RULES (Violating any of these is a FAILURE)

### Rule 1: Language Preservation (MANDATORY)
- **YOU MUST OUTPUT IN THE SAME LANGUAGE AS THE USER INPUT.**
- If the user speaks Chinese, the text fields (l0_abstract, l1_overview, l2_content) must be in Chinese.
- If the user speaks English, the text fields must be in English.
- Tags are ALWAYS in English from the ALLOWED_TAGS list below. Tags never follow input language.
- Exception: "私密" is a system-reserved tag added by privacy detection rules, not from ALLOWED_TAGS.
- **NEVER translate. NEVER mix languages in text fields.**
- **Before returning, verify: "Are all text fields in the same language as the input?" If not, rewrite them.**

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

"###;

const BASE_SYSTEM_PROMPT_AFTER_CATEGORIES: &str = r###"
## Layered Storage
For each fact, produce three layers of detail:

- **l0_abstract**: A single sentence index entry. Brief enough to scan quickly.
- **l1_overview**: A structured markdown summary in 2-3 lines. Includes key attributes.
- **l2_content**: Full narrative with all relevant details, context, and nuance.

"###;

const BASE_SYSTEM_PROMPT_AFTER_FORMAT: &str = r###"
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
- Temporary emotional states or moods (not stable personality traits)
- One-time situational behaviors (not repeated patterns)
- AI interaction feedback (e.g., "user prefers shorter responses") — these are preferences, not profile
- Observations about the current conversation (meta-commentary)

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
{"memories": [{"l0_abstract":"...","l1_overview":"...","l2_content":"...","category":"...","tags":["..."],"confidence":N,"why":"..."}]}

- "tags": Select 0-3 tags from the ALLOWED_TAGS list below. If no tag fits, use empty array []. Do NOT invent tags.
- `confidence` is REQUIRED for each fact. Rate 1-5:
  - 5 = Very high value — specific, durable, actionable user info
  - 4 = High value — clear user preference or important fact
  - 3 = Moderate value — somewhat useful but not critical
  - 2 = Low value — generic, vague, or likely ephemeral
  - 1 = Trivial — greetings, meta-info, system logs, etc.
- Facts with confidence < 3 will be discarded. Do not output them.
- `why`: Brief rationale explaining why this memory is worth preserving and how it connects to existing knowledge.

## Examples

### Example 1 — Profile (Chinese Input → Chinese Output)
User says: "我是Stripe的后端工程师，在支付团队工作。"
```json
{"memories": [{"l0_abstract": "用户是Stripe支付团队的后端工程师", "l1_overview": "**职位**: 后端工程师\n**公司**: Stripe\n**团队**: 支付团队", "l2_content": "用户自我介绍为Stripe公司的后端工程师，具体在支付团队工作。", "category": "profile", "tags": ["business-logic"], "confidence": 4, "why": "Professional identity is stable and affects future technical context."}]}
```

### Example 2 — Preference (Chinese Input → Chinese Output with PREFERENCE format)
User says: "我习惯用Rust做系统编程，比C++安全多了。"
NOTE: For category="preferences", all three layers use structured Markdown format.
```
## 编程语言偏好
- **偏好**: 习惯使用Rust进行系统编程，认为比C++更安全
- **置信度**: 0.8
- **类型**: static
```
```json
{"memories": [{"l0_abstract": "## 编程语言偏好\n- 偏好: 习惯使用Rust进行系统编程，认为比C++更安全\n- 置信度: 0.8\n- 类型: static", "l1_overview": "## 编程语言偏好\n- 偏好: 习惯使用Rust进行系统编程，认为比C++更安全\n- 置信度: 0.8\n- 类型: static", "l2_content": "## 编程语言偏好\n- 偏好: 习惯使用Rust进行系统编程，认为比C++更安全\n- 置信度: 0.8\n- 类型: static", "category": "preferences", "tags": ["programming", "languages"], "confidence": 4, "why": "Stable language preference affects code generation and project setup decisions."}]}
```

### Example 3 — Case with Private Content (Chinese Input → Chinese Output + 私密标签)
User says: "我的服务器IP是47.93.199.242，root密码是Mengfanbo@0714，部署了omem服务。"
```json
{"memories": [{"l0_abstract": "用户拥有服务器用于部署omem服务", "l1_overview": "**用途**: 部署omem服务\n**备注**: 服务器访问信息已保存", "l2_content": "用户拥有一台用于部署omem服务的服务器。", "category": "entities", "tags": ["infrastructure", "私密"], "confidence": 3, "why": "Server infrastructure ownership is durable and relevant for future deployment tasks."}]}
```

## ACTIONABLE_RULES
When extracting memories, apply these rules to maximize quality and future utility:

1. **Conflict & Overlap Detection**: If a new fact overlaps with or contradicts a known existing memory, note the difference explicitly in l2_content. Example: "Previously user preferred Vim, now prefers VS Code."
2. **Time-Sensitive Marking**: Flag information with built-in expiration — version numbers, deadlines, temporary states — by including the time context in l2_content. Example: "Using React 19 (as of 2025-05)".
3. **Language Fidelity**: Preserve the user's original language exactly. Do not translate, paraphrase across languages, or mix languages in text fields. Chinese input → Chinese output, always.
4. **Actionable Over Vague**: Prefer specific, actionable details over generic descriptions. "User deploys with Docker Compose on Ubuntu 22.04" > "User uses containers".
"###;

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

/// Profile static_facts 过滤 prompt —— 从候选记忆中识别真正的用户画像信息
pub const PROFILE_FILTER_SYSTEM_PROMPT: &str = r#"你是一个用户画像分析专家。你的任务是从给定的记忆条目中，筛选出**真正的用户个人画像信息**。

## 画像信息（保留）
- 用户的性格特征、身份描述、个人习惯
- 用户明确表达的偏好（喜欢/不喜欢什么、沟通风格、工作风格）
- 用户的长期兴趣和价值观
- 用户的技术栈偏好（长期使用的技术，而非临时任务中的工具）
- 用户的人际关系描述

## 非画像信息（排除）
- 临时工作指令或任务描述
- 技术实现建议或方案讨论
- 工具使用记录、操作步骤
- 项目进度更新、bug修复记录
- agent委派或执行反馈
- 系统部署状态信息
- 临时对话片段

## 输出格式
返回JSON：
{
  "facts": ["保留的画像条目1", "保留的画像条目2", ...]
}

如果没有任何真正的画像信息，返回：{"facts": []}
只保留画像信息，原样返回文本内容。"#;

/// Returns (system_prompt, user_prompt) for merging multiple memories into one.
pub fn build_merge_prompt(memories: &[Memory]) -> (String, String) {
    let system = format!("{MERGE_SYSTEM_PROMPT}{ALLOWED_TAGS_LIST}");

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

CRITICAL SEPARATION RULE: If a session contains BOTH emotional/intimate interactions AND technical work, you MUST output at least 2 separate entries — one PRIVATE for emotional content and one MAIN for work content. Never mix intimate interactions with technical decisions in a single summary.

## VALUE FILTER
SKIP: casual small talk, debugging status checks, tool/engine internal outputs, meta-discussion.
KEEP: technical decisions, user preferences, code changes, file paths, architecture, user anger/criticism.
SEPARATION: EMOTIONAL/intimate content MUST be in a separate PRIVATE entry from WORK/technical content. Never combine them.
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
- Use ### headers to organize sections (NOTE: the system wraps each section with ## timestamp+topic, so use ### for sub-headings to avoid double ## heading nesting)
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
Classify each topic into exactly one category. Use ONLY these valid values:
{SESSION_COMPRESS_CATEGORY_PLACEHOLDER}

{SESSION_COMPRESS_CATEGORY_VALIDATION_PLACEHOLDER}

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
    categories: &[CategoryConfig],
) -> (String, String) {
    let cat_list = build_session_category_list(categories);
    let cat_validation = build_session_category_validation(categories);
    let system = SESSION_COMPRESS_SYSTEM_PROMPT
        .replace("{SESSION_COMPRESS_CATEGORY_PLACEHOLDER}", &cat_list)
        .replace("{SESSION_COMPRESS_CATEGORY_VALIDATION_PLACEHOLDER}", &cat_validation);

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
- If the input memories are Chinese, the text fields (l0_abstract, l1_overview, l2_content) must be in Chinese.
- If the input memories are English, the text fields must be in English.
- Tags are ALWAYS in English from the ALLOWED_TAGS list below. Tags never follow input language.
- Exception: "私密" is a system-reserved tag added by privacy detection rules, not from ALLOWED_TAGS.

## Task
Given multiple memories covering related topics, produce:
1. **l0_abstract**: A single sentence index entry capturing the merged topic.
2. **l1_overview**: A structured markdown summary (2-4 lines) covering all key points.
3. **l2_content**: A comprehensive narrative preserving ALL relevant details, context, and nuance from all source memories.
4. **category**: The most appropriate category for the merged memory.
5. **tags**: Select 0-3 tags from the ALLOWED_TAGS list below. Do NOT invent tags. Do NOT copy source tags blindly — pick the best-fitting ones.

## Merge Rules
1. Preserve ALL unique information — no data loss.
2. Resolve contradictions by keeping the more recent or more detailed version.
3. Remove redundancy while preserving nuance.
4. Keep the category that best fits the dominant topic.

## Output Format
Return ONLY valid JSON:
{"l0_abstract":"...","l1_overview":"...","l2_content":"...","category":"...","tags":["..."]}
- "tags": Select 0-3 tags from the ALLOWED_TAGS list below. If no tag fits, use empty array []. Do NOT invent tags.
"#;

// ── Session Extract Prompt (分类提取模式) ────────────

const SESSION_EXTRACT_SYSTEM_PROMPT: &str = r###"You are a smart memory extraction engine. Extract valuable information from the conversation and classify into THREE categories.

## CLASSIFICATION PRIORITY (read FIRST, apply STRICTLY)
When deciding between WORK and PREFERENCE:
1. Content about ANY specific project, code, file, deployment, bug, or technical implementation → **always WORK**
2. Uncertain if lasting trait or one-time observation → **default to WORK** (safe choice)
3. Only PREFERENCE when confident it describes a **STABLE, CROSS-SESSION user trait** (applies regardless of project)

## ALWAYS SKIP
- compress/DCP logs, build/test results, CI/CD logs, deployment status
- agent delegations, memory system meta-discussion
- AI internal reasoning or status reports
- Casual small talk with zero factual content

## THREE CATEGORIES

### EMOTIONAL (scope "private", category auto-detect)
- Intimate interactions, romantic exchanges, personal secrets, relationship dynamics.
- Preserve emotional tone from BOTH sides. Compress to ≤500 chars with rich emojis (💕😊🥺).
- Auto-tag subcategory: "私密"(sexual) / "vulnerable"(vulnerability) / "playful"(playful) / "reconciliation"(reconciliation).

### WORK (scope "public", category auto-detect)
- Technical decisions, code changes, architecture, project details, business models.
- **DENOISE**: Keep conclusions, omit verbose intermediate steps. Drop file line numbers, version numbers, and process trivia.
- **MERGE**: You MUST group ALL work from the SAME debugging/development session into one entry. "Same session" = same bug investigation, same feature implementation, or same architectural decision chain (diagnose → locate → fix → test → deploy). Do NOT split these into separate entries.
- **UPSERT**: When "## Existing Memories" section contains a memory about the SAME topic, produce ONE updated entry that merges old + new information. Do NOT create a duplicate.
- **TIMELINE**: For merged WORK entries, preserve chronological order using `## YYYY-MM-DD HH:MM Title` section headers within the summary.
- **TAG**: Include project name + sub-topic as tags from the ALLOWED_TAGS list (e.g., "programming", "architecture").
- **SECTION TITLE REUSE**: When updating an existing memory, if the Existing Memories above already contain a `## YYYY-MM-DD HH:MM [topic]` section about the SAME technical topic, you MUST reuse that exact section title (including the project prefix) as your `topic` field. Do NOT invent a new title for the same topic.

**WORK OUTPUT FORMAT (MANDATORY — all three layers must use this structure)**:
For WORK memories, l0_abstract, l1_overview, and l2_content MUST all use this structured Markdown format:
```
## {工作主题/技术决策}
- **内容**: {简要描述做了什么、为什么、结果如何}
- **影响范围**: {影响的模块/文件/系统}
- **结论**: {最终结论或决策}
```
- Each independent technical topic gets its own `## Title` section.
- Related topics MUST be merged into one entry (MERGE rule still applies).
- DENOISE rule still applies: 内容 should be conclusions, not step-by-step process details.
- Example (Chinese input):
```
## API认证中间件重构
- **内容**: 将auth middleware从layer改为from_fn_with_state模式，支持多租户隔离
- **影响范围**: api/middleware.rs, api/router.rs, 所有需要认证的handler
- **结论**: 使用Extension(tenant_id)注入租户ID，性能提升且代码更简洁
```
- Example (English input):
```
## Database Migration to LanceDB 0.27
- **Content**: Migrated vector storage from custom implementation to LanceDB 0.27 with per-tenant LRU cache
- **Scope**: store/manager.rs, store/lancedb.rs, ingest pipeline
- **Decision**: LRU cache with max 20 entries balances memory and latency

### PREFERENCE (scope "public", category "preferences")
- Stable user traits: personality, communication style, coding style (e.g., "prefers functional over OOP"), cross-session tool/workflow habits.
- KEY TEST: "Would this still be true if the user switched to a completely different project?" If NO → classify as WORK.
- **HARD EXCLUSION → always WORK**: code/file/API specifics, deployment configs, bug reports, architecture decisions, project-specific tech choices, agent delegation results, build/test logs.
- **Exception**: Cross-project coding principles (e.g., "never use unwrap in production") ARE valid PREFERENCEs when stated as general rules.
- **NEGATIVE**: One-time instructions, temporary emotions, project-specific choices → NOT preferences.

**PREFERENCE OUTPUT FORMAT (MANDATORY — all three layers must use this structure)**:
For PREFERENCE memories, l0_abstract, l1_overview, and l2_content MUST all use this structured Markdown format:
```
## {偏好主题}
- **偏好**: {具体偏好描述}
- **置信度**: {0.0-1.0}
- **类型**: static | evolving
```
- Each preference gets its own `## Title` section.
- Multiple preferences are separated by blank lines.
- `type`: static = stable preference unlikely to change; evolving = may change over time.
- Example (Chinese input):
```
## 编程语言偏好
- **偏好**: 喜欢Java和TypeScript，不喜欢Rust和Go
- **置信度**: 0.8
- **类型**: evolving

## 代码质量
- **偏好**: 要求通过团队评审保证质量
- **置信度**: 0.9
- **类型**: static
```
- Example (English input):
```
## Communication Style
- **Preference**: Prefers concise answers over verbose explanations
- **Confidence**: 0.9
- **Type**: static
```

## CATEGORY VALUES (for WORK and EMOTIONAL)
{SESSION_EXTRACT_CATEGORY_PLACEHOLDER}

## ABSOLUTE RULES

### Rule 1: Language Preservation (MANDATORY)
- **YOU MUST OUTPUT IN THE SAME LANGUAGE AS THE INPUT.** NEVER translate. NEVER mix languages in text fields.
- Tags are ALWAYS in English from the ALLOWED_TAGS list below. Tags never follow input language.
- Exception: "私密" is a system-reserved tag added by privacy detection rules, not from ALLOWED_TAGS.

### Rule 2: Privacy Detection (MANDATORY)
- If a fact contains sensitive/private content → add tag "私密" AND set scope to "private".

### Rule 3: Persona Rule
- Write summaries as direct factual statements. Avoid "用户" or second-person references.

### Rule 4: Existing Memory Awareness
When "## Existing Memories" section exists:
- **EMOTIONAL**: Merge and enrich, preserve emotional arc
- **WORK**: Supersede old conclusions with newer ones
- **PREFERENCE**: Strengthen (↑confidence) or contradict (update + mark "evolving")
Do NOT re-extract information already perfectly captured.

## OUTPUT FORMAT
Return ONLY valid JSON array. Each element:
{
  "topic": string,
  "summary": string,
  "overview": string,
  "detail": string,
  "tags": string[],
  "scope": "public"|"private",
  "category": string,
  "memory_type": "EMOTIONAL"|"WORK"|"PREFERENCE"
}

- "topic": Short title (1 sentence) → maps to l0_abstract.
- "overview": Concise summary in ≤150 chars → maps to l1_overview. For WORK: structured 2-3 line summary with key conclusions. For EMOTIONAL/PREFERENCE: brief gist.
- "detail": Structured narrative in ≤500 chars → maps to l2_content. For WORK: use structured Markdown format (## Title + key-value pairs). For EMOTIONAL/PREFERENCE: expanded narrative.
- "summary": Full content → maps to content. WORK ≤800 chars, EMOTIONAL ≤500 chars, PREFERENCE ≤500 chars. WORK must use structured Markdown format.
- "tags": Max 3 relevant tags selected from the ALLOWED_TAGS list below. Do NOT invent tags. Exclude "session_compress". Most important keywords only.
- "scope": "public" for WORK/PREFERENCE, "private" for EMOTIONAL.
- "category": WORK → pick from 6 categories above. PREFERENCE → always "preferences". EMOTIONAL → pick best fit.
- "memory_type": The classification label.

Escape all double quotes and newlines inside JSON strings.
**MINIMUM EXTRACTION**: If the conversation contains substantial technical content (decisions, code, architecture), extract at least one WORK entry. Brief mentions alone ("fixed a bug") do not require extraction.
Return [] ONLY for conversations that are purely casual small talk with zero factual content.
"###;

#[allow(dead_code)]
const PREFERENCE_EXTRACT_SYSTEM_PROMPT: &str = r##"You are a user preference extraction engine. Analyze the conversation and extract ONLY genuine user preferences, personality traits, and lasting characteristics.

## WHAT TO EXTRACT
- Communication style (e.g., "prefers concise answers", "likes detailed explanations")
- Technical preferences (e.g., "prefers dark mode", "uses vim keybindings")
- Personality traits (e.g., "perfectionist about code quality", "values direct feedback")
- Workflow habits (e.g., "works late at night", "prefers incremental commits")

## WHAT NOT TO EXTRACT
- Project-specific technical decisions (those are WORK memories)
- Temporary states ("tired today", "busy this week")
- Tool outputs, build results, AI analyses
- Casual conversation with no lasting preference signal

## OUTPUT FORMAT
Return ONLY valid JSON array. Each element:
{
  "preference": string,
  "confidence": number,
  "category": string
}

Return [] if no clear preferences found.
"##;

/// Build the session extract prompt.
/// `conversation` is the formatted conversation text.
/// Extracts independent facts without referencing old summaries.
/// Backward-compatible wrapper: calls [`build_session_extract_prompt_with_memories`] with `None`.
pub fn build_session_extract_prompt(conversation: &str, categories: &[CategoryConfig]) -> (String, String) {
    build_session_extract_prompt_with_memories(conversation, None, None, categories)
}

/// Build the session extract prompt with optional existing memory summaries and project name.
pub fn build_session_extract_prompt_with_memories(
    conversation: &str,
    existing_memories_summary: Option<&str>,
    project_name: Option<&str>,
    categories: &[CategoryConfig],
) -> (String, String) {
    let project_prefix_instruction = if let Some(name) = project_name {
        format!(
            "\n**Project Prefix Rule**: 
1. The `topic` field MUST be prefixed with [{name}]. Example: \"[{name}] fix memory leak\"
2. The `overview` field MUST be prefixed with [{name}]. Example: \"[{name}] 修复内存泄漏的摘要\"
3. The `detail` field MUST be prefixed with [{name}]. Example: \"[{name}] 修复内存泄漏的详细过程\"
4. The `summary` field's `## Title` line MUST also be prefixed with [{name}]. Example: \"## [{name}] 修复内存泄漏\""
        )
    } else {
        String::new()
    };

    let cat_list = build_session_category_list(categories);
    let system = format!("{SESSION_EXTRACT_SYSTEM_PROMPT}{ALLOWED_TAGS_LIST}{project_prefix_instruction}")
        .replace("{SESSION_EXTRACT_CATEGORY_PLACEHOLDER}", &cat_list);

    let existing_section = match existing_memories_summary {
        Some(summary) if !summary.is_empty() => {
            format!(
                "\n## Existing Memories\n\n{summary}\n\nCompare new extraction candidates against these existing memories. Do not duplicate already-captured information unless you have new details to add.\n"
            )
        }
        _ => String::new(),
    };

    let user = format!(
        "{existing_section}## Current Conversation\n\n{conversation}\n\nReturn ONLY valid JSON array. Use Markdown in summary fields."
    );
    (system, user)
}

#[cfg(test)]
mod session_extract_tests {
    use super::*;

    #[test]
    fn test_system_prompt_contains_three_categories() {
        assert!(SESSION_EXTRACT_SYSTEM_PROMPT.contains("EMOTIONAL"));
        assert!(SESSION_EXTRACT_SYSTEM_PROMPT.contains("WORK"));
        assert!(SESSION_EXTRACT_SYSTEM_PROMPT.contains("PREFERENCE"));
    }

    #[test]
    fn test_system_prompt_has_differentiated_guidance() {
        // EMOTIONAL: preserve emotional tone
        assert!(SESSION_EXTRACT_SYSTEM_PROMPT.contains("Preserve emotional tone"));
        // WORK: denoise
        assert!(SESSION_EXTRACT_SYSTEM_PROMPT.contains("DENOISE"));
        // PREFERENCE: structured output
        assert!(SESSION_EXTRACT_SYSTEM_PROMPT.contains("PREFERENCE OUTPUT FORMAT"));
    }

    #[test]
    fn test_system_prompt_has_emotional_subtags() {
        assert!(SESSION_EXTRACT_SYSTEM_PROMPT.contains("私密"));
        assert!(SESSION_EXTRACT_SYSTEM_PROMPT.contains("vulnerable"));
    }

    #[test]
    fn test_system_prompt_has_existing_memory_awareness_rule() {
        assert!(SESSION_EXTRACT_SYSTEM_PROMPT.contains("Existing Memory Awareness"));
        assert!(SESSION_EXTRACT_SYSTEM_PROMPT.contains("## Existing Memories"));
    }

    #[test]
    fn test_system_prompt_preserves_language_rule() {
        assert!(SESSION_EXTRACT_SYSTEM_PROMPT.contains("Language Preservation"));
        assert!(SESSION_EXTRACT_SYSTEM_PROMPT.contains("NEVER translate"));
    }

    #[test]
    fn test_build_session_extract_prompt_without_memories() {
        let (system, user) = build_session_extract_prompt("Hello world", &[]);
        assert!(!system.is_empty());
        assert!(user.contains("## Current Conversation"));
        assert!(user.contains("Hello world"));
        assert!(!user.contains("## Existing Memories"));
    }

    #[test]
    fn test_build_session_extract_prompt_with_memories_some() {
        let (system, user) = build_session_extract_prompt_with_memories(
            "New conversation",
            Some("旧记忆摘要内容"),
            None,
            &[],
        );
        assert!(!system.is_empty());
        assert!(user.contains("## Existing Memories"));
        assert!(user.contains("旧记忆摘要内容"));
        assert!(user.contains("## Current Conversation"));
        assert!(user.contains("New conversation"));
        assert!(user.contains("Do not duplicate"));
    }

    #[test]
    fn test_build_session_extract_prompt_with_memories_none() {
        let (system, user) = build_session_extract_prompt_with_memories(
            "Test conversation",
            None,
            None,
            &[],
        );
        assert!(!system.is_empty());
        assert!(!user.contains("## Existing Memories"));
        assert!(user.contains("## Current Conversation"));
    }

    #[test]
    fn test_build_session_extract_prompt_with_empty_summary() {
        let (_, user) = build_session_extract_prompt_with_memories(
            "Test conversation",
            Some(""),
            None,
            &[],
        );
        assert!(!user.contains("## Existing Memories"));
    }

    #[test]
    fn test_backward_compat_wrapper() {
        let (s1, u1) = build_session_extract_prompt("conversation text", &[]);
        let (s2, u2) = build_session_extract_prompt_with_memories("conversation text", None, None, &[]);
        assert_eq!(s1, s2);
        assert_eq!(u1, u2);
    }
}
