use crate::model::{Avatar, Category};
use crate::normalize::{is_blocked_avatar_label, is_punct, nfkc};
use crate::resolve::{known_name_in, known_names_in};
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

/// Only a title that says the product itself is an original avatar overrides the model.
/// A 【対応】/【専用】 bracket never makes the product an avatar.
pub fn strong_category_override(title: &str) -> Option<Category> {
    let folded = nfkc(title).to_lowercase();
    if folded.contains("オリジナル3dモデル") {
        Some(Category::Avatar)
    } else {
        None
    }
}

/// Title phrases used only after BOOTH category and tags decline to settle.
/// A settled BOOTH category stays as it is. Locks are handled by the caller.
pub fn title_fill_category(title: &str, model: Category) -> Option<Category> {
    let folded = nfkc(title).to_lowercase();
    if avatar_package(&folded) {
        return Some(Category::Avatar);
    }
    if has_world_prop(&folded) {
        return Some(Category::World);
    }
    let keyword = category_for_listed_product(title, "");
    if model == Category::Other && keyword != Category::Other {
        return Some(keyword);
    }
    if model == Category::Gimmick && keyword == Category::Accessory {
        return Some(keyword);
    }
    None
}

/// A title that lists avatar distributable formats, not a wearable that happens to ship a package.
fn avatar_package(lower: &str) -> bool {
    if blocks_avatar_package(lower) {
        return false;
    }
    if lower.contains("editable model data") {
        return true;
    }
    static SLASH: OnceLock<Regex> = OnceLock::new();
    let slash = SLASH.get_or_init(|| Regex::new(r"vrchat\s*/\s*3d\s*model").unwrap());
    if slash.is_match(lower) {
        return true;
    }
    let markers = [".xavater", ".vroid", ".unitypackage"];
    markers
        .iter()
        .filter(|marker| lower.contains(*marker))
        .count()
        >= 2
}

fn blocks_avatar_package(lower: &str) -> bool {
    const WORDS: &[&str] = &[
        "テクスチャ",
        "texture",
        "瞳",
        "メイク",
        "ニップル",
        "nipple",
        "ヘア",
        "髪",
        "ウィッグ",
        "hair",
        "メガネ",
        "眼鏡",
        "glasses",
        "アクセサリ",
        "ピアス",
        "イヤリング",
        "earring",
        "ネックレス",
        "チョーカー",
        "ヘイロー",
        "halo",
        "ディスク",
        "カード",
        "衣装",
        "服",
        "部屋着",
        "ルームウェア",
        "パーカー",
        "スカート",
        "シャツ",
        "ドレス",
        "ワンピース",
        "techwear",
        "ずぼん",
        "ズボン",
        "fullpack",
        "ギミック",
        "gimmick",
        "表情",
        "こたつ",
        "毛布",
        "布団",
    ];
    WORDS.iter().any(|word| lower.contains(word))
        || crate::normalize::contains_ascii_word(lower, "ring")
        || crate::normalize::contains_ascii_word(lower, "bag")
}

fn has_world_prop(lower: &str) -> bool {
    const WORDS: &[&str] = &[
        "こたつ",
        "毛布",
        "布団",
        "huton",
        "家具",
        "qvpen",
        "差し替えモデル",
        "pencil",
        "スイッチ",
        "asmr",
        "affinity",
    ];
    WORDS.iter().any(|word| lower.contains(word))
}

/// World assets beat a weak model guess. `ギミック` alone stays an avatar gimmick.
/// A world gimmick needs a world signal: ワールド, world, Udon, UdonSharp,
/// ワールド用, ワールドギミック, a shader for a world, or a world prefab.
pub fn world_hint_overrides(
    title: &str,
    description: &str,
    model: Category,
    confidence: f64,
) -> bool {
    let title_strength = world_strength(title);
    let body_strength = world_strength(description);
    if title_strength == 0 && body_strength == 0 {
        return false;
    }
    if title_strength >= 2 || body_strength >= 2 {
        return model != Category::Avatar || title_strength >= 2;
    }
    if title_strength == 1 {
        return true;
    }
    let described = nfkc(description).to_lowercase();
    let weak_model = matches!(
        model,
        Category::Gimmick | Category::Other | Category::Accessory | Category::World
    );
    if has_jp_world_asset(&described) {
        return weak_model || (confidence < 0.6 && model != Category::Avatar);
    }
    confidence < 0.6 && model != Category::Avatar
}

