use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq)]
pub struct PreferenceSlot {
    pub brand: String,
    pub item: String,
}

/// Detect "same brand, different item" preference patterns in text.
///
/// Supports:
/// - English: "like/love/prefer X from Y", "prefer Y's X", "like Y X"
/// - Chinese: "喜欢/爱吃/偏爱 品牌 的 商品"
///
/// Returns `Some(PreferenceSlot)` if a brand-item pattern is found, `None` otherwise.
pub fn infer_preference_slot(text: &str) -> Option<PreferenceSlot> {
    for pattern in PATTERNS.iter() {
        if let Some(caps) = pattern.regex.captures(text) {
            let (brand_raw, item_raw) = match pattern.order {
                CaptureOrder::BrandItem => (
                    caps.get(1).map(|m| m.as_str()),
                    caps.get(2).map(|m| m.as_str()),
                ),
                CaptureOrder::ItemBrand => (
                    caps.get(2).map(|m| m.as_str()),
                    caps.get(1).map(|m| m.as_str()),
                ),
            };

            if let (Some(brand), Some(item)) = (brand_raw, item_raw) {
                let brand = brand.trim().to_string();
                let item = item.trim().to_string();
                if !brand.is_empty() && !item.is_empty() {
                    return Some(PreferenceSlot { brand, item });
                }
            }
        }
    }
    None
}

/// Check if candidate's preference slot conflicts with any existing slot
/// (same brand but different item → should force CREATE).
pub fn is_same_brand_different_item(candidate: &PreferenceSlot, existing: &PreferenceSlot) -> bool {
    let brand_match = candidate.brand.to_lowercase() == existing.brand.to_lowercase();
    let item_match = candidate.item.to_lowercase() == existing.item.to_lowercase();
    brand_match && !item_match
}

#[derive(Debug)]
enum CaptureOrder {
    BrandItem,
    ItemBrand,
}

struct SlotPattern {
    regex: Regex,
    order: CaptureOrder,
}

static PATTERNS: LazyLock<Vec<SlotPattern>> = LazyLock::new(|| {
    vec![
        // Chinese: "喜欢/爱吃/偏爱 BRAND 的 ITEM"
        SlotPattern {
            regex: Regex::new(
                r"(?:喜欢|爱吃|偏爱|爱喝|爱用|常喝|常吃|常用|最爱)\s*(.+?)\s*的\s*(.+?)(?:\s*$|[，。,.])"
            ).expect("valid regex"),
            order: CaptureOrder::BrandItem,
        },
        // English: "prefer/like/love ITEM from BRAND"
        SlotPattern {
            regex: Regex::new(
                r"(?i)(?:prefer|like|love|enjoy|drink|eat|use)s?\s+(.+?)\s+from\s+(.+?)(?:\s*$|[,.])"
            ).expect("valid regex"),
            order: CaptureOrder::ItemBrand,
        },
        // English: "prefer/like BRAND's ITEM"
        SlotPattern {
            regex: Regex::new(
                r"(?i)(?:prefer|like|love|enjoy|drink|eat|use)s?\s+(.+?)'s\s+(.+?)(?:\s*$|[,.])"
            ).expect("valid regex"),
            order: CaptureOrder::BrandItem,
        },
        // English: "favorite ITEM is from BRAND" / "favorite ITEM is BRAND's"
        SlotPattern {
            regex: Regex::new(
                r"(?i)favorite\s+(.+?)\s+(?:is\s+from|is)\s+(.+?)(?:\s*$|[,.])"
            ).expect("valid regex"),
            order: CaptureOrder::ItemBrand,
        },
    ]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chinese_brand_item() {
        let slot = infer_preference_slot("喜欢星巴克的拿铁").expect("should match");
        assert_eq!(slot.brand, "星巴克");
        assert_eq!(slot.item, "拿铁");
    }

    #[test]
    fn test_chinese_different_verb() {
        let slot = infer_preference_slot("爱喝瑞幸的美式").expect("should match");
        assert_eq!(slot.brand, "瑞幸");
        assert_eq!(slot.item, "美式");
    }

    #[test]
    fn test_english_from_pattern() {
        let slot = infer_preference_slot("likes latte from Starbucks").expect("should match");
        assert_eq!(slot.brand, "Starbucks");
        assert_eq!(slot.item, "latte");
    }

    #[test]
    fn test_english_possessive_pattern() {
        let slot = infer_preference_slot("prefers Starbucks's latte").expect("should match");
        assert_eq!(slot.brand, "Starbucks");
        assert_eq!(slot.item, "latte");
    }

    #[test]
    fn test_no_match() {
        assert!(infer_preference_slot("user works at Google").is_none());
        assert!(infer_preference_slot("prefers dark mode").is_none());
    }

    #[test]
    fn test_same_brand_different_item() {
        let a = PreferenceSlot {
            brand: "Starbucks".to_string(),
            item: "latte".to_string(),
        };
        let b = PreferenceSlot {
            brand: "Starbucks".to_string(),
            item: "americano".to_string(),
        };
        assert!(is_same_brand_different_item(&a, &b));
    }

    #[test]
    fn test_same_brand_same_item() {
        let a = PreferenceSlot {
            brand: "Starbucks".to_string(),
            item: "latte".to_string(),
        };
        let b = PreferenceSlot {
            brand: "Starbucks".to_string(),
            item: "Latte".to_string(),
        };
        assert!(!is_same_brand_different_item(&a, &b));
    }

    #[test]
    fn test_different_brand() {
        let a = PreferenceSlot {
            brand: "Starbucks".to_string(),
            item: "latte".to_string(),
        };
        let b = PreferenceSlot {
            brand: "Luckin".to_string(),
            item: "latte".to_string(),
        };
        assert!(!is_same_brand_different_item(&a, &b));
    }
}
