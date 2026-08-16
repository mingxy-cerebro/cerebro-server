use crate::dream::DreamRequest;

/// Dream 引擎系统提示。对齐项目 prompt 惯例(const 字符串)。
/// 职责六条见 SPEC.md §6.1;type 透传规则见 ADR-5。
const DREAM_SYSTEM_PROMPT: &str = "\
你是记忆整理师(Dream 引擎)。你会收到一份记忆档全文(Markdown,条目带 frontmatter,metadata.type ∈ user/feedback/project/reference)和若干条新的 session 证据。请整理这份记忆档。

职责:
1. 去重合并:多条语义重复的旧条目合并为一条,source=merged
2. 证据更新:旧条目断言被新证据推翻或修订时,更新其内容,source=updated
3. 新知挖掘:session 中含未落档的持久事实,挖出为新条目,source=added,type 由你归类(user/feedback/project/reference 四选一)
4. 原样保留:未受影响的旧条目原样保留,source=kept,内容不改写
5. 淘汰:旧档中过时、无价值的条目不进入 entries
6. type 透传:旧档条目的 frontmatter type 原样保留;无 type 的旧条目和新挖条目由你归类四选一

输出纯 JSON(禁 markdown 围栏、禁 YAML、禁 <think> 标签,直接输出 JSON 对象):
{\"entries\":[{\"name\":\"条目名\",\"description\":\"一句话摘要\",\"type\":\"user|feedback|project|reference\",\"body\":\"完整内容\",\"links\":[\"相关条目名\"],\"source\":\"merged|updated|added|kept\"}],\"stats\":{\"merged\":N,\"updated\":N,\"added\":N,\"dropped\":N,\"total\":N}}

自检要求:
- entries 是完整的新记忆档(包含未变动的条目),不是仅变更清单
- stats.total 必须 == entries 数量
- merged + updated + added + kept 条数 == total
- dropped = 旧档中被淘汰的条目数
- 输出语言跟随记忆档主体语言(中文档输出中文)";

pub fn build_dream_prompt(req: &DreamRequest) -> (String, String) {
    let mut user = String::new();

    user.push_str("# 现有记忆档全文(含 frontmatter,type 字段必须透传)\n\n");
    user.push_str(&req.memory);

    user.push_str("\n\n# 新 session 证据\n");
    for (i, session) in req.sessions.iter().enumerate() {
        user.push_str(&format!("\n## Session {}\n{}\n", i + 1, session));
    }

    if let Some(since) = &req.since {
        user.push_str(&format!("\n# 证据时间下界(since): {since}\n"));
    }

    (DREAM_SYSTEM_PROMPT.to_string(), user)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_req() -> DreamRequest {
        DreamRequest {
            memory: "---\nname: pref\ntype: user\n---\n偏好 cargo build".to_string(),
            sessions: vec!["今天用了 dream 接口".to_string(), "又跑了一遍测试".to_string()],
            since: Some("2026-08-15T00:00:00Z".to_string()),
        }
    }

    #[test]
    fn user_prompt_contains_memory_and_numbered_sessions() {
        let (_, user) = build_dream_prompt(&sample_req());
        assert!(user.contains("偏好 cargo build"));
        assert!(user.contains("## Session 1"));
        assert!(user.contains("## Session 2"));
        assert!(user.contains("今天用了 dream 接口"));
        assert!(user.contains("since): 2026-08-15T00:00:00Z"));
    }

    #[test]
    fn user_prompt_omits_since_when_absent() {
        let mut req = sample_req();
        req.since = None;
        let (_, user) = build_dream_prompt(&req);
        assert!(!user.contains("since"));
    }

    #[test]
    fn system_prompt_declares_schema_type_and_source_rules() {
        let (system, _) = build_dream_prompt(&sample_req());
        // schema 四字段 + source 四值 + type 四值 + 透传规则 + 自检
        assert!(system.contains("\"type\""));
        assert!(system.contains("\"source\""));
        assert!(system.contains("merged|updated|added|kept"));
        assert!(system.contains("user|feedback|project|reference"));
        assert!(system.contains("透传"));
        assert!(system.contains("完整的新记忆档"));
    }
}