fn world_strength(text: &str) -> u8 {
    let lower = nfkc(text).to_lowercase();
    if lower.is_empty() {
        return 0;
    }
    if strong_world(&lower) {
        2
    } else if has_world_word(&lower) || has_jp_world_asset(&lower) {
        1
    } else {
        0
    }
}

fn strong_world(lower: &str) -> bool {
    if lower.contains("udon") || lower.contains("skybox") {
        return true;
    }
    const MARKED: &[&str] = &[
        "ワールド用",
        "ワールドギミック",
        "ワールドシェーダ",
        "ワールドプレハブ",
        "ワールドプロップ",
    ];
    if MARKED.iter().any(|phrase| lower.contains(phrase)) {
        return true;
    }
    let worldish = has_world_word(lower)
        || lower.contains("worldprefab")
        || lower.contains("worldshader")
        || lower.contains("worldgimmick")
        || lower.contains("worldprop")
        || lower.contains("worldtool");
    if !worldish {
        return false;
    }
    const WITH: &[&str] = &[
        "prefab",
        "プレハブ",
        "shader",
        "シェーダ",
        "gimmick",
        "ギミック",
        "prop",
        "tool",
        "ツール",
    ];
    WITH.iter().any(|phrase| lower.contains(phrase))
}

fn has_jp_world_asset(lower: &str) -> bool {
    lower.replace("ワールド風", "").contains("ワールド")
}

fn has_world_word(lower: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?:^|[^a-z])worlds?(?:[^a-z]|$)").unwrap());
    re.is_match(lower)
}

pub fn category_for_listed_product(title: &str, description: &str) -> Category {
    let from_title = classify_text(title, true);
    if from_title != Category::Other {
        return from_title;
    }
    let head: String = description.chars().take(160).collect();
    let folded = nfkc(&head).to_lowercase();
    if folded.contains("オリジナル3dモデル") {
        return Category::Avatar;
    }
    let short: String = description.chars().take(40).collect();
    classify_text(&short, false)
}

pub fn title_avatar_hints(title: &str, avatars: &[Avatar]) -> Vec<String> {
    let mut names = Vec::new();
    let mut compounds = Vec::new();
    let mut push = |name: &str| {
        let cleaned = name
            .trim()
            .trim_matches(|ch: char| ch.is_whitespace() || is_punct(ch));
        if cleaned.is_empty() {
            return;
        }
        if is_blocked_avatar_label(cleaned) {
            let Some(avatar) = crate::resolve::resolve_name(cleaned, avatars) else {
                return;
            };
            let display = avatar.display_name.clone();
            if !names.iter().any(|existing| existing == &display) {
                names.push(display);
            }
            return;
        }
        if !names.iter().any(|existing: &String| existing == cleaned) {
            names.push(cleaned.to_string());
        }
    };
    // 【X専用】/【X対応】 is an explicit name even when the library does not know it yet.
    for inner in bracket_inners(title) {
        if let Some(avatar) = crate::resolve::resolve_name(inner.trim(), avatars) {
            remember_compound(&mut compounds, inner.trim());
            remember_compound(&mut compounds, &avatar.display_name);
            push(&avatar.display_name);
            continue;
        }
        if let Some(stem) = dedication_stem(&inner) {
            if let Some(avatar) = crate::resolve::resolve_name(stem, avatars) {
                remember_compound(&mut compounds, stem);
                remember_compound(&mut compounds, &avatar.display_name);
                push(&avatar.display_name);
                continue;
            }
        }
        if let Some(name) = crate::normalize::dedication_inside(&inner) {
            let mut pieces = Vec::new();
            emit_dedication_name(&name, avatars, &mut pieces);
            for piece in pieces {
                push(&piece);
            }
        }
    }
    // Search the original title. Stripping 【プラム Plum】 would delete the name
    // along with the brackets. Promo text still cannot become a name unless it
    // is already an avatar, and those promo avatars are not matchable.
    // A dotted name that is one avatar hides its pieces, unless a piece is also written outside that name.
    let discovered = known_names_in(title, avatars);
    for name in &discovered {
        remember_compound(&mut compounds, name);
    }
    let outside = if compounds.is_empty() {
        Vec::new()
    } else {
        known_names_in(&mask_dotted_compounds(title, &compounds), avatars)
    };
    for name in discovered {
        let buried = compounds
            .iter()
            .any(|compound| is_dotted_piece(compound, &name))
            && !outside.iter().any(|kept| kept == &name);
        if !buried {
            push(&name);
        }
    }
    names
}

