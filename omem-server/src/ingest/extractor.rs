use std::sync::Arc;

use regex::Regex;

use crate::domain::error::OmemError;
use crate::ingest::prompts;
use crate::ingest::types::{ExtractedFact, ExtractionResult, IngestMessage};
use crate::llm::{complete_json, LlmService};

const DEFAULT_MAX_FACTS: usize = 50;
const DEFAULT_MAX_INPUT_CHARS: usize = 8000;
const VALID_CATEGORIES: &[&str] = &[
    "profile",
    "preferences",
    "entities",
    "events",
    "cases",
    "patterns",
];

pub struct FactExtractor {
    llm: Arc<dyn LlmService>,
    max_facts: usize,
    pub(crate) max_input_chars: usize,
}

impl FactExtractor {
    pub fn new(llm: Arc<dyn LlmService>) -> Self {
        Self {
            llm,
            max_facts: DEFAULT_MAX_FACTS,
            max_input_chars: DEFAULT_MAX_INPUT_CHARS,
        }
    }

    pub async fn extract(
        &self,
        messages: &[IngestMessage],
        entity_context: Option<&str>,
    ) -> Result<Vec<ExtractedFact>, OmemError> {
        if messages.is_empty() {
            return Ok(Vec::new());
        }

        let conversation_text = self.format_messages(messages);
        let cleaned = strip_envelope_metadata(&conversation_text);

        if cleaned.trim().is_empty() {
            return Ok(Vec::new());
        }

        let system = prompts::build_system_prompt(entity_context);
        let user = prompts::build_user_prompt(&cleaned);

        let result: ExtractionResult = complete_json(self.llm.as_ref(), &system, &user).await?;

        let facts = result
            .memories
            .into_iter()
            .filter(|f| !f.l0_abstract.trim().is_empty())
            .map(|mut f| {
                f.category = normalize_category(&f.category);
                f.quality_score = calculate_quality_score(&f.l0_abstract);
                f
            })
            .take(self.max_facts)
            .collect();

        Ok(facts)
    }

    pub async fn extract_with_prompts(
        &self,
        system: &str,
        user: &str,
    ) -> Result<Vec<ExtractedFact>, OmemError> {
        let result: ExtractionResult = complete_json(self.llm.as_ref(), system, user).await?;

        let facts = result
            .memories
            .into_iter()
            .filter(|f| !f.l0_abstract.trim().is_empty())
            .map(|mut f| {
                f.category = normalize_category(&f.category);
                f.quality_score = calculate_quality_score(&f.l0_abstract);
                f
            })
            .take(self.max_facts)
            .collect();

        Ok(facts)
    }

    fn format_messages(&self, messages: &[IngestMessage]) -> String {
        let mut full_text = String::new();
        for msg in messages {
            full_text.push_str(&msg.role);
            full_text.push_str(": ");
            full_text.push_str(&msg.content);
            full_text.push('\n');
        }

        if full_text.len() > self.max_input_chars {
            let start = full_text.len() - self.max_input_chars;
            let boundary = full_text[start..]
                .find('\n')
                .map(|i| start + i + 1)
                .unwrap_or(start);
            let boundary = if boundary >= full_text.len() {
                start
            } else {
                boundary
            };
            return full_text[boundary..].to_string();
        }

        full_text
    }
}

fn calculate_quality_score(text: &str) -> f32 {
    let mut score: f32 = 0.5;
    let len = text.len();

    if len > 50 {
        score += 0.05;
    }
    if len > 100 {
        score += 0.05;
    }
    if len > 200 {
        score += 0.05;
    }

    if Regex::new(r"[0-9]{4}").unwrap().is_match(text) {
        score += 0.1;
    }
    if Regex::new(r"\d{1,2}[年日月周]").unwrap().is_match(text) {
        score += 0.05;
    }
    if Regex::new(r"\d+\.[0-9]+|[0-9]+%|[0-9]+[ξ元美元]")
        .unwrap()
        .is_match(text)
    {
        score += 0.05;
    }

    if Regex::new(r"因此|所以|结论是|决定是|方案是|由于|因为")
        .unwrap()
        .is_match(text)
    {
        score += 0.1;
    }
    if Regex::new(r"[。！？]\s*[^。！？]{30,}")
        .unwrap()
        .is_match(text)
    {
        score += 0.05;
    }
    if Regex::new(r"(?m)^\s*[-*#]\s+\S").unwrap().is_match(text) {
        score += 0.05;
    }
    if Regex::new(r"[A-Z][a-z]+[A-Z]|[A-Z]{2,}")
        .unwrap()
        .is_match(text)
    {
        score += 0.05;
    }

    score.min(1.0).max(0.1)
}

