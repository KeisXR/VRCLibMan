//! BOOTH category id and tags decide a classification only when they are specific.
//! Confirmed ids come from public, non-R18 category pages and their item JSON.
//! Unknown ids fall back to the category name. Ambiguous cases stay for the model.

use crate::genre::{self, genre_from_tags};
use crate::model::Category;
use crate::normalize::nfkc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settlement {
    pub category: Category,
    pub genre: Option<String>,
}

/// `true` when category id or name, plus tags when the category is ambiguous,
/// is enough to skip Ollama and Apple.
pub fn settle(category_id: Option<i64>, category_name: &str, tags: &[String]) -> Option<Settlement> {
    if let Some(id) = category_id.filter(|id| *id > 0) {
        match id_decision(id, tags) {
            IdDecision::Settled(settled) => return Some(settled),
            IdDecision::NeedsModel => return None,
            IdDecision::Unknown => {}
        }
    }
    settle_name(category_name, tags)
}

pub fn category_line(name: &str, parent: &str) -> String {
    let name = name.trim();
    let parent = parent.trim();
    if name.is_empty() && parent.is_empty() {
        return "なし".into();
    }
    if name.is_empty() {
        return parent.to_string();
    }
    if parent.is_empty() || parent == name {
        return name.to_string();
    }
    format!("{name}（親: {parent}）")
}

pub fn tags_line(tags: &[String]) -> String {
    let names: Vec<&str> = tags
        .iter()
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
        .take(40)
        .collect();
    if names.is_empty() {
        "なし".into()
    } else {
        names.join("、")
    }
}

enum IdDecision {
    Settled(Settlement),
    NeedsModel,
    Unknown,
}

fn id_decision(id: i64, tags: &[String]) -> IdDecision {
    let settled = match id {
        // Public 3Dモデル children. Parent name is 3Dモデル.
        208 | 212 => Some(fixed(Category::Avatar, None, tags, false)),
        209 => Some(fixed(Category::Outfit, Some("outfit"), tags, true)),
        231 => Some(fixed(Category::Outfit, Some("shoes_socks"), tags, true)),
        230 => Some(fixed(Category::Hair, Some("hair"), tags, true)),
        217 => Some(fixed(Category::Accessory, Some("accessory"), tags, true)),
        215 => Some(fixed(Category::Other, Some("tool_editor"), tags, true)),
        216 => Some(fixed(
            Category::Gimmick,
            Some("expression_animation"),
            tags,
            true,
        )),
        // Known, but the side or the genre is not fixed by the id alone.
        214 => texture_from_tags(tags),
        211 => world_from_tags(tags),
        210 | 127 => from_tags_only(tags),
        _ => return IdDecision::Unknown,
    };
    match settled {
        Some(settled) => IdDecision::Settled(settled),
        None => IdDecision::NeedsModel,
    }
}

fn settle_name(category_name: &str, tags: &[String]) -> Option<Settlement> {
    let folded = nfkc(category_name).to_lowercase();
    if folded.is_empty() {
        return None;
    }
    // Longer phrases first so「3Dモデル（その他）」does not match a shorter label.
    const NAMES: &[(&str, NameKind)] = &[
        ("3dモーション・アニメーション", NameKind::Motion),
        ("3dツール・システム", NameKind::Tool),
        ("3d環境・ワールド", NameKind::World),
        ("3dモデル（その他）", NameKind::Loose),
        ("3dキャラクター", NameKind::Avatar),
        ("3d装飾品", NameKind::Accessory),
        ("3dテクスチャ", NameKind::Texture),
        ("3d小道具", NameKind::Loose),
        ("3d衣装", NameKind::Outfit),
        ("3d髪型", NameKind::Hair),
        ("3d靴", NameKind::Shoes),
        ("vroid", NameKind::Avatar),
    ];
    for (phrase, kind) in NAMES {
        if folded.contains(phrase) {
            return match kind {
                NameKind::Avatar => Some(fixed(Category::Avatar, None, tags, false)),
                NameKind::Outfit => Some(fixed(Category::Outfit, Some("outfit"), tags, true)),
                NameKind::Shoes => Some(fixed(Category::Outfit, Some("shoes_socks"), tags, true)),
                NameKind::Hair => Some(fixed(Category::Hair, Some("hair"), tags, true)),
                NameKind::Accessory => {
                    Some(fixed(Category::Accessory, Some("accessory"), tags, true))
                }
                NameKind::Tool => Some(fixed(Category::Other, Some("tool_editor"), tags, true)),
                NameKind::Motion => Some(fixed(
                    Category::Gimmick,
                    Some("expression_animation"),
                    tags,
                    true,
                )),
                NameKind::Texture => texture_from_tags(tags),
                NameKind::World => world_from_tags(tags),
                NameKind::Loose => from_tags_only(tags),
            };
        }
    }
    None
}

