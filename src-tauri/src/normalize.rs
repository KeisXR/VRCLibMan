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

/// Hiragana or kanji followed by katakana splits a name: まめひなたフェイス.
/// The same script running on does not, so まめひなたん and ショコラミント stay one word.
pub fn is_script_boundary(before: char, after: char) -> bool {
    (is_hiragana(before) || is_kanji(before)) && is_katakana(after)
}

fn is_hiragana(ch: char) -> bool {
    ('\u{3040}'..='\u{309f}').contains(&ch)
}

fn is_katakana(ch: char) -> bool {
    ('\u{30a0}'..='\u{30ff}').contains(&ch)
}

fn is_kanji(ch: char) -> bool {
    ('\u{3400}'..='\u{9fff}').contains(&ch)
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
    const SUFFIXES: &[&str] = &["対応", "専用", "向け", "ちゃん", "くん", "さん", "様"];
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
        if let Some((_, end)) = pairs
            .iter()
            .copied()
            .find(|(start, _)| *start == chars[index])
        {
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

pub fn label_variants(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |text: &str| {
        let trimmed = text.trim().trim_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '-' | '－' | '–' | '—' | '/' | '／' | '(' | ')' | '（' | '）'
                )
        });
        if trimmed.chars().count() >= 2 && !out.iter().any(|existing: &String| existing == trimmed)
        {
            out.push(trimmed.to_string());
        }
    };
    let source = nfkc(&strip_tag_brackets(raw));
    push(&source);
    let mut parts = vec![source];
    for separator in [
        " / ", " - ", " – ", " — ", "-", "・", "&", "＆", "／", "/", "、",
    ] {
        let mut next = Vec::new();
        for part in parts {
            if part.contains(separator) {
                next.extend(part.split(separator).map(str::to_string));
            } else {
                next.push(part);
            }
        }
        parts = next;
    }
    let mut expanded = Vec::new();
    for part in parts {
        let (outer, inners) = split_parens(&part);
        expanded.push(outer);
        expanded.extend(inners);
    }
    for part in expanded {
        push(&part);
        for token in bilingual_tokens(&part) {
            push(&token);
        }
    }
    out
}

