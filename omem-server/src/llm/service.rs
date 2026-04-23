use crate::domain::error::OmemError;

#[async_trait::async_trait]
pub trait LlmService: Send + Sync {
    async fn complete_text(&self, system: &str, user: &str) -> Result<String, OmemError>;
}

/// Strips thinking tags (`<think>` and `</think>`) and markdown fences from LLM output.
pub fn strip_markdown_fences(s: &str) -> String {
    let trimmed = s.trim();

    // Step 1: Strip thinking tags first (for reasoning models like MiniMax-M2.7)
    let without_thinking = strip_thinking_tags(trimmed);

    // Step 2: Strip markdown fences
    if let Some(rest) = without_thinking.strip_prefix("```json") {
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_string();
        }
    }

    if let Some(rest) = without_thinking.strip_prefix("```") {
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_string();
        }
    }

    without_thinking.trim().to_string()
}

/// Strips `<think>...</think>` and `<think>...` tags from LLM output.
fn strip_thinking_tags(s: &str) -> String {
    let mut result = s.to_string();

    // Handle standard thinking tags: <think>...</think>
    // Use a loop to handle multiple occurrences
    while let Some(start) = result.find("<think>") {
        if let Some(end) = result.find("</think>") {
            let before = &result[..start];
            let after = &result[end + "</think>".len()..];
            result = format!("{before}{after}");
        } else {
            // No closing tag found, remove everything from start onwards
            result = result[..start].to_string();
            break;
        }
    }

    result
}

/// Complete a prompt and parse the response as typed JSON.
/// Retries once with an error hint on parse failure.
pub async fn complete_json<T: serde::de::DeserializeOwned>(
    llm: &dyn LlmService,
    system: &str,
    user: &str,
) -> Result<T, OmemError> {
    let text = llm.complete_text(system, user).await?;
    let cleaned = strip_markdown_fences(&text);

    match serde_json::from_str(&cleaned) {
        Ok(v) => Ok(v),
        Err(_first_err) => {
            let retry_user = format!(
                "{user}\n\nYour previous response was not valid JSON. Return ONLY valid JSON."
            );
            let text = llm.complete_text(system, &retry_user).await?;
            let cleaned = strip_markdown_fences(&text);
            serde_json::from_str(&cleaned)
                .map_err(|e| OmemError::Llm(format!("JSON parse failed after retry: {e}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_json_fence() {
        let input = "```json\n{\"key\": \"value\"}\n```";
        assert_eq!(strip_markdown_fences(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn strip_plain_fence() {
        let input = "```\n{\"a\": 1}\n```";
        assert_eq!(strip_markdown_fences(input), "{\"a\": 1}");
    }

    #[test]
    fn no_fence_passthrough() {
        let input = "{\"key\": \"value\"}";
        assert_eq!(strip_markdown_fences(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn strip_with_whitespace() {
        let input = "  \n```json\n  {\"x\": true}  \n```\n  ";
        assert_eq!(strip_markdown_fences(input), "{\"x\": true}");
    }

    #[test]
    fn strip_fence_no_newline_after_lang() {
        let input = "```json{\"y\": 42}```";
        assert_eq!(strip_markdown_fences(input), "{\"y\": 42}");
    }

    #[test]
    fn already_clean_json() {
        let input = "  {\"hello\": \"world\"}  ";
        assert_eq!(strip_markdown_fences(input), "{\"hello\": \"world\"}");
    }

    #[test]
    fn strip_multiline_json() {
        let input = "```json\n{\n  \"items\": [\n    1,\n    2\n  ]\n}\n```";
        let result = strip_markdown_fences(input);
        assert!(result.starts_with('{'));
        assert!(result.ends_with('}'));
        assert!(result.contains("\"items\""));
    }

    #[test]
    fn strip_thinking_tags() {
        let input = "<think>\nLet me analyze the facts carefully.\n</think>\n{\"key\": \"value\"}";
        assert_eq!(strip_markdown_fences(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn strip_thinking_tags_multiline() {
        let input = "<think>\nThe user is a developer.\nI need to extract facts.\n</think>\n{\"memories\": [{\"l0_abstract\": \"User is a developer\"}]}";
        assert_eq!(
            strip_markdown_fences(input),
            "{\"memories\": [{\"l0_abstract\": \"User is a developer\"}]}"
        );
    }

    #[test]
    fn strip_thinking_tags_no_closer() {
        let input = "<think>\nThis thinking never ends\n{\"key\": \"value\"}";
        assert_eq!(strip_markdown_fences(input), "{\"key\": \"value\"}");
    }

    #[test]
    fn strip_thinking_tags_with_json_fence() {
        let input = "<think>\nAnalyzing...\n</think>\n```json\n{\"key\": \"value\"}\n```";
        assert_eq!(strip_markdown_fences(input), "{\"key\": \"value\"}");
    }
}