fn remember_compound(compounds: &mut Vec<String>, source: &str) {
    if !(source.contains('・') || source.contains('･')) {
        return;
    }
    if !compounds.iter().any(|existing| existing == source) {
        compounds.push(source.to_string());
    }
}

fn is_dotted_piece(compound: &str, name: &str) -> bool {
    compound != name
        && compound
            .split(|ch| matches!(ch, '・' | '･'))
            .any(|piece| piece.trim() == name)
}

fn mask_dotted_compounds(title: &str, compounds: &[String]) -> String {
    let mut masked = title.to_string();
    for compound in compounds {
        if compound.contains('・') || compound.contains('･') {
            masked = masked.replace(compound.as_str(), " ");
        }
    }
    masked
}

fn dedication_stem(inner: &str) -> Option<&str> {
    let original = inner.trim();
    let stem = original
        .strip_suffix("専用")
        .or_else(|| original.strip_suffix("対応"))?
        .trim();
    if stem.is_empty() { None } else { Some(stem) }
}

fn emit_dedication_name(name: &str, avatars: &[Avatar], names: &mut Vec<String>) {
    let mut push = |piece: &str| {
        let cleaned = piece
            .trim()
            .trim_matches(|ch: char| ch.is_whitespace() || is_punct(ch));
        if cleaned.is_empty() {
            return;
        }
        if is_blocked_avatar_label(cleaned) {
            let Some(avatar) = crate::resolve::resolve_name(cleaned, avatars) else {
                return;
            };
            let display = avatar.display_name.clone();
            if !names.iter().any(|existing| existing == &display) {
                names.push(display);
            }
            return;
        }
        if !names.iter().any(|existing: &String| existing == cleaned) {
            names.push(cleaned.to_string());
        }
    };
    if let Some(avatar) = crate::resolve::resolve_name(name, avatars) {
        push(&avatar.display_name);
        return;
    }
    let compound = name.contains('・') || name.contains('･');
    if !compound {
        for variant in crate::normalize::label_variants(name) {
            if !variant.contains('(') {
                push(&variant);
            }
        }
    }
    for piece in name.split(|ch| matches!(ch, '/' | '／' | '、' | '&' | '＆' | '・' | '･')) {
        push(piece);
    }
}

/// Names written in 【桔梗/萌対応】. One-character names stay so a known avatar can match.
pub fn explicit_dedication_pieces(title: &str, avatars: &[Avatar]) -> Vec<String> {
    let mut names = Vec::new();
    for inner in bracket_inners(title) {
        if let Some(stem) = dedication_stem(&inner) {
            if let Some(avatar) = crate::resolve::resolve_name(stem, avatars) {
                if !names
                    .iter()
                    .any(|existing| existing == &avatar.display_name)
                {
                    names.push(avatar.display_name.clone());
                }
                continue;
            }
        }
        let Some(name) = crate::normalize::dedication_inside(&inner) else {
            continue;
        };
        emit_dedication_name(&name, avatars, &mut names);
    }
    names
}