pub fn strip_tag_brackets(title: &str) -> String {
    let chars: Vec<char> = title.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '【' {
            if let Some(end) = chars[index + 1..].iter().position(|ch| *ch == '】') {
                let inner: String = chars[index + 1..index + 1 + end].iter().collect();
                if let Some(name) = dedication_inside(&inner) {
                    out.push(' ');
                    out.push_str(&name);
                    out.push(' ');
                }
                index += end + 2;
                continue;
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

pub fn dedication_inside(inner: &str) -> Option<String> {
    let original = inner.trim();
    let Some(stem) = original
        .strip_suffix("専用")
        .or_else(|| original.strip_suffix("対応"))
    else {
        return None;
    };
    let stem = stem.trim();
    // 【MA対応】や【オリジナル3Dモデル】は種別であって、対応先の名前ではない。
    if stem.chars().count() < 2 || bracket_is_tag(original) || is_blocked_avatar_label(stem) {
        return None;
    }
    Some(stem.to_string())
}

pub fn bracket_is_tag(inner: &str) -> bool {
    const KEYS: &[&str] = &[
        "セール",
        "半額",
        "新作",
        "再販",
        "無料",
        "限定",
        "予約",
        "配布",
        "オリジナル",
        "3Dモデル",
        "３Dモデル",
        "VRChat",
        "MA対応",
        "旧",
        "髪型",
        "衣装",
        "ギミック",
        "ワールド",
    ];
    KEYS.iter().any(|key| inner.contains(key))
}

pub fn is_promo_name(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return true;
    }
    const KEYS: &[&str] = &["セール", "半額", "新作", "再販", "無料配布", "無料"];
    KEYS.iter().any(|key| trimmed.contains(key)) || trimmed == "限定"
}

pub fn is_blocked_avatar_label(name: &str) -> bool {
    let trimmed = name.trim();
    let count = trimmed.chars().count();
    if !(2..=40).contains(&count) {
        return true;
    }
    if is_promo_name(trimmed) || matches!(trimmed, "対応" | "専用" | "向け" | "用") {
        return true;
    }
    if is_platform_label(trimmed) {
        return true;
    }
    if trimmed.contains('。')
        || trimmed.contains('！')
        || trimmed.contains('？')
        || trimmed.contains('@')
        || trimmed.contains('#')
        || trimmed.contains('【')
        || trimmed.contains('】')
        || trimmed.contains('「')
        || trimmed.contains('」')
        || trimmed.contains('¥')
        || trimmed.contains('￥')
        || trimmed.contains("ます")
        || trimmed.contains("ください")
        || trimmed.contains("利用規約")
        || trimmed.contains("オリジナル")
        || trimmed.contains("アバター")
        || trimmed.contains("3D")
        || trimmed.contains("３D")
        || trimmed.contains("モデル")
        || trimmed.contains("動画")
        || trimmed.contains("セットアップ")
        || trimmed.contains("心拍")
        || trimmed.contains("VRモード")
        || trimmed.contains("vrモード")
    {
        return true;
    }
    let lower = nfkc(trimmed).to_lowercase();
    if lower.chars().any(|ch| ch.is_ascii_digit())
        && lower.contains('種')
        && lower.chars().count() <= 6
    {
        return true;
    }
    const PHRASES: &[&str] = &[
        "modular avatar",
        "qvpen",
        "liltoon",
        "unitypackage",
        "xavater",
        "shader",
    ];
    if PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return true;
    }
    let tokens = label_tokens(&lower);
    const DENY: &[&str] = &[
        "avatar",
        "unity",
        "fbx",
        "psd",
        "vrchat",
        "udon",
        "prefab",
        "png",
        "jpg",
        "jpeg",
        "clip",
        "blender",
        "vrm",
        "modular",
        "texture",
        "textures",
        "hair",
        "outfit",
        "world",
        "gimmick",
        "liltoon",
        "qvpen",
        "shader",
        "full",
        "pack",
        "eye",
        "eyes",
        "face",
        "animation",
        "pb",
        "ma",
        "pc",
        "quest",
        "re",
        "osc",
        "vr",
        "セーブ",
        "physbone",
    ];
    tokens.iter().any(|token| DENY.contains(&token.as_str()))
}

pub fn is_name_alias(text: &str) -> bool {
    let trimmed = text
        .trim()
        .trim_matches(|ch: char| ch.is_whitespace() || is_punct(ch));
    let count = trimmed.chars().count();
    if !(2..=16).contains(&count) || is_blocked_avatar_label(trimmed) {
        return false;
    }
    let lower = nfkc(trimmed).to_lowercase();
    if lower.contains("ver")
        || lower.contains("vroid")
        || lower.contains("プロファイル")
        || lower.contains("profile")
        || lower.contains('版')
        || lower.contains('.')
    {
        return false;
    }
    trimmed
        .chars()
        .all(|ch| is_cjk(ch) || ch.is_ascii_alphabetic())
}

fn is_platform_label(name: &str) -> bool {
    let lower = nfkc(name).trim().to_lowercase();
    if matches!(
        lower.as_str(),
        "ma" | "pb" | "pc" | "quest" | "steam" | "android" | "ios"
    ) {
        return true;
    }
    lower.ends_with('版') && lower.chars().count() <= 8
}

pub fn is_avatar_count_title(title: &str) -> bool {
    let text = nfkc(title).to_lowercase();
    text.contains("アバター対応") && !text.contains("オリジナル3dモデル")
}

pub fn preferred_display_name(raw: &str) -> Option<String> {
    let mut usable = Vec::new();
    let mut push = |text: &str| {
        let trimmed = text
            .trim()
            .trim_matches(|ch: char| ch.is_whitespace() || is_punct(ch));
        if trimmed.chars().count() < 2
            || is_blocked_avatar_label(trimmed)
            || is_noise_token(trimmed)
        {
            return;
        }
        if !usable.iter().any(|existing: &String| existing == trimmed) {
            usable.push(trimmed.to_string());
        }
    };
    for variant in label_variants(raw) {
        let mixed = variant.split_whitespace().nth(1).is_some()
            && !variant.chars().all(|ch| is_cjk(ch) || ch.is_whitespace());
        if !mixed {
            push(&variant);
        }
    }
    for token in nfkc(raw).split_whitespace() {
        push(token);
    }
    if let Some(cjk) = usable
        .iter()
        .filter(|item| {
            item.chars().any(is_cjk) && item.chars().all(|ch| is_cjk(ch) || ch.is_whitespace())
        })
        .max_by_key(|item| item.chars().count())
    {
        return Some(cjk.clone());
    }
    usable.first().cloned()
}

pub fn strip_title_noise(title: &str) -> String {
    let mut text = nfkc(title);
    for noise in [
        "オリジナル3Dモデル",
        "オリジナル3dモデル",
        "VRChat向け",
        "VRChat",
    ] {
        text = text.replace(noise, " ");
    }
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:¥|￥)\s*\d[\d,]*(?:\s*[〜～~\-]\s*\d[\d,]*)?|(?:¥|￥)\s*\d[\d,]*\s*[〜～~]?|[（(]\s*仮\s*[）)]|(?:ver\.?|version)\s*\d+(?:\.\d+)*|\bv\s*\d+(?:\.\d+)*\b|(?:unitypackage|xavater|unity|prefab|fbx|vrm|psd|png|jpe?g|blender|blend)|3\s*d\s*models?|3dモデル|３dモデル",
        )
        .unwrap()
    });
    re.replace_all(&text, " ").to_string()
}

