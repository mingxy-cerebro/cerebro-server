use regex::Regex;

pub fn strip_private_content(text: &str) -> String {
    let re = match Regex::new(r"(?si)<private>.*?</private>") {
        Ok(r) => r,
        Err(_) => return text.to_string(),
    };
    re.replace_all(text, "[REDACTED]").to_string()
}

pub fn is_fully_private(text: &str) -> bool {
    let stripped = strip_private_content(text)
        .replace("[REDACTED]", "")
        .trim()
        .to_string();
    stripped.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_private() {
        let input = "my api key is <private>sk-12345</private> and I use it daily";
        let result = strip_private_content(input);
        assert_eq!(result, "my api key is [REDACTED] and I use it daily");
        assert!(!result.contains("sk-12345"));
    }

    #[test]
    fn test_strip_private_multiline() {
        let input = "start\n<private>\nline1\nline2\n</private>\nend";
        let result = strip_private_content(input);
        assert_eq!(result, "start\n[REDACTED]\nend");
    }

    #[test]
    fn test_strip_private_case_insensitive() {
        let input = "<Private>secret</PRIVATE>";
        let result = strip_private_content(input);
        assert_eq!(result, "[REDACTED]");
    }

    #[test]
    fn test_strip_multiple_private_sections() {
        let input = "<private>a</private> public <private>b</private>";
        let result = strip_private_content(input);
        assert_eq!(result, "[REDACTED] public [REDACTED]");
    }

    #[test]
    fn test_no_private_tags() {
        let input = "nothing private here";
        let result = strip_private_content(input);
        assert_eq!(result, "nothing private here");
    }

    #[test]
    fn test_fully_private() {
        let input = "<private>everything is secret</private>";
        assert!(is_fully_private(input));
    }

    #[test]
    fn test_fully_private_multiple() {
        let input = "<private>part1</private> <private>part2</private>";
        assert!(is_fully_private(input));
    }

    #[test]
    fn test_mixed_content_not_fully_private() {
        let input = "<private>secret</private> but this is public";
        assert!(!is_fully_private(input));
    }

    #[test]
    fn test_empty_string() {
        assert!(is_fully_private(""));
    }

    #[test]
    fn test_whitespace_only_after_strip() {
        let input = "  <private>key</private>  ";
        assert!(is_fully_private(input));
    }
}
