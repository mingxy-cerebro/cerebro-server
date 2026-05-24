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
- Remove: intermediate steps, verbose process details, outdated information, repetitive descriptions.

### Rule 3: Format Preservation
- Maintain `## YYYY-MM-DD HH:MM Topic` section structure for distinct events.
- Each section covers ONE distinct event/decision/milestone.
- Chronological order (oldest first).

### Rule 4: Precision Over Recall
- It is BETTER to lose minor details than to keep redundant content.
- The refined content MUST be shorter than the sum of all input contents.
- Target: compress to 30-60% of original total length.

## OUTPUT FORMAT
Return ONLY valid JSON:
{
  "refined_content": "Deduplicated content in section format",
  "l0_abstract": "Topic label covering the full scope (≤100 chars)",
  "l1_overview": "Timeline in arrow format: A→B→C→result (≤150 chars)",
  "l2_content": "Key facts: decisions, conclusions, data (≤300 chars)"
}

## l1_overview FORMAT (MANDATORY)
Must use arrow notation: `verb phrase→verb phrase→result`
Examples:
- "diagnosed bug→traced to handler→fixed with lookup table→verified→deployed v1.16.10"
- "requirement analysis→design review→implemented→tested→released"
- "identified perf issue→benchmarked 3 solutions→chose option B→deployed→latency reduced 70%"
Each node = verb phrase (what happened), arrows = temporal/causal progression.

## l2_content FORMAT
Compress to structured key facts only:
- Root cause: X
- Fix: Y
- Verification: Z
- Key metric: N
Remove all narrative/process description."#;

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