fn label_tokens(lower: &str) -> Vec<String> {
    lower
        .split(|ch: char| ch.is_whitespace() || is_punct(ch))
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

fn is_noise_token(token: &str) -> bool {
    let lower = nfkc(token).to_lowercase();
    if lower.chars().count() <= 1 {
        return true;
    }
    const NOISE: &[&str] = &[
        "unitypackage",
        "xavater",
        "unity",
        "prefab",
        "fbx",
        "vrm",
        "vrchat",
        "model",
        "models",
        "3d",
        "ver",
        "version",
        "psd",
        "png",
        "jpg",
        "jpeg",
        "blender",
        "blend",
        "clip",
        "udon",
        "shader",
        "texture",
        "textures",
        "outfit",
        "hair",
        "world",
        "gimmick",
        "avatar",
        "liltoon",
        "qvpen",
        "仮",
        "向け",
        "オリジナル",
    ];
    NOISE.contains(&lower.as_str())
}

/// The other-script name sitting next to `anchor`, ignoring spaces, punctuation, and の.
/// `しお（Sio）` yields しお when the anchor is Sio, and Sio when the anchor is しお.
pub fn adjacent_reading(anchor: &str, text: &str) -> Option<String> {
    let folded = nfkc(text);
    let want_cjk = !anchor.chars().any(is_cjk);
    let needle: Vec<char> = nfkc(anchor).to_lowercase().chars().collect();
    if needle.is_empty() || (needle.len() < 2 && want_cjk) {
        return None;
    }
    let original: Vec<char> = folded.chars().collect();
    let haystack: Vec<char> = folded.to_lowercase().chars().collect();
    if haystack.len() < needle.len() {
        return None;
    }
    let mut found = Vec::new();
    for index in 0..=haystack.len() - needle.len() {
        if haystack[index..index + needle.len()] != needle[..] {
            continue;
        }
        let end = index + needle.len();
        if !reading_edge(&original, index, end, want_cjk) {
            continue;
        }
        if let Some(name) = reading_beside(&original, index, end, want_cjk) {
            if !found.iter().any(|existing: &String| existing == &name) {
                found.push(name);
            }
        }
    }
    if found.len() == 1 { found.pop() } else { None }
}

fn reading_edge(chars: &[char], start: usize, end: usize, anchor_is_latin: bool) -> bool {
    let continues = |ch: char| {
        if anchor_is_latin {
            ch.is_ascii_alphabetic()
        } else {
            is_cjk(ch)
        }
    };
    (start == 0 || !continues(chars[start - 1])) && (end >= chars.len() || !continues(chars[end]))
}

fn reading_beside(chars: &[char], start: usize, end: usize, want_cjk: bool) -> Option<String> {
    reading_run(chars, start, true, want_cjk).or_else(|| reading_run(chars, end, false, want_cjk))
}

fn reading_run(chars: &[char], edge: usize, look_left: bool, want_cjk: bool) -> Option<String> {
    let script = |ch: char| {
        if want_cjk {
            is_cjk(ch)
        } else {
            ch.is_ascii_alphabetic()
        }
    };
    if look_left {
        let mut index = edge;
        while index > 0 && reading_gap(chars[index - 1]) {
            index -= 1;
        }
        if index == 0 || !script(chars[index - 1]) {
            return None;
        }
        let mut begin = index;
        while begin > 0 && script(chars[begin - 1]) {
            begin -= 1;
        }
        accept_reading(&chars[begin..index])
    } else {
        let mut index = edge;
        while index < chars.len() && reading_gap(chars[index]) {
            index += 1;
        }
        if index >= chars.len() || !script(chars[index]) {
            return None;
        }
        let begin = index;
        while index < chars.len() && script(chars[index]) {
            index += 1;
        }
        accept_reading(&chars[begin..index])
    }
}

fn reading_gap(ch: char) -> bool {
    ch.is_whitespace() || is_punct(ch) || ch == 'の' || ch == 'ノ'
}

fn accept_reading(chars: &[char]) -> Option<String> {
    let name: String = chars.iter().collect();
    if name.contains("対応") || name.contains("専用") || name.contains("向け") {
        return None;
    }
    let count = name.chars().count();
    if name.chars().any(is_cjk) {
        if !(1..=8).contains(&count) {
            return None;
        }
        const PARTICLES: &str = "のをにはがでと版式";
        if count == 1 && PARTICLES.contains(chars[0]) {
            return None;
        }
        if count > 1 && is_blocked_avatar_label(&name) {
            return None;
        }
        return Some(name);
    }
    if (2..=16).contains(&count) && is_name_alias(&name) {
        Some(name)
    } else {
        None
    }
}

pub fn paren_pieces(raw: &str) -> Vec<String> {
    let folded = nfkc(raw);
    if !folded.contains('(') {
        return Vec::new();
    }
    let (outer, inners) = split_parens(&folded);
    let mut pieces = Vec::new();
    let mut push = |text: &str| {
        let trimmed = text
            .trim()
            .trim_matches(|ch: char| ch.is_whitespace() || is_punct(ch));
        let count = trimmed.chars().count();
        let single_cjk = count == 1 && trimmed.chars().all(is_cjk);
        if (count >= 2 || single_cjk) && !pieces.iter().any(|existing: &String| existing == trimmed)
        {
            pieces.push(trimmed.to_string());
        }
    };
    push(&outer);
    for inner in &inners {
        push(inner);
    }
    if pieces.len() >= 2 {
        pieces
    } else {
        Vec::new()
    }
}

fn split_parens(input: &str) -> (String, Vec<String>) {
    let chars: Vec<char> = input.chars().collect();
    let mut outer = String::new();
    let mut inners = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '(' || chars[index] == '（' {
            let mut depth = 1;
            let mut cursor = index + 1;
            let start = cursor;
            while cursor < chars.len() && depth > 0 {
                if chars[cursor] == '(' || chars[cursor] == '（' {
                    depth += 1;
                } else if chars[cursor] == ')' || chars[cursor] == '）' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                cursor += 1;
            }
            if depth == 0 && cursor < chars.len() {
                let inner: String = chars[start..cursor].iter().collect();
                if inner.trim().chars().count() >= 1 {
                    inners.push(inner.trim().to_string());
                }
                outer.push(' ');
                index = cursor + 1;
                continue;
            }
            let inner: String = chars[index + 1..].iter().collect();
            if inner.trim().chars().count() >= 1 {
                inners.push(inner.trim().to_string());
            }
            break;
        }
        outer.push(chars[index]);
        index += 1;
    }
    (outer, inners)
}