/// A known avatar written in the title with a version, such as 「クルミ ver1.2」,
/// stays as that whole string even when the model omits the version.
fn is_name_separator(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '/' | '／'
                | '、'
                | '【'
                | '】'
                | '「'
                | '」'
                | '('
                | ')'
                | '（'
                | '）'
                | '_'
                | '・'
                | '･'
        )
}

pub fn versioned_known_names(title: &str, avatars: &[Avatar]) -> Vec<String> {
    let mut names = Vec::new();
    let pattern = Regex::new(r"(?i)ver\.?\s*\d+(?:\.\d+)*").expect("version pattern");
    let chars: Vec<(usize, char)> = title.char_indices().collect();
    for found in pattern.find_iter(title) {
        let Some(ver_index) = chars.iter().position(|(index, _)| *index == found.start()) else {
            continue;
        };
        let mut cursor = ver_index;
        while cursor > 0 && (chars[cursor - 1].1.is_whitespace() || chars[cursor - 1].1 == '_') {
            cursor -= 1;
        }
        let name_end = cursor;
        while cursor > 0 && !is_name_separator(chars[cursor - 1].1) {
            cursor -= 1;
        }
        if name_end == 0 || cursor >= name_end {
            continue;
        }
        let base = title[chars[cursor].0..chars[name_end].0]
            .trim()
            .trim_matches('_');
        if base.chars().count() < 2 || known_names_in(base, avatars).is_empty() {
            continue;
        }
        let full = title[chars[cursor].0..found.end()]
            .trim()
            .trim_matches('_')
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !full.is_empty() && !names.iter().any(|existing: &String| existing == &full) {
            names.push(full);
        }
    }
    names
}

pub fn names_for_extraction(
    title: &str,
    description: &str,
    llm_names: &[String],
    avatars: &[Avatar],
) -> Vec<String> {
    let listed = listed_avatar_names(description, avatars);
    let hints = title_avatar_hints(title, avatars);
    let mut names = Vec::new();
    let mut push = |name: &str| {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return;
        }
        if is_blocked_avatar_label(trimmed) {
            let Some(avatar) = crate::resolve::resolve_name(trimmed, avatars) else {
                return;
            };
            let display = avatar.display_name.clone();
            if !names.iter().any(|existing| existing == &display) {
                names.push(display);
            }
            return;
        }
        if !names.iter().any(|existing: &String| existing == trimmed) {
            names.push(trimmed.to_string());
        }
    };
    for name in &listed {
        push(name);
    }
    for name in &hints {
        push(name);
    }
    for name in versioned_known_names(title, avatars) {
        push(&name);
    }
    for name in versioned_known_names(description, avatars) {
        push(&name);
    }
    if listed.is_empty() {
        for name in llm_names {
            push(name);
        }
    }
    names
}

pub fn clip_description(description: &str, budget: usize) -> String {
    if description.chars().count() <= budget {
        return description.to_string();
    }
    let Some(section) = compatibility_section(description) else {
        return description.chars().take(budget).collect();
    };
    let section: String = section.chars().take(budget / 2).collect();
    if let Some(first) = section
        .lines()
        .find(|line| line.trim().chars().count() >= 4)
    {
        let head: String = description.chars().take(budget).collect();
        if head.contains(first.trim()) {
            return head;
        }
    }
    let head_budget = budget.saturating_sub(section.chars().count() + 5);
    let mut clipped: String = description.chars().take(head_budget).collect();
    clipped.push_str("\n...\n");
    clipped.push_str(&section);
    clipped
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
    const MARKERS: &[&str] = &[
        "対応アバター",
        "対応モデル",
        "対応一覧",
        "対応キャラ",
        "対応素体",
    ];
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
    if parts.is_empty() && (section.contains('"') || section.contains('“') || section.contains('”'))
    {
        for part in section.split(['"', '“', '”']) {
            let trimmed = part.trim();
            if looks_like_entry(trimmed) {
                parts.push(trimmed.to_string());
            }
        }
    }
    if parts.is_empty() {
        for line in section.lines() {
            let trimmed = line
                .trim()
                .trim_matches(|ch| ch == '"' || ch == '“' || ch == '”');
            if looks_like_entry(trimmed) {
                parts.push(trimmed.to_string());
            }
        }
    }
    parts
        .into_iter()
        .filter(|part| acceptable_entry(part))
        .collect()
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
    if chars > 160
        && text.to_lowercase().matches("_v").count() == 0
        && text.matches("booth.pm").count() < 2
    {
        return false;
    }
    looks_like_entry(text)
}

