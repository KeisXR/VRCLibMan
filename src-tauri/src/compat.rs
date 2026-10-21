use crate::model::{Avatar, Category};
use crate::normalize::is_punct;
use crate::resolve::known_name_in;
use regex::Regex;
use std::sync::OnceLock;

/// Names written in a description's compatibility list.
/// A title such as "25アバター対応" is only a count, so it is ignored.
pub fn listed_avatar_names(description: &str, avatars: &[Avatar]) -> Vec<String> {
    let Some(section) = compatibility_section(description) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for entry in entries(&section) {
        let label = label_from_entry(&entry);
        if label.chars().count() < 2 {
            continue;
        }
        let name = known_name_in(&label, avatars).unwrap_or_else(|| strip_shop(&label));
        let name = name.trim().to_string();
        if name.chars().count() < 2 {
            continue;
        }
        if !names.iter().any(|existing: &String| existing == &name) {
            names.push(name);
        }
    }
    names
}

pub fn category_for_listed_product(title: &str) -> Category {
    if title.contains("ヘア") || title.contains("髪") || title.contains("ウィッグ") {
        Category::Hair
    } else if title.contains("ギミック") || title.contains("シェーダー") {
        Category::Gimmick
    } else {
        Category::Outfit
    }
}

fn compatibility_section(description: &str) -> Option<String> {
    const MARKERS: &[&str] = &["対応アバター", "対応一覧", "対応キャラ", "対応素体"];
    let start = MARKERS.iter().filter_map(|marker| description.find(marker)).min()?;
    let rest = &description[start..];
    let after_heading = rest.find('\n').map(|index| &rest[index + 1..]).unwrap_or("");
    let mut section = String::new();
    for line in after_heading.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_next_heading(trimmed) {
            break;
        }
        section.push_str(trimmed);
        section.push('\n');
    }
    if section.trim().is_empty() {
        None
    } else {
        Some(section)
    }
}

fn is_next_heading(line: &str) -> bool {
    const HEADS: &[&str] = &[
        "基本仕様",
        "内容物",
        "更新履歴",
        "利用規約",
        "製作者",
        "メッシュ",
        "改変",
        "同梱",
        "注意",
    ];
    let lower = line.to_lowercase();
    HEADS.iter().any(|head| line.starts_with(head))
        && !lower.contains("booth.pm")
        && !lower.contains("_ver")
        && !lower.contains("_v")
}

fn entries(section: &str) -> Vec<String> {
    let mut parts = Vec::new();
    if section.contains('"') || section.contains('“') || section.contains('”') {
        for part in section.split(['"', '“', '”']) {
            let trimmed = part.trim();
            if looks_like_entry(trimmed) {
                parts.push(trimmed.to_string());
            }
        }
    }
    if parts.is_empty() {
        for line in section.lines() {
            let trimmed = line.trim().trim_matches(|ch| ch == '"' || ch == '“' || ch == '”');
            if looks_like_entry(trimmed) {
                parts.push(trimmed.to_string());
            }
        }
    }
    parts
}

fn looks_like_entry(text: &str) -> bool {
    if text.chars().count() < 2 {
        return false;
    }
    let lower = text.to_lowercase();
    lower.contains("booth.pm") || lower.contains("_ver") || lower.contains("_v") || lower.contains("__")
}