#[derive(Clone, Copy)]
enum NameKind {
    Avatar,
    Outfit,
    Shoes,
    Hair,
    Accessory,
    Tool,
    Motion,
    Texture,
    World,
    Loose,
}

fn fixed(
    category: Category,
    default_genre: Option<&str>,
    tags: &[String],
    allow_tag_genre: bool,
) -> Settlement {
    let from_tags = if allow_tag_genre {
        genre_from_tags(category, tags).filter(|genre| {
            genre::validate(category, Some(genre)) == *genre
                && genre::category_for_genre(genre) == Some(category)
        })
    } else {
        None
    };
    Settlement {
        category,
        genre: from_tags.or_else(|| default_genre.map(str::to_string)),
    }
}

fn texture_from_tags(tags: &[String]) -> Option<Settlement> {
    genre_from_tags(Category::Texture, tags)
        .filter(|genre| genre::validate(Category::Texture, Some(genre)) == *genre)
        .map(|genre| Settlement {
            category: Category::Texture,
            genre: Some(genre),
        })
}

fn world_from_tags(tags: &[String]) -> Option<Settlement> {
    genre_from_tags(Category::World, tags)
        .filter(|genre| genre::validate(Category::World, Some(genre)) == *genre)
        .map(|genre| Settlement {
            category: Category::World,
            genre: Some(genre),
        })
}

/// 3D小道具 and 3Dモデル（その他）: tags pick the side, or the model does.
fn from_tags_only(tags: &[String]) -> Option<Settlement> {
    let kept: Vec<&str> = tags
        .iter()
        .map(String::as_str)
        .filter(|tag| !tag.trim().is_empty() && !genre::generic_tag(tag))
        .collect();
    if kept.is_empty() {
        return None;
    }
    let text = kept.join("\n");
    if world_side_tag(&text) {
        let genre = genre_from_tags(Category::World, tags).unwrap_or_else(|| "world".into());
        let genre = if genre::validate(Category::World, Some(&genre)) == genre {
            genre
        } else {
            "world".into()
        };
        return Some(Settlement {
            category: Category::World,
            genre: Some(genre),
        });
    }
    let genre = genre_from_tags(Category::Other, tags)?;
    let category = genre::category_for_genre(&genre)?;
    if genre::validate(category, Some(&genre)) != genre {
        return None;
    }
    Some(Settlement {
        category,
        genre: Some(genre),
    })
}

