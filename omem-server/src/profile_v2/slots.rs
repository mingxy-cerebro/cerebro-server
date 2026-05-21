#[derive(Debug, Clone)]
pub struct SlotDefinition {
    pub name: &'static str,
    pub display_name: &'static str,
    pub is_multi: bool,
    pub description: &'static str,
}

pub static BUILTIN_SLOTS: &[SlotDefinition] = &[
    SlotDefinition { name: "communication_style", display_name: "沟通风格", is_multi: false, description: "用户偏好的沟通方式" },
    SlotDefinition { name: "tone", display_name: "语气偏好", is_multi: false, description: "用户偏好的语气" },
    SlotDefinition { name: "code_style", display_name: "代码风格", is_multi: false, description: "用户偏好的代码编写风格" },
    SlotDefinition { name: "error_handling", display_name: "错误处理", is_multi: false, description: "用户偏好的错误处理方式" },
    SlotDefinition { name: "naming_convention", display_name: "命名规范", is_multi: false, description: "用户偏好的变量/函数命名风格" },
    SlotDefinition { name: "testing_strategy", display_name: "测试策略", is_multi: false, description: "用户偏好的测试方法" },
    SlotDefinition { name: "workflow_preference", display_name: "工作流偏好", is_multi: false, description: "用户偏好的开发工作流程" },
    SlotDefinition { name: "commit_style", display_name: "提交风格", is_multi: false, description: "用户偏好的git commit风格" },
    SlotDefinition { name: "emoji_preference", display_name: "Emoji偏好", is_multi: false, description: "用户对emoji使用的偏好" },
    SlotDefinition { name: "self_reference", display_name: "自称方式", is_multi: false, description: "AI自称方式偏好" },
    SlotDefinition { name: "address_style", display_name: "称呼方式", is_multi: false, description: "AI称呼用户的方式偏好" },
    SlotDefinition { name: "language", display_name: "语言", is_multi: true, description: "用户偏好的编程语言" },
    SlotDefinition { name: "framework_preference", display_name: "框架偏好", is_multi: true, description: "用户偏好的开发框架" },
    SlotDefinition { name: "preferred_tools", display_name: "工具偏好", is_multi: true, description: "用户偏好的开发工具" },
];

pub fn is_valid_slot_name(name: &str) -> bool {
    if BUILTIN_SLOTS.iter().any(|s| s.name == name) {
        return true;
    }
    if let Some(rest) = name.strip_prefix("custom:") {
        !rest.is_empty() && rest.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    } else {
        false
    }
}

pub fn get_slot_definition(name: &str) -> Option<&'static SlotDefinition> {
    BUILTIN_SLOTS.iter().find(|s| s.name == name)
}

pub fn is_multi_slot(name: &str) -> bool {
    get_slot_definition(name).map_or(false, |s| s.is_multi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_slots_count() { assert_eq!(BUILTIN_SLOTS.len(), 14); }

    #[test]
    fn single_value_count() { assert_eq!(BUILTIN_SLOTS.iter().filter(|s| !s.is_multi).count(), 11); }

    #[test]
    fn multi_value_count() { assert_eq!(BUILTIN_SLOTS.iter().filter(|s| s.is_multi).count(), 3); }

    #[test]
    fn valid_names() {
        assert!(is_valid_slot_name("language"));
        assert!(is_valid_slot_name("custom:foo_bar"));
    }

    #[test]
    fn invalid_names() {
        assert!(!is_valid_slot_name("CUSTOM:FOO"));
        assert!(!is_valid_slot_name("custom:"));
        assert!(!is_valid_slot_name("bad slot!"));
        assert!(!is_valid_slot_name("custom:UPPER"));
        assert!(!is_valid_slot_name(""));
    }

    #[test]
    fn multi_slot_check() {
        assert!(is_multi_slot("language"));
        assert!(!is_multi_slot("tone"));
    }
}
