/// Bump when extraction prompt, schema, or genre/category rules change materially.
/// 4: title phrases fill products that BOOTH category and tags did not settle.
pub const CLASSIFIER_VERSION: i32 = 4;

/// Title and description decide the label. Shop names are not features.
pub fn classify_listing(
    title: &str,
    _shop: &str,
    description: &str,
) -> (crate::model::Category, String) {
    let category = crate::compat::category_for_listed_product(title, description);
    let genre = crate::genre::classify_title_gap(category, title, description)
        .or_else(|| crate::genre::classify_text(category, title, description))
        .unwrap_or_else(|| crate::genre::validate(category, None));
    (category, genre)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Local only. Reads a feedback export from `VRCLIBMAN_FEEDBACK_JSON` and prints
    /// match counts. Titles are never printed. Ignored so CI does not need the file.
    #[test]
    #[ignore]
    fn feedback_listing_match_counts() {
        let path = std::env::var("VRCLIBMAN_FEEDBACK_JSON").unwrap_or_default();
        if path.is_empty() {
            eprintln!("VRCLIBMAN_FEEDBACK_JSON is unset");
            return;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{err}"));
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let manuals = value["manualClassifications"].as_array().expect("manual");
        let mut matched = 0usize;
        let mut misses: Vec<(String, String)> = Vec::new();
        for item in manuals {
            let title = item["title"].as_str().unwrap_or("");
            let shop = item["shop"].as_str().unwrap_or("");
            let truth_c = item["category"].as_str().unwrap_or("");
            let truth_g = item["genre"].as_str().unwrap_or("");
            let (category, genre) = classify_listing(title, shop, "");
            if category.as_str() == truth_c && genre == truth_g {
                matched += 1;
            } else {
                misses.push((
                    format!("{}/{}", category.as_str(), genre),
                    format!("{truth_c}/{truth_g}"),
                ));
            }
        }
        misses.sort();
        let mut folded: Vec<(String, usize)> = Vec::new();
        for pair in &misses {
            let key = format!("{} -> {}", pair.0, pair.1);
            if let Some(found) = folded.iter_mut().find(|(label, _)| label == &key) {
                found.1 += 1;
            } else {
                folded.push((key, 1));
            }
        }
        eprintln!("match {matched}/{}", manuals.len());
        for (label, count) in folded {
            eprintln!("miss {count} {label}");
        }
    }

    #[test]
    fn fictional_titles_follow_the_keyword_rules() {
        use crate::model::Category;
        let listing = |title: &str| classify_listing(title, "", "");
        assert_eq!(
            listing("サンプルこたつセット"),
            (Category::World, "furniture_props".into())
        );
        assert_eq!(
            listing("サンプル毛布"),
            (Category::World, "furniture_props".into())
        );
        assert_eq!(
            listing("Sample Earring"),
            (Category::Accessory, "accessory".into())
        );
        assert_eq!(
            listing("Sample Ring"),
            (Category::Accessory, "accessory".into())
        );
        assert_eq!(
            listing("Sample Glasses"),
            (Category::Accessory, "accessory".into())
        );
        assert_eq!(
            listing("Sample Bag"),
            (Category::Accessory, "accessory".into())
        );
        assert_eq!(
            listing("サンプル磁気ディスク"),
            (Category::Accessory, "accessory".into())
        );
        assert_eq!(
            listing("サンプルカード"),
            (Category::Accessory, "accessory".into())
        );
        assert_eq!(listing("Sample String"), (Category::Other, "other".into()));
        assert_eq!(
            listing("Sample Techwear"),
            (Category::Outfit, "outfit".into())
        );
        assert_eq!(
            listing("サンプルルームウェア"),
            (Category::Outfit, "outfit".into())
        );
        assert_eq!(
            listing("サンプルずぼん"),
            (Category::Outfit, "outfit".into())
        );
        assert_eq!(
            listing("Sample Fullpack"),
            (Category::Outfit, "outfit".into())
        );
        assert_eq!(
            listing("サンプル衣装キット"),
            (Category::Outfit, "outfit".into())
        );
        assert_eq!(
            listing("サンプルなでなで"),
            (Category::Gimmick, "gimmick".into())
        );
        assert_eq!(
            listing("Sample KissSystem"),
            (Category::Gimmick, "gimmick".into())
        );
        assert_eq!(
            listing("Sample Phone"),
            (Category::Gimmick, "gimmick".into())
        );
        assert_eq!(
            listing("サンプルSPS"),
            (Category::Gimmick, "gimmick".into())
        );
        assert_eq!(listing("心音キット"), (Category::Gimmick, "gimmick".into()));
        assert_eq!(
            listing("Sample Face Tracking Add-on"),
            (Category::Gimmick, "expression_animation".into())
        );
        assert_eq!(
            listing("サンプルシェイプキー"),
            (Category::Gimmick, "expression_animation".into())
        );
        assert_eq!(listing("Sample Add-on"), (Category::Other, "other".into()));
        assert_eq!(
            listing("Sample QvPen差し替えモデル"),
            (Category::World, "texture_material".into())
        );
        assert_eq!(
            listing("サンプルネオンスイッチ"),
            (Category::World, "texture_material".into())
        );
        assert_eq!(
            listing("Udonのスイッチ"),
            (Category::World, "world_gimmick".into())
        );
        assert_eq!(
            listing("Sample ASMR System"),
            (Category::World, "world_gimmick".into())
        );
        assert_eq!(
            listing("Sample CPU Affinity"),
            (Category::World, "tool_other".into())
        );
        assert_eq!(
            listing("サンプルワールド"),
            (Category::World, "world".into())
        );
        assert_eq!(
            listing("Sample Eye texture"),
            (Category::Texture, "eye_texture".into())
        );
        assert_eq!(
            listing("サンプルニップルテクスチャ"),
            (Category::Texture, "makeup_skin".into())
        );
        assert_eq!(
            listing("もちふぃった～"),
            (Category::Other, "tool_editor".into())
        );
        assert_eq!(
            listing(
                "【VRChat / 3D model】サンプル子 (.xavater / .unitypackage / .vroid) Editable model data"
            ),
            (Category::Avatar, "other".into())
        );
        assert_eq!(
            listing("サンプル衣装 (.xavater / .vroid)"),
            (Category::Outfit, "outfit".into())
        );
        assert_eq!(
            listing("サンプル子.unitypackage"),
            (Category::Other, "other".into())
        );
        assert_eq!(
            listing("VRChat向け3Dモデル"),
            (Category::Other, "other".into())
        );
        assert_ne!(listing("サンプルペンライト").0, Category::World);
        assert_eq!(listing("汎用の小物"), (Category::Other, "other".into()));
        assert_eq!(
            classify_listing("無関係な題", "Phone Shop", ""),
            (Category::Other, "other".into())
        );
        assert_eq!(CLASSIFIER_VERSION, 4);
    }
}