fn label_from_entry(entry: &str) -> String {
    static URL_RE: OnceLock<Regex> = OnceLock::new();
    static VERSION_RE: OnceLock<Regex> = OnceLock::new();
    let url_re = URL_RE.get_or_init(|| Regex::new(r"https?://\S+").unwrap());
    let version_re = VERSION_RE
        .get_or_init(|| Regex::new(r"(?i)(?:_+|\s)(?:ver\.?|v)[\d_.]+|_{2,}[\d.]+").unwrap());
    let without_url = url_re.replace_all(entry, " ");
    let without_paren = strip_parentheticals(&without_url);
    let without_version = version_re.replace_all(&without_paren, " ");
    without_version.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_parentheticals(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '(' || chars[index] == '（' {
            let mut depth = 1;
            let mut cursor = index + 1;
            while cursor < chars.len() && depth > 0 {
                if chars[cursor] == '(' || chars[cursor] == '（' {
                    depth += 1;
                } else if chars[cursor] == ')' || chars[cursor] == '）' {
                    depth -= 1;
                }
                cursor += 1;
            }
            if depth == 0 {
                out.push(' ');
                index = cursor;
                continue;
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn strip_shop(label: &str) -> String {
    let tokens: Vec<&str> = label.split_whitespace().filter(|token| !token.is_empty()).collect();
    if tokens.len() <= 1 {
        return label.trim().to_string();
    }
    let mut start = 0;
    while start + 1 < tokens.len() && is_shop_token(tokens[start]) {
        start += 1;
    }
    let rest = &tokens[start..];
    if rest.len() <= 1 {
        return rest.join(" ");
    }
    // The version marker sits on the avatar name, so the last word is that name.
    // An earlier word stays only when の/ノ ties it into the name (量産型のらきゃっと ぷらす).
    if let Some(bound) = rest.iter().position(|token| token.contains('の') || token.contains('ノ')) {
        return rest[bound..].join(" ");
    }
    rest[rest.len() - 1].to_string()
}

fn is_shop_token(token: &str) -> bool {
    let trimmed = token.trim_matches(|ch: char| ch.is_whitespace() || is_punct(ch));
    if trimmed.chars().count() < 2 {
        return true;
    }
    const SUFFIXES: &[&str] = &["屋", "工房", "研究所", "ショップ", "通り", "工廠", "商店"];
    if SUFFIXES.iter().any(|suffix| trimmed.ends_with(suffix)) {
        return true;
    }
    trimmed.chars().any(|ch| ch.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn avatar(id: &str, name: &str) -> Avatar {
        Avatar {
            id: id.into(),
            display_name: name.into(),
            base_body: None,
            aliases: Vec::new(),
            is_builtin: true,
            notes: String::new(),
        }
    }

    fn sample_avatars() -> Vec<Avatar> {
        vec![
            avatar("lime", "ライム"),
            avatar("plum", "プラム"),
            avatar("chocolat", "ショコラ"),
            avatar("shinano", "しなの"),
        ]
    }

    const LIST: &str = "\
部屋着(3アバター対応)\n\
対応アバター、モデルデータは付属しません\n\
\"サンプル工房 ライム_ver1.00 https://booth.pm/ja/items/1001\" \
\"プラム_ver1.01 https://booth.pm/ja/items/1002\" \
\"あまとうさぎ ショコラ_ver1.02(Breasts_big 0~100) https://booth.pm/ja/items/1003\" \
\"こまいぬ通り しなの_v1.02(Hip_big 0~100) https://booth.pm/ja/items/1004\"\n\
基本仕様\n\
布の説明です。\n";

    #[test]
    fn list_names_come_from_the_description_not_the_title_count() {
        let names = listed_avatar_names(LIST, &sample_avatars());
        assert_eq!(names, vec!["ライム", "プラム", "ショコラ", "しなの"]);
        assert!(!names.iter().any(|name| name.contains("アバター対応")));
        assert!(!names.iter().any(|name| name.contains("工房") || name.contains("通り")));
    }

    #[test]
    fn nested_parentheses_do_not_swallow_the_avatar_name() {
        let text = "対応アバター\n\"IKUSIA 瑞希_ver2.02(Breast_big(limit) 0~100) https://booth.pm/ja/items/9\"\n基本仕様\n";
        assert_eq!(listed_avatar_names(text, &[]), vec!["瑞希".to_string()]);
        let reading = "対応アバター\n\"Sharu(シャル) ミリアム_v1.8 https://booth.pm/ja/items/8\"\n基本仕様\n";
        assert_eq!(listed_avatar_names(reading, &[]), vec!["ミリアム".to_string()]);
    }

    #[test]
    fn shop_prefix_is_removed_but_a_name_joined_by_no_stays() {
        let text = "\
対応アバター
\"こここや リリエ_ver2.2 https://booth.pm/ja/items/1\"
\"ソラハ_ver1.4 https://booth.pm/ja/items/2\"
\"ぷらすわん 真冬_v1.0.7 https://booth.pm/ja/items/3\"
\"ミルフィ_v1.5.0 https://booth.pm/ja/items/4\"
\"アバターショップ コトブキヤ 旅枕ヨカ__0.9.1 https://booth.pm/ja/items/5\"
\"IMR義体工廠 量産型のらきゃっと ぷらす_ver1.02 https://booth.pm/ja/items/6\"
基本仕様
";
        assert_eq!(
            listed_avatar_names(text, &[]),
            vec![
                "リリエ".to_string(),
                "ソラハ".to_string(),
                "真冬".to_string(),
                "ミルフィ".to_string(),
                "旅枕ヨカ".to_string(),
                "量産型のらきゃっと ぷらす".to_string(),
            ]
        );
    }

    #[test]
    fn sibling_prose_is_not_a_compatibility_list() {
        let text = "素体について\nライム、プラム、ショコラは共通素体です。衣装を着せ替えできます。\n";
        assert!(listed_avatar_names(text, &sample_avatars()).is_empty());
    }
}
