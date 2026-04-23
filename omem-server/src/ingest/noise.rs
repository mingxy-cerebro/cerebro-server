use regex::Regex;

const DEFAULT_NOISE_THRESHOLD: f32 = 0.82;
const DEFAULT_MAX_LEARNED: usize = 200;
const DEDUP_SIMILARITY: f32 = 0.95;

pub struct NoiseFilter {
    regex_patterns: Vec<Regex>,
    prototypes: Vec<Vec<f32>>,
    learned: Vec<Vec<f32>>,
    threshold: f32,
    max_learned: usize,
}

impl NoiseFilter {
    pub fn new(prototypes: Vec<Vec<f32>>) -> Self {
        Self {
            regex_patterns: build_regex_patterns(),
            prototypes,
            learned: Vec::new(),
            threshold: DEFAULT_NOISE_THRESHOLD,
            max_learned: DEFAULT_MAX_LEARNED,
        }
    }

    #[cfg(test)]
    fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold;
        self
    }

    #[cfg(test)]
    fn with_max_learned(mut self, max: usize) -> Self {
        self.max_learned = max;
        self
    }

    pub fn is_noise(&self, text: &str, text_vector: Option<&[f32]>) -> bool {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return true;
        }

        for pat in &self.regex_patterns {
            if pat.is_match(trimmed) {
                return true;
            }
        }

        if let Some(vec) = text_vector {
            for proto in self.prototypes.iter().chain(self.learned.iter()) {
                if cosine_similarity(vec, proto) >= self.threshold {
                    return true;
                }
            }
        }

        false
    }

    pub fn learn_noise(&mut self, text_vector: Vec<f32>) {
        for existing in self.prototypes.iter().chain(self.learned.iter()) {
            if cosine_similarity(&text_vector, existing) >= DEDUP_SIMILARITY {
                return;
            }
        }

        if self.learned.len() >= self.max_learned {
            self.learned.remove(0);
        }

        self.learned.push(text_vector);
    }

    pub fn learned_count(&self) -> usize {
        self.learned.len()
    }
}