fn normalize_category(raw: &str) -> String {
    let lower = raw.trim().to_lowercase();
    if VALID_CATEGORIES.contains(&lower.as_str()) {
        lower
    } else {
        "profile".to_string()
    }
}

pub fn strip_envelope_metadata(text: &str) -> String {
    let system_channel = Regex::new(r"(?m)^(?:\w+:\s*)?System:\s*\[.*?\]\s*Channel.*$")
        .expect("valid regex: system_channel");
    let result = system_channel.replace_all(text, "");

    let conv_info = Regex::new(r"(?ms)Conversation info \(untrusted metadata\):\s*\{.*?\}")
        .expect("valid regex: conv_info");
    let result = conv_info.replace_all(&result, "");

    let sender_info = Regex::new(r"(?ms)Sender \(untrusted metadata\):\s*\{.*?\}")
        .expect("valid regex: sender_info");
    let result = sender_info.replace_all(&result, "");

    result.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_score_length_bonus() {
        let short = "User likes cats";
        let medium =
            "User prefers Rust over C++ for systems programming due to memory safety features";
        let long = "User deployed v2.0 to production last Friday and fixed a critical bug in the payment pipeline that caused duplicate charges for users in the EU region";

        let s = calculate_quality_score(short);
        let m = calculate_quality_score(medium);
        let l = calculate_quality_score(long);

        assert!(s >= 0.1 && s <= 0.55);
        assert!(m > s);
        assert!(l > m);
    }

    #[test]
    fn quality_score_number_bonus() {
        let no_num = "User works at Google";
        let with_year = "User joined Stripe in 2023 as a backend engineer";
        let with_date = "User fixed the bug last Monday and shipped the fix three days ago";

        let s0 = calculate_quality_score(no_num);
        let s1 = calculate_quality_score(with_year);
        let s2 = calculate_quality_score(with_date);

        assert!(s1 > s0);
        assert!(s2 > s0);
    }

    #[test]
    fn quality_score_conclusion_bonus() {
        let plain = "User uses Vim";
        let conclusion = "User prefers Vim because of its modal editing and high customizability";

        assert!(calculate_quality_score(conclusion) > calculate_quality_score(plain));
    }

    #[test]
    fn quality_score_bounded() {
        let empty = "";
        let huge = "A".repeat(10000);

        let se = calculate_quality_score(empty);
        let sh = calculate_quality_score(&huge);

        assert!(se >= 0.1 && se <= 1.0);
        assert!(sh >= 0.1 && sh <= 1.0);
    }

    #[test]
    fn quality_score_list_structure() {
        let no_list = "User works at Stripe";
        let with_list = "User works at Stripe, Google, Meta";

        assert!(calculate_quality_score(with_list) >= calculate_quality_score(no_list));
    }

    #[test]
    fn strip_envelope_system_channel_line() {
        let input = "System: [2024-01-01T00:00:00Z] Channel #general\nuser: hello";
        let result = strip_envelope_metadata(input);
        assert!(!result.contains("Channel #general"));
        assert!(result.contains("user: hello"));
    }

    #[test]
    fn strip_envelope_conversation_info_block() {
        let input = "Conversation info (untrusted metadata):\n{\"platform\": \"slack\", \"channel\": \"#dev\"}\nuser: hello";
        let result = strip_envelope_metadata(input);
        assert!(!result.contains("untrusted metadata"));
        assert!(!result.contains("slack"));
        assert!(result.contains("user: hello"));
    }

    #[test]
    fn strip_envelope_sender_info_block() {
        let input = "Sender (untrusted metadata):\n{\"name\": \"John\"}\nuser: hello";
        let result = strip_envelope_metadata(input);
        assert!(!result.contains("untrusted metadata"));
        assert!(!result.contains("John"));
        assert!(result.contains("user: hello"));
    }

    #[test]
    fn strip_envelope_preserves_clean_text() {
        let input = "user: I like Rust\nassistant: Great choice!";
        let result = strip_envelope_metadata(input);
        assert_eq!(result, input);
    }
}
