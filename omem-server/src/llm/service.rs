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
/// Applies JSON repair (trailing commas, unescaped chars) before parsing.
/// Retries once with an error hint on parse failure.
pub async fn complete_json<T: serde::de::DeserializeOwned>(
    llm: &dyn LlmService,
    system: &str,
    user: &str,
) -> Result<T, OmemError> {
    let text = llm.complete_text(system, user).await?;
    let cleaned = strip_markdown_fences(&text);

    match serde_json::from_str(&cleaned) {
        Ok(v) => return Ok(v),
        Err(first_err) => {
            tracing::debug!(error = %first_err, len = cleaned.len(), "JSON parse failed, trying repair");
        }
    }

    let repaired = try_repair_json(&cleaned);
    if let Ok(v) = serde_json::from_str(&repaired) {
        tracing::info!("JSON repaired successfully after fixing common issues");
        return Ok(v);
    }

    let retry_user = format!(
        "{user}\n\nYour previous response was not valid JSON. \
         Ensure ALL double quotes inside string values are escaped (\\\"). \
         Ensure ALL newlines inside strings are escaped (\\n). \
         Return ONLY valid JSON."
    );
    let text = llm.complete_text(system, &retry_user).await?;
    let cleaned = strip_markdown_fences(&text);

    match serde_json::from_str(&cleaned) {
        Ok(v) => return Ok(v),
        Err(retry_err) => {
            tracing::debug!(error = %retry_err, "retry JSON parse failed, trying repair");
        }
    }

    let repaired = try_repair_json(&cleaned);
    serde_json::from_str(&repaired)
        .map_err(|e| OmemError::Llm(format!("JSON parse failed after retry: {e}")))
}

fn try_repair_json(s: &str) -> String {
    let s = fix_trailing_commas(s);
    fix_unescaped_chars_in_strings(&s)
}

fn fix_trailing_commas(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut result = String::with_capacity(s.len());

    for i in 0..len {
        if chars[i] == ',' {
            let mut j = i + 1;
            while j < len && chars[j].is_whitespace() {
                j += 1;
            }
            if j < len && (chars[j] == ']' || chars[j] == '}') {
                continue;
            }
        }
        result.push(chars[i]);
    }

    result
}

fn fix_unescaped_chars_in_strings(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 2);
    let mut in_string = false;
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if !in_string {
            result.push(ch);
            if ch == '"' {
                in_string = true;
            }
            continue;
        }

        match ch {
            '\\' => {
                result.push(ch);
                if let Some(next) = chars.next() {
                    result.push(next);
                }
            }
            '"' => {
                result.push(ch);
                in_string = false;
            }
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            _ => result.push(ch),
        }
    }

    result
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