fn bilingual_tokens(text: &str) -> Vec<String> {
    let tokens: Vec<&str> = text
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .collect();
    if tokens.len() < 2 {
        return Vec::new();
    }
    let latin = tokens
        .iter()
        .any(|token| token.chars().any(|ch| ch.is_ascii_alphabetic()));
    let cjk = tokens.iter().any(|token| token.chars().any(is_cjk));
    if !(latin && cjk) {
        return Vec::new();
    }
    tokens.into_iter().map(str::to_string).collect()
}

pub fn peel_product_words(input: &str) -> String {
    let mut text = input.trim().to_string();
    loop {
        let lower = text.to_lowercase();
        const TAILS: &[&str] = &[
            "テクスチャ",
            "textures",
            "texture",
            "ウィッグ",
            "ヘア",
            "ワンピース",
            "パーカー",
            "スカート",
            "ドレス",
            "衣装",
            "パック",
            "pack",
            "セット",
            "set",
            "対応",
            "専用",
            "向け",
        ];
        let mut peeled = None;
        for tail in TAILS {
            if lower.ends_with(tail) && text.chars().count() > tail.chars().count() {
                let keep = text.chars().count() - tail.chars().count();
                peeled = Some(text.chars().take(keep).collect::<String>());
                break;
            }
        }
        if let Some(next) = peeled {
            text = next
                .trim()
                .trim_end_matches(|ch: char| ch.is_whitespace() || is_punct(ch))
                .to_string();
            continue;
        }
        if let Some(stripped) = text.strip_suffix('用') {
            if stripped.chars().count() >= 2 {
                text = stripped.trim().to_string();
                continue;
            }
        }
        break;
    }
    text
}

