use crate::normalize::{is_cjk, nfkc};
use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

const ASCII_STOP: &[&str] = &[
    "model",
    "models",
    "original",
    "avatar",
    "avatars",
    "outfit",
    "outfits",
    "hair",
    "dress",
    "body",
    "base",
    "texture",
    "textures",
    "unity",
    "asset",
    "assets",
    "download",
    "downloads",
    "package",
    "file",
    "data",
    "booth",
    "vrchat",
    "free",
    "sale",
    "ver",
    "version",
    "set",
    "pack",
    "fbx",
    "vrm",
    "psd",
    "blend",
    "prefab",
    "quest",
];

#[derive(Debug, Clone)]
pub struct LabelScore {
    pub product_id: String,
    pub score: f64,
    pub id_hit: bool,
}

pub fn contains_booth_id(label: &str, booth_id: &str) -> bool {
    if booth_id.len() < 5 || !booth_id.chars().all(|ch| ch.is_ascii_digit()) {
        return false;
    }
    let label = nfkc(label);
    let chars: Vec<char> = label.chars().collect();
    let id: Vec<char> = booth_id.chars().collect();
    if chars.len() < id.len() {
        return false;
    }
    for index in 0..=chars.len() - id.len() {
        if chars[index..index + id.len()] != id[..] {
            continue;
        }
        let left_digit = index > 0 && chars[index - 1].is_ascii_digit();
        let right_digit =
            index + id.len() < chars.len() && chars[index + id.len()].is_ascii_digit();
        if !left_digit && !right_digit {
            return true;
        }
    }
    false
}

pub fn score_label(label: &str, title: &str, booth_id: &str) -> (f64, bool) {
    if contains_booth_id(label, booth_id) {
        return (1.0, true);
    }
    let file_spaced = normalize_match_text(label);
    let title_spaced = normalize_match_text(title);
    let file_compact = compact(&file_spaced);
    let title_compact = compact(&title_spaced);
    let mut best: f64 = 0.0;

    if title_compact.chars().count() >= 4 && file_compact.contains(&title_compact) {
        best = best.max(0.94);
    }
    if file_compact.chars().count() >= 6 && title_compact.contains(&file_compact) {
        best = best.max(0.86);
    }
    if ascii_token_hit(&title_spaced, &file_spaced) {
        best = best.max(0.90);
    }
    if title_compact.chars().count() >= 4 && file_compact.chars().count() >= 4 {
        best = best.max(dice(&file_compact, &title_compact));
    }
    (best, false)
}

pub fn choose_match(scores: &[LabelScore]) -> Option<(String, f64)> {
    let mut rows: Vec<&LabelScore> = scores.iter().filter(|score| score.score > 0.0).collect();
    if rows.is_empty() {
        return None;
    }
    rows.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let best = rows[0];
    if best.score < 0.85 {
        return None;
    }
    let second = rows.get(1).map(|score| score.score).unwrap_or(0.0);
    if !best.id_hit && (best.score - second) < 0.04 {
        return None;
    }
    Some((best.product_id.clone(), best.score))
}

fn normalize_match_text(input: &str) -> String {
    let mut text = nfkc(input).to_lowercase();
    if let Some(index) = text.rfind('.') {
        let extension = &text[index + 1..];
        if (1..=12).contains(&extension.len())
            && extension.chars().all(|ch| ch.is_ascii_alphanumeric())
        {
            text.truncate(index);
        }
    }
    for noise in [
        "オリジナル3dモデル",
        "unitypackage",
        "vrchat",
        "booth",
        "対応",
        "専用",
        "向け",
    ] {
        text = text.replace(noise, " ");
    }
    static VER: OnceLock<Regex> = OnceLock::new();
    let ver = VER.get_or_init(|| Regex::new(r"(?i)ver\.?\s*\d+(?:\.\d+)*").unwrap());
    text = ver.replace_all(&text, " ").to_string();

    let mut spaced = String::new();
    for ch in text.chars() {
        if ch.is_whitespace() || (!ch.is_alphanumeric() && !is_cjk(ch)) {
            spaced.push(' ');
        } else {
            spaced.push(ch);
        }
    }
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn compact(input: &str) -> String {
    input.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn ascii_token_hit(title_spaced: &str, file_spaced: &str) -> bool {
    let needles = significant_ascii_tokens(title_spaced);
    if needles.is_empty() {
        return false;
    }
    let haystack: HashSet<String> = file_spaced
        .split_whitespace()
        .filter_map(ascii_token)
        .collect();
    needles.iter().any(|token| haystack.contains(token))
}

fn significant_ascii_tokens(input: &str) -> Vec<String> {
    input
        .split_whitespace()
        .filter_map(ascii_token)
        .filter(|token| token.len() >= 4 && !ASCII_STOP.contains(&token.as_str()))
        .collect()
}

fn ascii_token(token: &str) -> Option<String> {
    let trimmed = token
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
        .to_lowercase();
    if trimmed.len() >= 3 && trimmed.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        Some(trimmed)
    } else {
        None
    }
}

fn dice(left: &str, right: &str) -> f64 {
    let left_grams = bigrams(left);
    let right_grams = bigrams(right);
    if left_grams.is_empty() || right_grams.is_empty() {
        return 0.0;
    }
    let mut counts = std::collections::HashMap::<String, i32>::new();
    for gram in &left_grams {
        *counts.entry(gram.clone()).or_insert(0) += 1;
    }
    let mut overlap = 0;
    for gram in &right_grams {
        if let Some(count) = counts.get_mut(gram) {
            if *count > 0 {
                overlap += 1;
                *count -= 1;
            }
        }
    }
    (2.0 * overlap as f64) / (left_grams.len() + right_grams.len()) as f64
}

fn bigrams(input: &str) -> Vec<String> {
    let chars: Vec<char> = input.chars().collect();
    if chars.len() < 2 {
        return Vec::new();
    }
    chars
        .windows(2)
        .map(|pair| format!("{}{}", pair[0], pair[1]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn booth_id_must_be_bounded_by_non_digits() {
        assert!(contains_booth_id("item-4876459.zip", "4876459"));
        assert!(!contains_booth_id("148764599.zip", "4876459"));
        assert!(!contains_booth_id("48764590.zip", "4876459"));
    }

    #[test]
    fn title_containment_and_english_token() {
        let (contained, id_hit) = score_label(
            "しなの対応ワンピース.zip",
            "しなの対応ワンピース",
            "1000001",
        );
        assert!(!id_hit);
        assert!(contained >= 0.94);

        let (token, _) = score_label(
            "Lime_outfit_v2.unitypackage",
            "ライム -Lime- / オリジナル3Dモデル",
            "1000002",
        );
        assert!(token >= 0.90);

        let (stone, _) = score_label("limestone_pack.zip", "ライム -Lime-", "1000002");
        assert!(stone < 0.85);

        let (short, _) = score_label("服.zip", "服", "1000003");
        assert!(short < 0.85);
    }

    #[test]
    fn ambiguous_scores_are_not_linked() {
        let scores = vec![
            LabelScore {
                product_id: "a".into(),
                score: 0.94,
                id_hit: false,
            },
            LabelScore {
                product_id: "b".into(),
                score: 0.94,
                id_hit: false,
            },
        ];
        assert!(choose_match(&scores).is_none());

        let identified = vec![
            LabelScore {
                product_id: "a".into(),
                score: 1.0,
                id_hit: true,
            },
            LabelScore {
                product_id: "b".into(),
                score: 0.94,
                id_hit: false,
            },
        ];
        assert_eq!(choose_match(&identified).unwrap().0, "a");
    }
}