fn looks_like_entry(text: &str) -> bool {
    if text.chars().count() < 2 {
        return false;
    }
    let lower = text.to_lowercase();
    lower.contains("booth.pm")
        || lower.contains("_ver")
        || lower.contains("_v")
        || lower.contains("__")
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
    without_version
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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
    let tokens: Vec<&str> = label
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .collect();
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
    if let Some(bound) = rest
        .iter()
        .position(|token| token.contains('の') || token.contains('ノ'))
    {
        return rest[bound..].join(" ");
    }
    rest[rest.len() - 1].to_string()
}

fn choose_name(label: &str, avatars: &[Avatar]) -> Option<String> {
    let candidate = if let Some(known) = known_name_in(label, avatars) {
        known
    } else if let Some(quoted) = crate::normalize::quoted_segments(label)
        .into_iter()
        .find_map(|segment| {
            let cleaned = tidy_name(&strip_shop(segment.trim()));
            plausible_avatar_name(&cleaned).then_some(cleaned)
        })
    {
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
    if is_blocked_avatar_label(name) || name.starts_with('の') || name.starts_with('ノ') {
        return false;
    }
    if name.contains("対応") || name.to_lowercase().contains("ver") {
        return false;
    }
    name.split_whitespace().count() <= 4
}

fn bracket_inners(title: &str) -> Vec<String> {
    let chars: Vec<char> = title.chars().collect();
    let mut inners = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '【' {
            if let Some(end) = chars[index + 1..].iter().position(|ch| *ch == '】') {
                let inner: String = chars[index + 1..index + 1 + end].iter().collect();
                inners.push(inner);
                index += end + 2;
                continue;
            }
        }
        index += 1;
    }
    inners
}