fn build_regex_patterns() -> Vec<Regex> {
    let patterns = [
        r"(?i)^(hello|hi|hey|greetings|good\s+(morning|afternoon|evening))[\s!.,?]*$",
        r"(?i)^(你好|嗨|早上好|下午好|晚上好|新的一天开始了)[\s!.,?！。？]*$",
        r"(?i)^HEARTBEAT[\s]*$",
        r"(?i)(I\s+don'?t\s+have\s+any\s+information|no\s+relevant\s+memories?\s+found|I\s+couldn'?t\s+find\s+any)",
        r"(?i)(I\s+don'?t\s+have\s+(?:specific\s+)?(?:details?|data|records?)\s+(?:about|on|for))",
        r"(?i)^(do\s+you\s+remember|what\s+do\s+you\s+know\s+about|can\s+you\s+recall)",
        r"(?i)(你还记得|你知道.*吗|你记得.*吗)",
        r"(?i)^(我没有相关的记忆|我没有.*的信息|没有找到相关)[\s]*",
        r"(?i)(query\s*->\s*none|no\s+explicit\s+solution|search\s+returned\s+0\s+results)",
        r"(?i)^(thanks|thank\s+you|ok|okay|sure|got\s+it|understood)[\s!.,?]*$",
        r"(?i)^(谢谢|好的|明白了|收到|了解)[\s!.,?！。？]*$",
    ];

    patterns.iter().filter_map(|p| Regex::new(p).ok()).collect()
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

pub const NOISE_PROTOTYPE_TEXTS: &[&str] = &[
    "Do you remember what I told you?",
    "你还记得我喜欢什么吗",
    "I don't have any information about that",
    "我没有相关的记忆",
    "Hello, how are you doing today?",
    "新的一天开始了",
    "No relevant memories found for this query",
    "I couldn't find any matching records",
    "What do you know about me?",
    "你知道我是谁吗",
    "Thanks for letting me know",
    "好的，我知道了",
    "HEARTBEAT",
    "query -> none, no results",
    "I don't have specific details about that topic",
    "没有找到相关的信息",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_filter() -> NoiseFilter {
        NoiseFilter::new(Vec::new())
    }

    #[test]
    fn test_regex_catches_greeting() {
        let f = empty_filter();
        assert!(f.is_noise("Hello!", None));
        assert!(f.is_noise("hi", None));
        assert!(f.is_noise("Hey!", None));
        assert!(f.is_noise("Good morning!", None));
    }

    #[test]
    fn test_regex_catches_chinese_greeting() {
        let f = empty_filter();
        assert!(f.is_noise("你好", None));
        assert!(f.is_noise("你好！", None));
        assert!(f.is_noise("早上好", None));
        assert!(f.is_noise("新的一天开始了", None));
    }

    #[test]
    fn test_regex_catches_agent_refusal() {
        let f = empty_filter();
        assert!(f.is_noise("I don't have any information about that", None));
        assert!(f.is_noise("No relevant memories found", None));
        assert!(f.is_noise("I couldn't find any matching data", None));
    }

    #[test]
    fn test_regex_catches_meta_question() {
        let f = empty_filter();
        assert!(f.is_noise("do you remember my name?", None));
        assert!(f.is_noise("What do you know about me?", None));
        assert!(f.is_noise("你还记得我喜欢什么吗", None));
    }

    #[test]
    fn test_regex_catches_diagnostic() {
        let f = empty_filter();
        assert!(f.is_noise("query -> none, no results found", None));
        assert!(f.is_noise("search returned 0 results", None));
    }

    #[test]
    fn test_regex_catches_thanks() {
        let f = empty_filter();
        assert!(f.is_noise("thanks", None));
        assert!(f.is_noise("Thank you!", None));
        assert!(f.is_noise("ok", None));
        assert!(f.is_noise("谢谢", None));
        assert!(f.is_noise("好的", None));
    }

    #[test]
    fn test_real_content_not_noise() {
        let f = empty_filter();
        assert!(!f.is_noise("I prefer using Rust for backends", None));
        assert!(!f.is_noise("User works at Stripe on payment infrastructure", None));
        assert!(!f.is_noise("我喜欢用 Rust 写后端服务", None));
        assert!(!f.is_noise("The deployment to production failed on Jan 5", None));
    }

    #[test]
    fn test_empty_text_is_noise() {
        let f = empty_filter();
        assert!(f.is_noise("", None));
        assert!(f.is_noise("   ", None));
    }

    #[test]
    fn test_prototype_matching() {
        let proto = vec![1.0, 0.0, 0.0];
        let f = NoiseFilter::new(vec![proto]);

        let similar_vec = vec![0.99, 0.1, 0.0];
        assert!(f.is_noise("some text", Some(&similar_vec)));

        let different_vec = vec![0.0, 1.0, 0.0];
        assert!(!f.is_noise("some real content", Some(&different_vec)));
    }

    #[test]
    fn test_feedback_learning() {
        let mut f = empty_filter();

        let noise_vec = vec![1.0, 0.0, 0.0];
        f.learn_noise(noise_vec);
        assert_eq!(f.learned_count(), 1);

        let similar_text_vec = vec![0.99, 0.1, 0.0];
        assert!(f.is_noise("future noise", Some(&similar_text_vec)));
    }

    #[test]
    fn test_learning_dedup() {
        let mut f = empty_filter();

        let v1 = vec![1.0, 0.0, 0.0];
        let v2 = vec![0.999, 0.01, 0.0];
        f.learn_noise(v1);
        f.learn_noise(v2);
        assert_eq!(f.learned_count(), 1);
    }

    #[test]
    fn test_learning_cap() {
        let mut f = empty_filter().with_max_learned(5);

        for i in 0..10 {
            let mut v = vec![0.0; 3];
            v[i % 3] = 1.0;
            v[(i + 1) % 3] = (i as f32) * 0.1;
            f.learn_noise(v);
        }

        assert!(f.learned_count() <= 5);
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let v = vec![1.0, 2.0, 3.0];
        let s = cosine_similarity(&v, &v);
        assert!((s - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let s = cosine_similarity(&a, &b);
        assert!(s.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_zero_vector() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![0.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 0.0).abs() < f32::EPSILON);
        assert!((cosine_similarity(&b, &a) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        let s = cosine_similarity(&a, &b);
        assert!((s - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn test_threshold_respected() {
        let proto = vec![1.0, 0.0, 0.0];
        let f = NoiseFilter::new(vec![proto]).with_threshold(0.99);

        let vec_085 = vec![0.85, 0.5, 0.0];
        assert!(!f.is_noise("text", Some(&vec_085)));

        let vec_near = vec![0.999, 0.01, 0.0];
        assert!(f.is_noise("text", Some(&vec_near)));
    }
}