/// World-side words. A bare「ギミック」stays an avatar gimmick.
fn world_side_tag(text: &str) -> bool {
    let lower = nfkc(text).to_lowercase();
    const WORDS: &[&str] = &[
        "ワールド",
        "udon",
        "skybox",
        "スカイボックス",
        "家具",
        "ベッド",
        "椅子",
        "いす",
        "ソファ",
        "建物",
        "地形",
    ];
    if WORDS.iter().any(|word| lower.contains(word)) {
        return true;
    }
    lower
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .any(|part| part == "world" || part == "worlds")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn confirmed_ids_settle_without_tags() {
        let tool = settle(Some(215), "別の名前", &[]).unwrap();
        assert_eq!(tool.category, Category::Other);
        assert_eq!(tool.genre.as_deref(), Some("tool_editor"));
        assert_eq!(settle(Some(208), "", &[]).unwrap().category, Category::Avatar);
        assert_eq!(settle(Some(212), "", &[]).unwrap().category, Category::Avatar);
        assert_eq!(
            settle(Some(209), "", &[]).unwrap().genre.as_deref(),
            Some("outfit")
        );
        assert_eq!(
            settle(Some(231), "", &[]).unwrap().genre.as_deref(),
            Some("shoes_socks")
        );
        assert_eq!(settle(Some(230), "", &[]).unwrap().category, Category::Hair);
        assert_eq!(
            settle(Some(217), "", &[]).unwrap().genre.as_deref(),
            Some("accessory")
        );
        assert_eq!(
            settle(Some(216), "", &[]).unwrap().genre.as_deref(),
            Some("expression_animation")
        );
    }

    #[test]
    fn id_wins_over_a_conflicting_name() {
        let settled = settle(Some(215), "3D衣装", &tags(&["衣装"])).unwrap();
        assert_eq!(settled.category, Category::Other);
        assert_eq!(settled.genre.as_deref(), Some("tool_editor"));
    }

    #[test]
    fn unknown_id_uses_the_confirmed_name() {
        let outfit = settle(Some(999), "配布用3D衣装", &[]).unwrap();
        assert_eq!(outfit.category, Category::Outfit);
        assert_eq!(outfit.genre.as_deref(), Some("outfit"));
        assert_eq!(settle(Some(999), "VRoid", &[]).unwrap().category, Category::Avatar);
        assert!(settle(Some(999), "未確認の分類", &tags(&["家具"])).is_none());
        assert!(settle(None, "3Dモデル", &[]).is_none());
    }

    #[test]
    fn props_need_a_specific_tag_or_the_model() {
        assert!(settle(Some(210), "3D小道具", &[]).is_none());
        assert!(settle(Some(210), "3D小道具", &tags(&["VRChat", "Unity", "3Dモデル"])).is_none());
        let furniture = settle(Some(210), "3D小道具", &tags(&["VRChat", "家具"])).unwrap();
        assert_eq!(furniture.category, Category::World);
        assert_eq!(furniture.genre.as_deref(), Some("furniture_props"));
        let sky = settle(Some(210), "3D小道具", &tags(&["Skybox"])).unwrap();
        assert_eq!(sky.genre.as_deref(), Some("skybox"));
        let gimmick = settle(Some(210), "3D小道具", &tags(&["ギミック"])).unwrap();
        assert_eq!(gimmick.category, Category::Gimmick);
        assert_eq!(gimmick.genre.as_deref(), Some("gimmick"));
        assert!(settle(Some(127), "3Dモデル（その他）", &tags(&["VRChat"])).is_none());
        let prop_outfit = settle(Some(127), "3Dモデル（その他）", &tags(&["衣装"])).unwrap();
        assert_eq!(prop_outfit.category, Category::Outfit);
    }

    #[test]
    fn texture_and_world_without_useful_tags_stay_for_the_model() {
        assert!(settle(Some(214), "3Dテクスチャ", &[]).is_none());
        assert!(settle(Some(214), "3Dテクスチャ", &tags(&["VRChat"])).is_none());
        let eye = settle(Some(214), "3Dテクスチャ", &tags(&["瞳"])).unwrap();
        assert_eq!(eye.category, Category::Texture);
        assert_eq!(eye.genre.as_deref(), Some("eye_texture"));
        assert!(settle(Some(211), "3D環境・ワールド", &[]).is_none());
        let world = settle(Some(211), "3D環境・ワールド", &tags(&["Udon"])).unwrap();
        assert_eq!(world.category, Category::World);
        assert_eq!(world.genre.as_deref(), Some("world_gimmick"));
    }

    #[test]
    fn a_specific_category_still_settles_when_tags_are_generic() {
        let outfit = settle(Some(209), "3D衣装", &tags(&["VRChat", "Unity"])).unwrap();
        assert_eq!(outfit.category, Category::Outfit);
        assert_eq!(outfit.genre.as_deref(), Some("outfit"));
        let shoes = settle(None, "3D衣装", &tags(&["靴"])).unwrap();
        assert_eq!(shoes.genre.as_deref(), Some("shoes_socks"));
    }

    #[test]
    fn prompt_lines_name_the_category_and_tags() {
        assert_eq!(
            category_line("3D衣装", "3Dモデル"),
            "3D衣装（親: 3Dモデル）"
        );
        assert_eq!(category_line("", ""), "なし");
        assert_eq!(tags_line(&tags(&["ダミー", "衣装"])), "ダミー、衣装");
        assert_eq!(tags_line(&[]), "なし");
    }
}
