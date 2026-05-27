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

pub const REFINE_SYSTEM_PROMPT: &str = r#"Memory refinement engine. Input: existing memories + new fact on same topic. Output: ONE deduplicated, compressed memory.

## RULES

1. **Language**: Output language = input language. Never translate. Tags in English (except "私密").
2. **Compress**: Target output = 40-60% of total input length. Minimum 25%. Aggressively deduplicate and rewrite verbosely.
3. **Preserve**: Keep ALL distinct facts, technical details, key data points, decisions. Remove: filler, process steps, meta-commentary, transitional text, repetitive descriptions.
4. **Format**: `## YYYY-MM-DD HH:MM Topic` section per distinct event. Chronological. Never drop entire section — compress within.
5. **Merge**: Same event in multiple sections → merge into one with latest timestamp. State each fact ONCE.

## OUTPUT — valid JSON only
{
  "refined_content": "Deduplicated content in ## YYYY-MM-DD HH:MM Topic format",
  "l0_abstract": "Topic label ≤100 chars",
  "l1_overview": "Arrow notation: verb→verb→result, ≤150 chars. No paragraphs.",
  "l2_content": "Key facts: decisions, data points, conclusions, ≤300 chars. No paragraphs."
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