pub fn suggested_display_name(title: &str) -> String {
    let stripped = strip_tag_brackets(title);
    for segment in quoted_segments(&stripped) {
        if let Some(name) = preferred_display_name(&segment) {
            return peel_product_words(&name);
        }
    }
    let cleaned = strip_title_noise(&stripped);
    if let Some(name) = preferred_display_name(&cleaned) {
        let peeled = peel_product_words(&name);
        if let Some(better) = preferred_display_name(&peeled) {
            return better;
        }
        return peeled;
    }
    "名称未設定".to_string()
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

    #[test]
    fn sale_brackets_are_not_avatar_names() {
        assert_eq!(
            suggested_display_name("【半額セール中】真冬 Mafuyu / オリジナル3Dモデル"),
            "真冬"
        );
        assert_eq!(
            suggested_display_name("【半額セール中】【オリジナル3Dモデル】Nayu - ナユ"),
            "ナユ"
        );
        assert!(is_promo_name("半額セール中"));
        assert!(is_blocked_avatar_label("半額セール中"));
        assert!(is_blocked_avatar_label("19アバター対応"));
        assert!(is_blocked_avatar_label("19アバター対応(仮)"));
    }

    #[test]
    fn title_noise_leaves_the_avatar_token() {
        assert_eq!(
            suggested_display_name("¥0〜 Savi 3D model xavater unitypackage v(仮)"),
            "Savi"
        );
    }

    #[test]
    fn bilingual_labels_split_into_matchable_parts() {
        let chocolat = label_variants("ショコラ -Chocolat-");
        assert!(chocolat.iter().any(|part| part == "ショコラ"));
        assert!(chocolat.iter().any(|part| part == "Chocolat"));
        let paren = label_variants("シフォン(Chiffon)");
        assert!(paren.iter().any(|part| part == "シフォン"));
        assert!(paren.iter().any(|part| part == "Chiffon"));
        let spaced = label_variants("真冬 Mafuyu");
        assert!(spaced.iter().any(|part| part == "真冬"));
        assert!(spaced.iter().any(|part| part == "Mafuyu"));
        assert_eq!(normalize_label("プラムちゃん"), "プラム");
        assert_eq!(normalize_label("もち山金魚様"), "もち山金魚");
        assert_eq!(
            preferred_display_name("manuka-マヌカ").as_deref(),
            Some("マヌカ")
        );
        assert_eq!(
            preferred_display_name("エク - Eku").as_deref(),
            Some("エク")
        );
    }

    #[test]
    fn unclosed_and_fullwidth_parentheses_split_before_matching() {
        let eru = paren_pieces("エルキュナ２(Eruqyuna2");
        assert!(eru.iter().any(|part| part == "エルキュナ2"), "{eru:?}");
        assert!(eru.iter().any(|part| part == "Eruqyuna2"), "{eru:?}");
        assert!(eru.iter().all(|part| !part.contains('(')));
        let moe = paren_pieces("萌（Moe");
        assert!(moe.iter().any(|part| part == "萌"), "{moe:?}");
        assert!(moe.iter().any(|part| part == "Moe"), "{moe:?}");
        let closed = label_variants("シフォン(Chiffon)");
        assert!(closed.iter().any(|part| part == "シフォン"));
        assert!(closed.iter().any(|part| part == "Chiffon"));
    }

    #[test]
    fn platforms_and_editions_are_not_avatar_names() {
        for name in [
            "MA",
            "PB",
            "PC版",
            "Quest版",
            "支援版",
            "24種",
            "Full",
            "Eye",
            "Pack",
            "Re",
            "セーブ",
            "VRモード",
            "OSC心拍計",
        ] {
            assert!(is_blocked_avatar_label(name), "{name}");
        }
        assert!(!is_name_alias("PC版"));
        assert!(!is_name_alias("ver.2.5"));
        assert!(!is_name_alias(".vroid"));
        assert!(is_name_alias("Eku"));
        assert!(is_name_alias("ナユ"));
        assert_eq!(
            adjacent_reading("Sio", "しお（Sio）のモデル").as_deref(),
            Some("しお")
        );
        assert_eq!(
            adjacent_reading("エク", "エク - Eku").as_deref(),
            Some("Eku")
        );
    }
}
