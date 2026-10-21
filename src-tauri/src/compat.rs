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
        let Some(name) = choose_name(&label, avatars) else {
            continue;
        };
        if !names.iter().any(|existing: &String| existing == &name) {
            names.push(name);
        }
    }
    names
}

pub fn category_for_listed_product(title: &str, description: &str) -> Category {
    let from_title = classify_text(title, true);
    if from_title != Category::Other {
        return from_title;
    }
    let head: String = description.chars().take(40).collect();
    classify_text(&head, false)
}

fn compatibility_section(description: &str) -> Option<String> {
    let lines: Vec<&str> = description.lines().collect();
    let start = lines.iter().position(|line| {
        let trimmed = line.trim();
        trimmed.chars().count() <= 80 && !trimmed.contains('。') && line_has_marker(trimmed)
    })?;
    let mut section = String::new();
    for line in lines.iter().skip(start + 1) {
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

fn line_has_marker(line: &str) -> bool {
    const MARKERS: &[&str] = &["対応アバター", "対応モデル", "対応一覧", "対応キャラ", "対応素体"];
    MARKERS.iter().any(|marker| line.contains(marker))
}

fn is_next_heading(line: &str) -> bool {
    if line.chars().count() > 48 || looks_like_entry(line) {
        return false;
    }
    let folded: String = line
        .chars()
        .filter(|ch| ch.is_alphanumeric() || is_cjk_char(*ch))
        .collect();
    const HEADS: &[&str] = &[
        "基本仕様",
        "内容物",
        "商品内容",
        "パッケージ",
        "更新",
        "利用規約",
        "製作者",
        "メッシュ",
        "改変",
        "同梱",
        "注意",
        "クレジット",
        "導入",
        "シェーダー",
        "着せ替え",
        "連絡",
        "履歴",
        "ハッシュタグ",
        "推奨",
        "モデル情報",
        "ポリゴン",
        "仕様",
    ];
    HEADS.iter().any(|head| folded.contains(head))
}

fn is_cjk_char(ch: char) -> bool {
    ('\u{3040}'..='\u{30ff}').contains(&ch) || ('\u{3400}'..='\u{9fff}').contains(&ch)
}

fn entries(section: &str) -> Vec<String> {
    let mut parts = Vec::new();
    if section.contains("booth.pm") {
        parts.extend(split_on_item_urls(section));
    }
    if parts.is_empty() && (section.contains('"') || section.contains('“') || section.contains('”')) {
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
    parts.into_iter().filter(|part| acceptable_entry(part)).collect()
}

fn split_on_item_urls(section: &str) -> Vec<String> {
    static URL_RE: OnceLock<Regex> = OnceLock::new();
    let url_re = URL_RE.get_or_init(|| Regex::new(r"https?://\S*booth\.pm\S*").unwrap());
    let mut parts = Vec::new();
    let mut last = 0;
    for found in url_re.find_iter(section) {
        let chunk = section[last..found.end()]
            .trim()
            .trim_matches(|ch: char| ch == '"' || ch == '“' || ch == '”' || ch.is_whitespace());
        if !chunk.is_empty() {
            parts.push(chunk.to_string());
        }
        last = found.end();
    }
    parts
}

fn acceptable_entry(text: &str) -> bool {
    if text.contains('。')
        || text.contains('！')
        || text.contains('？')
        || text.contains('@')
        || text.contains("ます")
        || text.contains("ください")
        || text.contains("利用規約")
        || text.contains("テンプレート")
        || text.contains("シェーダー")
        || text.contains("削除方法")
    {
        return false;
    }
    let chars = text.chars().count();
    if chars > 160 && text.to_lowercase().matches("_v").count() == 0 && text.matches("booth.pm").count() < 2 {
        return false;
    }
    looks_like_entry(text)
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

fn choose_name(label: &str, avatars: &[Avatar]) -> Option<String> {
    let candidate = if let Some(known) = known_name_in(label, avatars) {
        known
    } else if let Some(quoted) = crate::normalize::quoted_segments(label).into_iter().find_map(|segment| {
        let cleaned = tidy_name(&strip_shop(segment.trim()));
        plausible_avatar_name(&cleaned).then_some(cleaned)
    }) {
        quoted
    } else {
        tidy_name(&strip_shop(label))
    };
    plausible_avatar_name(&candidate).then_some(candidate)
}

fn tidy_name(name: &str) -> String {
    name.trim_matches(|ch: char| ch.is_whitespace() || is_punct(ch))
        .to_string()
}

fn plausible_avatar_name(name: &str) -> bool {
    let count = name.chars().count();
    if !(2..=24).contains(&count) || name.starts_with('の') || name.starts_with('ノ') {
        return false;
    }
    if name.contains('。')
        || name.contains('@')
        || name.contains('#')
        || name.contains('【')
        || name.contains('】')
        || name.contains('「')
        || name.contains('」')
        || name.contains("ます")
        || name.contains("ください")
        || name.contains("オリジナル")
        || name.contains("3D")
        || name.contains("３D")
        || name.contains("モデル")
        || name.contains("対応")
        || name.to_lowercase().contains("ver")
    {
        return false;
    }
    const DENY: &[&str] = &[
        "avatar", "liltoon", "unity", "fbx", "psd", "vrchat", "shader", "udon", "prefab", "png", "jpg",
        "clip", "blender", "vrm", "modular",
    ];
    let lower = name.to_lowercase();
    if DENY.iter().any(|word| lower == *word) {
        return false;
    }
    name.split_whitespace().count() <= 4
}

fn classify_text(text: &str, allow_broad: bool) -> Category {
    let lower = text.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| lower.contains(word));
    if has(&["テクスチャ", "texture", "eyetexture", "eye texture", "瞳"]) {
        return Category::Texture;
    }
    if has(&["ヘア", "髪", "ウィッグ", "ポニーテール", "お団子", "前髪", "みつあみ", "hair"]) {
        return Category::Hair;
    }
    if has(&[
        "メガネ",
        "眼鏡",
        "glasses",
        "アクセサリ",
        "ピアス",
        "イヤリング",
        "ネックレス",
        "チョーカー",
        "ヘイロー",
        "halo",
    ]) {
        return Category::Accessory;
    }
    if !allow_broad {
        return Category::Other;
    }
    if has(&["ギミック", "シェーダー", "shader", "ツール"]) {
        return Category::Gimmick;
    }
    if has(&["ワールド", "world", "skybox"]) {
        return Category::World;
    }
    if has(&[
        "衣装",
        "服",
        "部屋着",
        "パーカー",
        "スカート",
        "シャツ",
        "ドレス",
        "ワンピース",
        "techwear",
        "tech wear",
    ]) {
        return Category::Outfit;
    }
    Category::Other
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
    fn text_after_the_list_is_not_an_avatar_name() {
        let instructions = "\
対応アバター
\"サンプル工房 ライム_ver1.00 https://booth.pm/ja/items/1001\"
着せ替え方法
はみ出たメッシュの削除方法 1. アバターの体のメッシュを選択します。
";
        assert_eq!(listed_avatar_names(instructions, &sample_avatars()), vec!["ライム".to_string()]);

        let credits = "\
対応アバター
ライム_ver1.00 https://booth.pm/ja/items/1001
クレジット
ほのさん@example 大葉菜々さん@example
";
        let names = listed_avatar_names(credits, &sample_avatars());
        assert_eq!(names, vec!["ライム".to_string()]);
        assert!(names.iter().all(|name| !name.contains('@')));

        let terms = "\
♥対応アバター♥
ショコラ_ver1.02 https://booth.pm/ja/items/1003 ライム_ver1.00 https://booth.pm/ja/items/1001
୨୧：利用規約：୨୧
のテンプレートを使用しています。本利用規約に同意したものとみなします。
";
        assert_eq!(
            listed_avatar_names(terms, &sample_avatars()),
            vec!["ショコラ".to_string(), "ライム".to_string()]
        );

        let shader = "\
対応アバター
プラム_ver1.01 https://booth.pm/ja/items/1002
✤ 対応シェーダー ✤
導入には下記のシェーダーが必要です。lilToon
Avatar
";
        let names = listed_avatar_names(shader, &sample_avatars());
        assert_eq!(names, vec!["プラム".to_string()]);
        assert!(names.iter().all(|name| name != "lilToon" && name != "Avatar"));
    }

    #[test]
    fn listed_products_are_not_all_outfits() {
        assert_eq!(category_for_listed_product("お団子ポニーテール", ""), Category::Hair);
        assert_eq!(category_for_listed_product("Boston Glasses", ""), Category::Accessory);
        assert_eq!(category_for_listed_product("Noctilume EyeTexture", ""), Category::Texture);
        assert_eq!(category_for_listed_product("部屋着15", "対応アバター"), Category::Outfit);
        assert_eq!(category_for_listed_product("40アバター対応", "対応アバター"), Category::Other);
    }

    #[test]
    fn sibling_prose_is_not_a_compatibility_list() {
        let text = "素体について\nライム、プラム、ショコラは共通素体です。衣装を着せ替えできます。\n";
        assert!(listed_avatar_names(text, &sample_avatars()).is_empty());
    }
}
