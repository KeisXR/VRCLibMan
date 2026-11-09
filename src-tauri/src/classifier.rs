/// Bump when extraction prompt, schema, or genre/category rules change materially.
/// 5: a settled BOOTH category keeps a fitting genre. Title phrases do not replace it.
/// Category 201 (software) settles as other / tool_editor.
/// 6: category-name variants 3Dアクセサリ, 3Dモーション, and アバター用テクスチャ settle
/// the same way as the longer official names. Ids 157, 142, and 6 stay unsettled.
/// 7: world placement, avatar gimmicks, outfits, stickers, and conversion profiles
/// beat incidental words. Coarse tool/software categories yield when the text says so.
/// 8: keep explicit world use, wearable compatibility, specific world asset genres,
/// avatar GPU particles, and title animations on their intended classification paths.
/// 9: no language model. The title and description rules fill what the BOOTH category and
/// tags leave open, and download file names suggest avatars when the description has no list.
pub const CLASSIFIER_VERSION: i32 = 9;

/// Stored with a rule result. Name resolution and review use their own facts. At this value
/// a weak world hint in the description does not override the rule category.
pub const RULE_CONFIDENCE: f64 = 0.7;

/// The result for a product the BOOTH category and tags left open, from its title and
/// description. Avatar names are read later from the same text and from file names.
pub fn rule_extraction(title: &str, description: &str) -> crate::model::Extraction {
    let (category, genre) = classify_listing(title, "", description);
    crate::model::Extraction {
        category,
        genre: Some(genre),
        compatible_avatars: Vec::new(),
        is_generic: crate::compat::says_generic(title, description),
        confidence: RULE_CONFIDENCE,
    }
}

/// Title and description decide the label. Shop names are not features.
pub fn classify_listing(
    title: &str,
    _shop: &str,
    description: &str,
) -> (crate::model::Category, String) {
    let category = crate::compat::category_for_listed_product(title, description);
    let genre = crate::genre::prefer_listing(category, title, description)
        .or_else(|| crate::genre::classify_title_gap(category, title, description))
        .or_else(|| crate::genre::classify_text(category, title, description))
        .unwrap_or_else(|| crate::genre::validate(category, None));
    (category, genre)
}

