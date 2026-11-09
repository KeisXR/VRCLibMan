use super::*;

const ID: &str = "7777001";

fn imported(
    title: &str,
    description: &str,
    category: Option<(i64, &str)>,
    tags: &[&str],
) -> Connection {
    let mut connection = open_memory().unwrap();
    let mut item = serde_json::json!({
        "boothId": ID, "title": title, "description": description, "tags": tags,
    });
    if let Some((id, name)) = category {
        item["category"] = serde_json::json!({"id": id, "name": name});
    }
    import_export_json(&mut connection, &item.to_string()).unwrap();
    connection
}

fn expect(connection: &Connection, category: Category, genre: &str) {
    let product = require_product(connection, ID).unwrap();
    assert_eq!(product.category, Some(category), "{}", product.title);
    assert_eq!(product.genre.as_deref(), Some(genre), "{}", product.title);
}

fn extract_gimmick(connection: &mut Connection, genre: &str) {
    apply_extraction(
        connection,
        ID,
        &Extraction {
            category: Category::Gimmick,
            genre: Some(genre.into()),
            compatible_avatars: vec![],
            is_generic: false,
            confidence: 0.95,
        },
        0.6,
    )
    .unwrap();
}

#[test]
fn explicit_world_use_in_a_gimmick_description_survives_both_save_paths() {
    for description in [
        "UdonSharpを使用したワールド用ギミックです。",
        "UdonSharpを使用しています。",
        "ワールド用ギミックです。",
        "World Prefabとして使用するギミックです。",
        "A gimmick for VRChat worlds.",
    ] {
        for category in [
            None,
            Some((215, "3Dツール・システム")),
            Some((201, "ソフトウェア")),
        ] {
            let mut connection = imported("表示ギミック", description, category, &[]);
            if category.is_some() {
                assert!(apply_booth_settlement(&mut connection, ID).unwrap());
            } else {
                extract_gimmick(&mut connection, "gimmick");
            }
            expect(&connection, Category::World, "world_gimmick");
        }
    }
    let mut connection = imported(
        "撫で音ギミック",
        "影シェーダーを同梱。お試しワールド https://vrchat.com/home/world/wrld_example",
        Some((215, "3Dツール・システム")),
        &[],
    );
    assert!(apply_booth_settlement(&mut connection, ID).unwrap());
    expect(&connection, Category::Gimmick, "gimmick");
}

#[test]
fn mochifitter_compatibility_keeps_settled_wearables_out_of_tools() {
    for (title, id, name, category, genre) in [
        (
            "もちふぃった～対応 パーカー",
            209,
            "3D衣装",
            Category::Outfit,
            "outfit",
        ),
        (
            "Mochifitter対応 ショートヘア",
            230,
            "3D髪型",
            Category::Hair,
            "hair",
        ),
        (
            "もちふぃった～変換プロファイル",
            215,
            "3Dツール・システム",
            Category::Other,
            "tool_editor",
        ),
    ] {
        let mut connection = imported(title, "対応データです。", Some((id, name)), &[]);
        assert!(apply_booth_settlement(&mut connection, ID).unwrap());
        expect(&connection, category, genre);
    }
}

#[test]
fn placement_does_not_replace_a_specific_world_asset_genre() {
    for (title, description, tags, genre) in [
        (
            "ワールド用ベッド",
            "ワールドに配置できる家具です。",
            vec!["家具"],
            "furniture_props",
        ),
        (
            "お部屋セット",
            "ワールドに配置できます。",
            vec!["家具"],
            "furniture_props",
        ),
        (
            "ワールド用Skybox",
            "ワールドに配置できます。",
            vec!["Skybox"],
            "skybox",
        ),
        (
            "ワールド用シェーダー",
            "ワールドに配置できます。",
            vec!["シェーダー"],
            "world_shader",
        ),
    ] {
        let mut connection = imported(title, description, Some((211, "3D環境・ワールド")), &tags);
        assert!(apply_booth_settlement(&mut connection, ID).unwrap());
        expect(&connection, Category::World, genre);
    }
    let (category, genre) = crate::classifier::classify_listing(
        "ワールド用ベッド",
        "",
        "ワールドに配置できる家具です。",
    );
    assert_eq!(
        (category, genre.as_str()),
        (Category::World, "furniture_props")
    );
}

#[test]
fn gpu_particles_need_world_use_before_leaving_the_avatar_side() {
    for title in ["アバター用GPUパーティクル", "Avatar GPU Particle"] {
        let mut connection = imported(title, "アバターに取り付けるエフェクトです。", None, &[]);
        extract_gimmick(&mut connection, "particle_effect");
        expect(&connection, Category::Gimmick, "particle_effect");
    }
    let mut connection = imported(
        "GPU Snow - GPU Particle Volumes",
        "ワールド用のGPU雪パーティクル。シェーダーを同梱。",
        Some((215, "3Dツール・システム")),
        &[],
    );
    assert!(apply_booth_settlement(&mut connection, ID).unwrap());
    expect(&connection, Category::World, "tool_other");
}

#[test]
fn animation_in_the_title_keeps_the_motion_genre() {
    for title in ["まばたきアニメーションギミック", "Blink Animation Gimmick"] {
        let mut connection = imported(
            title,
            "まばたきモーションです。",
            Some((216, "3Dモーション・アニメーション")),
            &[],
        );
        assert!(apply_booth_settlement(&mut connection, ID).unwrap());
        expect(&connection, Category::Gimmick, "expression_animation");
    }
}

#[test]
fn version_seven_classifications_can_be_reclassified_without_changing_manual_locks() {
    let mut connection = imported(
        "もちふぃった～対応 パーカー",
        "衣装です。",
        Some((209, "3D衣装")),
        &[],
    );
    assert!(apply_booth_settlement(&mut connection, ID).unwrap());
    connection
        .execute(
            "UPDATE products SET classifier_version = 7 WHERE booth_id = ?1",
            [ID],
        )
        .unwrap();
    assert_eq!(
        products_for_extract(&connection, ExtractTarget::Stale)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(classification_status(&connection).unwrap().stale_count, 1);
    assert!(apply_booth_settlement(&mut connection, ID).unwrap());
    expect(&connection, Category::Outfit, "outfit");
    assert!(
        products_for_extract(&connection, ExtractTarget::Stale)
            .unwrap()
            .is_empty()
    );

    move_product_genre(&mut connection, ID, "asset", "tool_editor").unwrap();
    connection
        .execute(
            "UPDATE products SET classifier_version = 7 WHERE booth_id = ?1",
            [ID],
        )
        .unwrap();
    assert!(
        products_for_extract(&connection, ExtractTarget::Stale)
            .unwrap()
            .is_empty()
    );
    assert!(!apply_booth_settlement(&mut connection, ID).unwrap());
    expect(&connection, Category::Other, "tool_editor");
}