fn classify_text(text: &str, allow_broad: bool) -> Category {
    let lower = nfkc(text).to_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| lower.contains(word));
    let word = |token: &str| crate::normalize::contains_ascii_word(&lower, token);
    if has(&[
        "テクスチャ",
        "texture",
        "eyetexture",
        "eye texture",
        "瞳",
        "ニップル",
        "nipple",
    ]) {
        return Category::Texture;
    }
    if has(&[
        "ヘア",
        "髪",
        "ウィッグ",
        "ポニーテール",
        "お団子",
        "前髪",
        "みつあみ",
        "hair",
    ]) {
        return Category::Hair;
    }
    if has(&[
        "メガネ",
        "眼鏡",
        "glasses",
        "アクセサリ",
        "ピアス",
        "イヤリング",
        "earring",
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
    if has(&["ディスク", "カード"]) || word("ring") || word("bag") {
        return Category::Accessory;
    }
    if has(&["オリジナル3dモデル"]) || avatar_package(&lower) {
        return Category::Avatar;
    }
    if world_strength(text) >= 2 || has_world_prop(&lower) {
        return Category::World;
    }
    if has(&[
        "表情",
        "アニメーション",
        "animation",
        "ボディ",
        "なでなで",
        "kisssystem",
        "キス",
        "スマートフォン",
        "シェイプキー",
        "shapekey",
        "shape key",
        "face tracking",
        "facetracking",
        "フェイストラッキング",
    ]) || has_body_word(&lower)
        || word("sps")
        || word("phone")
    {
        return Category::Gimmick;
    }
    if has(&["ギミック", "シェーダー", "shader", "ツール"]) {
        return Category::Gimmick;
    }
    if has(&[
        "衣装",
        "服",
        "部屋着",
        "ルームウェア",
        "パーカー",
        "スカート",
        "シャツ",
        "ドレス",
        "ワンピース",
        "techwear",
        "tech wear",
        "ずぼん",
        "ズボン",
        "fullpack",
        "full pack",
    ]) {
        return Category::Outfit;
    }
    if has(&["キット"]) {
        return Category::Gimmick;
    }
    if world_strength(text) == 1 {
        return Category::World;
    }
    Category::Other
}

fn has_body_word(lower: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?:^|[^a-z])body(?:[^a-z]|$)").unwrap());
    re.is_match(lower)
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
            owned: false,
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
        assert!(
            !names
                .iter()
                .any(|name| name.contains("工房") || name.contains("通り"))
        );
    }

    #[test]
    fn nested_parentheses_do_not_swallow_the_avatar_name() {
        let text = "対応アバター\n\"IKUSIA 瑞希_ver2.02(Breast_big(limit) 0~100) https://booth.pm/ja/items/9\"\n基本仕様\n";
        assert_eq!(listed_avatar_names(text, &[]), vec!["瑞希".to_string()]);
        let reading =
            "対応アバター\n\"Sharu(シャル) ミリアム_v1.8 https://booth.pm/ja/items/8\"\n基本仕様\n";
        assert_eq!(
            listed_avatar_names(reading, &[]),
            vec!["ミリアム".to_string()]
        );
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
        assert_eq!(
            listed_avatar_names(instructions, &sample_avatars()),
            vec!["ライム".to_string()]
        );

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
        assert!(
            names
                .iter()
                .all(|name| name != "lilToon" && name != "Avatar")
        );
    }

    #[test]
    fn listed_products_are_not_all_outfits() {
        assert_eq!(
            category_for_listed_product("お団子ポニーテール", ""),
            Category::Hair
        );
        assert_eq!(
            category_for_listed_product("Boston Glasses", ""),
            Category::Accessory
        );
        assert_eq!(
            category_for_listed_product("Noctilume EyeTexture", ""),
            Category::Texture
        );
        assert_eq!(
            category_for_listed_product("部屋着15", "対応アバター"),
            Category::Outfit
        );
        assert_eq!(
            category_for_listed_product("40アバター対応", "対応アバター"),
            Category::Other
        );
        assert_eq!(
            category_for_listed_product("Noctilume EyeTexture", ""),
            Category::Texture
        );
        assert_eq!(category_for_listed_product("こたつ", ""), Category::World);
        assert_eq!(category_for_listed_product("毛布", ""), Category::World);
        assert_eq!(
            category_for_listed_product("磁気ディスク", ""),
            Category::Accessory
        );
        assert_eq!(
            category_for_listed_product("ネオンヘイロー", ""),
            Category::Accessory
        );
        assert_eq!(
            category_for_listed_product("まめひなた表情アニメーション", ""),
            Category::Gimmick
        );
        assert_eq!(category_for_listed_product("huton", ""), Category::World);
        assert_eq!(
            category_for_listed_product("まめひなた body", ""),
            Category::Gimmick
        );
        assert_eq!(
            category_for_listed_product("VRChat向け3Dモデル", ""),
            Category::Other
        );
        assert_eq!(
            category_for_listed_product("ライム -Lime-", "ライムのオリジナル3Dモデルです。"),
            Category::Avatar
        );
    }

    #[test]
    fn world_signals_do_not_swallow_avatar_gimmicks() {
        assert_eq!(
            category_for_listed_product("木製のワールド小物", ""),
            Category::World
        );
        assert_eq!(
            category_for_listed_product("Udonのスイッチ", ""),
            Category::World
        );
        assert_eq!(
            category_for_listed_product("UdonSharpツール", ""),
            Category::World
        );
        assert_eq!(
            category_for_listed_product("ワールド用シェーダー", ""),
            Category::World
        );
        assert_eq!(
            category_for_listed_product("World Prefab", ""),
            Category::World
        );
        assert_eq!(
            category_for_listed_product("表情ギミック", ""),
            Category::Gimmick
        );
        assert_eq!(
            category_for_listed_product("ワールド風パーカー", ""),
            Category::Outfit
        );
        assert_eq!(
            category_for_listed_product("汎用の小物", "どのアバターでも使える小物です"),
            Category::Other
        );
        assert!(world_hint_overrides(
            "木製のワールド小物",
            "",
            Category::Gimmick,
            0.95
        ));
        assert!(world_hint_overrides(
            "見本の小物",
            "ワールドに置く小物です",
            Category::Gimmick,
            0.95
        ));
        assert!(world_hint_overrides(
            "ワールド用シェーダー",
            "",
            Category::Outfit,
            0.95
        ));
        assert!(!world_hint_overrides(
            "表情ギミック",
            "any world",
            Category::Gimmick,
            0.9
        ));
        assert!(!world_hint_overrides(
            "見本の衣装",
            "a world",
            Category::Outfit,
            0.9
        ));
        assert!(!world_hint_overrides(
            "汎用の小物",
            "どのアバターでも使える小物です",
            Category::Gimmick,
            0.9
        ));
        assert!(!world_hint_overrides(
            "サンプルこたつ",
            "",
            Category::Gimmick,
            0.95
        ));
        assert_eq!(
            title_fill_category("サンプルこたつ", Category::Gimmick),
            Some(Category::World)
        );
        assert!(!world_hint_overrides(
            "サンプルペンライト",
            "",
            Category::Gimmick,
            0.95
        ));
        assert_eq!(
            strong_category_override("【VRChat / 3D model】サンプル子"),
            None
        );
        assert_eq!(
            title_fill_category("【VRChat / 3D model】サンプル子", Category::Other),
            Some(Category::Avatar)
        );
        assert_eq!(
            strong_category_override("サンプル衣装 (.xavater / .vroid)"),
            None
        );
        assert_eq!(strong_category_override("VRChat向け3Dモデル"), None);
    }

    #[test]
    fn title_dedication_is_a_name_and_sale_brackets_are_not() {
        let hints = title_avatar_hints("【プラム専用】Hairグラデーション", &sample_avatars());
        assert_eq!(hints, vec!["プラム".to_string()]);
        let sale = title_avatar_hints("【半額セール中】真冬 Mafuyu / オリジナル3Dモデル", &[]);
        assert!(sale.iter().all(|name| name != "半額セール中"));
        assert!(title_avatar_hints("架空ちゃん対応パーカー", &[]).is_empty());
        assert_eq!(
            title_avatar_hints("【架空ちゃん専用】パーカー", &[]),
            vec!["架空ちゃん".to_string()]
        );
        assert!(title_avatar_hints("【MA対応】ペン", &sample_avatars()).is_empty());
        let mut kipfel = sample_avatars();
        kipfel.push(Avatar {
            id: "kipfel".into(),
            display_name: "キプフェル".into(),
            base_body: None,
            aliases: vec!["Kipfel".into()],
            is_builtin: false,
            owned: false,
            notes: String::new(),
        });
        assert_eq!(
            title_avatar_hints("【Kipfel】ねこネコパーカー", &kipfel),
            vec!["キプフェル".to_string()]
        );
        let eye = title_avatar_hints(
            "【プラム Plum】Eye Textures Pack (Full Pack)",
            &sample_avatars(),
        );
        assert_eq!(eye, vec!["プラム".to_string()]);
        let outfit = title_avatar_hints("しなの・ライム・マヌカ対応 ワンピース", &sample_avatars());
        assert!(outfit.contains(&"しなの".to_string()));
        assert!(outfit.contains(&"ライム".to_string()));
        assert!(outfit.iter().all(|name| name != "ワンピース"));
    }

    #[test]
    fn clip_keeps_a_compatibility_section_past_the_head() {
        let mut description = "導入文です。\n".repeat(400);
        description.push_str(
            "対応アバター\nライム_ver1.00 https://booth.pm/ja/items/1001\n基本仕様\n布です。\n",
        );
        let clipped = clip_description(&description, 500);
        assert!(clipped.contains("ライム_ver1.00"));
        assert!(clipped.chars().count() < description.chars().count());
    }

    #[test]
    fn a_known_avatar_keeps_the_version_written_in_the_title() {
        let avatars = vec![avatar("kurumi", "クルミ")];
        let names = names_for_extraction("クルミ ver1.2 対応のワンピース", "", &[], &avatars);
        assert!(names.iter().any(|name| name == "クルミ ver1.2"));
        assert!(names.iter().any(|name| name == "クルミ"));
        assert!(versioned_known_names("見知らぬ子 ver1.2", &avatars).is_empty());
        let from_description =
            names_for_extraction("部屋着", "対応アバター\nクルミ ver1.2\n", &[], &avatars);
        assert!(from_description.iter().any(|name| name == "クルミ ver1.2"));
    }

    #[test]
    fn attribute_brackets_are_not_avatars_unless_one_is_known() {
        let hints = title_avatar_hints("【NSFW対応】【脱衣対応】服", &[]);
        assert!(hints.is_empty(), "{hints:?}");
        let mut avatars = sample_avatars();
        avatars.push(avatar("body", "素体"));
        let kept = title_avatar_hints("【素体対応】服", &avatars);
        assert_eq!(kept, vec!["素体".to_string()]);
        assert!(title_avatar_hints("【素体対応】服", &sample_avatars()).is_empty());
    }

    #[test]
    fn a_middle_dot_splits_unless_the_whole_name_is_one_avatar() {
        let split = title_avatar_hints("【ソラネ・クルミ対応】ワンピース", &sample_avatars());
        assert!(split.iter().any(|name| name == "ソラネ"), "{split:?}");
        assert!(split.iter().any(|name| name == "クルミ"), "{split:?}");
        assert!(
            split.iter().all(|name| name != "ソラネ・クルミ"),
            "{split:?}"
        );
        let mut avatars = sample_avatars();
        avatars.push(avatar("pair", "ソラネ・クルミ"));
        let whole = title_avatar_hints("【ソラネ・クルミ対応】ワンピース", &avatars);
        assert_eq!(whole, vec!["ソラネ・クルミ".to_string()]);
        avatars.push(avatar("kurumi", "クルミ"));
        let preferred = title_avatar_hints("【ソラネ・クルミ対応】ワンピース", &avatars);
        assert_eq!(preferred, vec!["ソラネ・クルミ".to_string()]);
        let half = title_avatar_hints("【ソラネ･クルミ対応】ワンピース", &avatars);
        assert_eq!(half, vec!["ソラネ・クルミ".to_string()]);
        let listed = title_avatar_hints("クルミ、【ソラネ・クルミ対応】", &avatars);
        assert!(
            listed.iter().any(|name| name == "ソラネ・クルミ"),
            "{listed:?}"
        );
        assert!(listed.iter().any(|name| name == "クルミ"), "{listed:?}");
        assert!(listed.iter().all(|name| name != "ソラネ"), "{listed:?}");
    }

    #[test]
    fn sibling_prose_is_not_a_compatibility_list() {
        let text =
            "素体について\nライム、プラム、ショコラは共通素体です。衣装を着せ替えできます。\n";
        assert!(listed_avatar_names(text, &sample_avatars()).is_empty());
    }
}