/// Software (201) and 3Dツール (215) stay tools unless the title and description
/// name a world placement, a world utility, or an avatar gimmick.
pub fn coarse_tool_override(
    title: &str,
    description: &str,
) -> Option<(crate::model::Category, String)> {
    let (category, genre) = classify_listing(title, "", description);
    match category {
        crate::model::Category::World => Some((category, genre)),
        crate::model::Category::Gimmick if crate::compat::avatar_gimmick_title(title) => {
            Some((category, genre))
        }
        _ => None,
    }
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
        assert_eq!(CLASSIFIER_VERSION, 9);
    }

    #[test]
    fn feedback_phrases_keep_the_corrected_labels() {
        use crate::model::Category;
        let listing = |title: &str, description: &str| classify_listing(title, "", description);
        let expect = |title: &str, description: &str, category: Category, genre: &str| {
            assert_eq!(
                listing(title, description),
                (category, genre.to_string()),
                "{title}"
            );
        };

        // World gimmicks that were falling through to other/tool_editor.
        expect(
            "ヨドコロちゃんのポテトチップス",
            "VRChatワールドに配置できる。ギミックにより食べられる。Udonで動く。",
            Category::World,
            "world_gimmick",
        );
        expect(
            "VRChat CPU Affinity Auto Change",
            "PC上でVRChatのCPU割り当てを変えるアプリ。",
            Category::World,
            "tool_other",
        );
        expect(
            "TextShareBoard",
            "unitypackageのprefabをワールドに配置するテキスト共有ボード。",
            Category::World,
            "world_gimmick",
        );
        expect(
            "ワールド用の防音室ギミック",
            "アバターの声を減衰する。",
            Category::World,
            "world_gimmick",
        );
        expect(
            "Binaural ASMR System",
            "ワールド設置型のUdonギミック。",
            Category::World,
            "world_gimmick",
        );
        expect(
            "GPU Snow - GPU Particle Volumes",
            "ワールド用のGPU雪パーティクル。シェーダーを同梱。",
            Category::World,
            "tool_other",
        );
        expect(
            "Udon PassCode",
            "数字で表示を切り替えるワールドギミック。",
            Category::World,
            "world_gimmick",
        );

        // Avatar gimmicks. A bundled shader or a related animation does not retitle them.
        expect(
            "すりすりギミック",
            "Modular Avatarで設定済み。影のシェーダーをオンオフできる。",
            Category::Gimmick,
            "gimmick",
        );
        expect(
            "なめらか心音ギミック OSC対応",
            "OSCで心拍に合わせる。呼吸アニメーションシステムと併用できる。",
            Category::Gimmick,
            "gimmick",
        );
        expect(
            "撫で音ギミック VRCアバターギミック",
            "なでなでで音が鳴る。影シェーダーを同梱しています。お試しワールドがあります。",
            Category::Gimmick,
            "gimmick",
        );

        // Outfits. Parts lists and hair credits do not change the genre.
        expect(
            "40アバター対応 at2am. MA対応",
            "VRChat想定の衣装です。カラバリ31色。ポリゴン数とマテリアルの表。靴下とサンダルを含む。",
            Category::Outfit,
            "outfit",
        );
        expect(
            "ねこネコパーカー MA対応",
            "ねこになれるパーカーコーデ。使用した髪のクレジット。",
            Category::Outfit,
            "outfit",
        );

        // A bobbing nameplate stays an accessory genre.
        expect(
            "ひらがな3D文字 (アニメーション付き)",
            "アクセサリーや名札になる。軽く上下に動く。",
            Category::Gimmick,
            "accessory",
        );

        // Stickers are not hair. Hair-color edits in the illustration do not count.
        let sticker = listing(
            "Boop絵文字・ステッカー",
            "PNGとPSD。髪色、服、装飾の色を変更できる。",
        );
        assert_ne!(sticker.0, Category::Hair);
        assert_ne!(sticker.1, "hair");
        assert_eq!(sticker, (Category::Other, "other".into()));

        // Conversion profiles are tools. Shape-key names are not shoes.
        expect(
            "もちふぃった～変換プロファイル プラム用",
            "ハイヒールシェイプキーとkisekae_Shoesに対応。",
            Category::Other,
            "tool_editor",
        );
        expect(
            "あのめあちゃん もちふぃった～変換プロファイル",
            "プロファイルのみ。アバターと衣装とツールは同梱しません。",
            Category::Other,
            "tool_editor",
        );
        assert_ne!(
            listing(
                "もちふぃった～変換プロファイル プラム用",
                "ハイヒールシェイプキーとkisekae_Shoesに対応。"
            )
            .1,
            "shoes_socks"
        );

        // Editor tools stay tools, including ones that mention textures or shape keys.
        expect(
            "LipSyncをちっちゃくかわいく プラム対応",
            "LipSync用シェイプキーを変更したメッシュを生成するツールです。",
            Category::Other,
            "tool_editor",
        );
        expect(
            "非対応アイテクスチャを使えるようにするツール",
            "アバター間で瞳テクスチャを変換する。",
            Category::Other,
            "tool_editor",
        );
        expect(
            "ライティング調節ツール Light Limit Changer",
            "アバターの明るさメニューを生成するツールです。Modular Avatar前提。",
            Category::Other,
            "tool_editor",
        );
        expect(
            "IntParameterCompressor",
            "ExpressionParameterを節約するUnity専用のツールです。",
            Category::Other,
            "tool_editor",
        );
        expect(
            "BlendShare",
            "シェイプキーを配布するUnityツールです。",
            Category::Other,
            "tool_editor",
        );
        expect(
            "追加シェイプキー",
            "BlendShareで導入する追加シェイプキーのツールです。",
            Category::Other,
            "tool_editor",
        );
        expect(
            "KonoAsset アセット管理ツール",
            "衣装の場所を探すためのツールです。",
            Category::Other,
            "tool_editor",
        );
        expect(
            "Booth購入金額集計ツール",
            "購入履歴を集計するChrome拡張のツールです。",
            Category::Other,
            "tool_editor",
        );
        expect(
            "もちふぃった～",
            "衣装を非対応アバターに合わせるためのツールです。複雑なギミックは動かないことがあります。",
            Category::Other,
            "tool_editor",
        );
        expect(
            "アバター衣装変換ツール きせった",
            "衣装編集やアバター対応に使うツールです。",
            Category::Other,
            "tool_editor",
        );
        expect(
            "EasyAvatarOptimizer 最適化ツール",
            "アバターを自動で分析するツールです。",
            Category::Other,
            "tool_editor",
        );
        expect(
            "ベロウゴカシ 汎用ツール",
            "舌ボーンを入れるツールです。表情変化ギミックと競合することがあります。",
            Category::Other,
            "tool_editor",
        );
    }
}
