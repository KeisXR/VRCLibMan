use crate::model::{Avatar, Category};
use crate::normalize::{is_blocked_avatar_label, is_cjk, is_punct, nfkc, preferred_display_name};
use crate::resolve::{known_name_in, known_names_in};
use regex::Regex;
use std::sync::OnceLock;

/// Names written in a description's compatibility list.
/// A title such as "25アバター対応" is only a count, so it is ignored.
pub fn listed_avatar_names(description: &str, avatars: &[Avatar]) -> Vec<String> {
    let Some(section) = compatibility_section(description) else {
        return Vec::new();
    };
    let bare_heading = section_heading(description).is_some_and(heading_stands_alone);
    let mut names = Vec::new();
    for entry in entries(&section, bare_heading) {
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

/// Only a title that says the product itself is an original avatar overrides the rule category.
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
/// A settled outfit, hair, or other specific BOOTH category stays as it is.
/// Locks are handled by the caller. Description is consulted for placement,
/// clothing, and editor-tool phrases that the title does not carry.
pub fn title_fill_category(title: &str, description: &str, model: Category) -> Option<Category> {
    let folded = nfkc(title).to_lowercase();
    if avatar_package(&folded) {
        return Some(Category::Avatar);
    }
    if has_world_prop(&folded) || world_placement(&folded) {
        return Some(Category::World);
    }
    if !avatar_gimmick_title(title)
        && world_placement(description)
        && !matches!(model, Category::Avatar | Category::Outfit | Category::Hair)
    {
        return Some(Category::World);
    }
    if editor_tool_title(title)
        && !avatar_gimmick_title(title)
        && !world_side_title(&folded)
        && matches!(
            model,
            Category::Other
                | Category::Gimmick
                | Category::Texture
                | Category::Hair
                | Category::Accessory
        )
    {
        return Some(Category::Other);
    }
    let keyword = category_for_listed_product(title, description);
    if model == Category::Other && keyword != Category::Other {
        return Some(keyword);
    }
    if model == Category::Gimmick
        && matches!(
            keyword,
            Category::Accessory | Category::Outfit | Category::World
        )
    {
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

fn world_side_title(lower: &str) -> bool {
    world_strength(lower) >= 1 || has_world_prop(lower) || world_placement(lower)
}

/// The title names an avatar gimmick. World placement in the same title stays a world.
pub fn avatar_gimmick_title(title: &str) -> bool {
    let lower = nfkc(title).to_lowercase();
    if lower.is_empty() || world_side_title(&lower) {
        return false;
    }
    const WORDS: &[&str] = &["ギミック", "gimmick", "なでなで", "kisssystem", "すりすり"];
    WORDS.iter().any(|word| lower.contains(word))
}

/// The product is put into a world. A sample-world link or 「お試しワールド」 is not this.
pub fn world_placement(text: &str) -> bool {
    let lower = nfkc(text).to_lowercase();
    const PHRASES: &[&str] = &[
        "ワールド設置",
        "ワールドに配置",
        "ワールドに置く",
        "ワールドへ配置",
        "ワールドへ置く",
        "ワールドギミック",
        "world gimmick",
        "worldに置",
        "worldに配置",
        "をワールドに",
    ];
    PHRASES.iter().any(|phrase| lower.contains(phrase))
}

/// GPU particle wording chooses a genre only after the world category is established.
pub fn gpu_particle(text: &str) -> bool {
    let lower = nfkc(text).to_lowercase();
    const PHRASES: &[&str] = &[
        "gpu particle",
        "particle volume",
        "gpuパーティクル",
        "gpu パーティクル",
        "パーティクルボリューム",
    ];
    PHRASES.iter().any(|phrase| lower.contains(phrase))
}

pub fn mochifitter_title(title: &str) -> bool {
    let lower = nfkc(title).to_lowercase();
    lower.contains("もちふぃった") || lower.contains("mochifitter")
}

pub fn conversion_profile_title(title: &str) -> bool {
    let lower = nfkc(title).to_lowercase();
    lower.contains("変換プロファイル")
        || (lower.contains("プロファイル") && mochifitter_title(&lower))
}

pub fn editor_tool_title(title: &str) -> bool {
    let lower = nfkc(title).to_lowercase();
    lower.contains("ツール")
        || lower.contains("エディタ拡張")
        || lower.contains("editor extension")
        || lower.contains("editor拡張")
}

/// The opening of the description says this product is an editor or conversion tool.
pub fn editor_tool_lead(description: &str) -> bool {
    let head: String = nfkc(description).to_lowercase().chars().take(180).collect();
    if head.contains("ツールは同梱")
        || head.contains("ツールを同梱")
        || head.contains("ツールは同梱しません")
    {
        return false;
    }
    head.contains("ツールです")
        || head.contains("ツールは")
        || head.contains("するツール")
        || head.contains("向けのツール")
        || head.contains("用のツール")
        || head.contains("unityツール")
        || head.contains("エディタ拡張")
        || head.contains("editor extension")
}

pub fn clothing_title(title: &str) -> bool {
    let lower = nfkc(title).to_lowercase();
    const WORDS: &[&str] = &[
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
    ];
    WORDS.iter().any(|word| lower.contains(word))
}

pub fn shoe_title(title: &str) -> bool {
    mentions_shoe(&nfkc(title).to_lowercase())
}

pub fn sticker_title(title: &str) -> bool {
    let lower = nfkc(title).to_lowercase();
    lower.contains("ステッカー") || lower.contains("絵文字")
}

/// Animation on a nameplate or accessory is a feature of that accessory.
pub fn nameplate_accessory(title: &str, description: &str) -> bool {
    let title_l = nfkc(title).to_lowercase();
    let text = format!("{title_l}\n{}", nfkc(description).to_lowercase());
    let feature = title_l.contains("アニメーション付き") || title_l.contains("名札");
    let accessory = text.contains("アクセサリ") || text.contains("名札");
    feature && accessory && !title_l.contains("表情")
}

/// Clothing as the product. A disclaimer that clothing is not included does not count.
pub fn outfit_description(description: &str) -> bool {
    let head: String = nfkc(description).to_lowercase().chars().take(240).collect();
    if head.contains("同梱しません")
        || head.contains("は同梱しない")
        || head.contains("衣装とツール")
        || head.contains("衣装/ツール")
        || head.contains("衣装を非対応")
        || head.contains("衣装は同梱")
    {
        return false;
    }
    if head.contains("衣装です") || head.contains("想定の衣装") || head.contains("用の衣装")
    {
        return true;
    }
    let colors = head.contains("カラバリ") || head.contains("カラーバリエーション");
    let spec = head.contains("ポリゴン") || head.contains("マテリアル");
    colors && spec
}

/// Shoe words that name the product. Shape-key identifiers such as `kisekae_Shoes` do not.
pub fn mentions_shoe(text: &str) -> bool {
    let stripped = nfkc(text)
        .to_lowercase()
        .replace("kisekae_shoes", "")
        .replace("kisekae_shoe", "");
    stripped.contains("靴")
        || stripped.contains("シューズ")
        || stripped.contains("靴下")
        || stripped.contains("ソックス")
        || crate::normalize::contains_ascii_word(&stripped, "shoe")
        || crate::normalize::contains_ascii_word(&stripped, "shoes")
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
    let described = nfkc(description).to_lowercase();
    // 「お試しワールド」やワールドURLだけでは、アバターギミックをワールドにしない。
    if avatar_gimmick_title(title) && !explicit_world_use(&described) {
        return false;
    }
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

/// Direct world use, rather than a sample-world URL next to unrelated shader wording.
/// The caller passes normalized lowercase text.
fn explicit_world_use(lower: &str) -> bool {
    if lower.contains("udon") || lower.contains("skybox") || world_placement(lower) {
        return true;
    }
    const MARKED: &[&str] = &[
        "ワールド用",
        "ワールドギミック",
        "ワールドシェーダ",
        "ワールドプレハブ",
        "ワールドプロップ",
        "world prefab",
        "worldprefab",
        "world shader",
        "worldshader",
        "worldgimmick",
        "world prop",
        "worldprop",
        "world tool",
        "worldtool",
        "for world",
        "for vrchat world",
    ];
    MARKED.iter().any(|phrase| lower.contains(phrase))
}

fn strong_world(lower: &str) -> bool {
    if explicit_world_use(lower) {
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
    if conversion_profile_title(title) {
        return Category::Other;
    }
    let title_l = nfkc(title).to_lowercase();
    if world_side_title(&title_l) {
        return Category::World;
    }
    if avatar_gimmick_title(title) {
        if explicit_world_use(&nfkc(description).to_lowercase()) {
            return Category::World;
        }
        return Category::Gimmick;
    }
    // 「アイテクスチャを使えるようにするツール」はテクスチャ商品ではない。
    if (editor_tool_title(title) || mochifitter_title(title)) && !clothing_title(title) {
        return Category::Other;
    }
    let from_title = classify_text(title, true);
    // A shape-key pack or texture utility whose description says it is a tool
    // is not an avatar gimmick. The title still wins when it names a gimmick.
    if from_title == Category::Gimmick
        && editor_tool_lead(description)
        && !avatar_gimmick_title(title)
    {
        return Category::Other;
    }
    if from_title != Category::Other {
        return from_title;
    }
    // 「衣装変換ツール」のように題がツールなら、説明の衣装語では衣装にしない。
    if editor_tool_title(title) || mochifitter_title(title) {
        return Category::Other;
    }
    if explicit_world_use(&nfkc(description).to_lowercase()) {
        return Category::World;
    }
    if outfit_description(description) {
        return Category::Outfit;
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
/// stays as that whole string even when another source omits the version.
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

/// Avatar names for one product. The description's compatibility list and the title come
/// first. `suggested` names come from outside the text, such as download file names, and
/// are used only when the description has no compatibility list.
pub fn names_for_extraction(
    title: &str,
    description: &str,
    suggested: &[String],
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
        for name in suggested {
            push(name);
        }
    }
    names
}

/// Known avatars named in download file names and variation names, such as
/// `Outfit_Manuka_ver2.zip` or `ショコラ・ライム対応`. A piece counts only when it resolves
/// to exactly one avatar, so an alias shared by two avatars (`mao`) adds nothing. Version
/// marks, file extensions, and ASCII pieces shorter than three letters are skipped, because
/// catalog aliases include `V2`, `v3`, and `Re`. Unknown names never become avatars here.
pub fn avatar_names_in_files(labels: &[String], avatars: &[Avatar]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for label in labels {
        for piece in file_label_pieces(label) {
            let Some(avatar) = crate::resolve::resolve_name(&piece, avatars) else {
                continue;
            };
            if !names.iter().any(|name| name == &avatar.display_name) {
                names.push(avatar.display_name.clone());
            }
        }
    }
    names
}

fn file_label_pieces(label: &str) -> Vec<String> {
    nfkc(label)
        .split(|ch: char| !ch.is_alphanumeric())
        .map(|piece| piece.trim_end_matches('版'))
        .filter(|piece| usable_file_piece(piece))
        .map(str::to_string)
        .collect()
}

fn usable_file_piece(piece: &str) -> bool {
    const EXTENSIONS: &[&str] = &[
        "zip", "unitypackage", "7z", "rar", "fbx", "blend", "psd", "png", "jpg", "pdf", "txt",
    ];
    static VERSION: OnceLock<Regex> = OnceLock::new();
    let version = VERSION
        .get_or_init(|| Regex::new(r"(?i)^(?:v|ver|version)?\d+[a-z]?$").expect("version piece"));
    let count = piece.chars().count();
    if count < 2 || version.is_match(piece) {
        return false;
    }
    if piece.is_ascii() {
        return count >= 3 && !EXTENSIONS.iter().any(|ext| piece.eq_ignore_ascii_case(ext));
    }
    true
}

/// The title or description says the item fits any avatar: 汎用, どのアバターでも,
/// 素体を問わない. 「汎用性」 alone is a selling point and does not count.
pub fn says_generic(title: &str, description: &str) -> bool {
    let text = nfkc(&format!("{title}\n{description}"));
    let general = text
        .match_indices("汎用")
        .any(|(index, word)| !text[index + word.len()..].starts_with('性'));
    general
        || [
            "どのアバターでも",
            "どんなアバターでも",
            "アバターを問わ",
            "素体を問わ",
            "全アバター対応",
            "全てのアバター",
            "すべてのアバター",
        ]
        .iter()
        .any(|phrase| text.contains(phrase))
}

fn compatibility_section(description: &str) -> Option<String> {
    let lines: Vec<&str> = description.lines().collect();
    let start = lines.iter().position(|line| is_section_heading(line.trim()))?;
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

fn is_section_heading(trimmed: &str) -> bool {
    trimmed.chars().count() <= 80 && !trimmed.contains('。') && line_has_marker(trimmed)
}

fn section_heading(description: &str) -> Option<&str> {
    description
        .lines()
        .map(str::trim)
        .find(|line| is_section_heading(line))
}

/// Plain lines are read only under a line that is the heading and nothing else.
/// 「対応アバター：ライム」 or a sentence naming the list says nothing about the lines below it.
fn heading_stands_alone(heading: &str) -> bool {
    const TITLES: &[&str] = &[
        "対応キャラクター",
        "対応アバター",
        "対応モデル",
        "対応キャラ",
        "対応素体",
        "対応一覧",
    ];
    const REMAINDERS: &[&str] = &[
        "",
        "一覧",
        "リスト",
        "順不同",
        "について",
        "は以下",
        "は下記",
        "は以下の通り",
        "は下記の通り",
        "は以下のとおり",
        "は下記のとおり",
    ];
    let mut rest: String = heading
        .chars()
        .filter(|ch| is_cjk(*ch) || ch.is_ascii_alphanumeric())
        .collect();
    for title in TITLES {
        rest = rest.replace(title, "");
    }
    if REMAINDERS.contains(&rest.as_str()) {
        return true;
    }
    // 「対応アバター / Compatible Avatars」のような併記。
    let lower = rest.to_lowercase();
    lower.is_ascii() && (lower.contains("avatar") || lower.contains("model"))
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

fn entries(section: &str, bare_heading: bool) -> Vec<String> {
    let mut parts = Vec::new();
    if section.contains("booth.pm") {
        parts.extend(split_on_item_urls(section));
        if !parts.is_empty() {
            parts.extend(line_entries(after_item_urls(section), bare_heading));
        }
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
        parts.extend(line_entries(section, bare_heading));
    }
    parts
        .into_iter()
        .filter(|part| acceptable_entry(part))
        .collect()
}

/// Marked lines always count. Plain name lines count only under a bare heading, and
/// the first line that does not read as a name ends them, because what follows
/// a note is more notes. Marked lines further down still count.
fn line_entries(text: &str, bare_heading: bool) -> Vec<String> {
    let mut parts = Vec::new();
    let mut plain_open = bare_heading;
    for line in text.lines() {
        let trimmed = line
            .trim()
            .trim_matches(|ch| ch == '"' || ch == '“' || ch == '”');
        let bare = strip_bullet(trimmed);
        if looks_like_entry(bare) {
            parts.push(bare.to_string());
            continue;
        }
        if type_tail_re().is_match(bare)
            && plain_name_line(type_tail_re().replace_all(bare, " ").trim())
        {
            parts.push(bare.to_string());
            continue;
        }
        if !plain_open || is_list_note(bare) {
            continue;
        }
        if plain_name_line(bare) {
            parts.push(bare.to_string());
        } else {
            plain_open = false;
        }
    }
    parts
}

/// `/ オリジナル3Dモデル` after a name says what the name is. Only the name is kept.
fn type_tail_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\s*[/／]\s*(?:vrchat\s*(?:向け|用)?\s*)?(?:オリジナル\s*)?(?:[3３]\s*[dｄ]\s*)?(?:モデル|アバター|素体|models?|avatars?)(?:\s|\z)",
        )
        .unwrap()
    })
}

fn strip_bullet(line: &str) -> &str {
    line.trim_start_matches(|ch: char| {
        ch.is_whitespace()
            || matches!(
                ch,
                '・' | '･'
                    | '•'
                    | '‣'
                    | '∙'
                    | '◦'
                    | '●'
                    | '○'
                    | '◯'
                    | '▪'
                    | '▫'
                    | '*'
                    | '＊'
                    | '-'
                    | '－'
                    | '‐'
                    | '−'
                    | '–'
                    | '—'
            )
    })
}

/// 「※順不同」や「(順不同)」は一覧の補足であり、一覧の終わりではない。
fn is_list_note(line: &str) -> bool {
    line.starts_with('※') || strip_parentheticals(line).trim().is_empty()
}

/// A short line that can be one name. Names are not checked against the avatar list,
/// so anything that reads as a note, or holds more than one name, is left out.
fn plain_name_line(line: &str) -> bool {
    const HEADING_MARKS: &str = "■□◆◇★☆♥♡❤✤✦✧◎▼▽▲△【〔《≪<୨";
    const NOT_A_NAME: &[&str] = &[
        "：",
        ":",
        "/",
        "／",
        "、",
        ",",
        "，",
        "!",
        "?",
        "http",
        "です",
        "でした",
        "ました",
        "ません",
        "について",
        "場合",
        "予定",
        "可能",
        "確認",
        "以下",
        "以上",
        "以外",
        "下記",
        "上記",
        "その他",
        "こちら",
        "順次",
        "追加",
        "必要",
        "動作",
        "詳細",
        "対象",
        "購入",
    ];
    let has_cjk = line.chars().any(is_cjk);
    // ASCII の文は、最後の単語だけが名前として残ってしまう。
    let max_words = if has_cjk { 3 } else { 1 };
    (2..=30).contains(&line.chars().count())
        && line.split_whitespace().count() <= max_words
        && (has_cjk || line.chars().any(|ch| ch.is_ascii_alphabetic()))
        && !line.starts_with(|ch: char| HEADING_MARKS.contains(ch))
        && !line.ends_with("など")
        && !NOT_A_NAME.iter().any(|word| line.contains(word))
}

fn item_url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"https?://\S*booth\.pm\S*").unwrap())
}

fn after_item_urls(section: &str) -> &str {
    item_url_re()
        .find_iter(section)
        .last()
        .map_or(section, |found| &section[found.end()..])
}

fn split_on_item_urls(section: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut last = 0;
    for found in item_url_re().find_iter(section) {
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
    true
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
    let without_type = type_tail_re().replace_all(&without_url, " ");
    let without_paren = strip_parentheticals(&without_type);
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
    } else if let Some(name) = reading_pair_name(label) {
        name
    } else {
        tidy_name(&strip_shop(label))
    };
    plausible_avatar_name(&candidate).then_some(candidate)
}

/// `うらら -Urara-` is one name in two scripts, not a shop followed by a name.
fn reading_pair_name(label: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^(\S+)\s+[-－–—]\s*(\S+?)\s*[-－–—]?$").unwrap());
    let found = re.captures(label.trim())?;
    let has_cjk = |text: &str| text.chars().any(is_cjk);
    if has_cjk(&found[1]) == has_cjk(&found[2]) {
        return None;
    }
    preferred_display_name(label)
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
    if has_hair_product(&lower) {
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
    if world_strength(text) >= 2 || has_world_prop(&lower) || world_placement(&lower) {
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
    if has(&["ギミック", "gimmick"]) {
        return Category::Gimmick;
    }
    if has(&["ツール", "エディタ拡張", "editor extension"]) {
        return Category::Other;
    }
    if has(&["シェーダー", "shader"]) {
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

/// 「髪色」 is a color edit, not a hair product. 「前髪」 still counts.
fn has_hair_product(lower: &str) -> bool {
    let stripped = lower.replace("髪の色", "").replace("髪色", "");
    [
        "ヘア",
        "髪",
        "ウィッグ",
        "ポニーテール",
        "お団子",
        "前髪",
        "みつあみ",
        "hair",
    ]
    .iter()
    .any(|word| stripped.contains(word))
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
            hero_image_path: None,
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
            title_fill_category("サンプルこたつ", "", Category::Gimmick),
            Some(Category::World)
        );
        assert!(!world_hint_overrides(
            "撫で音ギミック",
            "お試しワールドで展示しています。 https://vrchat.com/home/world/wrld_example",
            Category::Gimmick,
            0.95
        ));
        assert_eq!(
            category_for_listed_product(
                "撫で音ギミック",
                "影シェーダーを同梱しています。お試しワールドがあります。"
            ),
            Category::Gimmick
        );
        assert_eq!(
            category_for_listed_product(
                "40アバター対応",
                "VRChat想定の衣装です。カラバリとポリゴン数。"
            ),
            Category::Outfit
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
            title_fill_category("【VRChat / 3D model】サンプル子", "", Category::Other),
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
            hero_image_path: None,
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
    fn file_names_add_only_avatars_that_resolve_to_one() {
        let mut avatars = sample_avatars();
        avatars.push(Avatar {
            aliases: vec!["Manuka".into()],
            ..avatar("manuka", "マヌカ")
        });
        avatars.push(Avatar {
            aliases: vec!["V2".into()],
            ..avatar("v2-alias", "見本ブイツー")
        });
        avatars.push(Avatar {
            aliases: vec!["mao".into()],
            ..avatar("mao-a", "真央")
        });
        avatars.push(Avatar {
            aliases: vec!["Mao".into()],
            ..avatar("mao-b", "マオ")
        });
        let labels = vec![
            "Sample_Manuka_ver2.zip".to_string(),
            "Sample_V2_v1.0.6.unitypackage".to_string(),
            "mao_v1.0.6.zip".to_string(),
            "ショコラ・ライム対応版".to_string(),
            "フルセット".to_string(),
        ];
        assert_eq!(
            avatar_names_in_files(&labels, &avatars),
            vec!["マヌカ".to_string(), "ショコラ".to_string(), "ライム".to_string()]
        );
    }

    #[test]
    fn file_names_wait_for_a_missing_compatibility_list() {
        let avatars = sample_avatars();
        let files = avatar_names_in_files(&["Dress_Plum.zip".to_string()], &avatars);
        assert_eq!(files, vec!["プラム".to_string()]);
        let listed = names_for_extraction("ワンピース", LIST, &files, &avatars);
        assert!(listed.iter().any(|name| name == "しなの"));
        let open = names_for_extraction("【4アバター対応】ワンピース", "布の服です。", &files, &avatars);
        assert_eq!(open, vec!["プラム".to_string()]);
    }

    #[test]
    fn generic_phrases_mark_items_for_any_avatar() {
        assert!(says_generic("汎用ヘアピン", ""));
        assert!(says_generic("ヘアピン", "どのアバターでも使えます"));
        assert!(says_generic("ペンライト", "素体を問わず使えます"));
        assert!(!says_generic("ワンピース", "汎用性の高いデザインです"));
        assert!(!says_generic("【ライム対応】ワンピース", "ライム専用です"));
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
    fn a_name_followed_by_a_type_word_is_listed() {
        let text = "対応アバター\nうらら / オリジナル3Dモデル\nまめひなた／3Dモデル\nライム / 素体\n基本仕様\n布です。\n";
        assert_eq!(
            listed_avatar_names(text, &[]),
            vec![
                "うらら".to_string(),
                "まめひなた".to_string(),
                "ライム".to_string()
            ]
        );
    }

    #[test]
    fn a_reading_after_the_name_stays_one_avatar() {
        let text = "対応アバター\nうらら -Urara- / オリジナル3Dモデル\n基本仕様\n";
        assert_eq!(listed_avatar_names(text, &[]), vec!["うらら".to_string()]);

        let mut urara = avatar("urara", "うらら");
        urara.aliases = vec!["Urara".into()];
        let alias_only = "対応アバター\n-Urara- / オリジナル3Dモデル\n";
        assert_eq!(
            listed_avatar_names(alias_only, &[urara]),
            vec!["うらら".to_string()]
        );
    }

    #[test]
    fn a_type_word_is_dropped_from_lines_with_a_url_too() {
        let linked = "対応アバター\nうらら -Urara- / オリジナル3Dモデル\nhttps://booth.pm/ja/items/1\nプラム / オリジナル3Dモデル https://booth.pm/ja/items/2\n基本仕様\n";
        assert_eq!(
            listed_avatar_names(linked, &[]),
            vec!["うらら".to_string(), "プラム".to_string()]
        );
        let mixed =
            "対応アバター\nライム_ver1.00 https://booth.pm/ja/items/1001\nうらら / オリジナル3Dモデル\n";
        assert_eq!(
            listed_avatar_names(mixed, &[]),
            vec!["ライム".to_string(), "うらら".to_string()]
        );
    }

    #[test]
    fn plain_and_bulleted_names_are_listed_under_a_bare_heading() {
        let text = "♥対応アバター♥\n(順不同)\n・うらら\n- Milfy\n＊ライム\nシフォン(Chiffon)\n◆サイズ\n1024px\n";
        assert_eq!(
            listed_avatar_names(text, &[]),
            vec![
                "うらら".to_string(),
                "Milfy".to_string(),
                "ライム".to_string(),
                "シフォン".to_string()
            ]
        );
    }

    #[test]
    fn prose_in_the_section_ends_the_plain_names() {
        let text = "対応アバター\nうらら\nご購入前に動作をご確認ください\nサポート対象外\n";
        assert_eq!(listed_avatar_names(text, &[]), vec!["うらら".to_string()]);
        let plain_note = "対応アバター\nうらら\nその他のモデルも順次追加予定です\nミルティナ\n";
        assert_eq!(
            listed_avatar_names(plain_note, &[]),
            vec!["うらら".to_string()]
        );
        let english = "対応アバター\nUrara\nPlease read the terms\nMilfy\n";
        assert_eq!(listed_avatar_names(english, &[]), vec!["Urara".to_string()]);
        let typed_sentence = "対応アバター\nモデルごとに調整しています / オリジナル3Dモデル\n";
        assert!(listed_avatar_names(typed_sentence, &[]).is_empty());
        let after_heading = "対応アバター\nうらら\n利用規約\nミルティナ\n";
        assert_eq!(
            listed_avatar_names(after_heading, &[]),
            vec!["うらら".to_string()]
        );
    }

    #[test]
    fn plain_names_need_a_heading_that_is_only_a_heading() {
        let inline = "対応アバター：ライム\n高品質テクスチャ\nサイズ自由\n";
        assert!(listed_avatar_names(inline, &[]).is_empty());
        let sentence = "対応モデルは順次増やしていきます\n見本の小物\n";
        assert!(listed_avatar_names(sentence, &[]).is_empty());
        let typed = "対応アバター：ライム\nうらら / オリジナル3Dモデル\n";
        assert_eq!(listed_avatar_names(typed, &[]), vec!["うらら".to_string()]);
        for heading in [
            "対応アバター",
            "【対応アバター】",
            "対応アバター一覧",
            "対応アバター(順不同)",
            "対応キャラクター",
            "対応アバター / Compatible Avatars",
        ] {
            assert!(heading_stands_alone(heading), "{heading}");
        }
    }

    #[test]
    fn lines_outside_a_compatibility_section_are_not_names() {
        assert!(listed_avatar_names("うらら / オリジナル3Dモデル\nライム\n", &[]).is_empty());
        let before = "うらら / オリジナル3Dモデル\n対応アバター\nライム\n";
        assert_eq!(listed_avatar_names(before, &[]), vec!["ライム".to_string()]);
        let after = "対応アバター\nライム\n基本仕様\nうらら / オリジナル3Dモデル\n";
        assert_eq!(listed_avatar_names(after, &[]), vec!["ライム".to_string()]);
    }

    #[test]
    fn sibling_prose_is_not_a_compatibility_list() {
        let text =
            "素体について\nライム、プラム、ショコラは共通素体です。衣装を着せ替えできます。\n";
        assert!(listed_avatar_names(text, &sample_avatars()).is_empty());
    }
}
