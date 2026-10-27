use crate::model::Category;

pub const ASSET_GENRES: &[(&str, &str)] = &[
    ("outfit", "衣装"),
    ("shoes_socks", "靴・靴下"),
    ("hair", "髪型"),
    ("accessory", "アクセサリ"),
    ("head_ears_tail", "頭・耳・しっぽ"),
    ("makeup_skin", "メイク・肌テクスチャ"),
    ("eye_texture", "アイテクスチャ"),
    ("expression_animation", "表情・アニメーション"),
    ("pose", "ポーズ"),
    ("gimmick", "ギミック"),
    ("particle_effect", "パーティクル・エフェクト"),
    ("shader", "シェーダー"),
    ("tool_editor", "ツール・エディタ拡張"),
    ("other", "その他"),
];
pub const WORLD_GENRES: &[(&str, &str)] = &[
    ("world", "ワールド本体"),
    ("furniture_props", "家具・小物（ベッドなど）"),
    ("world_gimmick", "ワールドギミック（Udon）"),
    ("texture_material", "テクスチャ置換・マテリアル"),
    ("skybox", "Skybox"),
    ("lighting_postprocess", "ライティング・ポストプロセス"),
    ("world_shader", "ワールド用シェーダー"),
    ("building_terrain", "建物・地形パーツ"),
    ("audio_bgm", "音・BGM"),
    ("tool_other", "ツール・その他"),
];

/// Route / query key for products whose genre is missing or not one of their kind's genres.
pub const UNSET: &str = "unset";

/// Browse key for free products that are not on disk. Not a stored genre.
pub const FREE: &str = "free";

/// Genres of a browse kind (`asset` / `world`).
pub fn kind_genres(kind: &str) -> Option<&'static [(&'static str, &'static str)]> {
    match kind {
        "world" => Some(WORLD_GENRES),
        "asset" => Some(ASSET_GENRES),
        _ => None,
    }
}

pub fn genres(category: Category) -> &'static [(&'static str, &'static str)] {
    if category == Category::World {
        WORLD_GENRES
    } else {
        ASSET_GENRES
    }
}

/// Category stored when a person moves a product onto this genre.
/// Asset genres stay on the asset side; every world genre is `world`.
pub fn category_for_genre(genre: &str) -> Option<Category> {
    if WORLD_GENRES.iter().any(|(key, _)| *key == genre) {
        return Some(Category::World);
    }
    let category = match genre {
        "outfit" | "shoes_socks" => Category::Outfit,
        "hair" => Category::Hair,
        "accessory" | "head_ears_tail" => Category::Accessory,
        "makeup_skin" | "eye_texture" => Category::Texture,
        "expression_animation" | "pose" | "gimmick" | "particle_effect" => Category::Gimmick,
        "shader" | "tool_editor" | "other" => Category::Other,
        _ => return None,
    };
    if validate(category, Some(genre)) != genre {
        return None;
    }
    Some(category)
}

pub fn validate(category: Category, genre: Option<&str>) -> String {
    let fallback = if category == Category::World {
        "tool_other"
    } else {
        "other"
    };
    genre
        .filter(|value| genres(category).iter().any(|(key, _)| key == value))
        .unwrap_or(fallback)
        .to_string()
}

/// Deterministic first-pass classification. Keep specific rules before broad ones.
pub fn classify_text(category: Category, title: &str, description: &str) -> Option<String> {
    let text = format!("{title}\n{description}").to_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| text.contains(word));
    let key = if category == Category::World {
        if has(&["skybox", "スカイボックス"]) {
            "skybox"
        } else if has(&["ベッド", "椅子", "いす", "家具", "ソファ"]) {
            "furniture_props"
        } else if has(&["udon", "ワールドギミック", "ギミック"]) {
            "world_gimmick"
        } else if has(&["ライティング", "post process", "ポストプロセス"]) {
            "lighting_postprocess"
        } else if has(&["シェーダー", "shader"]) {
            "world_shader"
        } else if has(&["bgm", "効果音", "音源"]) {
            "audio_bgm"
        } else if has(&["建物", "地形", "building", "terrain"]) {
            "building_terrain"
        } else if has(&["テクスチャ", "マテリアル", "material"]) {
            "texture_material"
        } else if has(&["ワールド", "world"]) {
            "world"
        } else {
            return None;
        }
    } else if has(&["アイテクスチャ", "瞳", "eye texture", "eyetexture"]) {
        "eye_texture"
    } else if has(&["靴", "シューズ", "shoe", "靴下", "ソックス"]) {
        "shoes_socks"
    } else if has(&["髪型", "髪", "ヘア", "hair"]) {
        "hair"
    } else if has(&["メイク", "肌テクスチャ", "skin texture"]) {
        "makeup_skin"
    } else if has(&["表情", "アニメーション", "animation"]) {
        "expression_animation"
    } else if has(&["ポーズ", "pose"]) {
        "pose"
    } else if has(&["パーティクル", "particle", "エフェクト", "effect"]) {
        "particle_effect"
    } else if has(&["シェーダー", "shader"]) {
        "shader"
    } else if has(&["ツール", "エディタ拡張", "editor extension"]) {
        "tool_editor"
    } else if has(&["耳", "しっぽ", "尻尾"]) {
        "head_ears_tail"
    } else if has(&["アクセサリ", "accessory", "ピアス", "メガネ"]) {
        "accessory"
    } else if has(&["ギミック", "gimmick"]) {
        "gimmick"
    } else if has(&["衣装", "服", "outfit", "dress"]) {
        "outfit"
    } else {
        return None;
    };
    Some(key.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keyword_rules() {
        assert_eq!(
            classify_text(Category::Texture, "きらきら瞳", "ダミー"),
            Some("eye_texture".into())
        );
        assert_eq!(
            classify_text(Category::World, "夜の Skybox", "ダミー"),
            Some("skybox".into())
        );
        assert_eq!(
            classify_text(Category::World, "家具セット", "ベッドと椅子"),
            Some("furniture_props".into())
        );
    }
    #[test]
    fn mismatched_genre_falls_back() {
        assert_eq!(validate(Category::World, Some("hair")), "tool_other");
        assert_eq!(validate(Category::Hair, Some("skybox")), "other");
        assert_eq!(validate(Category::Accessory, Some(FREE)), "other");
        assert_eq!(validate(Category::World, Some(FREE)), "tool_other");
    }

    #[test]
    fn move_category_stays_inside_the_genre_kind() {
        for (key, _) in ASSET_GENRES {
            let category = category_for_genre(key).unwrap();
            assert_ne!(category, Category::World, "{key}");
            assert_eq!(validate(category, Some(key)), *key);
        }
        for (key, _) in WORLD_GENRES {
            assert_eq!(category_for_genre(key), Some(Category::World));
            assert_eq!(validate(Category::World, Some(key)), *key);
        }
        assert_eq!(category_for_genre("unset"), None);
        assert_eq!(category_for_genre("avatar"), None);
    }
}
