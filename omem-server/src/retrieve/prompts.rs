use serde::Deserialize;

#[derive(Deserialize)]
pub struct RefineItem {
    pub id: String,
    pub relevance: String,
    pub reasoning: String,
}

#[derive(Deserialize)]
pub struct RefineResponse {
    pub items: Vec<RefineItem>,
}

pub const REFINE_SYSTEM_PROMPT: &str = r#"You are a memory relevance judge. Given a user's current conversation context and a list of candidate memories, judge each memory's relevance to the conversation.

## Your Task
For each memory, output exactly one relevance level with a brief reasoning.

## Relevance Levels
- **high**: Directly answers or closely relates to the user's current question/conversation. Contains specific facts, code, config, or context the user would need.
- **medium**: Tangentially related or provides useful background context, but not directly needed for the current question.
- **irrelevant**: No meaningful connection to the current conversation.

## Rules
1. Judge ONLY based on the user's current question and conversation context provided.
2. Do NOT judge based on general topic similarity — be specific about whether the memory helps answer THIS question.
3. Preserve the user's original language — respond in the same language as the user's question.
4. Keep reasoning to ONE short sentence.
5. Output valid JSON only, no markdown fences.

## Output Format
{"items": [{"id": "<memory_id>", "relevance": "high|medium|irrelevant", "reasoning": "<one sentence>"}]}
"#;

pub fn build_refine_user_prompt(
    memories: &[(String, &str, Option<&str>)],
    query: &str,
    conversation_context: Option<&[String]>,
) -> String {
    let mut prompt = String::with_capacity(2048);

    prompt.push_str("## User's Current Question\n");
    prompt.push_str(query);
    prompt.push('\n');

    if let Some(ctx) = conversation_context {
        if !ctx.is_empty() {
            prompt.push_str("\n## Recent Conversation Context\n");
            for msg in ctx {
                prompt.push_str("- ");
                prompt.push_str(msg);
                prompt.push('\n');
            }
        }
    }

    prompt.push_str("\n## Candidate Memories\n");
    for (id, content, l1) in memories {
        prompt.push_str(&format!("[id:{}] ", id));
        if let Some(overview) = l1 {
            prompt.push_str(&format!("(overview: {}) ", overview));
            let truncated = if content.chars().count() > 300 {
                let end = content.char_indices().nth(300).map(|(i, _)| i).unwrap_or(content.len());
                &content[..end]
            } else {
                content
            };
            prompt.push_str(truncated);
        } else {
            let truncated = if content.chars().count() > 500 {
                let end = content.char_indices().nth(500).map(|(i, _)| i).unwrap_or(content.len());
                &content[..end]
            } else {
                content
            };
            prompt.push_str(truncated);
        }
        prompt.push('\n');
    }

    prompt.push_str("\nJudge each memory's relevance. Output JSON only.");
    prompt
}
