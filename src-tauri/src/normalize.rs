use regex::Regex;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

pub fn nfkc(input: &str) -> String {
    input.nfkc().collect()
}

pub fn is_punct(ch: char) -> bool {
    ch.is_ascii_punctuation()
        || matches!(
            ch,
            '・' | '･'
                | '·'
                | '—'
                | '–'
                | '‐'
                | '−'
                | '～'
                | '「'
                | '」'
                | '『'
                | '』'
                | '【'
                | '】'
                | '（'
                | '）'
                | '“'
                | '”'
                | '。'
                | '、'
                | '！'
                | '？'
                | '：'
                | '；'
                | '＊'
                | '＋'
                | '＃'
                | '＆'
                | '　'
        )
}

pub fn is_boundary_char(ch: char) -> bool {
    ch.is_whitespace() || is_punct(ch) || ch.is_ascii_digit()
}

pub fn is_cjk(ch: char) -> bool {
    ('\u{3040}'..='\u{30ff}').contains(&ch)
        || ('\u{3400}'..='\u{9fff}').contains(&ch)
        || ('\u{ac00}'..='\u{d7af}').contains(&ch)
}

/// Fold a raw avatar label to a comparison key.
/// Punctuation and spaces are removed, and trailing 対応 / version markers are stripped.
pub fn normalize_label(input: &str) -> String {
    let folded = nfkc(input).trim().to_lowercase();
    let mut compact = String::new();
    for ch in folded.chars() {
        if ch.is_whitespace() || is_punct(ch) {
            continue;
        }
        compact.push(ch);
    }
    strip_label_tails(&compact)
}

fn strip_label_tails(input: &str) -> String {
    let mut current = input.to_string();
    loop {
        let without_version = strip_version_tail(&current);
        let without_suffix = strip_relation_suffix(&without_version);
        if without_suffix == current {
            break;
        }
        current = without_suffix;
    }
    current
}

fn strip_relation_suffix(input: &str) -> String {
    const SUFFIXES: &[&str] = &["対応", "専用", "向け"];
    for suffix in SUFFIXES {
        if let Some(stripped) = input.strip_suffix(suffix) {
            if stripped.chars().count() >= 2 {
                return stripped.to_string();
            }
        }
    }
    if let Some(stripped) = input.strip_suffix("用") {
        if stripped.chars().count() >= 2 {
            return stripped.to_string();
        }
    }
    input.to_string()
}

fn strip_version_tail(input: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?i)(?:ver\.?|v)\d+(?:\.\d+)*$").unwrap());
    re.replace(input, "").to_string()
}

pub fn quoted_segments(title: &str) -> Vec<String> {
    let pairs = [('「', '」'), ('『', '』'), ('"', '"'), ('“', '”')];
    let chars: Vec<char> = title.chars().collect();
    let mut out = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if let Some((_, end)) = pairs.iter().copied().find(|(start, _)| *start == chars[index]) {
            if let Some(relative) = chars[index + 1..].iter().position(|ch| *ch == end) {
                let segment: String = chars[index + 1..index + 1 + relative].iter().collect();
                if segment.trim().chars().count() >= 1 {
                    out.push(segment.trim().to_string());
                }
                index = index + relative + 2;
                continue;
            }
        }
        index += 1;
    }
    out
}

pub fn suggested_display_name(title: &str) -> String {
    for segment in quoted_segments(title) {
        let count = segment.chars().count();
        if (1..=40).contains(&count) {
            return segment;
        }
    }

    let mut text = nfkc(title);
    for noise in ["オリジナル3Dモデル", "オリジナル3dモデル", "VRChat向け", "VRChat"] {
        text = text.replace(noise, " ");
    }
    static VER: OnceLock<Regex> = OnceLock::new();
    let ver = VER.get_or_init(|| Regex::new(r"(?i)ver\.?\s*\d+(?:\.\d+)*").unwrap());
    text = ver.replace_all(&text, " ").to_string();

    let mut spaced = String::new();
    for ch in text.chars() {
        if ch.is_whitespace() || is_punct(ch) {
            spaced.push(' ');
        } else {
            spaced.push(ch);
        }
    }
    let tokens: Vec<String> = spaced
        .split_whitespace()
        .map(|token| token.to_string())
        .filter(|token| !token.is_empty())
        .take(8)
        .collect();
    if let Some(cjk) = tokens.iter().find(|token| token.chars().any(is_cjk)) {
        return cjk.chars().take(40).collect();
    }
    let joined = tokens.join(" ");
    let trimmed = joined.trim();
    if trimmed.is_empty() {
        "名称未設定".to_string()
    } else {
        trimmed.chars().take(40).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_halfwidth_katakana_and_strips_relation_suffix() {
        assert_eq!(normalize_label("ﾗｲﾑ対応"), "ライム");
        assert_eq!(normalize_label(" Manuka ver1.02 "), "manuka");
        assert_eq!(normalize_label("「しなの」専用"), "しなの");
    }

    #[test]
    fn reads_quoted_display_name() {
        assert_eq!(
            suggested_display_name("オリジナル3Dモデル「マヌカ」ver1.02"),
            "マヌカ"
        );
    }

    #[test]
    fn display_name_prefers_cjk_token() {
        assert_eq!(
            suggested_display_name("ライム -Lime- / オリジナル3Dモデル"),
            "ライム"
        );
    }
}
