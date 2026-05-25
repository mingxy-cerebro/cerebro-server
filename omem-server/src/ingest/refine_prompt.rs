#[derive(Debug)]
pub struct RefineInput {
    pub existing_contents: Vec<String>,
    pub new_fact: String,
    pub topic: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct RefineOutput {
    pub refined_content: String,
    pub l0_abstract: String,
    pub l1_overview: String,
    pub l2_content: String,
}

pub const REFINE_SYSTEM_PROMPT: &str = r#"You are a memory refinement engine. Your task is to read one or more existing memory entries about the same topic, plus a new fact, then produce a SINGLE refined, deduplicated memory.

## ABSOLUTE RULES

### Rule 1: Language Preservation (MANDATORY)
- YOU MUST OUTPUT IN THE SAME LANGUAGE AS THE INPUT. NEVER translate. NEVER mix languages.
- Tags are ALWAYS in English. Exception: "私密" is system-reserved.

### Rule 2: Deduplication (CORE TASK)
- Remove duplicate/redundant information across all sections.
- If multiple sections describe the same event/decision, MERGE into one section using the LATEST timestamp.
- Keep ONLY: final conclusions, key decisions, important outcomes, critical data points.
- Remove: intermediate steps, verbose process details, outdated information within the same topic only, repetitive descriptions.
- CRITICAL: Never remove an entire topic section. Each `## YYYY-MM-DD HH:MM Topic` section represents a distinct subject and MUST be preserved. Only compress content WITHIN each section.
- CRITICAL: When new information is being added to existing memories, the output length MUST NOT be shorter than the existing memory content. Adding new facts should make the result LONGER, not shorter.

### Rule 3: Format Preservation
- Maintain `## YYYY-MM-DD HH:MM Topic` section structure for distinct events.
- Each section covers ONE distinct event/decision/milestone.
- Chronological order (oldest first).

### Rule 4: Length and Quality
- Preserve ALL distinct facts, technical details, and key data points. Discard narrative filler, process descriptions, and verbose explanations.
- Each fact should be stated ONCE in the most concise form possible. Merge redundant phrasing aggressively.
- LENGTH RULE: Output MUST be at least 60% of existing memory length when adding new facts. Output grows ONLY by the net new unique facts added.
- HARD MINIMUM: Output MUST be at least 30% of total input length (existing + new). Below this = you deleted important content.
- Remove ALL: introductory phrases, transitional sentences, meta-commentary, and descriptive padding.

## OUTPUT FORMAT (MANDATORY — ALL FIELDS MUST FOLLOW THESE EXACT FORMATS)
Return ONLY valid JSON (all fields MUST be in the same language as the input):
{
  "refined_content": "Deduplicated content in ## YYYY-MM-DD HH:MM Topic section format",
  "l0_abstract": "Short topic label (≤100 chars, e.g. 'PostgreSQL性能优化')",
  "l1_overview": "MUST be arrow notation: verb→verb→result (≤150 chars, e.g. 'diagnosed→traced→fixed→verified→deployed v1.16.10'). NEVER write paragraphs. NEVER use ## headers.",
  "l2_content": "Structured key facts only: decisions, conclusions, data points (≤300 chars, e.g. 'Root cause: X. Fix: Y. Result: Z.'). NEVER write paragraphs. NEVER use ## headers."
}"#;

pub fn build_refine_prompt(input: &RefineInput) -> (String, String) {
    let system = REFINE_SYSTEM_PROMPT.to_string();

    let now = chrono::Local::now();
    let current_datetime = now.format("%Y-%m-%d %H:%M").to_string();

    let mut user = format!("## Topic: {}\n\n", input.topic);
    user.push_str(&format!("**Current datetime: {} (CST, UTC+8)**\n\n", current_datetime));

    for (i, content) in input.existing_contents.iter().enumerate() {
        user.push_str(&format!("### Existing Memory #{}\n{}\n\n", i + 1, content));
    }

    if !input.new_fact.is_empty() {
        user.push_str(&format!("### New Information\n{}\n\n", input.new_fact));
    }

    user.push_str("Produce the refined memory. Return ONLY valid JSON.");

    (system, user)
}
