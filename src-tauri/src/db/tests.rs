//! Tests of the whole `db` module. They reach every file through `super::*`.
use super::*;
use crate::import_data::parse_export;
use crate::view::{avatar_cards, avatar_detail, products_by_ids};

#[test]
fn reclassifying_drops_links_made_from_a_version_number_in_the_title() {
    let mut connection = open_memory().unwrap();
    connection
            .execute_batch(
                "INSERT INTO avatars (id, display_name, base_body, aliases_json, is_builtin, owned, notes)
                 VALUES ('fictional-v3a', '架空フェイ', NULL, '[\"FictionalFei\",\"v3\"]', 0, 0, ''),
                        ('fictional-v3b', '架空ハゲ', NULL, '[\"V3\"]', 0, 0, '');",
            )
            .unwrap();
    let title = "【アバター衣装変換ツール】架空の変換ツール V3.6.0";
    import_export_json(
            &mut connection,
            &format!(r#"[{{"boothId":"8800991","title":"{title}","description":"衣装を変換するツールです。"}}]"#),
        )
        .unwrap();
    // The state an older classifier left behind.
    connection
            .execute_batch(
                "INSERT INTO product_avatars VALUES ('8800991','fictional-v3a','extracted'),('8800991','fictional-v3b','extracted');
                 UPDATE products SET classifier_version = 9, compatible_avatars_json = '[\"架空フェイ\",\"架空ハゲ\"]' WHERE booth_id = '8800991';",
            )
            .unwrap();
    let stale = products_for_extract(&connection, ExtractTarget::All).unwrap();
    assert!(stale.iter().any(|job| job.booth_id == "8800991"));
    let extraction = crate::classifier::rule_extraction(title, "衣装を変換するツールです。");
    apply_extraction(&mut connection, "8800991", &extraction, 0.6).unwrap();
    reresolve_all(&mut connection).unwrap();
    let linked: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM product_avatars WHERE product_id = '8800991'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(linked, 0);
}

#[test]
fn only_registered_local_paths_may_be_opened() {
    let connection = open_memory().unwrap();
    connection
            .execute_batch(
                "INSERT INTO products (booth_id, title, imported_at, updated_at) VALUES ('4242', '架空の服', 't', 't');
                 INSERT INTO product_files (booth_id, filename, local_path) VALUES ('4242', 'pack.zip', '/lib/4242/pack.zip');
                 INSERT INTO local_files (path, filename, scanned_at) VALUES ('/lib/scan/other.unitypackage', 'other.unitypackage', 't');",
            )
            .unwrap();
    assert!(is_known_local_path(&connection, "/lib/4242/pack.zip").unwrap());
    assert!(is_known_local_path(&connection, "/lib/scan/other.unitypackage").unwrap());
    assert!(!is_known_local_path(&connection, "/lib/4242").unwrap());
    assert!(!is_known_local_path(&connection, "/lib/4242/../4242/pack.zip").unwrap());
    assert!(!is_known_local_path(&connection, "/etc/passwd").unwrap());
    assert!(!is_known_local_path(&connection, "").unwrap());
    assert!(!is_known_local_path(&connection, "  ").unwrap());
}

#[test]
fn file_tab_labels_come_from_titles_and_registered_paths() {
    let connection = open_memory().unwrap();
    connection
            .execute_batch(
                "INSERT INTO products (booth_id, title, imported_at, updated_at) VALUES ('9000001', '架空の服', 't', 't');
                 INSERT INTO products (booth_id, title, is_adult, imported_at, updated_at) VALUES ('9000002', '架空の髪', 1, 't', 't');
                 INSERT INTO products (booth_id, title, imported_at, updated_at) VALUES ('9000003', '  ', 't', 't');
                 INSERT INTO product_files (booth_id, filename, local_path) VALUES ('9000001', 'pack.zip', '/lib/9000001/pack.zip');
                 INSERT INTO product_files (booth_id, filename) VALUES ('9000002', 'missing.zip');
                 INSERT INTO local_files (path, filename, linked_product_id, scanned_at) VALUES ('/lib/loose.zip', 'loose.zip', '9000002', 't');
                 INSERT INTO local_files (path, filename, scanned_at) VALUES ('/lib/unlinked.zip', 'unlinked.zip', 't');",
            )
            .unwrap();
    let ids: Vec<String> = ["9000001", "9000002", "9000003", "9999999"]
        .map(String::from)
        .to_vec();
    let labels = product_labels(&connection, &ids).unwrap();
    assert_eq!(labels.len(), 2, "blank titles and unknown IDs get no label");
    assert_eq!(labels["9000001"].title, "架空の服");
    assert!(!labels["9000001"].adult);
    assert!(labels["9000002"].adult);
    let mut linked = linked_file_products(&connection).unwrap();
    linked.sort();
    assert_eq!(
        linked,
        vec![
            ("/lib/9000001/pack.zip".to_string(), "9000001".to_string()),
            ("/lib/loose.zip".to_string(), "9000002".to_string()),
        ]
    );
}

#[test]
fn bookmarks_persist_edits_and_intentionally_empty_lists() {
    let connection = open_memory().unwrap();
    assert_eq!(browser_bookmarks(&connection).unwrap().len(), 2);
    save_browser_bookmark(
        &connection,
        None,
        "テストのサイト",
        "https://example.com/path?q=1",
    )
    .unwrap();
    ensure_ready(&connection).unwrap();
    assert!(
        browser_bookmarks(&connection).unwrap().iter().any(
            |item| item.title == "テストのサイト" && item.url == "https://example.com/path?q=1"
        )
    );
    save_browser_bookmark(
        &connection,
        Some("https://example.com/path?q=1"),
        "変更した名前",
        "https://example.com/other",
    )
    .unwrap();
    assert!(
        browser_bookmarks(&connection)
            .unwrap()
            .iter()
            .any(|item| item.title == "変更した名前")
    );
    let before = browser_bookmarks(&connection).unwrap();
    assert!(save_browser_bookmark(&connection, None, "不正", "javascript:alert(1)").is_err());
    assert_eq!(browser_bookmarks(&connection).unwrap(), before);
    for item in before {
        delete_browser_bookmark(&connection, &item.url).unwrap();
    }
    ensure_ready(&connection).unwrap();
    assert!(browser_bookmarks(&connection).unwrap().is_empty());
}

#[test]
fn version_one_migration_preserves_library_and_saved_model() {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(SCHEMA).unwrap();
    connection.execute_batch("PRAGMA user_version = 1;
                INSERT INTO settings VALUES ('library_folder', '/saved/library');
                INSERT INTO products (booth_id, title, imported_at, updated_at) VALUES ('9001', '既存商品', 't', 't');
                INSERT INTO local_files (path, filename, linked_product_id, link_source, scanned_at) VALUES ('/tmp/existing.zip', 'existing.zip', '9001', 'manual', 't');").unwrap();
    migrate(&connection).unwrap();
    migrate(&connection).unwrap();
    ensure_ready(&connection).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        SCHEMA_VERSION
    );
    let detail_at: Option<String> = connection
        .query_row(
            "SELECT booth_detail_at FROM products WHERE booth_id = '9001'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(detail_at, None);
    let category_id: Option<i64> = connection
        .query_row(
            "SELECT booth_category_id FROM products WHERE booth_id = '9001'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(category_id, None);
    let moves: i64 = connection
        .query_row("SELECT COUNT(*) FROM classification_moves", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(moves, 0);
    let unpublished: Option<i64> = connection
        .query_row(
            "SELECT unpublished FROM products WHERE booth_id = '9001'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(unpublished, None);
    assert_eq!(
        require_product(&connection, "9001")
            .unwrap()
            .genre
            .as_deref(),
        Some("other")
    );
    assert_eq!(
        settings(&connection).unwrap().library_folder,
        "/saved/library"
    );
    assert_eq!(managed_products(&connection).unwrap().len(), 1);
    assert!(!product_excluded(&connection, "9001").unwrap());
    assert_eq!(
        load_files(&connection).unwrap()[0].link_source.as_deref(),
        Some("manual")
    );
}

#[test]
fn exclusions_hide_products_from_views_and_jobs_and_restore_without_data_loss() {
    let mut connection = open_memory().unwrap();
    import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
    apply_extraction(
        &mut connection,
        "1001",
        &extraction(Category::Avatar, &["ライム"], false, 0.99),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "1004",
        &extraction(Category::Outfit, &["ライム"], false, 0.99),
        0.6,
    )
    .unwrap();
    let original = require_product(&connection, "1004").unwrap();
    let before = load_snapshot(&connection).unwrap();
    set_product_excluded(&mut connection, "1004", true).unwrap();
    set_product_excluded(&mut connection, "1004", true).unwrap();
    let excluded = load_snapshot(&connection).unwrap();
    assert_eq!(excluded.products.len(), before.products.len() - 1);
    assert!(excluded.links.iter().all(|link| link.product_id != "1004"));
    assert!(
        avatar_detail(&excluded, "manuka")
            .unwrap()
            .unsupported_outfits
            .iter()
            .all(|item| item.booth_id != "1004")
    );
    assert!(
        crate::view::product_options(&excluded)
            .iter()
            .all(|p| p.booth_id != "1004")
    );
    assert!(
        products_for_extract(&connection, ExtractTarget::All)
            .unwrap()
            .iter()
            .all(|p| p.booth_id != "1004")
    );
    assert!(
        products_for_extract(&connection, ExtractTarget::Unclassified)
            .unwrap()
            .iter()
            .all(|p| p.booth_id != "1004")
    );
    assert!(
        managed_products(&connection)
            .unwrap()
            .iter()
            .any(|p| p.booth_id == "1004" && p.excluded)
    );
    apply_extraction(
        &mut connection,
        "1004",
        &extraction(Category::Other, &[], true, 0.1),
        0.6,
    )
    .unwrap();
    reresolve_all(&mut connection).unwrap();
    assert_eq!(
        require_product(&connection, "1004").unwrap().category,
        original.category
    );
    set_product_excluded(&mut connection, "1004", false).unwrap();
    let restored = load_snapshot(&connection).unwrap();
    assert_eq!(restored.products.len(), before.products.len());
    assert!(
        avatar_detail(&restored, "manuka")
            .unwrap()
            .unsupported_outfits
            .iter()
            .any(|item| item.booth_id == "1004")
    );
    assert!(
        restored
            .links
            .iter()
            .any(|link| link.product_id == "1004" && link.avatar_id == "lime")
    );
    assert_eq!(
        require_product(&connection, "1004").unwrap().category,
        original.category
    );
    assert!(set_product_excluded(&mut connection, "9999", true).is_err());
}

/// The counts and the options are read with SQL; they must match the snapshot's versions,
/// including excluded products, an unknown category, unlinked candidates and recorded files.
#[test]
fn library_counts_and_options_read_with_sql_match_the_snapshot() {
    let mut connection = open_memory().unwrap();
    import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
    mark_owned_body(&mut connection, "1001").unwrap();
    apply_extraction(
        &mut connection,
        "1004",
        &extraction(Category::Outfit, &["ライム"], false, 0.99),
        0.6,
    )
    .unwrap();
    let same = |connection: &Connection| {
        let snapshot = load_snapshot(connection).unwrap();
        assert_eq!(
            serde_json::to_value(library_stats(connection).unwrap()).unwrap(),
            serde_json::to_value(crate::view::library_stats(&snapshot)).unwrap()
        );
        assert_eq!(
            serde_json::to_value(product_options(connection).unwrap()).unwrap(),
            serde_json::to_value(crate::view::product_options(&snapshot)).unwrap()
        );
    };
    same(&connection);
    let now = now_iso();
    for (path, is_dir, linked, candidate) in [
        ("/lib/a.zip", 0, Some("1004"), None),
        ("/lib/dir", 1, Some("1004"), None),
        ("/lib/b.zip", 0, Some("1002"), None),
        ("/lib/c.zip", 0, None, Some("1002")),
        ("/lib/d.zip", 0, None, Some("1003")),
        ("/lib/e.zip", 0, None, None),
    ] {
        connection
                .execute(
                    "INSERT INTO local_files (path, filename, is_dir, size, linked_product_id, candidate_product_id, match_score, scanned_at)
                     VALUES (?1, ?1, ?2, 1, ?3, ?4, 0.5, ?5)",
                    params![path, is_dir, linked, candidate, now],
                )
                .unwrap();
    }
    connection
            .execute(
                "INSERT INTO product_files (booth_id, variation, filename, downloaded, local_path) VALUES ('1003', '', 'x.zip', 1, '/lib/x.zip'), ('1003', '', 'y.zip', 0, '')",
                [],
            )
            .unwrap();
    connection
        .execute(
            "UPDATE products SET category = 'retired' WHERE booth_id = '1005'",
            [],
        )
        .unwrap();
    same(&connection);
    let before = library_stats(&connection).unwrap();
    assert!(before.owned_avatars >= 1);
    assert_eq!(before.unlinked_files, 3);
    assert_eq!(before.registered_files, 3);
    // An excluded product leaves the counts, its files and its body ownership with it.
    set_product_excluded(&mut connection, "1002", true).unwrap();
    set_product_excluded(&mut connection, "1001", true).unwrap();
    same(&connection);
    let after = library_stats(&connection).unwrap();
    assert_eq!(after.products, before.products - 2);
    assert_eq!(after.owned_avatars, before.owned_avatars - 1);
    assert_eq!(after.unlinked_files, 2);
    assert_eq!(after.registered_files, 2);
}

#[test]
fn excluded_body_stays_excluded_after_reimport_and_does_not_count_as_owned() {
    let mut connection = open_memory().unwrap();
    import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
    mark_owned_body(&mut connection, "1001").unwrap();
    assert!(
        load_snapshot(&connection)
            .unwrap()
            .avatars
            .iter()
            .any(|a| a.id == "lime" && a.owned)
    );
    set_product_excluded(&mut connection, "1001", true).unwrap();
    let updated = SAMPLE.replace("共通素体", "更新された説明");
    import_products(&mut connection, &parse_export(&updated).unwrap()).unwrap();
    ensure_ready(&connection).unwrap();
    assert!(product_excluded(&connection, "1001").unwrap());
    let snapshot = load_snapshot(&connection).unwrap();
    assert!(snapshot.products.iter().all(|p| p.booth_id != "1001"));
    assert!(snapshot.avatars.iter().any(|a| a.id == "lime" && !a.owned));
    assert_eq!(crate::view::library_stats(&snapshot).products, 5);
    set_product_excluded(&mut connection, "1001", false).unwrap();
    assert!(
        load_snapshot(&connection)
            .unwrap()
            .products
            .iter()
            .any(|p| p.booth_id == "1001")
    );
}

#[test]
fn rescanning_preserves_excluded_file_links_without_reintroducing_them() {
    let mut connection = open_memory().unwrap();
    import_products(
        &mut connection,
        &parse_export(&SAMPLE.replace("1004", "91004").replace("1005", "91005")).unwrap(),
    )
    .unwrap();
    let entry = |id: &str| crate::scan::ScanEntry {
        path: format!("/tmp/vrclibman-exclusions/{id}.zip"),
        filename: format!("{id}.zip"),
        is_dir: false,
        size: 10,
        labels: vec![format!("{id}.zip")],
    };
    let mut outcome = ScanOutcome {
        entries: vec![entry("91004"), entry("91005")],
        truncated: false,
    };
    apply_scan(&mut connection, "/tmp/vrclibman-exclusions", &outcome).unwrap();
    set_product_excluded(&mut connection, "91004", true).unwrap();
    outcome.entries.push(crate::scan::ScanEntry {
        path: "/tmp/vrclibman-exclusions/new-1004.zip".into(),
        ..entry("91004")
    });
    apply_scan(&mut connection, "/tmp/vrclibman-exclusions", &outcome).unwrap();
    assert_eq!(load_files(&connection).unwrap().len(), 2);
    assert_eq!(load_snapshot(&connection).unwrap().files.len(), 1);
    set_product_excluded(&mut connection, "91004", false).unwrap();
    let restored = load_snapshot(&connection).unwrap();
    assert_eq!(restored.files.len(), 2);
    assert!(
        restored
            .files
            .iter()
            .any(|f| f.linked_product_id.as_deref() == Some("91004"))
    );
}

#[test]
fn stale_scan_cannot_revert_the_folder_or_replace_newer_results() {
    let mut connection = open_memory().unwrap();
    let outcome = ScanOutcome {
        entries: vec![crate::scan::ScanEntry {
            path: "/tmp/vrclibman-new/item.zip".into(),
            filename: "item.zip".into(),
            is_dir: false,
            size: 10,
            labels: vec!["item".into()],
        }],
        truncated: false,
    };
    set_setting(&connection, "library_folder", "/tmp/vrclibman-new").unwrap();
    apply_library_scan(&mut connection, "/tmp/vrclibman-new", &outcome).unwrap();
    let stale = ScanOutcome {
        entries: vec![],
        truncated: false,
    };
    assert!(apply_library_scan(&mut connection, "/tmp/vrclibman-old", &stale).is_err());
    assert_eq!(
        settings(&connection).unwrap().library_folder,
        "/tmp/vrclibman-new"
    );
    assert_eq!(load_files(&connection).unwrap().len(), 1);
}

#[test]
fn truncated_scans_replace_only_visited_automatic_rows() {
    let mut connection = open_memory().unwrap();
    connection
        .execute_batch(
            "INSERT INTO products (booth_id, title, imported_at, updated_at) VALUES
             ('4242', 'Synthetic package', 't', 't'), ('4243', 'Excluded package', 't', 't');",
        )
        .unwrap();
    set_product_excluded(&mut connection, "4243", true).unwrap();
    let root = "/dummy-library";
    for (name, product, source) in [
        ("seen-4242.zip", "4242", "auto"),
        ("unseen-4242.zip", "4242", "auto"),
        ("manual-4242.zip", "4242", "manual"),
        ("unseen-manual-4242.zip", "4242", "manual"),
        ("unseen-4243.zip", "4243", "auto"),
    ] {
        connection.execute(
                "INSERT INTO local_files (path, filename, linked_product_id, link_source, size, scanned_at)
                 VALUES (?1, ?2, ?3, ?4, 5, 't')",
                params![format!("{root}/{name}"), name, product, source],
            ).unwrap();
    }
    let entry = |name: &str| crate::scan::ScanEntry {
        path: format!("{root}/{name}"),
        filename: name.into(),
        is_dir: false,
        size: 99,
        labels: vec![name.into()],
    };
    let report = apply_scan(
        &mut connection,
        root,
        &ScanOutcome {
            entries: vec![entry("seen-4242.zip"), entry("manual-4242.zip")],
            truncated: true,
        },
    )
    .unwrap();

    assert!(report.truncated);
    let files = load_files(&connection).unwrap();
    assert_eq!(files.len(), 5);
    assert_eq!(
        files
            .iter()
            .find(|f| f.filename == "seen-4242.zip")
            .unwrap()
            .size,
        99
    );
    let manual = files
        .iter()
        .find(|f| f.filename == "manual-4242.zip")
        .unwrap();
    assert_eq!(manual.size, 99);
    assert_eq!(manual.link_source.as_deref(), Some("manual"));
    for name in [
        "unseen-4242.zip",
        "unseen-manual-4242.zip",
        "unseen-4243.zip",
    ] {
        assert_eq!(files.iter().find(|f| f.filename == name).unwrap().size, 5);
    }
}

#[test]
fn a_complete_scan_still_removes_unseen_automatic_rows() {
    let mut connection = open_memory().unwrap();
    connection
        .execute_batch(
            "INSERT INTO local_files (path, filename, link_source, scanned_at)
             VALUES ('/dummy-library/unseen.zip', 'unseen.zip', 'auto', 't');",
        )
        .unwrap();
    apply_scan(
        &mut connection,
        "/dummy-library",
        &ScanOutcome {
            entries: vec![],
            truncated: false,
        },
    )
    .unwrap();
    assert!(load_files(&connection).unwrap().is_empty());
}

#[test]
fn excluded_products_cannot_start_or_route_downloads() {
    let mut connection = open_memory().unwrap();
    import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
    set_product_excluded(&mut connection, "1004", true).unwrap();
    set_setting(&connection, "route_downloads", "1").unwrap();
    set_setting(
        &connection,
        "library_folder",
        "/tmp/vrclibman-excluded-downloads",
    )
    .unwrap();
    let notice: DownloadFinished = serde_json::from_value(serde_json::json!({
        "boothId": "1004", "path": "/tmp/no-file-should-be-touched.zip"
    }))
    .unwrap();
    assert!(
        prepare_download(&mut connection, &notice)
            .unwrap_err()
            .contains("除外")
    );
    assert!(
        plan_browser_download(&connection, "1004", "", "body.zip")
            .unwrap_err()
            .contains("除外")
    );
}

#[test]
fn retired_design_setting_does_not_affect_settings_updates() {
    let connection = open_memory().unwrap();
    set_setting(&connection, "appearance", "liquid").unwrap();
    ensure_ready(&connection).unwrap();
    let saved = update_settings(
        &connection,
        &SettingsPatch {
            confidence_threshold: Some(0.75),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(saved.confidence_threshold, 0.75);
    assert!(
        serde_json::to_value(saved)
            .unwrap()
            .get("appearance")
            .is_none()
    );
}

#[test]
fn settings_patches_merge_current_values_and_validate_before_writing() {
    let connection = open_memory().unwrap();
    update_settings(
        &connection,
        &SettingsPatch {
            avatar_hero: Some("small".into()),
            ..Default::default()
        },
    )
    .unwrap();
    set_setting(&connection, "library_folder", "/unavailable/saved/library").unwrap();
    set_setting(&connection, "builtin_browser", "1").unwrap();
    let saved = update_settings(
        &connection,
        &SettingsPatch {
            confidence_threshold: Some(0.75),
            background_mode: Some(true),
            launch_at_login: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(saved.avatar_hero, "small");
    assert_eq!(saved.confidence_threshold, 0.75);
    assert!(saved.builtin_browser);
    assert_eq!(
        saved.background_mode,
        crate::presence::background_supported()
    );
    assert_eq!(
        saved.launch_at_login,
        crate::presence::background_supported()
    );
    let invalid = SettingsPatch {
        library_folder: Some(String::new()),
        route_downloads: Some(true),
        confidence_threshold: Some(0.1),
        ..Default::default()
    };
    assert!(update_settings(&connection, &invalid).is_err());
    assert_eq!(settings(&connection).unwrap().confidence_threshold, 0.75);
    assert!(
        update_settings(
            &connection,
            &SettingsPatch {
                library_folder: Some("relative".into()),
                ..Default::default()
            }
        )
        .is_err()
    );
    let saved = update_settings(
        &connection,
        &SettingsPatch {
            background_mode: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!saved.background_mode && !saved.launch_at_login);
}

#[test]
fn booth_link_target_defaults_to_the_system_browser_and_roundtrips() {
    let connection = open_memory().unwrap();
    assert_eq!(settings(&connection).unwrap().booth_link_target, "external");
    let saved = update_settings(
        &connection,
        &SettingsPatch {
            booth_link_target: Some("builtin".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(saved.booth_link_target, "builtin");
    assert!(!saved.builtin_browser);
    ensure_ready(&connection).unwrap();
    assert_eq!(settings(&connection).unwrap().booth_link_target, "builtin");
    connection
        .execute("DELETE FROM settings WHERE key = 'booth_link_target'", [])
        .unwrap();
    assert_eq!(settings(&connection).unwrap().booth_link_target, "external");
    ensure_ready(&connection).unwrap();
    assert_eq!(
        setting(&connection, "booth_link_target")
            .unwrap()
            .as_deref(),
        Some("external")
    );
    let saved = update_settings(
        &connection,
        &SettingsPatch {
            booth_link_target: Some("  somewhere  ".into()),
            builtin_browser: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(saved.booth_link_target, "external");
    assert!(saved.builtin_browser);
    let saved = update_settings(
        &connection,
        &SettingsPatch {
            booth_link_target: Some(" builtin ".into()),
            builtin_browser: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(saved.booth_link_target, "builtin");
    assert!(!saved.builtin_browser);
}

#[test]
fn failed_settings_write_rolls_back_the_entire_patch() {
    let connection = open_memory().unwrap();
    connection.execute_batch("CREATE TRIGGER fail_settings BEFORE UPDATE ON settings WHEN NEW.key = 'avatar_hero' BEGIN SELECT RAISE(ABORT, 'fixture write failure'); END;").unwrap();
    let patch = SettingsPatch {
        avatar_hero: Some("small".into()),
        confidence_threshold: Some(0.75),
        ..Default::default()
    };
    assert!(update_settings(&connection, &patch).is_err());
    let saved = settings(&connection).unwrap();
    assert_eq!(saved.avatar_hero, crate::model::DEFAULT_AVATAR_HERO);
    assert_eq!(saved.confidence_threshold, 0.6);
}

#[test]
fn products_by_category_returns_only_requested_category() {
    let connection = open_memory().unwrap();
    for (booth_id, title, category) in [
        ("910001", "二番目のワールド", "world"),
        ("910002", "衣装", "outfit"),
        ("910003", "一番目のワールド", "world"),
    ] {
        connection
            .execute(
                "INSERT INTO products
                     (booth_id, title, category, imported_at, updated_at)
                     VALUES (?1, ?2, ?3, '2026-01-01', '2026-01-01')",
                params![booth_id, title, category],
            )
            .unwrap();
    }

    assert_eq!(
        product_ids_by_category(&connection, Category::World).unwrap(),
        vec!["910003".to_string(), "910001".to_string()]
    );
}

#[test]
fn path_compare_folds_case_only_on_windows() {
    assert_eq!(
        normalize_path_key_os("/Users/A/VRCLibMan/File.ZIP", false),
        "/Users/A/VRCLibMan/File.ZIP"
    );
    assert!(is_under_os(
        "/Users/A/VRCLibMan/File.ZIP",
        "/Users/A/VRCLibMan",
        false
    ));
    assert!(!is_under_os(
        "/Users/A/VRCLibMan/File.ZIP",
        "/users/a/vrclibman",
        false
    ));
    assert!(!is_under_os(
        "/Users/A/Library/a.txt",
        "/Users/A/Lib",
        false
    ));
    assert_eq!(
        normalize_path_key_os("C:/Users/A/VRCLibMan/File.ZIP", true),
        "c:\\users\\a\\vrclibman\\file.zip"
    );
    assert!(is_under_os(
        "C:/Users/A/Lib/a.txt",
        "c:\\users\\a\\lib",
        true
    ));
    assert!(!is_under_os(
        "C:/Users/A/Library/a.txt",
        "C:/Users/A/Lib",
        true
    ));
}

const SAMPLE: &str = r#"{
        "products": [
            {"boothId":"1001","title":"ライム -Lime- / オリジナル3Dモデル","shop":"あまとうさぎ","description":"共通素体"},
            {"boothId":"1002","title":"プラム -Plum- / オリジナル3Dモデル","shop":"あまとうさぎ","description":"共通素体"},
            {"boothId":"1003","title":"オリジナル3Dモデル「マヌカ」ver1.02","shop":"STUDIO JINGO","description":"マヌカ"},
            {"boothId":"1004","title":"ライム対応ワンピース","shop":"sample","description":"ライム対応"},
            {"boothId":"1005","title":"汎用ペンライト","shop":"sample","description":"汎用ギミック"},
            {"boothId":"1006","title":"しなの専用ヘア","shop":"sample","description":"しなの専用"}
        ]
    }"#;

fn extraction(category: Category, names: &[&str], generic: bool, confidence: f64) -> Extraction {
    Extraction {
        category,
        genre: None,
        compatible_avatars: names.iter().map(|name| (*name).to_string()).collect(),
        is_generic: generic,
        confidence,
    }
}

#[test]
fn shared_body_outfit_does_not_mark_siblings_owned() {
    let mut connection = open_memory().unwrap();
    import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
    apply_extraction(
        &mut connection,
        "1001",
        &extraction(
            Category::Avatar,
            &["ライム", "プラム", "シフォン", "ショコラ"],
            false,
            0.93,
        ),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "1002",
        &extraction(Category::Avatar, &["プラム"], false, 0.9),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "1003",
        &extraction(Category::Avatar, &["マヌカ"], false, 0.9),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "1004",
        &extraction(Category::Outfit, &["ライム"], false, 0.88),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "1005",
        &extraction(Category::Gimmick, &[], true, 0.84),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "1006",
        &extraction(Category::Hair, &["しなの"], false, 0.86),
        0.6,
    )
    .unwrap();

    // This compatibility fixture owns all editions; metadata alone is not ownership.
    connection.execute_batch("INSERT OR REPLACE INTO purchased_editions SELECT booth_id, '{\"mode\":\"all\",\"avatarIds\":[]}' FROM products").unwrap();
    let snapshot = load_snapshot(&mut connection).unwrap();
    let lime_links: Vec<_> = snapshot
        .links
        .iter()
        .filter(|link| link.product_id == "1001")
        .map(|link| link.avatar_id.as_str())
        .collect();
    assert_eq!(lime_links, vec!["lime"]);

    let cards = avatar_cards(&snapshot);
    assert!(cards.iter().any(|card| card.id == "lime" && !card.owned));
    assert!(!cards.iter().any(|card| card.id == "plum"));
    assert!(cards.iter().any(|card| card.id == "shinano" && !card.owned));

    let plum = avatar_detail(&snapshot, "plum").unwrap();
    assert!(
        plum.sections
            .iter()
            .flat_map(|section| &section.items)
            .any(|item| { item.booth_id == "1004" && item.via_base_body })
    );
    let manuka = avatar_detail(&snapshot, "manuka").unwrap();
    assert!(
        !manuka
            .sections
            .iter()
            .flat_map(|section| &section.items)
            .any(|item| item.booth_id == "1004")
    );
    assert!(
        manuka
            .generic_items
            .iter()
            .any(|item| item.booth_id == "1005")
    );
}

#[test]
fn manual_link_survives_reresolve_and_description_change_clears_extraction() {
    let mut connection = open_memory().unwrap();
    import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
    apply_extraction(
        &mut connection,
        "1004",
        &extraction(Category::Outfit, &["ライム"], false, 0.9),
        0.6,
    )
    .unwrap();
    update_product(
        &mut connection,
        &ProductUpdate {
            purchased_editions: None,
            booth_id: "1004".into(),
            category: Some("outfit".into()),
            genre: None,
            reset_genre: false,
            is_generic: Some(false),
            avatar_ids: Some(vec!["manuka".into()]),
        },
    )
    .unwrap();
    reresolve_all(&mut connection).unwrap();
    let snapshot = load_snapshot(&mut connection).unwrap();
    let links: Vec<_> = snapshot
        .links
        .iter()
        .filter(|link| link.product_id == "1004")
        .map(|link| link.avatar_id.as_str())
        .collect();
    assert_eq!(links, vec!["manuka"]);

    let changed = parse_export(
        r#"[{"boothId":"1004","title":"ライム対応ワンピース","description":"説明を更新した"}]"#,
    )
    .unwrap();
    import_products(&mut connection, &changed).unwrap();
    let snapshot = load_snapshot(&mut connection).unwrap();
    let product = snapshot
        .products
        .iter()
        .find(|product| product.booth_id == "1004")
        .unwrap();
    assert!(product.category.is_none() || product.category_locked);
    assert!(product.extracted_at.is_none());
    assert!(product.description_snippet.contains("更新"));
}

#[test]
fn manual_genre_lock_survives_extraction_and_genre_query_excludes_products() {
    let mut connection = open_memory().unwrap();
    import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
    update_product(
        &mut connection,
        &ProductUpdate {
            purchased_editions: None,
            booth_id: "1004".into(),
            category: Some("outfit".into()),
            genre: Some("shader".into()),
            reset_genre: false,
            is_generic: None,
            avatar_ids: None,
        },
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "1004",
        &Extraction {
            category: Category::Outfit,
            genre: Some("outfit".into()),
            compatible_avatars: vec![],
            is_generic: false,
            confidence: 0.9,
        },
        0.6,
    )
    .unwrap();
    let product = require_product(&connection, "1004").unwrap();
    assert_eq!(product.genre.as_deref(), Some("shader"));
    assert!(product.genre_locked);
    assert!(
        product_ids_by_genre(&connection, "asset", Some("shader"))
            .unwrap()
            .contains(&"1004".into())
    );
    set_product_excluded(&mut connection, "1004", true).unwrap();
    assert!(
        !product_ids_by_genre(&connection, "asset", Some("shader"))
            .unwrap()
            .contains(&"1004".into())
    );
}

#[test]
fn automatic_genre_is_separate_from_unchanged_and_fixed() {
    let mut connection = open_memory().unwrap();
    import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
    let update = |genre: Option<&str>, category: Option<&str>, reset_genre: bool| ProductUpdate {
        purchased_editions: None,
        booth_id: "1004".into(),
        category: category.map(str::to_string),
        genre: genre.map(str::to_string),
        reset_genre,
        is_generic: None,
        avatar_ids: None,
    };
    update_product(
        &mut connection,
        &update(Some("shader"), Some("outfit"), false),
    )
    .unwrap();
    // Nothing about the genre sent: the fixed genre stays fixed.
    update_product(&mut connection, &update(None, None, false)).unwrap();
    let product = require_product(&connection, "1004").unwrap();
    assert_eq!(
        (product.genre.as_deref(), product.genre_locked),
        (Some("shader"), true)
    );

    // 「自動」 unlocks the genre and chooses it again from the category and text.
    update_product(&mut connection, &update(None, None, true)).unwrap();
    let product = require_product(&connection, "1004").unwrap();
    assert!(!product.genre_locked);
    let auto = product.genre.clone().unwrap();
    assert_ne!(auto, "shader");
    assert!(
        crate::genre::ASSET_GENRES
            .iter()
            .any(|(key, _)| *key == auto)
    );
    assert!(product.category_locked, "the category stays fixed");
    assert!(
        !classification_pinned(&product),
        "reclassification may change the genre again"
    );

    // A category change with 「自動」 picks a genre of the new category, not a leftover.
    update_product(&mut connection, &update(None, Some("world"), true)).unwrap();
    let product = require_product(&connection, "1004").unwrap();
    assert_eq!(product.category, Some(Category::World));
    assert!(!product.genre_locked);
    assert!(
        crate::genre::WORLD_GENRES
            .iter()
            .any(|(key, _)| Some(*key) == product.genre.as_deref())
    );

    assert!(update_product(&mut connection, &update(Some("skybox"), None, true)).is_err());
}

#[test]
fn world_genre_query_lists_missing_or_foreign_genres_as_unset() {
    let mut connection = open_memory().unwrap();
    import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
    update_product(
        &mut connection,
        &ProductUpdate {
            purchased_editions: None,
            booth_id: "1004".into(),
            category: Some("world".into()),
            genre: Some("skybox".into()),
            reset_genre: false,
            is_generic: None,
            avatar_ids: None,
        },
    )
    .unwrap();
    let has = |ids: Vec<String>| ids.contains(&"1004".to_string());
    assert!(has(product_ids_by_genre(
        &connection,
        "world",
        Some("skybox")
    )
    .unwrap()));
    assert!(!has(product_ids_by_genre(
        &connection,
        "world",
        Some(crate::genre::UNSET)
    )
    .unwrap()));
    connection
        .execute(
            "UPDATE products SET genre = 'hair' WHERE booth_id = '1004'",
            [],
        )
        .unwrap();
    assert!(has(product_ids_by_genre(
        &connection,
        "world",
        Some(crate::genre::UNSET)
    )
    .unwrap()));
    assert!(has(
        product_ids_by_genre(&connection, "world", None).unwrap()
    ));
    assert!(!has(product_ids_by_genre(
        &connection,
        "asset",
        Some(crate::genre::UNSET)
    )
    .unwrap()));
    connection
        .execute(
            "UPDATE products SET genre = NULL WHERE booth_id = '1004'",
            [],
        )
        .unwrap();
    assert!(has(product_ids_by_genre(
        &connection,
        "world",
        Some(crate::genre::UNSET)
    )
    .unwrap()));
    assert!(!has(product_ids_by_genre(
        &connection,
        "world",
        Some("skybox")
    )
    .unwrap()));
}

#[test]
fn booth_category_settles_without_a_model_and_locks_and_adult_rows_stay() {
    let mut connection = open_memory().unwrap();
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
    let json = r#"{
            "products": [
                {
                    "boothId": "424242",
                    "title": "架空の道具",
                    "description": "説明だけです",
                    "category": {"id": 215, "name": "3Dツール・システム", "parent": {"name": "3Dモデル"}},
                    "tags": ["VRChat", "Unity"]
                },
                {
                    "boothId": "424243",
                    "title": "架空の小道具",
                    "description": "タグが足りません",
                    "category": {"id": 210, "name": "3D小道具", "parent": {"name": "3Dモデル"}},
                    "tags": ["VRChat", "3Dモデル"]
                },
                {
                    "boothId": "424244",
                    "title": "架空の制限商品",
                    "description": "保存しない説明",
                    "isAdult": true,
                    "category": {"id": 209, "name": "3D衣装", "parent": {"name": "3Dモデル"}},
                    "tags": ["衣装"]
                },
                {
                    "boothId": "424245",
                    "title": "架空の衣装",
                    "description": "手で移します",
                    "category": {"id": 209, "name": "3D衣装", "parent": {"name": "3Dモデル"}},
                    "tags": ["VRChat"]
                },
                {
                    "boothId": "424246",
                    "title": "消えた見本",
                    "description": ""
                },
                {
                    "boothId": "424247",
                    "title": "ロックした道具",
                    "description": "手動のカテゴリです",
                    "category": {"id": 215, "name": "3Dツール・システム", "parent": {"name": "3Dモデル"}},
                    "tags": ["ダミー"]
                }
            ]
        }"#;
    import_export_json(&mut connection, json).unwrap();
    let category_of = |connection: &Connection, id: &str| {
        connection
                .query_row(
                    "SELECT booth_category_id, IFNULL(booth_category_name, ''), IFNULL(booth_tags, ''), description_snippet,
                            CASE WHEN booth_detail_at IS NULL THEN 0 ELSE 1 END
                     FROM products WHERE booth_id = ?1",
                    params![id],
                    |row| {
                        Ok((
                            row.get::<_, Option<i64>>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                        ))
                    },
                )
                .unwrap()
    };
    let tool = category_of(&connection, "424242");
    assert_eq!(tool.0, Some(215));
    assert_eq!(tool.1, "3Dツール・システム");
    assert!(tool.2.contains("VRChat"));
    assert_eq!(tool.4, 1);
    import_export_json(
        &mut connection,
        r#"{"products":[{"boothId":"424242","title":"架空の道具","description":"説明だけです"}]}"#,
    )
    .unwrap();
    let kept = category_of(&connection, "424242");
    assert_eq!(kept.0, Some(215));
    assert!(kept.2.contains("Unity"));
    let adult = category_of(&connection, "424244");
    assert_eq!(adult.0, None);
    assert_eq!(adult.3, "");
    assert!(!adult.2.contains("衣装"));
    let adult_flag: Option<i64> = connection
        .query_row(
            "SELECT is_adult FROM products WHERE booth_id = '424244'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(adult_flag, Some(1));
    assert!(apply_booth_settlement(&mut connection, "424242").unwrap());
    let settled = product_by_id(&connection, "424242").unwrap().unwrap();
    assert_eq!(settled.category, Some(Category::Other));
    assert_eq!(settled.genre.as_deref(), Some("tool_editor"));
    assert!(!apply_booth_settlement(&mut connection, "424243").unwrap());
    assert!(
        product_by_id(&connection, "424243")
            .unwrap()
            .unwrap()
            .category
            .is_none()
    );
    move_product_genre(&mut connection, "424245", "asset", "hair").unwrap();
    assert!(!apply_booth_settlement(&mut connection, "424245").unwrap());
    let locked = product_by_id(&connection, "424245").unwrap().unwrap();
    assert_eq!(locked.category, Some(Category::Hair));
    assert_eq!(locked.genre.as_deref(), Some("hair"));
    assert!(locked.category_locked && locked.genre_locked);
    connection
            .execute(
                "UPDATE products SET category = 'avatar', category_locked = 1, genre_locked = 0, extracted_at = NULL WHERE booth_id = '424247'",
                [],
            )
            .unwrap();
    assert!(!apply_booth_settlement(&mut connection, "424247").unwrap());
    let disagreed = product_by_id(&connection, "424247").unwrap().unwrap();
    assert_eq!(disagreed.category, Some(Category::Avatar));
    let pending = products_needing_booth_detail(&connection).unwrap();
    assert_eq!(pending, vec!["424246".to_string()]);
    assert!(!pending.iter().any(|id| id == "424244"));
    let stop = store_booth_detail(
        &mut connection,
        "424246",
        &crate::booth_detail::DetailOutcome::Missing,
    )
    .unwrap();
    assert!(!stop);
    let missing = product_by_id(&connection, "424246").unwrap().unwrap();
    assert_eq!(
        missing.description_snippet,
        crate::model::UNAVAILABLE_DESCRIPTION
    );
    assert!(
        products_needing_booth_detail(&connection)
            .unwrap()
            .is_empty()
    );
    let again = store_booth_detail(
        &mut connection,
        "424243",
        &crate::booth_detail::DetailOutcome::RetryLater,
    )
    .unwrap();
    assert!(again);
}

#[test]
fn booth_settlement_keeps_its_label_when_the_title_has_other_phrases() {
    let mut connection = open_memory().unwrap();
    import_export_json(
        &mut connection,
        r#"{
                "products": [{
                    "boothId": "7702101",
                    "title": "サンプルこたつセット",
                    "description": "家具の説明",
                    "category": {"id": 209, "name": "3D衣装", "parent": {"name": "3Dモデル"}},
                    "tags": ["VRChat"]
                }]
            }"#,
    )
    .unwrap();
    assert!(apply_booth_settlement(&mut connection, "7702101").unwrap());
    let settled = require_product(&connection, "7702101").unwrap();
    assert_eq!(settled.category, Some(Category::Outfit));
    assert_eq!(settled.genre.as_deref(), Some("outfit"));
    assert!(!settled.category_locked && !settled.genre_locked);

    move_product_genre(&mut connection, "7702101", "asset", "gimmick").unwrap();
    assert!(!apply_booth_settlement(&mut connection, "7702101").unwrap());
    let pinned = require_product(&connection, "7702101").unwrap();
    assert_eq!(pinned.category, Some(Category::Gimmick));
    assert_eq!(pinned.genre.as_deref(), Some("gimmick"));
}

#[test]
fn coarse_tool_text_overrides_software_and_tool_categories_only() {
    let mut connection = open_memory().unwrap();
    import_export_json(
            &mut connection,
            r#"{
                "products": [
                    {
                        "boothId": "7702301",
                        "title": "架空の共有ボード",
                        "description": "prefabをワールドに配置する。",
                        "category": {"id": 215, "name": "3Dツール・システム", "parent": {"name": "3Dモデル"}},
                        "tags": ["VRChat", "シェーダー"]
                    },
                    {
                        "boothId": "7702302",
                        "title": "架空 CPU Affinity",
                        "description": "PC用の割り当てアプリです。",
                        "category": {"id": 201, "name": "ソフトウェア"},
                        "tags": ["VRChat"]
                    },
                    {
                        "boothId": "7702303",
                        "title": "架空の撫で音ギミック",
                        "description": "影シェーダーを同梱しています。お試しワールドがあります。",
                        "category": {"id": 215, "name": "3Dツール・システム"},
                        "tags": ["シェーダー", "ギミック"]
                    },
                    {
                        "boothId": "7702304",
                        "title": "架空の追加シェイプキー",
                        "description": "シェイプキーを配布するデータです。",
                        "category": {"id": 215, "name": "3Dツール・システム"},
                        "tags": ["シェイプキー"]
                    },
                    {
                        "boothId": "7702305",
                        "title": "架空Udonスイッチ",
                        "description": "衣装です。",
                        "category": {"id": 209, "name": "3D衣装"},
                        "tags": ["VRChat"]
                    },
                    {
                        "boothId": "7702306",
                        "title": "架空の設置ギミック",
                        "description": "ワールドに配置するギミックです。",
                        "category": {"id": 215, "name": "3Dツール・システム"},
                        "tags": ["Udon"]
                    }
                ]
            }"#,
        )
        .unwrap();
    let expect = |connection: &Connection, id: &str, category: Category, genre: &str| {
        let product = require_product(connection, id).unwrap();
        assert_eq!(product.category, Some(category), "{id}");
        assert_eq!(product.genre.as_deref(), Some(genre), "{id}");
    };
    assert!(apply_booth_settlement(&mut connection, "7702301").unwrap());
    expect(&connection, "7702301", Category::World, "world_gimmick");
    assert!(apply_booth_settlement(&mut connection, "7702302").unwrap());
    expect(&connection, "7702302", Category::World, "tool_other");
    assert!(apply_booth_settlement(&mut connection, "7702303").unwrap());
    expect(&connection, "7702303", Category::Gimmick, "gimmick");
    assert!(apply_booth_settlement(&mut connection, "7702304").unwrap());
    expect(&connection, "7702304", Category::Other, "tool_editor");
    assert!(apply_booth_settlement(&mut connection, "7702305").unwrap());
    expect(&connection, "7702305", Category::Outfit, "outfit");

    move_product_genre(&mut connection, "7702306", "asset", "tool_editor").unwrap();
    assert!(!apply_booth_settlement(&mut connection, "7702306").unwrap());
    let pinned = require_product(&connection, "7702306").unwrap();
    assert_eq!(pinned.category, Some(Category::Other));
    assert_eq!(pinned.genre.as_deref(), Some("tool_editor"));
    assert!(pinned.category_locked && pinned.genre_locked);
}

fn classifier_version_of(connection: &Connection, booth_id: &str) -> i64 {
    connection
        .query_row(
            "SELECT classifier_version FROM products WHERE booth_id = ?1",
            params![booth_id],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn settled_booth_categories_keep_a_fitting_genre_and_ignore_title_phrases() {
    let mut connection = open_memory().unwrap();
    import_export_json(
            &mut connection,
            r#"{
                "products": [
                    {
                        "boothId": "7702201",
                        "title": "オリジナル3Dモデル「架空子」",
                        "description": "髪と瞳の説明",
                        "category": {"id": 209, "name": "3D衣装", "parent": {"name": "3Dモデル"}},
                        "tags": ["髪", "瞳", "アクセサリ"]
                    },
                    {
                        "boothId": "7702202",
                        "title": "架空Udonスイッチ",
                        "description": "ワールド用のスイッチです",
                        "category": {"id": 209, "name": "3D衣装", "parent": {"name": "3Dモデル"}},
                        "tags": ["VRChat"]
                    },
                    {
                        "boothId": "7702203",
                        "title": "架空の靴",
                        "description": "靴です",
                        "category": {"id": 209, "name": "3D衣装", "parent": {"name": "3Dモデル"}},
                        "tags": ["靴"]
                    },
                    {
                        "boothId": "7702204",
                        "title": "架空の道具",
                        "description": "ツールです",
                        "category": {"id": 215, "name": "3Dツール・システム", "parent": {"name": "3Dモデル"}},
                        "tags": ["衣装", "瞳", "ギミック", "表情"]
                    },
                    {
                        "boothId": "7702205",
                        "title": "オリジナル3Dモデル「架空子」",
                        "description": "ソフトウェアです",
                        "category": {"id": 201, "name": "ソフトウェア", "parent": {"name": "3Dモデル"}},
                        "tags": ["衣装"]
                    },
                    {
                        "boothId": "7702206",
                        "title": "架空の瞳",
                        "description": "瞳テクスチャです",
                        "category": {"id": 214, "name": "3Dテクスチャ", "parent": {"name": "3Dモデル"}},
                        "tags": ["瞳"]
                    },
                    {
                        "boothId": "7702207",
                        "title": "架空の布地",
                        "description": "テクスチャ枠の衣装タグ",
                        "category": {"id": 214, "name": "3Dテクスチャ", "parent": {"name": "3Dモデル"}},
                        "tags": ["衣装"]
                    },
                    {
                        "boothId": "7702208",
                        "title": "架空の家具",
                        "description": "小道具です",
                        "category": {"id": 210, "name": "3D小道具", "parent": {"name": "3Dモデル"}},
                        "tags": ["家具"]
                    },
                    {
                        "boothId": "7702209",
                        "title": "架空の服",
                        "description": "その他枠の衣装タグ",
                        "category": {"id": 127, "name": "3Dモデル（その他）", "parent": {"name": "3Dモデル"}},
                        "tags": ["衣装"]
                    },
                    {
                        "boothId": "7702210",
                        "title": "未確認その1",
                        "description": "カテゴリが不明です",
                        "category": {"id": 157, "name": "未確認の分類"},
                        "tags": ["VRChat"]
                    },
                    {
                        "boothId": "7702211",
                        "title": "未確認その2",
                        "description": "カテゴリが不明です",
                        "category": {"id": 142, "name": "未確認の分類"},
                        "tags": []
                    },
                    {
                        "boothId": "7702212",
                        "title": "未確認その3",
                        "description": "カテゴリが不明です",
                        "category": {"id": 6, "name": "未確認の分類"},
                        "tags": []
                    },
                    {
                        "boothId": "7702213",
                        "title": "オリジナル3Dモデル「架空子」",
                        "description": "カテゴリが無い本体候補"
                    },
                    {
                        "boothId": "7702214",
                        "title": "架空Udonスイッチ",
                        "description": "カテゴリが無いワールド候補"
                    }
                ]
            }"#,
        )
        .unwrap();
    let expect = |connection: &Connection, id: &str, category: Category, genre: &str| {
        let product = require_product(connection, id).unwrap();
        assert_eq!(product.category, Some(category), "{id}");
        assert_eq!(product.genre.as_deref(), Some(genre), "{id}");
    };
    assert!(apply_booth_settlement(&mut connection, "7702201").unwrap());
    expect(&connection, "7702201", Category::Outfit, "outfit");
    assert!(apply_booth_settlement(&mut connection, "7702202").unwrap());
    expect(&connection, "7702202", Category::Outfit, "outfit");
    assert!(apply_booth_settlement(&mut connection, "7702203").unwrap());
    expect(&connection, "7702203", Category::Outfit, "shoes_socks");
    assert!(apply_booth_settlement(&mut connection, "7702204").unwrap());
    expect(&connection, "7702204", Category::Other, "tool_editor");
    assert!(apply_booth_settlement(&mut connection, "7702205").unwrap());
    expect(&connection, "7702205", Category::Other, "tool_editor");
    assert!(apply_booth_settlement(&mut connection, "7702206").unwrap());
    expect(&connection, "7702206", Category::Texture, "eye_texture");
    assert!(!apply_booth_settlement(&mut connection, "7702207").unwrap());
    assert!(
        require_product(&connection, "7702207")
            .unwrap()
            .category
            .is_none()
    );
    assert!(apply_booth_settlement(&mut connection, "7702208").unwrap());
    expect(&connection, "7702208", Category::World, "furniture_props");
    assert!(apply_booth_settlement(&mut connection, "7702209").unwrap());
    expect(&connection, "7702209", Category::Outfit, "outfit");
    for id in ["7702210", "7702211", "7702212"] {
        assert!(
            !apply_booth_settlement(&mut connection, id).unwrap(),
            "{id}"
        );
        assert!(
            require_product(&connection, id).unwrap().category.is_none(),
            "{id}"
        );
    }
    move_product_genre(&mut connection, "7702201", "asset", "gimmick").unwrap();
    assert!(!apply_booth_settlement(&mut connection, "7702201").unwrap());
    expect(&connection, "7702201", Category::Gimmick, "gimmick");

    assert!(!apply_booth_settlement(&mut connection, "7702213").unwrap());
    apply_extraction(
        &mut connection,
        "7702213",
        &extraction(Category::Other, &[], false, 0.9),
        0.6,
    )
    .unwrap();
    assert_eq!(
        require_product(&connection, "7702213").unwrap().category,
        Some(Category::Avatar)
    );
    apply_extraction(
        &mut connection,
        "7702214",
        &extraction(Category::Gimmick, &[], false, 0.9),
        0.6,
    )
    .unwrap();
    assert_eq!(
        require_product(&connection, "7702214").unwrap().category,
        Some(Category::World)
    );
}

#[test]
fn catalog_and_owned_bodies_are_current_and_other_rows_stay_old() {
    let mut connection = open_memory().unwrap();
    connection
            .execute(
                "INSERT INTO avatars (id, display_name, base_body, aliases_json, is_builtin, owned, notes)
                 VALUES ('fictional-body', '架空丸', NULL, '[]', 0, 0, '')",
                [],
            )
            .unwrap();
    connection
        .execute(
            "INSERT INTO avatar_sources (booth_id, avatar_id) VALUES ('8800881', 'fictional-body')",
            [],
        )
        .unwrap();
    import_products(
            &mut connection,
            &parse_export(
                r#"{
                    "products": [
                        {"boothId":"8800881","title":"架空丸","description":"本体です。"},
                        {"boothId":"8800882","title":"オリジナル3Dモデル「架空子」","description":"架空子の説明です。"},
                        {"boothId":"8800883","title":"未分類の見本","description":"まだ分類しません。"},
                        {"boothId":"8800884","title":"消えた見本","description":"あとで公開終了にします。"},
                        {"boothId":"8800885","title":"手で固定した見本","description":"手動ロックです。"},
                        {"boothId":"8800886","title":"古い分類の見本","description":"ルールが古いです。"},
                        {"boothId":"8800887","title":"確認済みの見本","description":"モデル結果を確認済みにします。"}
                    ]
                }"#,
            )
            .unwrap(),
        )
        .unwrap();
    let current = i64::from(crate::classifier::CLASSIFIER_VERSION);
    let catalog = require_product(&connection, "8800881").unwrap();
    assert_eq!(catalog.category, Some(Category::Avatar));
    assert!(catalog.extracted_at.is_some());
    assert_eq!(classifier_version_of(&connection, "8800881"), current);
    let owned = require_product(&connection, "8800882").unwrap();
    assert_eq!(owned.category, Some(Category::Avatar));
    assert!(owned.extracted_at.is_some());
    assert_eq!(classifier_version_of(&connection, "8800882"), current);
    assert!(
        require_product(&connection, "8800883")
            .unwrap()
            .extracted_at
            .is_none()
    );
    assert_eq!(classifier_version_of(&connection, "8800883"), 1);

    connection
        .execute(
            "UPDATE products SET description_snippet = ?1 WHERE booth_id = '8800884'",
            params![crate::model::UNAVAILABLE_DESCRIPTION],
        )
        .unwrap();
    note_unavailable(&connection, "8800884").unwrap();
    let ended = require_product(&connection, "8800884").unwrap();
    assert_eq!(
        ended.description_snippet,
        crate::model::UNAVAILABLE_DESCRIPTION
    );
    assert!(ended.extracted_at.is_some());
    assert_eq!(classifier_version_of(&connection, "8800884"), 1);

    apply_extraction(
        &mut connection,
        "8800885",
        &extraction(Category::Outfit, &[], false, 0.9),
        0.6,
    )
    .unwrap();
    move_product_genre(&mut connection, "8800885", "asset", "gimmick").unwrap();
    connection
        .execute(
            "UPDATE products SET classifier_version = 1 WHERE booth_id = '8800885'",
            [],
        )
        .unwrap();
    apply_extraction(
        &mut connection,
        "8800886",
        &extraction(Category::Outfit, &[], true, 0.9),
        0.6,
    )
    .unwrap();
    connection
        .execute(
            "UPDATE products SET classifier_version = 1 WHERE booth_id = '8800886'",
            [],
        )
        .unwrap();
    apply_extraction(
        &mut connection,
        "8800887",
        &extraction(Category::Hair, &[], false, 0.9),
        0.6,
    )
    .unwrap();
    connection
            .execute(
                "UPDATE products SET classifier_version = 1, user_confirmed = 1 WHERE booth_id = '8800887'",
                [],
            )
            .unwrap();

    let stale = products_for_extract(&connection, ExtractTarget::Stale).unwrap();
    let stale_ids: Vec<_> = stale.iter().map(|job| job.booth_id.as_str()).collect();
    assert!(stale_ids.contains(&"8800886"), "{stale_ids:?}");
    assert!(stale_ids.contains(&"8800887"), "{stale_ids:?}");
    for kept in ["8800881", "8800882", "8800883", "8800884", "8800885"] {
        assert!(!stale_ids.contains(&kept), "{kept} {stale_ids:?}");
    }
    assert_eq!(
        classification_status(&connection).unwrap().stale_count as usize,
        stale_ids.len()
    );
    assert_eq!(classifier_version_of(&connection, "8800885"), 1);
    assert!(
        require_product(&connection, "8800885")
            .unwrap()
            .category_locked
    );
    assert!(
        require_product(&connection, "8800885")
            .unwrap()
            .genre_locked
    );

    connection
        .execute(
            "UPDATE products SET classifier_version = 1 WHERE booth_id IN ('8800881', '8800882')",
            [],
        )
        .unwrap();
    settle_dictionary(&mut connection).unwrap();
    assert_eq!(classifier_version_of(&connection, "8800881"), current);
    assert_eq!(classifier_version_of(&connection, "8800882"), current);
    assert_eq!(classifier_version_of(&connection, "8800884"), 1);
    assert_eq!(classifier_version_of(&connection, "8800885"), 1);
    assert_eq!(classifier_version_of(&connection, "8800887"), 1);
    assert_eq!(
        require_product(&connection, "8800881").unwrap().category,
        Some(Category::Avatar)
    );
    assert_eq!(
        require_product(&connection, "8800882").unwrap().category,
        Some(Category::Avatar)
    );
    assert!(!skip_settled(&mut connection, "8800887").unwrap());
    assert_eq!(classifier_version_of(&connection, "8800887"), 1);
    connection
        .execute(
            "UPDATE products SET classifier_version = 1 WHERE booth_id = '8800882'",
            [],
        )
        .unwrap();
    assert!(skip_settled(&mut connection, "8800882").unwrap());
    assert_eq!(classifier_version_of(&connection, "8800882"), current);
    assert_eq!(
        require_product(&connection, "8800882").unwrap().category,
        Some(Category::Avatar)
    );
    let after = products_for_extract(&connection, ExtractTarget::Stale).unwrap();
    let after_ids: Vec<_> = after.iter().map(|job| job.booth_id.as_str()).collect();
    assert!(!after_ids.contains(&"8800881"));
    assert!(!after_ids.contains(&"8800882"));
    assert!(after_ids.contains(&"8800886"));
    assert!(after_ids.contains(&"8800887"));
}

#[test]
fn opening_a_new_library_does_not_commit_each_seed_and_catalog_row() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let connection = Connection::open_in_memory().unwrap();
    // The schema steps commit on their own. Only what follows them is counted.
    migrate(&connection).unwrap();
    let commits = Arc::new(AtomicUsize::new(0));
    let counter = commits.clone();
    connection
        .commit_hook(Some(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            false
        }))
        .unwrap();
    let connection = finish_open(connection, false).unwrap();
    let avatars: i64 = connection
        .query_row("SELECT COUNT(*) FROM avatars", [], |row| row.get(0))
        .unwrap();
    assert!(avatars > 800, "{avatars}");
    // On disk every commit syncs the file. A commit per row was about 2,900 syncs.
    let commits = commits.load(Ordering::SeqCst);
    assert!(commits <= 8, "{commits} commits");
}

#[test]
fn sample_export_on_file_database_reaches_list_and_detail() {
    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let dir = Temp(std::env::temp_dir().join(format!(
            "vrclibman-sample-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir_all(&dir.0).unwrap();
    let json = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../examples/sample-booth-export.json"),
    )
    .unwrap();
    let parsed = parse_export(&json).unwrap();
    assert_eq!(parsed.products.len(), 7);

    let db_path = dir.0.join("library.sqlite");
    let mut connection = open(&db_path).unwrap();
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
    let report = import_products(&mut connection, &parsed).unwrap();
    assert_eq!(report.inserted, 7);
    assert_eq!(report.skipped, 0);
    let before = load_snapshot(&mut connection).unwrap();
    let mut owned_ids: Vec<_> = avatar_cards(&before)
        .into_iter()
        .filter(|card| card.owned)
        .map(|card| card.id)
        .collect();
    owned_ids.sort();
    assert_eq!(
        owned_ids,
        vec!["lime".to_string(), "manuka".to_string(), "plum".to_string()]
    );
    assert_eq!(
        before
            .products
            .iter()
            .filter(|product| product.category.is_none())
            .count(),
        4
    );
    assert!(
        before
            .products
            .iter()
            .any(|product| product.booth_id == "4876459"
                && product.description_snippet.contains("共通素体"))
    );

    // サンプルの説明文に書いてある名前だけを、抽出結果として渡す。
    let facts = [
        (
            "4876459",
            extraction(
                Category::Avatar,
                &["ライム", "プラム", "シフォン", "ショコラ"],
                false,
                0.93,
            ),
        ),
        (
            "7770415",
            extraction(Category::Avatar, &["プラム", "ライム"], false, 0.9),
        ),
        (
            "5058077",
            extraction(Category::Avatar, &["マヌカ"], false, 0.9),
        ),
        (
            "8800101",
            extraction(
                Category::Outfit,
                &["しなの", "ライム", "マヌカ"],
                false,
                0.88,
            ),
        ),
        (
            "8800102",
            extraction(Category::Hair, &["マヌカ"], false, 0.86),
        ),
        ("8800103", extraction(Category::Gimmick, &[], true, 0.84)),
        (
            "8800104",
            extraction(Category::Outfit, &["架空ちゃん"], false, 0.9),
        ),
    ];
    for (booth_id, extraction) in facts {
        apply_extraction(&mut connection, booth_id, &extraction, 0.6).unwrap();
    }

    drop(connection);
    let mut connection = open(&db_path).unwrap();
    let again = import_products(&mut connection, &parsed).unwrap();
    assert_eq!(again.unchanged, 7);
    // This compatibility fixture owns all editions; metadata alone is not ownership.
    connection.execute_batch("INSERT OR REPLACE INTO purchased_editions SELECT booth_id, '{\"mode\":\"all\",\"avatarIds\":[]}' FROM products").unwrap();
    let snapshot = load_snapshot(&mut connection).unwrap();
    let mut ids: Vec<_> = avatar_cards(&snapshot)
        .into_iter()
        .map(|card| card.id)
        .collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        vec![
            "lime".to_string(),
            "manuka".to_string(),
            "plum".to_string(),
            "shinano".to_string()
        ]
    );
    assert!(
        snapshot
            .avatars
            .iter()
            .any(|avatar| avatar.id == "shinano" && !avatar.owned)
    );
    assert!(
        snapshot
            .avatars
            .iter()
            .any(|avatar| avatar.id == "lime" && avatar.owned)
    );
    assert!(snapshot.avatars.iter().any(|avatar| avatar.id == "shinano"));
    assert!(
        snapshot
            .avatars
            .iter()
            .all(|avatar| avatar.id != "owned-8800104")
    );

    let lime = avatar_detail(&snapshot, "lime").unwrap();
    let body = lime
        .owned_products
        .iter()
        .find(|item| item.booth_id == "4876459")
        .unwrap();
    assert_eq!(body.category, Some(Category::Avatar));
    assert!(!body.via_base_body);
    assert!(lime.siblings.iter().any(|sibling| sibling.id == "plum"));
    assert!(lime.siblings.iter().all(|sibling| sibling.id != "manuka"));
    let lime_outfits = section_items(&lime, "outfit");
    assert_eq!(lime_outfits, vec![("8800101".to_string(), false)]);
    assert!(section_items(&lime, "hair").is_empty());
    assert!(
        lime.generic_items
            .iter()
            .any(|item| item.booth_id == "8800103")
    );

    let plum = avatar_detail(&snapshot, "plum").unwrap();
    assert_eq!(
        section_items(&plum, "outfit"),
        vec![("8800101".to_string(), true)]
    );
    assert!(
        plum.generic_items
            .iter()
            .any(|item| item.booth_id == "8800103")
    );

    let manuka = avatar_detail(&snapshot, "manuka").unwrap();
    assert_eq!(
        section_items(&manuka, "outfit"),
        vec![("8800101".to_string(), false)]
    );
    assert_eq!(
        section_items(&manuka, "hair"),
        vec![("8800102".to_string(), false)]
    );
    assert!(
        manuka
            .generic_items
            .iter()
            .any(|item| item.booth_id == "8800103")
    );
    assert!(manuka.siblings.is_empty());

    assert!(
        snapshot
            .products
            .iter()
            .all(|product| product.category.is_some())
    );
    let review: Vec<&Product> = snapshot
        .products
        .iter()
        .filter(|product| product.needs_review)
        .collect();
    assert_eq!(review.len(), 1);
    assert_eq!(review[0].booth_id, "8800104");
    assert!(
        review[0]
            .unresolved_names
            .iter()
            .any(|name| name == "架空ちゃん")
    );
}

#[test]
fn many_compatible_names_come_from_the_description_list() {
    let mut connection = open_memory().unwrap();
    let json = r#"{
            "products": [{
                "boothId": "8550001",
                "title": "部屋着(3アバター対応)",
                "description": "対応アバター\n\"サンプル工房 ライム_ver1.00 https://booth.pm/ja/items/1001\" \"プラム_ver1.01 https://booth.pm/ja/items/1002\" \"あまとうさぎ ショコラ_ver1.02 https://booth.pm/ja/items/1003\"\n基本仕様\n布です。"
            }]
        }"#;
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    let snapshot = load_snapshot(&mut connection).unwrap();
    let product = snapshot
        .products
        .iter()
        .find(|product| product.booth_id == "8550001")
        .unwrap();
    assert_eq!(product.category, Some(Category::Outfit));
    assert!(
        !product
            .compatible_avatars
            .iter()
            .any(|name| name.contains("アバター対応"))
    );
    let mut ids: Vec<_> = snapshot
        .links
        .iter()
        .filter(|link| link.product_id == "8550001")
        .map(|link| link.avatar_id.as_str())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, vec!["chocolat", "lime", "plum"]);
    assert!(
        avatar_cards(&snapshot)
            .iter()
            .all(|card| !card.id.starts_with("owned-"))
    );
}

#[test]
fn fresh_database_keeps_saved_settings_and_starts_setup() {
    let connection = open_memory().unwrap();
    save_settings(
        &connection,
        &SettingsInput {
            confidence_threshold: 0.7,
            library_folder: String::new(),
            builtin_browser: false,
            browser_force_dark: false,
            browser_auto_register: true,
            booth_link_target: "external".into(),
            remote_library_folder: String::new(),
            remote_auto_sync: false,
            route_downloads: false,
            background_mode: false,
            launch_at_login: false,
            classification_dismissed_version: 0,
            ui_rail_width: 232,
            ui_rail_collapsed: false,
            ui_genre_width: 200,
            ui_genre_collapsed: false,
            preview_pane: false,
            preview_pane_width: 300,
            avatar_hero: crate::model::DEFAULT_AVATAR_HERO.into(),
            right_panel_tabs: crate::model::DEFAULT_RIGHT_PANEL_TABS.into(),
            check_update_on_startup: false,
            ui_update_dismissed_version: String::new(),
        },
    )
    .unwrap();
    ensure_ready(&connection).unwrap();
    assert_eq!(settings(&connection).unwrap().confidence_threshold, 0.7);
    assert!(!settings(&connection).unwrap().builtin_browser);
    assert!(!setup_status(&connection).unwrap().complete);
}

#[test]
fn existing_library_or_pairing_skips_setup_until_reopened() {
    let connection = open_memory().unwrap();
    assert!(setting(&connection, "setup_complete").unwrap().is_none());
    connection
            .execute(
                "INSERT INTO products (booth_id, title, imported_at, updated_at) VALUES ('4242', '架空の服', 't', 't')",
                [],
            )
            .unwrap();
    ensure_ready(&connection).unwrap();
    assert!(setup_status(&connection).unwrap().complete);
    set_setup_complete(&connection, false).unwrap();
    ensure_ready(&connection).unwrap();
    assert!(!setup_status(&connection).unwrap().complete);

    let paired = open_memory().unwrap();
    set_extension_origin(
        &paired,
        "chrome-extension://abcdefghijklmnopabcdefghijklmnop",
    )
    .unwrap();
    ensure_ready(&paired).unwrap();
    assert!(setup_status(&paired).unwrap().complete);
}

#[test]
fn rail_collapsed_defaults_off_and_keeps_other_layout_settings() {
    let connection = open_memory().unwrap();
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
    assert!(!settings(&connection).unwrap().ui_rail_collapsed);
    let saved = update_settings(
        &connection,
        &crate::model::SettingsPatch {
            ui_rail_collapsed: Some(true),
            ..crate::model::SettingsPatch::default()
        },
    )
    .unwrap();
    assert!(saved.ui_rail_collapsed);
    assert_eq!(saved.ui_rail_width, 232);
    let widened = update_settings(
        &connection,
        &crate::model::SettingsPatch {
            ui_rail_width: Some(300),
            ..crate::model::SettingsPatch::default()
        },
    )
    .unwrap();
    assert!(widened.ui_rail_collapsed);
    assert_eq!(widened.ui_rail_width, 300);
    // The genre column's closed state is saved on its own and leaves the sidebar alone.
    assert!(!widened.ui_genre_collapsed);
    let genre_closed = update_settings(
        &connection,
        &crate::model::SettingsPatch {
            ui_genre_collapsed: Some(true),
            ..crate::model::SettingsPatch::default()
        },
    )
    .unwrap();
    assert!(genre_closed.ui_genre_collapsed);
    assert!(genre_closed.ui_rail_collapsed);
    assert_eq!(genre_closed.ui_genre_width, 200);
    assert!(settings(&connection).unwrap().ui_genre_collapsed);
    connection
        .execute("DELETE FROM settings WHERE key = 'ui_rail_collapsed'", [])
        .unwrap();
    assert!(!settings(&connection).unwrap().ui_rail_collapsed);
}

#[test]
fn avatar_hero_defaults_to_small_and_accepts_large_or_off() {
    let connection = open_memory().unwrap();
    assert_eq!(settings(&connection).unwrap().avatar_hero, "small");
    let large = update_settings(
        &connection,
        &SettingsPatch {
            avatar_hero: Some("large".into()),
            ..SettingsPatch::default()
        },
    )
    .unwrap();
    assert_eq!(large.avatar_hero, "large");
    let off = update_settings(
        &connection,
        &SettingsPatch {
            avatar_hero: Some("off".into()),
            ..SettingsPatch::default()
        },
    )
    .unwrap();
    assert_eq!(off.avatar_hero, "off");
    let weird = update_settings(
        &connection,
        &SettingsPatch {
            avatar_hero: Some("liquid".into()),
            ..SettingsPatch::default()
        },
    )
    .unwrap();
    assert_eq!(weird.avatar_hero, "small");
    assert!(!settings(&connection).unwrap().preview_pane);
}

#[test]
fn preview_pane_defaults_off_without_a_schema_change() {
    let connection = open_memory().unwrap();
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
    assert!(!settings(&connection).unwrap().preview_pane);
    assert_eq!(settings(&connection).unwrap().preview_pane_width, 300);
    let saved = update_settings(
        &connection,
        &crate::model::SettingsPatch {
            preview_pane: Some(true),
            preview_pane_width: Some(420),
            ..crate::model::SettingsPatch::default()
        },
    )
    .unwrap();
    assert!(saved.preview_pane);
    assert_eq!(saved.preview_pane_width, 420);
    assert_eq!(saved.ui_rail_width, 232);
    let kept = update_settings(
        &connection,
        &crate::model::SettingsPatch {
            ui_rail_width: Some(240),
            ..crate::model::SettingsPatch::default()
        },
    )
    .unwrap();
    assert!(kept.preview_pane);
    assert_eq!(kept.preview_pane_width, 420);
    assert_eq!(kept.ui_rail_width, 240);
    let clamped = update_settings(
        &connection,
        &crate::model::SettingsPatch {
            preview_pane_width: Some(10),
            ..crate::model::SettingsPatch::default()
        },
    )
    .unwrap();
    assert_eq!(clamped.preview_pane_width, 240);
    assert!(clamped.preview_pane);
    let huge = update_settings(
        &connection,
        &crate::model::SettingsPatch {
            preview_pane_width: Some(9000),
            ..crate::model::SettingsPatch::default()
        },
    )
    .unwrap();
    assert_eq!(huge.preview_pane_width, 1600);
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
}

#[test]
fn browser_force_dark_defaults_off_without_a_schema_change() {
    let connection = open_memory().unwrap();
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
    assert!(!settings(&connection).unwrap().browser_force_dark);
    connection
        .execute("DELETE FROM settings WHERE key = 'browser_force_dark'", [])
        .unwrap();
    assert!(
        setting(&connection, "browser_force_dark")
            .unwrap()
            .is_none()
    );
    assert!(!settings(&connection).unwrap().browser_force_dark);
    let saved = update_settings(
        &connection,
        &crate::model::SettingsPatch {
            browser_force_dark: Some(true),
            ..crate::model::SettingsPatch::default()
        },
    )
    .unwrap();
    assert!(saved.browser_force_dark);
    assert!(!saved.builtin_browser);
    assert_eq!(saved.booth_link_target, "external");
    let kept = update_settings(
        &connection,
        &crate::model::SettingsPatch {
            avatar_hero: Some("small".into()),
            ..crate::model::SettingsPatch::default()
        },
    )
    .unwrap();
    assert!(kept.browser_force_dark);
    assert_eq!(kept.avatar_hero, "small");
    let cleared = update_settings(
        &connection,
        &crate::model::SettingsPatch {
            browser_force_dark: Some(false),
            ..crate::model::SettingsPatch::default()
        },
    )
    .unwrap();
    assert!(!cleared.browser_force_dark);
    assert_eq!(cleared.avatar_hero, "small");
    assert!(serde_json::from_str::<crate::model::SettingsPatch>(r#"{}"#).is_ok());
    let omitted =
        serde_json::from_str::<crate::model::SettingsPatch>(r#"{"browserForceDark":true}"#)
            .unwrap();
    assert_eq!(omitted.browser_force_dark, Some(true));
    assert!(omitted.builtin_browser.is_none());
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
}

#[test]
fn right_panel_tabs_keep_only_known_kinds_without_a_schema_change() {
    let connection = open_memory().unwrap();
    assert_eq!(
        settings(&connection).unwrap().right_panel_tabs,
        r#"{"tabs":["preview"],"active":0}"#
    );
    let saved = update_settings(
            &connection,
            &crate::model::SettingsPatch {
                right_panel_tabs: Some(
                    r#"{"tabs":["files","https://booth.pm/ja/items/1","preview","preview","browser"],"active":4}"#
                        .into(),
                ),
                ..crate::model::SettingsPatch::default()
            },
        )
        .unwrap();
    assert_eq!(
        saved.right_panel_tabs,
        r#"{"tabs":["files","preview"],"active":1}"#
    );
    assert!(!saved.right_panel_tabs.contains("booth.pm"));
    assert!(!saved.right_panel_tabs.contains("browser"));
    let one_browser = update_settings(
        &connection,
        &crate::model::SettingsPatch {
            right_panel_tabs: Some(r#"{"tabs":["browser","preview","browser"],"active":2}"#.into()),
            ..crate::model::SettingsPatch::default()
        },
    )
    .unwrap();
    // A stored "browser" tab is the old single session. Page tabs are not saved, so it drops out.
    assert_eq!(
        one_browser.right_panel_tabs,
        r#"{"tabs":["preview"],"active":0}"#
    );
    let saved = update_settings(
        &connection,
        &crate::model::SettingsPatch {
            right_panel_tabs: Some(saved.right_panel_tabs.clone()),
            ..crate::model::SettingsPatch::default()
        },
    )
    .unwrap();
    let kept = update_settings(
        &connection,
        &crate::model::SettingsPatch {
            preview_pane: Some(true),
            ..crate::model::SettingsPatch::default()
        },
    )
    .unwrap();
    assert_eq!(kept.right_panel_tabs, saved.right_panel_tabs);
    let broken = update_settings(
        &connection,
        &crate::model::SettingsPatch {
            right_panel_tabs: Some("not json".into()),
            ..crate::model::SettingsPatch::default()
        },
    )
    .unwrap();
    assert_eq!(
        broken.right_panel_tabs,
        r#"{"tabs":["preview"],"active":0}"#
    );
    let empty = update_settings(
        &connection,
        &crate::model::SettingsPatch {
            right_panel_tabs: Some(r#"{"tabs":[],"active":3}"#.into()),
            ..crate::model::SettingsPatch::default()
        },
    )
    .unwrap();
    assert_eq!(empty.right_panel_tabs, r#"{"tabs":[],"active":0}"#);
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
}

#[test]
fn builtin_browser_stays_off_until_the_user_enables_it() {
    let connection = open_memory().unwrap();
    assert!(!settings(&connection).unwrap().builtin_browser);
    save_settings(
        &connection,
        &SettingsInput {
            confidence_threshold: 0.6,
            library_folder: String::new(),
            builtin_browser: true,
            browser_force_dark: false,
            browser_auto_register: true,
            booth_link_target: "external".into(),
            remote_library_folder: String::new(),
            remote_auto_sync: false,
            route_downloads: false,
            background_mode: false,
            launch_at_login: false,
            classification_dismissed_version: 0,
            ui_rail_width: 232,
            ui_rail_collapsed: false,
            ui_genre_width: 200,
            ui_genre_collapsed: false,
            preview_pane: false,
            preview_pane_width: 300,
            avatar_hero: crate::model::DEFAULT_AVATAR_HERO.into(),
            right_panel_tabs: crate::model::DEFAULT_RIGHT_PANEL_TABS.into(),
            check_update_on_startup: false,
            ui_update_dismissed_version: String::new(),
        },
    )
    .unwrap();
    ensure_ready(&connection).unwrap();
    let saved = settings(&connection).unwrap();
    assert!(saved.builtin_browser);
}

#[test]
fn unavailable_items_are_not_left_unextracted() {
    let mut connection = open_memory().unwrap();
    let json = r#"{"products":[
            {"boothId":"6874323","title":"消えた商品","shop":"shop","description":"公開終了（取得できず）"},
            {"boothId":"1000","title":"普通の商品","shop":"shop","description":"説明"}
        ]}"#;
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    connection
        .execute(
            "UPDATE products SET description_snippet = ?1, category = NULL, extracted_at = 't',
                 needs_review = 1, user_confirmed = 0 WHERE booth_id = '1000'",
            params![crate::model::UNAVAILABLE_DESCRIPTION],
        )
        .unwrap();
    ensure_ready(&connection).unwrap();
    let snapshot = load_snapshot(&mut connection).unwrap();
    for id in ["6874323", "1000"] {
        let product = snapshot
            .products
            .iter()
            .find(|product| product.booth_id == id)
            .unwrap();
        assert_eq!(product.category, Some(Category::Other), "{id}");
        assert!(product.extracted_at.is_some(), "{id}");
        assert!(!product.needs_review, "{id}");
        assert!(product.user_confirmed, "{id}");
    }
}

#[test]
fn unclassified_work_matches_the_unclassified_target() {
    let mut connection = open_memory().unwrap();
    assert!(
        !has_unclassified_work(&connection).unwrap(),
        "an empty library has nothing to classify"
    );
    let json = r#"{"products":[{"boothId":"1000","title":"見本の衣装","shop":"shop","description":"説明"}]}"#;
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    assert!(has_unclassified_work(&connection).unwrap());
    assert_eq!(
        products_for_extract(&connection, ExtractTarget::Unclassified)
            .unwrap()
            .len(),
        1
    );
    connection
        .execute(
            "INSERT INTO product_exclusions (booth_id, excluded_at) VALUES ('1000', 't')",
            [],
        )
        .unwrap();
    assert!(
        !has_unclassified_work(&connection).unwrap(),
        "an excluded product is not classified"
    );
    assert!(
        products_for_extract(&connection, ExtractTarget::Unclassified)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn unpublished_library_flag_survives_merge_and_is_not_unavailable() {
    let mut connection = open_memory().unwrap();
    let hidden = r#"{"boothId":"424242","title":"ダミー衣装","shop":"ダミー工房","description":"説明だけ","isFree":true,"unpublished":true}"#;
    import_products(&mut connection, &parse_export(hidden).unwrap()).unwrap();
    let (free, unpublished, category, confirmed): (Option<i64>, Option<i64>, Option<String>, i64) = connection
            .query_row(
                "SELECT is_free, unpublished, category, user_confirmed FROM products WHERE booth_id = '424242'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
    assert_eq!(free, Some(1));
    assert_eq!(unpublished, Some(1));
    assert_eq!(category, None);
    assert_eq!(confirmed, 0);
    let again =
        r#"{"boothId":"424242","title":"ダミー衣装","shop":"ダミー工房","description":"説明だけ"}"#;
    import_products(&mut connection, &parse_export(again).unwrap()).unwrap();
    let kept: Option<i64> = connection
        .query_row(
            "SELECT unpublished FROM products WHERE booth_id = '424242'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(kept, Some(1));
    let gone =
        r#"{"boothId":"515151","title":"消えたダミー","description":"公開終了（取得できず）"}"#;
    import_products(&mut connection, &parse_export(gone).unwrap()).unwrap();
    let closed: (Option<String>, Option<i64>) = connection
        .query_row(
            "SELECT category, unpublished FROM products WHERE booth_id = '515151'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(closed.0.as_deref(), Some("other"));
    assert_eq!(closed.1, None);
    let snapshot = load_snapshot(&connection).unwrap();
    let shown = products_by_ids(&snapshot, &["424242".into(), "515151".into()]);
    assert_eq!(shown[0].booth_id, "424242");
    assert!(shown[0].unpublished);
    assert!(shown[0].is_free);
    assert_eq!(shown[0].category, None);
    assert!(!shown[1].unpublished);
    assert_eq!(shown[1].category, Some(Category::Other));
    let clear = r#"{"boothId":"424242","title":"ダミー衣装","unpublished":false}"#;
    import_products(&mut connection, &parse_export(clear).unwrap()).unwrap();
    let cleared: Option<i64> = connection
        .query_row(
            "SELECT unpublished FROM products WHERE booth_id = '424242'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(cleared, Some(0));
    let after = load_snapshot(&connection).unwrap();
    let hidden_view = products_by_ids(&after, &["424242".into()]);
    assert!(!hidden_view[0].unpublished);
    assert!(hidden_view[0].is_free);
}

#[test]
fn sale_brackets_and_alias_forms_follow_rules_not_a_weak_model() {
    let mut connection = open_memory().unwrap();
    let json = r#"{
            "products": [
                {"boothId":"5007531","title":"【半額セール中】真冬 Mafuyu / オリジナル3Dモデル","shop":"s","description":"真冬のオリジナル3Dモデルです。"},
                {"boothId":"4023598","title":"【半額セール中】【オリジナル3Dモデル】Nayu - ナユ","shop":"s","description":"ナユのオリジナル3Dモデルです。"},
                {"boothId":"4876459","title":"ライム -Lime- / オリジナル3Dモデル","shop":"こまど","description":"ライムのオリジナル3Dモデルです。"},
                {"boothId":"1011","title":"【プラム専用】Hairグラデーション","shop":"s","description":"髪です。"},
                {"boothId":"1012","title":"ショコラ -Chocolat- 対応ワンピース","shop":"s","description":"衣装です。"},
                {"boothId":"1013","title":"rurune -ルルネ- 用ヘア","shop":"s","description":"髪です。"},
                {"boothId":"1014","title":"manuka-マヌカ対応部屋着","shop":"s","description":"服です。"},
                {"boothId":"1015","title":"Selestia対応アクセサリ","shop":"s","description":"小物です。"},
                {"boothId":"1016","title":"プラムちゃん専用ウィッグ","shop":"s","description":"髪です。"}
            ]
        }"#;
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    apply_extraction(
        &mut connection,
        "5007531",
        &extraction(Category::Avatar, &["真冬 Mafuyu"], false, 0.4),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "4023598",
        &extraction(Category::Avatar, &["Nayu"], false, 0.9),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "4876459",
        &extraction(Category::Outfit, &[], false, 0.2),
        0.6,
    )
    .unwrap();
    for (id, category) in [
        ("1011", Category::Outfit),
        ("1012", Category::Gimmick),
        ("1013", Category::Outfit),
        ("1014", Category::Gimmick),
        ("1015", Category::Outfit),
        ("1016", Category::Outfit),
    ] {
        apply_extraction(
            &mut connection,
            id,
            &extraction(category, &[], false, 0.2),
            0.6,
        )
        .unwrap();
    }

    let snapshot = load_snapshot(&mut connection).unwrap();
    assert!(
        snapshot
            .avatars
            .iter()
            .all(|avatar| avatar.display_name != "半額セール中")
    );
    let linked = |id: &str| -> Vec<String> {
        snapshot
            .links
            .iter()
            .filter(|link| link.product_id == id)
            .map(|link| link.avatar_id.clone())
            .collect()
    };
    let name_of = |id: &str| {
        snapshot
            .avatars
            .iter()
            .find(|avatar| avatar.id == id)
            .map(|avatar| avatar.display_name.clone())
            .unwrap_or_default()
    };
    let mafuyu = linked("5007531");
    let nayu = linked("4023598");
    assert_eq!(mafuyu.len(), 1);
    assert_eq!(nayu.len(), 1);
    assert_ne!(mafuyu[0], nayu[0]);
    assert_eq!(name_of(&mafuyu[0]), "真冬");
    assert_eq!(name_of(&nayu[0]), "ナユ");
    assert_eq!(
        snapshot
            .avatars
            .iter()
            .find(|avatar| avatar.id == nayu[0])
            .unwrap()
            .aliases,
        vec!["Nayu".to_string()]
    );

    let lime = snapshot
        .products
        .iter()
        .find(|product| product.booth_id == "4876459")
        .unwrap();
    assert_eq!(lime.category, Some(Category::Avatar));
    assert!(!lime.needs_review);
    assert_eq!(linked("4876459"), vec!["lime".to_string()]);

    let hair = snapshot
        .products
        .iter()
        .find(|product| product.booth_id == "1011")
        .unwrap();
    assert_eq!(hair.category, Some(Category::Outfit));
    assert_eq!(linked("1011"), vec!["plum".to_string()]);
    assert!(!hair.needs_review);

    assert_eq!(linked("1012"), vec!["chocolat".to_string()]);
    assert_eq!(linked("1013"), vec!["rurune".to_string()]);
    assert_eq!(linked("1014"), vec!["manuka".to_string()]);
    assert_eq!(linked("1015"), vec!["celestia".to_string()]);
    assert_eq!(linked("1016"), vec!["plum".to_string()]);
    for id in ["1012", "1013", "1014", "1015", "1016"] {
        let product = snapshot
            .products
            .iter()
            .find(|product| product.booth_id == id)
            .unwrap();
        assert!(
            !product.needs_review,
            "{id} category={:?} names={:?} unresolved={:?} links={:?}",
            product.category,
            product.compatible_avatars,
            product.unresolved_names,
            linked(id)
        );
    }
}

#[test]
fn title_fragments_and_weak_keywords_do_not_override_the_model() {
    let mut connection = open_memory().unwrap();
    let json = r#"{
            "products": [
                {"boothId":"7825066","title":"【プラム Plum】Eye Textures Pack (Full Pack)","shop":"s","description":"瞳のテクスチャです。"},
                {"boothId":"4511962","title":"むにゅりっぷ LipSync【プラム対応】","shop":"s","description":"プラムのオリジナル3Dモデル向けの口ギミックです。"},
                {"boothId":"2000","title":"エク対応ギミック","shop":"s","description":"エク用です。"},
                {"boothId":"7592511","title":"ミルキーチル","shop":"s","description":"Eku対応の衣装です。"},
                {"boothId":"2001","title":"エク - Eku","shop":"s","description":"エクのオリジナル3Dモデルです。"}
            ]
        }"#;
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    apply_extraction(
        &mut connection,
        "7825066",
        &extraction(Category::Texture, &["プラム"], false, 0.9),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "4511962",
        &extraction(Category::Gimmick, &["プラム"], false, 0.9),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "2000",
        &extraction(Category::Gimmick, &["エク"], false, 0.9),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "7592511",
        &extraction(Category::Outfit, &["Eku"], false, 0.9),
        0.6,
    )
    .unwrap();
    let before = load_snapshot(&mut connection).unwrap();
    let eye = before
        .products
        .iter()
        .find(|product| product.booth_id == "7825066")
        .unwrap();
    assert!(
        eye.unresolved_names.is_empty(),
        "{:?}",
        eye.unresolved_names
    );
    assert!(eye.compatible_avatars.iter().all(|name| name == "プラム"));
    assert!(!eye.needs_review);
    let lips = before
        .products
        .iter()
        .find(|product| product.booth_id == "4511962")
        .unwrap();
    assert_eq!(lips.category, Some(Category::Gimmick));
    assert!(
        before
            .links
            .iter()
            .any(|link| link.product_id == "4511962" && link.avatar_id == "plum")
    );
    let catalog_eku = before
        .avatars
        .iter()
        .find(|avatar| avatar.display_name == "エク")
        .unwrap();
    assert!(
        before
            .links
            .iter()
            .any(|link| link.product_id == "2000" && link.avatar_id == catalog_eku.id)
    );

    apply_extraction(
        &mut connection,
        "2001",
        &extraction(Category::Avatar, &["エク"], false, 0.9),
        0.6,
    )
    .unwrap();
    reresolve_all(&mut connection).unwrap();
    let after = load_snapshot(&mut connection).unwrap();
    let eku_id = after
        .links
        .iter()
        .find(|link| link.product_id == "2001")
        .unwrap()
        .avatar_id
        .clone();
    assert!(
        after
            .links
            .iter()
            .any(|link| link.product_id == "2000" && link.avatar_id == eku_id)
    );
    assert!(
        after
            .links
            .iter()
            .any(|link| link.product_id == "7592511" && link.avatar_id == eku_id)
    );
    let eku = after
        .avatars
        .iter()
        .find(|avatar| avatar.id == eku_id)
        .unwrap();
    assert_eq!(eku.display_name, "エク");
    assert_eq!(eku.aliases, vec!["Eku".to_string()]);
    assert!(
        crate::resolve::resolve_name("Eku", &after.avatars)
            .unwrap()
            .id
            == eku_id
    );
    assert!(crate::resolve::resolve_name("PC版", &after.avatars).is_none());
}

#[test]
fn kipfel_title_links_after_the_avatar_product_exists() {
    let mut connection = open_memory().unwrap();
    let json = r#"{
            "products": [
                {"boothId":"8835445","title":"【Kipfel】ねこネコパーカー【MA対応】","shop":"s","description":"パーカーです。"},
                {"boothId":"5813187","title":"キプフェル Kipfel / オリジナル3Dモデル","shop":"s","description":"キプフェルのオリジナル3Dモデルです。"}
            ]
        }"#;
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    apply_extraction(
        &mut connection,
        "8835445",
        &extraction(Category::Outfit, &[], true, 0.9),
        0.6,
    )
    .unwrap();
    let early = load_snapshot(&mut connection).unwrap();
    let kipfel = early
        .avatars
        .iter()
        .find(|avatar| avatar.display_name == "キプフェル")
        .unwrap();
    assert!(
        early
            .links
            .iter()
            .any(|link| link.product_id == "8835445" && link.avatar_id == kipfel.id)
    );
    let avatar = early
        .products
        .iter()
        .find(|product| product.booth_id == "5813187")
        .unwrap();
    assert_eq!(avatar.category, Some(Category::Avatar));
    assert!(!avatar.needs_review);
    reresolve_all(&mut connection).unwrap();
    let snapshot = load_snapshot(&mut connection).unwrap();
    let hoodie = snapshot
        .products
        .iter()
        .find(|product| product.booth_id == "8835445")
        .unwrap();
    assert!(!hoodie.is_generic);
    assert!(!hoodie.needs_review);
    assert!(
        snapshot
            .links
            .iter()
            .any(|link| link.product_id == "8835445" && link.avatar_id == kipfel.id)
    );
}

#[test]
fn texture_and_gimmick_without_avatars_are_generic() {
    let mut connection = open_memory().unwrap();
    let json = r#"{
            "products": [
                {"boothId":"7001","title":"瞳テクスチャ","shop":"s","description":"目の模様です。"},
                {"boothId":"7002","title":"光るギミック","shop":"s","description":"小物です。"},
                {"boothId":"7003","title":"【プラム対応】瞳","shop":"s","description":"プラムの瞳です。"}
            ]
        }"#;
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    apply_extraction(
        &mut connection,
        "7001",
        &extraction(Category::Texture, &[], false, 0.9),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "7002",
        &extraction(Category::Gimmick, &[], false, 0.9),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "7003",
        &extraction(Category::Texture, &["プラム"], false, 0.9),
        0.6,
    )
    .unwrap();
    let snapshot = load_snapshot(&mut connection).unwrap();
    let texture = snapshot
        .products
        .iter()
        .find(|product| product.booth_id == "7001")
        .unwrap();
    let gimmick = snapshot
        .products
        .iter()
        .find(|product| product.booth_id == "7002")
        .unwrap();
    let named = snapshot
        .products
        .iter()
        .find(|product| product.booth_id == "7003")
        .unwrap();
    assert!(texture.is_generic);
    assert!(gimmick.is_generic);
    assert!(!named.is_generic);
}

#[test]
fn accessories_without_avatar_names_are_generic() {
    let mut connection = open_memory().unwrap();
    let json = r#"{
            "products": [
                {"boothId":"6409298","title":"Neon Halo","shop":"s","description":"光る輪です。"},
                {"boothId":"3318785","title":"ピアス【PB対応】","shop":"s","description":"耳飾りです。"}
            ]
        }"#;
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    apply_extraction(
        &mut connection,
        "6409298",
        &extraction(Category::Accessory, &[], false, 1.0),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "3318785",
        &extraction(Category::Accessory, &["PB"], false, 1.0),
        0.6,
    )
    .unwrap();
    let snapshot = load_snapshot(&mut connection).unwrap();
    for id in ["6409298", "3318785"] {
        let product = snapshot
            .products
            .iter()
            .find(|product| product.booth_id == id)
            .unwrap();
        assert!(product.is_generic, "{id}");
        assert!(
            product.unresolved_names.is_empty(),
            "{id} {:?}",
            product.unresolved_names
        );
        assert!(!product.needs_review, "{id}");
        assert!(snapshot.links.iter().all(|link| link.product_id != id));
    }
}

#[test]
fn a_seen_reading_pair_folds_a_later_romaji_name() {
    let mut connection = open_memory().unwrap();
    let json = r#"{
            "products": [
                {"boothId":"5420885","title":"Dreamy Step","shop":"s","description":"衣装です。"},
                {"boothId":"6980080","title":"Calm Loop Hair","shop":"s","description":"髪です。"}
            ]
        }"#;
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    apply_extraction(
        &mut connection,
        "5420885",
        &extraction(Category::Outfit, &["萌(Moe)"], false, 0.9),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "6980080",
        &extraction(Category::Hair, &["Moe"], false, 0.9),
        0.6,
    )
    .unwrap();
    reresolve_all(&mut connection).unwrap();
    let snapshot = load_snapshot(&mut connection).unwrap();
    for id in ["5420885", "6980080"] {
        let product = snapshot
            .products
            .iter()
            .find(|product| product.booth_id == id)
            .unwrap();
        assert!(
            product.unresolved_names.is_empty(),
            "{id} {:?}",
            product.unresolved_names
        );
        assert!(
            snapshot.links.iter().any(|link| {
                link.product_id == id
                    && snapshot
                        .avatars
                        .iter()
                        .any(|avatar| avatar.id == link.avatar_id && avatar.display_name == "萌")
            }),
            "{id}"
        );
    }
}

#[test]
fn a_low_confidence_placeholder_stays_in_review() {
    let mut connection = open_memory().unwrap();
    // No explicit original-model title: this case still depends on model extraction.
    let json = r#"{"products":[{"boothId":"3001","title":"架空ちゃん","shop":"s","description":"架空ちゃんです。"}]}"#;
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    apply_extraction(
        &mut connection,
        "3001",
        &extraction(Category::Avatar, &[], false, 0.4),
        0.6,
    )
    .unwrap();
    let snapshot = load_snapshot(&mut connection).unwrap();
    let product = snapshot
        .products
        .iter()
        .find(|product| product.booth_id == "3001")
        .unwrap();
    assert!(product.needs_review);
}

#[test]
fn mamehinata_face_animation_is_grounded_on_the_established_placeholder() {
    let mut connection = open_memory().unwrap();
    let json = r#"{
            "products": [
                {"boothId":"6436480","title":"まめひなたフェイスアニメーションセット","shop":"s","description":"表情のアニメーションです。"},
                {"boothId":"4340548","title":"まめひなた Mamehinata / オリジナル3Dモデル","shop":"s","description":"まめひなたのオリジナル3Dモデルです。"}
            ]
        }"#;
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    apply_extraction(
        &mut connection,
        "6436480",
        &extraction(Category::Gimmick, &["まめひなた"], false, 1.0),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "4340548",
        &extraction(Category::Avatar, &["まめひなた"], false, 1.0),
        0.6,
    )
    .unwrap();
    reresolve_all(&mut connection).unwrap();
    let snapshot = load_snapshot(&mut connection).unwrap();
    let face = snapshot
        .products
        .iter()
        .find(|product| product.booth_id == "6436480")
        .unwrap();
    assert!(face.unresolved_names.is_empty());
    assert!(!face.needs_review);
    assert!(
        snapshot
            .links
            .iter()
            .any(|link| { link.product_id == "6436480" && link.avatar_id == "mf-4340548" })
    );
}

#[test]
fn catalog_booth_id_registers_the_avatar_product() {
    let mut connection = open_memory().unwrap();
    let json = r#"{
            "products": [
                {"boothId":"3016885","title":"透羽","shop":"mio","description":"アバターです。"},
                {"boothId":"8800991","title":"【架空丸対応】パーカー","shop":"s","description":"衣装です。"},
                {"boothId":"8800992","title":"【架空丸対応】ウィッグ","shop":"s","description":"髪です。"}
            ]
        }"#;
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    let snapshot = load_snapshot(&mut connection).unwrap();
    let body = snapshot
        .products
        .iter()
        .find(|product| product.booth_id == "3016885")
        .unwrap();
    assert_eq!(body.category, Some(Category::Avatar));
    assert!(body.extracted_at.is_some());
    assert!(!body.needs_review);
    assert!(snapshot.links.iter().any(|link| {
        link.product_id == "3016885"
            && snapshot.avatars.iter().any(|avatar| {
                avatar.id == link.avatar_id && avatar.display_name == "透羽" && avatar.owned
            })
    }));
    let kikyo = snapshot
        .avatars
        .iter()
        .find(|avatar| avatar.display_name == "桔梗")
        .unwrap();
    assert!(!kikyo.owned);
    let known = known_booth_ids(&connection).unwrap();
    assert!(known.iter().any(|item| item == "3016885"));
    assert!(known.iter().any(|item| item == "8800991"));
}

#[test]
fn a_named_original_avatar_outside_the_catalog_is_owned_on_import() {
    let mut connection = open_memory().unwrap();
    let json = r#"{"products":[
            {"boothId":"8800771","title":"九尾オリジナル3Dモデル「輝夜」-kaguya-","description":"対応アバター\nライム\n利用規約"},
            {"boothId":"8800772","title":"【輝夜対応】衣装","description":"衣装です。"}
        ]}"#;
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    reresolve_all(&mut connection).unwrap();
    let snapshot = load_snapshot(&mut connection).unwrap();
    let body = snapshot
        .products
        .iter()
        .find(|p| p.booth_id == "8800771")
        .unwrap();
    assert_eq!(body.category, Some(Category::Avatar));
    assert!(!body.needs_review);
    let kaguya = snapshot
        .avatars
        .iter()
        .find(|a| a.display_name == "輝夜")
        .unwrap();
    assert!(kaguya.owned);
    assert!(
        kaguya
            .aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case("kaguya"))
    );
    assert!(
        snapshot
            .links
            .iter()
            .any(|link| link.product_id == "8800771" && link.avatar_id == kaguya.id)
    );
    assert!(
        snapshot
            .links
            .iter()
            .any(|link| link.product_id == "8800772" && link.avatar_id == kaguya.id)
    );
    assert!(
        !snapshot
            .avatars
            .iter()
            .find(|a| a.id == "lime")
            .unwrap()
            .owned
    );
    // Reimport and reopening must not duplicate or forget the owned body.
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    settle_dictionary(&mut connection).unwrap();
    let after = load_snapshot(&mut connection).unwrap();
    assert_eq!(
        after
            .avatars
            .iter()
            .filter(|a| a.display_name == "輝夜")
            .count(),
        1
    );
    assert!(
        after
            .avatars
            .iter()
            .any(|a| a.display_name == "輝夜" && a.owned)
    );
}

#[test]
fn a_plain_compat_list_keeps_a_category_the_title_cannot_decide() {
    let mut connection = open_memory().unwrap();
    let json = r#"[{"boothId":"8800773","title":"見本の品","description":"対応アバター\nうらら\nライム\n基本仕様\n布です。"}]"#;
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    apply_extraction(
        &mut connection,
        "8800773",
        &extraction(Category::Outfit, &[], false, 0.9),
        0.6,
    )
    .unwrap();
    // The same payload again is an unchanged product, which still runs compatibility.
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    let kept = require_product(&connection, "8800773").unwrap();
    assert_eq!(kept.category, Some(Category::Outfit));
    assert!(
        link_ids(&connection, "8800773")
            .unwrap()
            .iter()
            .any(|id| id == "lime")
    );
}

#[test]
fn quoted_body_identity_wins_over_known_names_outside_the_quotation() {
    for (index, title, expected_name, expected_id) in [
        (
            0,
            "オリジナル3Dモデル「輝夜」 ライム対応",
            "輝夜",
            "owned-8800780",
        ),
        (
            1,
            "オリジナル3Dモデル「プラム」 ライム対応",
            "プラム",
            "plum",
        ),
    ] {
        let mut connection = open_memory().unwrap();
        let booth_id = format!("880078{index}");
        let json = serde_json::json!({"boothId": booth_id, "title": title}).to_string();
        import_products(&mut connection, &parse_export(&json).unwrap()).unwrap();
        reresolve_all(&mut connection).unwrap();
        let snapshot = load_snapshot(&mut connection).unwrap();
        let owned = snapshot
            .avatars
            .iter()
            .find(|a| a.id == expected_id)
            .unwrap();
        assert_eq!(owned.display_name, expected_name);
        assert!(owned.owned);
        assert_eq!(
            link_ids(&connection, &booth_id).unwrap(),
            vec![expected_id.to_string()]
        );
        assert!(
            !snapshot
                .avatars
                .iter()
                .find(|a| a.id == "lime")
                .unwrap()
                .owned
        );
    }
}

#[test]
fn failed_body_name_inference_does_not_block_import_or_reopening() {
    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let dir = Temp(std::env::temp_dir().join(format!(
            "vrclibman-unresolved-body-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    let path = dir.0.join("library.sqlite");
    let parsed = parse_export(r#"{"boothId":"8800782","title":"オリジナル3Dモデル「ReviewOnlyBody」","description":"ReviewOnlyBody（朏）"}"#).unwrap();
    {
        let mut connection = open(&path).unwrap();
        import_products(&mut connection, &parsed).unwrap();
        let product = require_product(&connection, "8800782").unwrap();
        assert!(product.needs_review);
        assert!(!product.user_confirmed);
        assert!(product.extracted_at.is_none());
        assert!(link_ids(&connection, "8800782").unwrap().is_empty());
    }
    {
        let mut connection = open(&path).unwrap();
        settle_dictionary(&mut connection).unwrap();
        let product = require_product(&connection, "8800782").unwrap();
        assert!(product.needs_review);
        assert!(product.extracted_at.is_none());
        assert!(link_ids(&connection, "8800782").unwrap().is_empty());
    }
}

#[test]
fn automatic_body_registration_propagates_database_errors_and_rolls_back() {
    let mut connection = open_memory().unwrap();
    let parsed =
        parse_export(r#"{"boothId":"8800783","title":"オリジナル3Dモデル「輝夜」"}"#).unwrap();
    upsert(&connection, &parsed.products[0]).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER fail_body_link BEFORE INSERT ON product_avatars
             BEGIN SELECT RAISE(ABORT, 'fixture body link failure'); END;",
        )
        .unwrap();
    let error = settle_dictionary(&mut connection).unwrap_err();
    assert!(error.contains("fixture body link failure"));
    assert!(!avatar_exists(&connection, "owned-8800783").unwrap());
    let product = require_product(&connection, "8800783").unwrap();
    assert!(product.extracted_at.is_none());
    assert!(!product.user_confirmed);
}

#[test]
fn named_original_bodies_settle_existing_imports_but_preserve_manual_decisions() {
    let mut connection = open_memory().unwrap();
    let parsed =
        parse_export(r#"{"boothId":"8800773","title":"九尾オリジナル3Dモデル「輝夜」-kaguya-"}"#)
            .unwrap();
    upsert(&connection, &parsed.products[0]).unwrap();
    connection.execute("UPDATE products SET category = 'outfit', category_locked = 1 WHERE booth_id = '8800773'", []).unwrap();
    settle_dictionary(&mut connection).unwrap();
    assert_eq!(
        require_product(&connection, "8800773").unwrap().category,
        Some(Category::Outfit)
    );
    assert!(link_ids(&connection, "8800773").unwrap().is_empty());
    connection.execute("UPDATE products SET category = NULL, category_locked = 0, links_locked = 1 WHERE booth_id = '8800773'", []).unwrap();
    settle_dictionary(&mut connection).unwrap();
    assert!(link_ids(&connection, "8800773").unwrap().is_empty());
    connection
        .execute(
            "UPDATE products SET links_locked = 0 WHERE booth_id = '8800773'",
            [],
        )
        .unwrap();
    settle_dictionary(&mut connection).unwrap();
    let snapshot = load_snapshot(&mut connection).unwrap();
    assert!(
        snapshot
            .avatars
            .iter()
            .any(|a| a.display_name == "輝夜" && a.owned)
    );
}

fn insert_dummy_product(connection: &Connection, booth_id: &str, title: &str, description: &str) {
    let json = serde_json::json!({
        "boothId": booth_id,
        "title": title,
        "description": description,
    })
    .to_string();
    let parsed = parse_export(&json).unwrap();
    upsert(connection, &parsed.products[0]).unwrap();
}

fn force_stuck_classified(connection: &Connection, booth_id: &str, display: &str) {
    let avatar_id = format!("owned-{booth_id}");
    connection
        .execute(
            "UPDATE products SET category = 'avatar', category_locked = 0, links_locked = 0,
                 user_confirmed = 0, needs_review = 0, is_generic = 0, confidence = 0.9,
                 extracted_at = '2026-01-01T00:00:00Z', classifier_version = 2,
                 compatible_avatars_json = ?1, unresolved_names_json = '[]'
                 WHERE booth_id = ?2",
            params![to_json(&vec![display.to_string()]).unwrap(), booth_id],
        )
        .unwrap();
    connection
        .execute(
            "INSERT OR IGNORE INTO avatars
                 (id, display_name, base_body, aliases_json, is_builtin, owned, notes)
                 VALUES (?1, ?2, NULL, '[]', 0, 0, '')",
            params![avatar_id, display],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE avatars SET owned = 0, display_name = ?2 WHERE id = ?1",
            params![avatar_id, display],
        )
        .unwrap();
    connection
        .execute(
            "DELETE FROM product_avatars WHERE product_id = ?1",
            params![booth_id],
        )
        .unwrap();
    connection
            .execute(
                "INSERT INTO product_avatars (product_id, avatar_id, source) VALUES (?1, ?2, 'extracted')",
                params![booth_id, avatar_id],
            )
            .unwrap();
}

fn assert_owned_original(connection: &Connection, booth_id: &str, display: &str) {
    let avatar_id = format!("owned-{booth_id}");
    let snapshot = load_snapshot(connection).unwrap();
    let matched: Vec<_> = snapshot
        .avatars
        .iter()
        .filter(|avatar| avatar.display_name == display)
        .collect();
    assert_eq!(matched.len(), 1, "{display}");
    assert!(matched[0].owned, "{display}");
    assert_eq!(matched[0].id, avatar_id);
    assert!(
        snapshot.links.iter().any(|link| {
            link.product_id == booth_id && link.avatar_id == avatar_id && link.source == "owned"
        }),
        "{booth_id} {:?}",
        snapshot
            .links
            .iter()
            .filter(|link| link.product_id == booth_id)
            .collect::<Vec<_>>()
    );
}

fn assert_still_unowned(connection: &Connection, booth_id: &str, display: &str) {
    let avatar_id = format!("owned-{booth_id}");
    let owned: i64 = connection
        .query_row(
            "SELECT owned FROM avatars WHERE id = ?1",
            params![avatar_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(owned, 0, "{display}");
    let source: String = connection
        .query_row(
            "SELECT source FROM product_avatars WHERE product_id = ?1 AND avatar_id = ?2",
            params![booth_id, avatar_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(source, "extracted", "{display}");
    let copies: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM avatars WHERE display_name = ?1",
            params![display],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(copies, 1, "{display}");
}

#[test]
fn model_classified_quoted_original_body_becomes_owned() {
    let mut connection = open_memory().unwrap();
    let booth_id = "7701001";
    let json = serde_json::json!({
        "boothId": booth_id,
        "title": "オリジナル3Dモデル「テスト子」-testko-",
        "description": "テスト子のダミー説明です。",
    })
    .to_string();
    import_products(&mut connection, &parse_export(&json).unwrap()).unwrap();
    // The model path used to leave this body classified but unowned.
    force_stuck_classified(&connection, booth_id, "テスト子");
    apply_extraction(
        &mut connection,
        booth_id,
        &extraction(Category::Avatar, &["テスト子"], false, 0.9),
        0.6,
    )
    .unwrap();
    assert_owned_original(&connection, booth_id, "テスト子");
    settle_dictionary(&mut connection).unwrap();
    assert_owned_original(&connection, booth_id, "テスト子");
}

#[test]
fn skip_settled_owns_a_classified_quoted_original_body_without_the_rules() {
    let mut connection = open_memory().unwrap();
    let booth_id = "7701002";
    insert_dummy_product(
        &connection,
        booth_id,
        "オリジナル3Dモデル「テスト子」-testko-",
        "テスト子のダミー説明です。",
    );
    force_stuck_classified(&connection, booth_id, "テスト子");
    let settled = skip_settled(&mut connection, booth_id).unwrap();
    assert!(settled);
    assert_owned_original(&connection, booth_id, "テスト子");
    let copies: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM avatars WHERE display_name = 'テスト子'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(copies, 1);
}

#[test]
fn quoted_original_body_guards_stay_unowned() {
    let mut connection = open_memory().unwrap();
    let cases = [
        (
            "7701101",
            "オリジナル3Dモデル「テスト錠」-testjo-",
            "テスト錠",
            "locked",
        ),
        (
            "7701102",
            "オリジナル3Dモデル「テスト鎖」-testsa-",
            "テスト鎖",
            "links",
        ),
        (
            "7701103",
            "オリジナル3Dモデル「テスト認」-testnin-",
            "テスト認",
            "confirmed",
        ),
        (
            "7701104",
            "オリジナル3Dモデル「テスト甲」「テスト乙」",
            "テスト甲",
            "quotes",
        ),
        (
            "7701105",
            "オリジナル3Dモデル「テスト除」-testjo-",
            "テスト除",
            "excluded",
        ),
        (
            "7701106",
            "オリジナル3Dモデル「テスト無」-testmu-",
            "テスト無",
            "unavailable",
        ),
    ];
    for (booth_id, title, display, _kind) in cases {
        insert_dummy_product(&connection, booth_id, title, "ダミー説明です。");
        force_stuck_classified(&connection, booth_id, display);
    }
    connection
            .execute(
                "UPDATE products SET category = 'outfit', category_locked = 1 WHERE booth_id = '7701101'",
                [],
            )
            .unwrap();
    connection
        .execute(
            "UPDATE products SET links_locked = 1 WHERE booth_id = '7701102'",
            [],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE products SET user_confirmed = 1 WHERE booth_id = '7701103'",
            [],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE products SET description_snippet = ?1 WHERE booth_id = '7701106'",
            params![crate::model::UNAVAILABLE_DESCRIPTION],
        )
        .unwrap();
    set_product_excluded(&mut connection, "7701105", true).unwrap();

    settle_dictionary(&mut connection).unwrap();
    for (booth_id, _title, display, _kind) in cases {
        let product = require_product(&connection, booth_id).unwrap();
        assert!(
            !settle_named_original_body(&mut connection, &product).unwrap(),
            "{display}"
        );
        let _ = skip_settled(&mut connection, booth_id).unwrap();
        assert_still_unowned(&connection, booth_id, display);
    }
}

#[test]
fn reimport_does_not_duplicate_a_quoted_original_body() {
    let mut connection = open_memory().unwrap();
    let booth_id = "7701003";
    let json = serde_json::json!({
        "boothId": booth_id,
        "title": "オリジナル3Dモデル「テスト子」-testko-",
        "description": "テスト子のダミー説明です。",
    })
    .to_string();
    let parsed = parse_export(&json).unwrap();
    import_products(&mut connection, &parsed).unwrap();
    apply_extraction(
        &mut connection,
        booth_id,
        &extraction(Category::Avatar, &["テスト子"], false, 0.9),
        0.6,
    )
    .unwrap();
    import_products(&mut connection, &parsed).unwrap();
    settle_dictionary(&mut connection).unwrap();
    assert_owned_original(&connection, booth_id, "テスト子");

    let stuck = "7701004";
    insert_dummy_product(
        &connection,
        stuck,
        "オリジナル3Dモデル「テスト再」-testre-",
        "テスト再のダミー説明です。",
    );
    force_stuck_classified(&connection, stuck, "テスト再");
    let again = serde_json::json!({
        "boothId": stuck,
        "title": "オリジナル3Dモデル「テスト再」-testre-",
        "description": "テスト再のダミー説明です。",
    })
    .to_string();
    import_products(&mut connection, &parse_export(&again).unwrap()).unwrap();
    assert_owned_original(&connection, stuck, "テスト再");
    assert_owned_original(&connection, booth_id, "テスト子");
}

#[test]
fn startup_repairs_a_classified_quoted_original_body() {
    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let dir = Temp(std::env::temp_dir().join(format!(
            "vrclibman-owned-body-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir_all(&dir.0).unwrap();
    let path = dir.0.join("library.sqlite");
    {
        let connection = open(&path).unwrap();
        insert_dummy_product(
            &connection,
            "7701005",
            "オリジナル3Dモデル「テスト子」-testko-",
            "テスト子のダミー説明です。",
        );
        force_stuck_classified(&connection, "7701005", "テスト子");
        let owned: i64 = connection
            .query_row(
                "SELECT owned FROM avatars WHERE id = 'owned-7701005'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(owned, 0);
    }
    {
        let connection = open(&path).unwrap();
        assert_owned_original(&connection, "7701005", "テスト子");
    }
    {
        let connection = open(&path).unwrap();
        assert_owned_original(&connection, "7701005", "テスト子");
    }
}

#[test]
fn an_unlisted_avatar_body_stays_unowned_until_marked() {
    let mut connection = open_memory().unwrap();
    let json = r#"{
            "products": [
                {"boothId":"3001","title":"架空丸","shop":"s","description":"架空丸です。"},
                {"boothId":"3002","title":"【桔梗対応】パーカー","shop":"s","description":"衣装です。"}
            ]
        }"#;
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    let before = load_snapshot(&mut connection).unwrap();
    assert!(
        before
            .avatars
            .iter()
            .any(|avatar| avatar.display_name == "桔梗" && !avatar.owned)
    );
    assert!(
        before
            .products
            .iter()
            .any(|product| product.booth_id == "3001" && product.extracted_at.is_none())
    );
    let id = mark_owned_body(&mut connection, "3001").unwrap();
    let after = load_snapshot(&mut connection).unwrap();
    let owned = after.avatars.iter().find(|avatar| avatar.id == id).unwrap();
    assert!(owned.owned);
    assert_eq!(owned.display_name, "架空丸");
    let body = after
        .products
        .iter()
        .find(|product| product.booth_id == "3001")
        .unwrap();
    assert_eq!(body.category, Some(Category::Avatar));
    assert!(!body.needs_review);
    assert!(
        after
            .avatars
            .iter()
            .any(|avatar| avatar.display_name == "桔梗" && !avatar.owned)
    );
    let cards = avatar_cards(&after);
    assert!(cards.iter().any(|card| card.id == id && card.owned));
    assert!(
        cards
            .iter()
            .any(|card| card.display_name == "桔梗" && !card.owned)
    );
}

#[test]
fn purchased_variations_stay_owned_and_partial_pages_keep_files() {
    let mut connection = open_memory().unwrap();
    let first = r#"{"boothId":"424242","title":"架空の服","isFree":true,"variations":[{"name":"通常","owned":true},{"name":"特典","owned":false}],"downloads":[{"filename":"body.unitypackage","variation":"通常","sizeBytes":12}]}"#;
    import_products(&mut connection, &parse_export(first).unwrap()).unwrap();
    let second = r#"{"boothId":"424242","title":"架空の服","variations":[{"name":"通常","owned":false}],"downloads":[{"filename":"extra.zip","sizeBytes":3}]}"#;
    let report = import_products(&mut connection, &parse_export(second).unwrap()).unwrap();
    assert_eq!(report.updated, 1);
    let owned: i64 = connection
        .query_row(
            "SELECT owned FROM product_variations WHERE booth_id = '424242' AND name = '通常'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(owned, 1);
    let bonus: i64 = connection
        .query_row(
            "SELECT owned FROM product_variations WHERE booth_id = '424242' AND name = '特典'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(bonus, 0);
    let files: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM product_files WHERE booth_id = '424242'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(files, 2);
    connection
            .execute(
                "UPDATE product_files SET downloaded = 1, local_path = 'C:\\Assets\\body.unitypackage' WHERE filename = 'body.unitypackage'",
                [],
            )
            .unwrap();
    let again = r#"{"boothId":"424242","title":"架空の服","downloads":[{"filename":"body.unitypackage","variation":"通常","sizeBytes":12}]}"#;
    import_products(&mut connection, &parse_export(again).unwrap()).unwrap();
    let downloaded: i64 = connection
        .query_row(
            "SELECT downloaded FROM product_files WHERE filename = 'body.unitypackage'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(downloaded, 1);
    let free: Option<i64> = connection
        .query_row(
            "SELECT is_free FROM products WHERE booth_id = '424242'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(free, Some(1));
    let explicit = r#"{"boothId":"424242","title":"架空の服","isFree":false}"#;
    import_products(&mut connection, &parse_export(explicit).unwrap()).unwrap();
    let free: Option<i64> = connection
        .query_row(
            "SELECT is_free FROM products WHERE booth_id = '424242'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(free, Some(0));
}

#[test]
fn an_import_without_files_warns_and_a_downloadable_id_keeps_its_row() {
    let mut connection = open_memory().unwrap();
    let empty = r#"{"boothId":"424242","title":"架空の服"}"#;
    let report = import_products(&mut connection, &parse_export(empty).unwrap()).unwrap();
    assert_eq!(report.variations, 0);
    assert_eq!(report.downloads, 0);
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("0 件"))
    );
    let placeholder = r#"{"boothId":"424242","title":"架空の服","variations":[{"name":"通常","owned":true}],"downloads":[{"filename":"2336025","downloadableId":"2336025","variation":"通常"}]}"#;
    import_products(&mut connection, &parse_export(placeholder).unwrap()).unwrap();
    let named = r#"{"boothId":"424242","title":"架空の服","downloads":[{"filename":"body.unitypackage","downloadableId":"2336025","variation":"通常","sizeBytes":12}]}"#;
    let report = import_products(&mut connection, &parse_export(named).unwrap()).unwrap();
    assert_eq!(report.downloads, 1);
    assert_eq!(report.variations, 0);
    assert!(
        report
            .warnings
            .iter()
            .all(|warning| !warning.contains("0 件"))
    );
    let filename: String = connection
            .query_row(
                "SELECT filename FROM product_files WHERE booth_id = '424242' AND downloadable_id = '2336025'",
                [],
                |row| row.get(0),
            )
            .unwrap();
    assert_eq!(filename, "body.unitypackage");
    assert_eq!(report.unnamed_downloads, 0);
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM product_files WHERE booth_id = '424242'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn a_legacy_id_filename_is_replaced_on_the_same_row() {
    let mut connection = open_memory().unwrap();
    let legacy = r#"{"boothId":"424242","title":"架空の服","downloads":[{"filename":"2336025","variation":"通常"}]}"#;
    let first = import_products(&mut connection, &parse_export(legacy).unwrap()).unwrap();
    assert_eq!(first.downloads, 1);
    assert_eq!(first.unnamed_downloads, 1);
    let named = r#"{"boothId":"424242","title":"架空の服","downloads":[{"filename":"body.unitypackage","downloadableId":"2336025","variation":"通常"}]}"#;
    let second = import_products(&mut connection, &parse_export(named).unwrap()).unwrap();
    assert_eq!(second.unnamed_downloads, 0);
    let (filename, id): (String, String) = connection
        .query_row(
            "SELECT filename, downloadable_id FROM product_files WHERE booth_id = '424242'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(filename, "body.unitypackage");
    assert_eq!(id, "2336025");
    let again = r#"{"boothId":"424242","title":"架空の服","downloads":[{"filename":"2336025","downloadableId":"2336025","variation":"通常"}]}"#;
    import_products(&mut connection, &parse_export(again).unwrap()).unwrap();
    let kept: String = connection
        .query_row(
            "SELECT filename FROM product_files WHERE booth_id = '424242'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(kept, "body.unitypackage");
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM product_files WHERE booth_id = '424242'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn a_folder_scan_creates_local_file_rows_from_product_ids() {
    let mut connection = open_memory().unwrap();
    let json = r#"{"boothId":"424242","title":"架空の服","downloads":[{"filename":"known.unitypackage"}]}"#;
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    let outcome = crate::scan::ScanOutcome {
        truncated: false,
        entries: vec![
            crate::scan::ScanEntry {
                path: "/library/424242/known.unitypackage".into(),
                filename: "known.unitypackage".into(),
                is_dir: false,
                size: 12,
                labels: vec!["known".into(), "424242".into()],
            },
            crate::scan::ScanEntry {
                path: "/library/clothes/pack-424242/bonus.zip".into(),
                filename: "bonus.zip".into(),
                is_dir: false,
                size: 4,
                labels: vec!["bonus".into()],
            },
            crate::scan::ScanEntry {
                path: "/library/other/note.txt".into(),
                filename: "note.txt".into(),
                is_dir: false,
                size: 1,
                labels: vec!["note".into()],
            },
        ],
    };
    apply_scan(&mut connection, "/library", &outcome).unwrap();
    let known: (i64, String) = connection
        .query_row(
            "SELECT downloaded, local_path FROM product_files
                 WHERE booth_id = '424242' AND filename = 'known.unitypackage'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(known.0, 1);
    assert_eq!(known.1, "/library/424242/known.unitypackage");
    let bonus: (i64, String) = connection
        .query_row(
            "SELECT downloaded, local_path FROM product_files
                 WHERE booth_id = '424242' AND filename = 'bonus.zip'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(bonus.0, 1);
    assert!(bonus.1.contains("bonus.zip"));
    let note: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM product_files WHERE filename = 'note.txt'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(note, 0);
}

/// 内蔵ブラウザで買った商品は、購入完了ページの題とショップだけで登録され、ファイル情報が無い。
/// カードには購入済みの印を渡し、ライブラリから読んだファイル行だけを加える（題・無料の印は変えない）。
#[test]
fn a_checkout_product_is_marked_purchased_and_takes_its_library_files() {
    let mut connection = open_memory().unwrap();
    import_export_json(
        &mut connection,
        r#"[{"boothId":"5512345","title":"見本のスカイボックス","shop":"見本工房","url":"https://booth.pm/ja/items/5512345"}]"#,
    )
    .unwrap();
    let purchased = |connection: &Connection| {
        load_snapshot(connection)
            .unwrap()
            .product_flags
            .into_iter()
            .find(|flag| flag.booth_id == "5512345")
            .unwrap()
            .purchased
    };
    assert!(!purchased(&connection));
    let bought = crate::purchases::CheckoutProduct {
        booth_id: "5512345".into(),
        title: "見本のスカイボックス".into(),
        shop: "見本工房".into(),
    };
    assert!(crate::purchases::record_checkout(&mut connection, "91234567", &[bought]).unwrap());
    // ギフトかどうかは詳細にしか出ないので、購入完了だけの記録ではまだ購入済みにしない。
    // 確かめていない購入として印を付け、カードに無料配布を探す操作を出さない。
    assert!(!purchased(&connection));
    let unverified = |connection: &Connection| {
        load_snapshot(connection)
            .unwrap()
            .product_flags
            .into_iter()
            .find(|flag| flag.booth_id == "5512345")
            .unwrap()
            .purchase_unverified
    };
    assert!(unverified(&connection));
    connection
        .execute(
            "UPDATE purchase_orders SET detail_read_at = '2026-11-22T00:00:00Z' WHERE order_id = '91234567'",
            [],
        )
        .unwrap();
    assert!(purchased(&connection));
    assert!(!unverified(&connection));
    // 贈ったギフトの注文は、自分の購入として扱わない。
    connection
        .execute(
            "UPDATE purchase_orders SET gift_detected = 1 WHERE order_id = '91234567'",
            [],
        )
        .unwrap();
    assert!(!purchased(&connection));
    // 本人がギフトでないと決めた注文は、購入として扱う。
    crate::purchases::set_gift_override(&connection, "91234567", Some(false)).unwrap();
    assert!(purchased(&connection));
    crate::purchases::set_gift_override(&connection, "91234567", Some(true)).unwrap();
    assert!(!purchased(&connection));
    crate::purchases::set_gift_override(&connection, "91234567", None).unwrap();
    connection
        .execute(
            "UPDATE purchase_orders SET gift_detected = 0 WHERE order_id = '91234567'",
            [],
        )
        .unwrap();
    assert!(purchased(&connection));

    // ライブラリの 1 商品分（extractPage と同じ形）。題が違っても、題は書き換えない。
    let library = crate::import_data::parse_export(
        r#"[{"boothId":"5512345","title":"ライブラリ側の題","shop":"","url":"https://booth.pm/ja/items/5512345",
             "variations":[{"name":"通常版","owned":true}],
             "downloads":[{"filename":"skybox_v1.zip","variation":"通常版","downloadableId":"7712345"},
                          {"filename":"readme.txt","variation":"通常版","downloadableId":"7712346"}]}]"#,
    )
    .unwrap();
    assert_eq!(
        store_purchased_files(&mut connection, &library.products[0]).unwrap(),
        2
    );
    assert_eq!(
        store_purchased_files(&mut connection, &library.products[0]).unwrap(),
        2
    );
    assert_eq!(product_file_count(&connection, "5512345").unwrap(), 2);
    let (title, free): (String, Option<i64>) = connection
        .query_row(
            "SELECT title, is_free FROM products WHERE booth_id = '5512345'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(title, "見本のスカイボックス");
    assert_eq!(free, None);
    let (variation, number, downloaded): (String, String, i64) = connection
        .query_row(
            "SELECT variation, downloadable_id, downloaded FROM product_files
             WHERE booth_id = '5512345' AND filename = 'skybox_v1.zip'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        (variation.as_str(), number.as_str(), downloaded),
        ("通常版", "7712345", 0)
    );

    // 除外中の商品とライブラリに無い商品には書かない。
    set_product_excluded(&mut connection, "5512345", true).unwrap();
    assert!(store_purchased_files(&mut connection, &library.products[0]).is_err());
    let mut stranger = library.products[0].clone();
    stranger.booth_id = "5599999".into();
    assert!(store_purchased_files(&mut connection, &stranger).is_err());
}

#[test]
fn free_files_fill_an_empty_product_without_touching_its_text() {
    let mut connection = open_memory().unwrap();
    let id = add_free_product(&mut connection, "https://booth.pm/ja/items/424242").unwrap();
    connection
        .execute(
            "UPDATE products SET title = 'ダミー', is_free = NULL WHERE booth_id = ?1",
            params![id],
        )
        .unwrap();
    let files = vec![IncomingDownload {
        filename: "dummy_v1.zip".into(),
        size_bytes: None,
        variation: String::new(),
        downloadable_id: "9400002".into(),
    }];
    assert_eq!(
        store_free_files(&mut connection, &id, &files, Some(true), Some(false)).unwrap(),
        1
    );
    assert_eq!(
        store_free_files(&mut connection, &id, &files, Some(true), Some(false)).unwrap(),
        1
    );
    let (title, free, adult): (String, Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT title, is_free, is_adult FROM products WHERE booth_id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(title, "ダミー");
    assert_eq!(free, Some(1));
    assert_ne!(adult, Some(1));
    let (downloaded, number): (i64, String) = connection
        .query_row(
            "SELECT downloaded, downloadable_id FROM product_files WHERE booth_id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((downloaded, number.as_str()), (0, "9400002"));
    assert!(store_free_files(&mut connection, "999999", &files, Some(true), None).is_err());
}

#[test]
fn free_url_and_routed_download_do_not_fetch_booth() {
    let mut connection = open_memory().unwrap();
    let id = add_free_product(&mut connection, "https://booth.pm/ja/items/424242?ref=1").unwrap();
    assert_eq!(id, "424242");
    assert!(add_free_product(&mut connection, "https://example.test/nope").is_err());
    let free: i64 = connection
        .query_row(
            "SELECT is_free FROM products WHERE booth_id = '424242'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(free, 1);
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM products WHERE booth_id = '424242'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    add_free_product(&mut connection, "https://booth.pm/ja/items/424242").unwrap();
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM products WHERE booth_id = '424242'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);

    let root = std::env::temp_dir().join(format!("vrclibman-accept-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let staging = root.join("Downloads").join("VRCLibMan").join("515151");
    let library = root.join("library");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::create_dir_all(&library).unwrap();
    let source = staging.join("free.unitypackage");
    std::fs::write(&source, b"free").unwrap();
    let notice = crate::model::DownloadFinished {
        booth_id: "515151".into(),
        path: source.display().to_string(),
        filename: "free.unitypackage".into(),
        size: Some(3),
        is_free: Some(true),
        is_adult: None,
        variation: "通常".into(),
        title: "無料の小物".into(),
    };
    assert!(
        prepare_download(&mut connection, &notice)
            .unwrap_err()
            .contains("オフ")
    );
    assert!(source.exists());
    let refused = save_settings(
        &connection,
        &SettingsInput {
            confidence_threshold: 0.6,
            library_folder: String::new(),
            builtin_browser: false,
            browser_force_dark: false,
            browser_auto_register: true,
            booth_link_target: "external".into(),
            remote_library_folder: String::new(),
            remote_auto_sync: false,
            route_downloads: true,
            background_mode: false,
            launch_at_login: false,
            classification_dismissed_version: 0,
            ui_rail_width: 232,
            ui_rail_collapsed: false,
            ui_genre_width: 200,
            ui_genre_collapsed: false,
            preview_pane: false,
            preview_pane_width: 300,
            avatar_hero: crate::model::DEFAULT_AVATAR_HERO.into(),
            right_panel_tabs: crate::model::DEFAULT_RIGHT_PANEL_TABS.into(),
            check_update_on_startup: false,
            ui_update_dismissed_version: String::new(),
        },
    );
    assert!(refused.unwrap_err().contains("ライブラリ"));
    save_settings(
        &connection,
        &SettingsInput {
            confidence_threshold: 0.6,
            library_folder: library.display().to_string(),
            builtin_browser: false,
            browser_force_dark: false,
            browser_auto_register: true,
            booth_link_target: "external".into(),
            remote_library_folder: String::new(),
            remote_auto_sync: false,
            route_downloads: true,
            background_mode: false,
            launch_at_login: false,
            classification_dismissed_version: 0,
            ui_rail_width: 232,
            ui_rail_collapsed: false,
            ui_genre_width: 200,
            ui_genre_collapsed: false,
            preview_pane: false,
            preview_pane_width: 300,
            avatar_hero: crate::model::DEFAULT_AVATAR_HERO.into(),
            right_panel_tabs: crate::model::DEFAULT_RIGHT_PANEL_TABS.into(),
            check_update_on_startup: false,
            ui_update_dismissed_version: String::new(),
        },
    )
    .unwrap();
    struct ClearDownloads;
    impl Drop for ClearDownloads {
        fn drop(&mut self) {
            crate::library_files::set_test_downloads(None);
        }
    }
    let _clear = ClearDownloads;
    crate::library_files::set_test_downloads(Some(root.join("Downloads")));
    let outside = root.join("secret.unitypackage");
    std::fs::write(&outside, b"no").unwrap();
    let bad = crate::model::DownloadFinished {
        path: outside.display().to_string(),
        filename: "secret.unitypackage".into(),
        ..notice.clone()
    };
    assert!(
        prepare_download(&mut connection, &bad)
            .unwrap_err()
            .contains("VRCLibMan")
    );
    assert!(outside.exists());
    let wrong = crate::model::DownloadFinished {
        booth_id: "900001".into(),
        ..notice.clone()
    };
    assert!(prepare_download(&mut connection, &wrong).is_err());
    assert!(source.exists());
    let plan = prepare_download(&mut connection, &notice).unwrap();
    let placed = crate::library_files::place_into_library(
        &plan.source,
        plan.library.as_deref(),
        &plan.booth_id,
    )
    .unwrap();
    record_placed_download(
        &connection,
        &plan.booth_id,
        &plan.variation,
        &placed,
        notice.size,
    )
    .unwrap();
    assert!(!source.exists());
    assert_eq!(placed, library.join("515151").join("free.unitypackage"));
    let created_free: i64 = connection
        .query_row(
            "SELECT is_free FROM products WHERE booth_id = '515151'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(created_free, 1);
    let title: String = connection
        .query_row(
            "SELECT title FROM products WHERE booth_id = '515151'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(title, "無料の小物");
    let downloaded: i64 = connection
        .query_row(
            "SELECT downloaded FROM product_files WHERE booth_id = '515151'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(downloaded, 1);
    let stored_size: i64 = connection
        .query_row(
            "SELECT size_bytes FROM product_files WHERE booth_id = '515151'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored_size, 4);
    std::fs::write(&placed, b"free!").unwrap();
    remember_file_size(&connection, "515151", "通常", "free.unitypackage", 5).unwrap();
    let stored_size: i64 = connection
        .query_row(
            "SELECT size_bytes FROM product_files WHERE booth_id = '515151'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored_size, 5);
    let _ = std::fs::remove_dir_all(&root);
}

fn browser_settings(
    connection: &Connection,
    library: &std::path::Path,
    browser: bool,
    route: bool,
) -> Result<(), String> {
    save_settings(
        connection,
        &SettingsInput {
            confidence_threshold: 0.6,
            library_folder: library.display().to_string(),
            builtin_browser: browser,
            browser_force_dark: false,
            browser_auto_register: true,
            booth_link_target: "external".into(),
            remote_library_folder: String::new(),
            remote_auto_sync: false,
            route_downloads: route,
            background_mode: false,
            launch_at_login: false,
            classification_dismissed_version: 0,
            ui_rail_width: 232,
            ui_rail_collapsed: false,
            ui_genre_width: 200,
            ui_genre_collapsed: false,
            preview_pane: false,
            preview_pane_width: 300,
            avatar_hero: crate::model::DEFAULT_AVATAR_HERO.into(),
            right_panel_tabs: crate::model::DEFAULT_RIGHT_PANEL_TABS.into(),
            check_update_on_startup: false,
            ui_update_dismissed_version: String::new(),
        },
    )
}

#[test]
fn browser_download_needs_the_opt_in_and_routing_before_any_file_moves() {
    let mut connection = open_memory().unwrap();
    let root = crate::library_files::resolved_temp_dir()
        .join(format!("vrclibman-browser-dl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let library = root.join("library");
    let downloads = root.join("Downloads");
    std::fs::create_dir_all(&library).unwrap();
    std::fs::create_dir_all(&downloads).unwrap();
    let payload = r#"{"boothId":"424242","title":"架空の服","downloads":[{"filename":"2336025","variation":"通常","downloadableId":"2336025"},{"filename":"note.txt","variation":"","downloadableId":""}]}"#;
    import_products(&mut connection, &parse_export(payload).unwrap()).unwrap();
    assert!(
        plan_browser_download(&connection, "424242", "通常", "2336025")
            .unwrap_err()
            .contains("オフです。拡張")
    );
    browser_settings(&connection, &library, true, false).unwrap();
    assert!(
        plan_browser_download(&connection, "424242", "通常", "2336025")
            .unwrap_err()
            .contains("自動整理はオフ")
    );
    browser_settings(&connection, &library, true, true).unwrap();
    assert!(
        plan_browser_download(&connection, "424242", "", "note.txt")
            .unwrap_err()
            .contains("ダウンロード番号")
    );
    let plan = plan_browser_download(&connection, "424242", "通常", "2336025").unwrap();
    assert_eq!(plan.downloadable_id, "2336025");
    let legacy = r#"{"boothId":"515151","title":"架空の服","downloads":[{"filename":"8187247","variation":""}]}"#;
    import_products(&mut connection, &parse_export(legacy).unwrap()).unwrap();
    let legacy = plan_browser_download(&connection, "515151", "", "8187247").unwrap();
    assert_eq!(legacy.downloadable_id, "8187247");

    struct ClearDownloads;
    impl Drop for ClearDownloads {
        fn drop(&mut self) {
            crate::library_files::set_test_downloads(None);
        }
    }
    let _clear = ClearDownloads;
    crate::library_files::set_test_downloads(Some(downloads.clone()));
    let staging = crate::library_files::prepare_staging_dir(&downloads, "424242").unwrap();
    let html = staging.join("login.html");
    std::fs::write(
        &html,
        b"<!doctype html><html><head><title>login</title></head></html>",
    )
    .unwrap();
    let html_plan = BrowserFilePlan {
        filename: "2336025".into(),
        ..plan.clone()
    };
    let refused = commit_browser_download(&mut connection, &html_plan, &html).unwrap_err();
    assert!(refused.contains("ログイン"));
    assert!(!html.exists());
    let still: String = connection
        .query_row(
            "SELECT filename FROM product_files WHERE booth_id = '424242' AND variation = '通常'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(still, "2336025");

    let packed = staging.join("body.unitypackage");
    std::fs::write(&packed, b"PK\x03\x04dummy").unwrap();
    let placed = commit_browser_download(&mut connection, &plan, &packed).unwrap();
    assert_eq!(placed, library.join("424242").join("body.unitypackage"));
    assert!(!packed.exists());
    let (filename, downloaded, id): (String, i64, String) = connection
        .query_row(
            "SELECT filename, downloaded, downloadable_id FROM product_files
                 WHERE booth_id = '424242' AND variation = '通常'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(filename, "body.unitypackage");
    assert_eq!(downloaded, 1);
    assert_eq!(id, "2336025");
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM product_files WHERE booth_id = '424242'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
    adopt_download_filename(
        &connection,
        "424242",
        "通常",
        "body.unitypackage",
        "2336025",
        "2336025",
    )
    .unwrap();
    let kept: String = connection
        .query_row(
            "SELECT filename FROM product_files WHERE booth_id = '424242' AND variation = '通常'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(kept, "body.unitypackage");
    let title: String = connection
        .query_row(
            "SELECT title FROM products WHERE booth_id = '424242'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!title.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn opening_the_database_keeps_generic_rows_until_extract() {
    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let dir = Temp(std::env::temp_dir().join(format!(
            "vrclibman-world-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir_all(&dir.0).unwrap();
    let db_path = dir.0.join("library.sqlite");
    let json = r#"{
            "products": [
                {"boothId":"9101","title":"見本の小物","shop":"見本","description":"どのアバターでも使える小物です"},
                {"boothId":"9102","title":"見本のワールド小物","shop":"見本","description":"ワールドに置く小物です"},
                {"boothId":"9103","title":"表情ギミック","shop":"見本","description":"アバターに付けるギミックです"},
                {"boothId":"9104","title":"ミホン用ギミック","shop":"見本","description":"ミホンに付けるギミックです"}
            ]
        }"#;
    {
        let mut connection = open(&db_path).unwrap();
        import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
        apply_extraction(
            &mut connection,
            "9101",
            &extraction(Category::Gimmick, &[], true, 0.9),
            0.6,
        )
        .unwrap();
        connection
            .execute(
                "UPDATE products SET category = 'gimmick', is_generic = 1, extracted_at = 't',
                     needs_review = 0 WHERE booth_id = '9102'",
                [],
            )
            .unwrap();
        let _ = connection.execute_batch("PRAGMA wal_checkpoint(FULL);");
    }
    {
        let connection = open(&db_path).unwrap();
        let snapshot = load_snapshot(&connection).unwrap();
        for id in ["9101", "9102"] {
            let product = snapshot
                .products
                .iter()
                .find(|product| product.booth_id == id)
                .unwrap();
            assert_eq!(product.category, Some(Category::Gimmick), "{id}");
            assert!(product.is_generic, "{id}");
        }
        for id in ["9103", "9104"] {
            let untouched = snapshot
                .products
                .iter()
                .find(|product| product.booth_id == id)
                .unwrap();
            assert!(untouched.category.is_none(), "{id}");
            assert!(!untouched.is_generic, "{id}");
        }
    }
    let mut connection = open(&db_path).unwrap();
    apply_extraction(
        &mut connection,
        "9102",
        &extraction(Category::Gimmick, &[], true, 0.84),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "9101",
        &extraction(Category::Gimmick, &[], true, 0.9),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "9103",
        &extraction(Category::Gimmick, &[], false, 0.9),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "9104",
        &extraction(Category::Gimmick, &["ミホン"], false, 0.9),
        0.6,
    )
    .unwrap();
    let snapshot = load_snapshot(&connection).unwrap();
    let world = snapshot
        .products
        .iter()
        .find(|product| product.booth_id == "9102")
        .unwrap();
    assert_eq!(world.category, Some(Category::World));
    assert!(!world.is_generic);
    let generic = snapshot
        .products
        .iter()
        .find(|product| product.booth_id == "9101")
        .unwrap();
    assert_eq!(generic.category, Some(Category::Gimmick));
    assert!(generic.is_generic);
    let gimmick = snapshot
        .products
        .iter()
        .find(|product| product.booth_id == "9103")
        .unwrap();
    // 対応アバターが無いギミックは、モデルが汎用でないと答えても汎用になる。
    assert_eq!(gimmick.category, Some(Category::Gimmick));
    assert!(gimmick.is_generic);
    let targeted = snapshot
        .products
        .iter()
        .find(|product| product.booth_id == "9104")
        .unwrap();
    assert_eq!(targeted.category, Some(Category::Gimmick));
    assert!(!targeted.is_generic);
}

#[test]
fn launch_at_login_is_stored_only_with_background_mode() {
    let connection = open_memory().unwrap();
    let saved = settings(&connection).unwrap();
    assert!(!saved.background_mode);
    assert!(!saved.launch_at_login);
    let base = SettingsInput {
        confidence_threshold: 0.6,
        library_folder: String::new(),
        builtin_browser: false,
        browser_force_dark: false,
        browser_auto_register: true,
        booth_link_target: "external".into(),
        remote_library_folder: String::new(),
        remote_auto_sync: false,
        route_downloads: false,
        background_mode: false,
        launch_at_login: true,
        classification_dismissed_version: 0,
        ui_rail_width: 232,
        ui_rail_collapsed: false,
        ui_genre_width: 200,
        ui_genre_collapsed: false,
        preview_pane: false,
        preview_pane_width: 300,
        avatar_hero: crate::model::DEFAULT_AVATAR_HERO.into(),
        right_panel_tabs: crate::model::DEFAULT_RIGHT_PANEL_TABS.into(),
        check_update_on_startup: false,
        ui_update_dismissed_version: String::new(),
    };
    save_settings(&connection, &base).unwrap();
    let saved = settings(&connection).unwrap();
    assert!(!saved.background_mode);
    assert!(!saved.launch_at_login);
    assert!(!crate::presence::autostart_should_enable(
        saved.background_mode,
        saved.launch_at_login
    ));
    save_settings(
        &connection,
        &SettingsInput {
            background_mode: true,
            launch_at_login: true,
            classification_dismissed_version: 0,
            ..base
        },
    )
    .unwrap();
    let saved = settings(&connection).unwrap();
    assert_eq!(
        saved.background_mode,
        crate::presence::background_supported()
    );
    assert_eq!(
        saved.launch_at_login,
        crate::presence::background_supported()
    );
    assert_eq!(
        crate::presence::autostart_should_enable(saved.background_mode, saved.launch_at_login),
        crate::presence::background_supported()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn linux_settings_preserve_background_preferences_without_a_desktop_host() {
    let connection = open_memory().unwrap();
    set_setting(&connection, "background_mode", "1").unwrap();
    set_setting(&connection, "launch_at_login", "1").unwrap();
    let saved = settings(&connection).unwrap();
    assert!(saved.background_supported);
    assert!(saved.background_mode && saved.launch_at_login);
    let saved = update_settings(
        &connection,
        &SettingsPatch {
            background_mode: Some(true),
            launch_at_login: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(saved.background_mode && saved.launch_at_login);
    assert_eq!(
        setting(&connection, "background_mode").unwrap().as_deref(),
        Some("1")
    );
    assert_eq!(
        setting(&connection, "launch_at_login").unwrap().as_deref(),
        Some("1")
    );
}

#[test]
fn opening_the_database_and_the_sweep_do_not_require_booth() {
    let mut connection = open_memory().unwrap();
    let root = std::env::temp_dir().join(format!("vrclibman-sweep-empty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Downloads")).unwrap();
    struct ClearDownloads;
    impl Drop for ClearDownloads {
        fn drop(&mut self) {
            crate::library_files::set_test_downloads(None);
        }
    }
    let _clear = ClearDownloads;
    crate::library_files::set_test_downloads(Some(root.join("Downloads")));
    let report = sweep_staged_downloads(&mut connection);
    assert_eq!(report.moved, 0);
    assert!(report.left.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn startup_sweep_leaves_files_when_routing_is_off() {
    let mut connection = open_memory().unwrap();
    let root = std::env::temp_dir().join(format!("vrclibman-sweep-off-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let source = root
        .join("Downloads")
        .join("VRCLibMan")
        .join("626262")
        .join("body.unitypackage");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, b"pkg").unwrap();
    struct ClearDownloads;
    impl Drop for ClearDownloads {
        fn drop(&mut self) {
            crate::library_files::set_test_downloads(None);
        }
    }
    let _clear = ClearDownloads;
    crate::library_files::set_test_downloads(Some(root.join("Downloads")));
    let report = sweep_staged_downloads(&mut connection);
    assert_eq!(report.moved, 0);
    assert!(source.exists());
    assert_eq!(std::fs::read(&source).unwrap(), b"pkg");
    assert!(report.left.iter().any(|item| {
        item.booth_id == "626262"
            && item.filename == "body.unitypackage"
            && item.reason.contains("オフ")
    }));
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM products WHERE booth_id = '626262'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn startup_sweep_moves_only_the_staged_file_without_network() {
    let mut connection = open_memory().unwrap();
    let root = crate::library_files::resolved_temp_dir()
        .join(format!("vrclibman-sweep-on-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let downloads = root.join("Downloads");
    let library = root.join("library");
    std::fs::create_dir_all(&library).unwrap();
    let source = downloads
        .join("VRCLibMan")
        .join("616161")
        .join("body.unitypackage");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, b"pkg").unwrap();
    let rejected_html = source.parent().unwrap().join("login.zip");
    std::fs::write(&rejected_html, b"<!doctype html><html>login</html>").unwrap();
    let nested = downloads
        .join("VRCLibMan")
        .join("616161")
        .join("extra")
        .join("nested.unitypackage");
    std::fs::create_dir_all(nested.parent().unwrap()).unwrap();
    std::fs::write(&nested, b"nested").unwrap();
    let invalid = downloads
        .join("VRCLibMan")
        .join("nope")
        .join("skip.unitypackage");
    std::fs::create_dir_all(invalid.parent().unwrap()).unwrap();
    std::fs::write(&invalid, b"skip").unwrap();
    let outside = root.join("outside.unitypackage");
    std::fs::write(&outside, b"secret").unwrap();
    let foreign = root
        .join("other")
        .join("VRCLibMan")
        .join("616161")
        .join("foreign.unitypackage");
    std::fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    std::fs::write(&foreign, b"foreign").unwrap();
    let pocket = root.join("pocket").join("717171").join("via.unitypackage");
    std::fs::create_dir_all(pocket.parent().unwrap()).unwrap();
    std::fs::write(&pocket, b"via").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, source.parent().unwrap().join("link.unitypackage"))
            .unwrap();
        std::os::unix::fs::symlink(
            pocket.parent().unwrap(),
            downloads.join("VRCLibMan").join("717171"),
        )
        .unwrap();
        let linked_root = root.join("linked-downloads");
        std::fs::create_dir_all(&linked_root).unwrap();
        std::os::unix::fs::symlink(root.join("pocket"), linked_root.join("VRCLibMan")).unwrap();
        assert!(crate::library_files::staged_download_candidates(&linked_root).is_empty());
        assert!(pocket.exists());
    }
    save_settings(
        &connection,
        &SettingsInput {
            confidence_threshold: 0.6,
            library_folder: library.display().to_string(),
            builtin_browser: false,
            browser_force_dark: false,
            browser_auto_register: true,
            booth_link_target: "external".into(),
            remote_library_folder: String::new(),
            remote_auto_sync: false,
            route_downloads: true,
            background_mode: false,
            launch_at_login: false,
            classification_dismissed_version: 0,
            ui_rail_width: 232,
            ui_rail_collapsed: false,
            ui_genre_width: 200,
            ui_genre_collapsed: false,
            preview_pane: false,
            preview_pane_width: 300,
            avatar_hero: crate::model::DEFAULT_AVATAR_HERO.into(),
            right_panel_tabs: crate::model::DEFAULT_RIGHT_PANEL_TABS.into(),
            check_update_on_startup: false,
            ui_update_dismissed_version: String::new(),
        },
    )
    .unwrap();
    struct ClearDownloads;
    impl Drop for ClearDownloads {
        fn drop(&mut self) {
            crate::library_files::set_test_downloads(None);
        }
    }
    let _clear = ClearDownloads;
    crate::library_files::set_test_downloads(Some(downloads));
    assert!(route_staged_download(&mut connection, &outside).is_err());
    assert!(outside.exists());
    assert_eq!(std::fs::read(&outside).unwrap(), b"secret");
    assert!(route_staged_download(&mut connection, &foreign).is_err());
    assert!(foreign.exists());
    let report = sweep_staged_downloads(&mut connection);
    assert_eq!(report.moved, 1);
    assert!(rejected_html.exists());
    assert!(!library.join("616161/login.zip").exists());
    assert!(
        report
            .left
            .iter()
            .any(|entry| entry.filename == "login.zip")
    );
    assert!(!source.exists());
    let placed = library.join("616161").join("body.unitypackage");
    assert_eq!(std::fs::read(&placed).unwrap(), b"pkg");
    assert!(nested.exists());
    assert!(invalid.exists());
    assert!(outside.exists());
    assert!(foreign.exists());
    assert!(pocket.exists());
    assert_eq!(std::fs::read(&pocket).unwrap(), b"via");
    #[cfg(unix)]
    {
        let link = root
            .join("Downloads")
            .join("VRCLibMan")
            .join("616161")
            .join("link.unitypackage");
        assert!(link.exists());
        assert!(report.left.iter().any(|item| {
            item.filename == "link.unitypackage" && item.reason.contains("シンボリックリンク")
        }));
        assert_eq!(std::fs::read(&outside).unwrap(), b"secret");
    }
    let (title, description, url): (String, String, String) = connection
        .query_row(
            "SELECT title, description_snippet, url FROM products WHERE booth_id = '616161'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(title, "商品 616161");
    assert!(description.is_empty());
    assert_eq!(url, "https://booth.pm/ja/items/616161");
    let variation: String = connection
            .query_row(
                "SELECT variation FROM product_files WHERE booth_id = '616161' AND filename = 'body.unitypackage'",
                [],
                |row| row.get(0),
            )
            .unwrap();
    assert_eq!(variation, "");
    let foreign_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM products WHERE booth_id = '717171'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(foreign_count, 0);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn stale_classification_counts_only_old_versions() {
    let mut connection = open_memory().unwrap();
    import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
    apply_extraction(
        &mut connection,
        "1004",
        &extraction(Category::Outfit, &["ライム"], false, 0.9),
        0.6,
    )
    .unwrap();
    let before = classification_status(&connection).unwrap().stale_count;
    connection
        .execute(
            "UPDATE products SET classifier_version = 1 WHERE booth_id = '1004'",
            [],
        )
        .unwrap();
    let after = classification_status(&connection).unwrap().stale_count;
    assert!(after > before);
    assert!(
        products_for_extract(&connection, ExtractTarget::Stale)
            .unwrap()
            .iter()
            .any(|job| job.booth_id == "1004")
    );
}

#[test]
fn booth_interval_is_safe_by_default_and_only_accepts_known_values() {
    let connection = open_memory().unwrap();
    assert_eq!(booth_page_interval_ms(&connection).unwrap(), 1000);
    for interval in [1000, 500, 250] {
        set_booth_page_interval_ms(&connection, interval).unwrap();
        assert_eq!(booth_page_interval_ms(&connection).unwrap(), interval);
    }
    assert!(set_booth_page_interval_ms(&connection, 50).is_err());
    assert_eq!(booth_page_interval_ms(&connection).unwrap(), 250);
    set_setting(&connection, "booth_page_interval_ms", "invalid").unwrap();
    assert_eq!(booth_page_interval_ms(&connection).unwrap(), 1000);
}

#[test]
fn pinned_move_keeps_kind_and_confirmation_across_extension_reimport() {
    let mut connection = open_memory().unwrap();
    let id = add_free_product(&mut connection, "https://booth.pm/ja/items/424242").unwrap();
    assert_eq!(id, "424242");
    let stub = require_product(&connection, "424242").unwrap();
    assert!(stub.category.is_none());
    assert!(stub.description_snippet.is_empty());
    assert!(!stub.category_locked && !stub.genre_locked);

    apply_extraction(
        &mut connection,
        "424242",
        &extraction(Category::Outfit, &[], false, 0.9),
        0.6,
    )
    .unwrap();
    let classified = require_product(&connection, "424242").unwrap();
    let extracted_at = classified.extracted_at.clone().unwrap();
    assert_eq!(classified.category, Some(Category::Outfit));
    assert!(!classified.user_confirmed);

    let previous =
        move_product_genre(&mut connection, "424242", "world", "furniture_props").unwrap();
    assert_eq!(previous.category.as_deref(), Some("outfit"));
    assert!(!previous.category_locked && !previous.genre_locked);
    let moved = require_product(&connection, "424242").unwrap();
    assert_eq!(moved.category, Some(Category::World));
    assert_eq!(moved.genre.as_deref(), Some("furniture_props"));
    assert!(moved.category_locked && moved.genre_locked);
    assert_eq!(moved.extracted_at.as_deref(), Some(extracted_at.as_str()));
    assert!(
        product_ids_by_genre(&connection, "world", Some("furniture_props"))
            .unwrap()
            .contains(&"424242".into())
    );
    assert!(
        !product_ids_by_genre(&connection, "asset", None)
            .unwrap()
            .contains(&"424242".into())
    );

    // A row confirmed from the old queue keeps its flags.
    connection
            .execute_batch(
                "UPDATE products SET user_confirmed = 1, needs_review = 0 WHERE booth_id = '424242';
                 INSERT INTO product_avatars (product_id, avatar_id, source) VALUES ('424242', 'lime', 'extracted');",
            )
            .unwrap();
    let confirmed = require_product(&connection, "424242").unwrap();
    assert!(confirmed.user_confirmed);
    assert!(!confirmed.needs_review);
    assert!(confirmed.category_locked && confirmed.genre_locked);
    assert!(!confirmed.links_locked);

    let resent = parse_export(
            r#"[{"boothId":"424242","title":"見本の小物","shop":"見本","description":"対応アバター\nライム_ver1.00 https://booth.pm/ja/items/1001\n基本仕様\n布です。"}]"#,
        )
        .unwrap();
    import_products(&mut connection, &resent).unwrap();
    // The same payload again is an unchanged product, which still runs compatibility.
    import_products(&mut connection, &resent).unwrap();

    let kept = require_product(&connection, "424242").unwrap();
    assert_eq!(kept.category, Some(Category::World));
    assert_eq!(kept.genre.as_deref(), Some("furniture_props"));
    assert!(kept.category_locked && kept.genre_locked);
    assert!(kept.user_confirmed);
    assert!(!kept.needs_review);
    assert_eq!(kept.extracted_at.as_deref(), Some(extracted_at.as_str()));
    assert!(kept.description_snippet.contains("ライム"));
    assert_eq!(
        link_ids(&connection, "424242").unwrap(),
        vec!["lime".to_string()]
    );

    apply_extraction(
        &mut connection,
        "424242",
        &extraction(Category::Hair, &["ライム"], false, 0.99),
        0.6,
    )
    .unwrap();
    let still = require_product(&connection, "424242").unwrap();
    assert_eq!(still.category, Some(Category::World));
    assert_eq!(still.genre.as_deref(), Some("furniture_props"));
    assert!(still.user_confirmed);

    for target in [
        ExtractTarget::Unclassified,
        ExtractTarget::Stale,
        ExtractTarget::All,
    ] {
        assert!(
            !products_for_extract(&connection, target)
                .unwrap()
                .iter()
                .any(|job| job.booth_id == "424242"),
            "{target:?}"
        );
    }
    connection
        .execute(
            "UPDATE products SET classifier_version = 1 WHERE booth_id = '424242'",
            [],
        )
        .unwrap();
    let pinned_stale = classification_status(&connection).unwrap().stale_count;
    assert!(
        !products_for_extract(&connection, ExtractTarget::Stale)
            .unwrap()
            .iter()
            .any(|job| job.booth_id == "424242")
    );

    let pinned = unpin_product_classification(&mut connection, "424242").unwrap();
    assert!(pinned.category_locked && pinned.genre_locked);
    let open = require_product(&connection, "424242").unwrap();
    assert_eq!(open.category, Some(Category::World));
    assert_eq!(open.genre.as_deref(), Some("furniture_props"));
    assert!(!open.category_locked && !open.genre_locked);
    assert_eq!(
        classification_status(&connection).unwrap().stale_count,
        pinned_stale + 1
    );
    assert!(
        products_for_extract(&connection, ExtractTarget::Stale)
            .unwrap()
            .iter()
            .any(|job| job.booth_id == "424242")
    );
    assert!(
        products_for_extract(&connection, ExtractTarget::All)
            .unwrap()
            .iter()
            .any(|job| job.booth_id == "424242")
    );

    restore_product_classification(&mut connection, &previous).unwrap();
    let restored = require_product(&connection, "424242").unwrap();
    assert_eq!(restored.category, Some(Category::Outfit));
    assert_eq!(restored.genre, previous.genre);
    assert!(!restored.category_locked && !restored.genre_locked);
    assert!(!restored.user_confirmed);

    assert!(move_product_genre(&mut connection, "424242", "asset", "skybox").is_err());
    assert!(move_product_genre(&mut connection, "424242", "world", "hair").is_err());
    assert!(move_product_genre(&mut connection, "424242", "avatar", "hair").is_err());
    move_product_genre(&mut connection, "424242", "asset", "shoes_socks").unwrap();
    let shoes = require_product(&connection, "424242").unwrap();
    assert_eq!(shoes.category, Some(Category::Outfit));
    assert_eq!(shoes.genre.as_deref(), Some("shoes_socks"));
    assert!(shoes.category_locked && shoes.genre_locked);
    move_product_genre(&mut connection, "424242", "asset", "hair").unwrap();
    let hair = require_product(&connection, "424242").unwrap();
    assert_eq!(hair.category, Some(Category::Hair));
    assert_eq!(hair.genre.as_deref(), Some("hair"));
}

#[test]
fn manual_owned_body_registers_pins_and_can_be_released() {
    let mut connection = open_memory().unwrap();
    let plain = r#"[
            {"boothId":"424301","title":"架空丸の小物","shop":"見本","description":"ダミーです。"},
            {"boothId":"424302","title":"色の違う小物","shop":"見本","description":"ダミーです。"},
            {"boothId":"424303","title":"名前の無い小物","shop":"見本","description":"ダミーです。"},
            {"boothId":"424304","title":"辞書の小物","shop":"見本","description":"ダミーです。"},
            {"boothId":"424305","title":"ただの小物","shop":"見本","description":"ダミーです。"},
            {"boothId":"424306","title":"除外する小物","shop":"見本","description":"ダミーです。"}
        ]"#;
    import_products(&mut connection, &parse_export(plain).unwrap()).unwrap();
    apply_extraction(
        &mut connection,
        "424301",
        &extraction(Category::Other, &["架空丸"], false, 0.9),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "424302",
        &extraction(Category::Other, &["ライム", "プラム"], false, 0.9),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "424303",
        &extraction(Category::Other, &[], false, 0.9),
        0.6,
    )
    .unwrap();
    apply_extraction(
        &mut connection,
        "424304",
        &extraction(Category::Other, &[], false, 0.9),
        0.6,
    )
    .unwrap();

    let before = require_product(&connection, "424302").unwrap();
    match register_manual_owned_body(&mut connection, "424302", None).unwrap() {
        RegisterOwnedBodyResult::NeedsName { candidates, .. } => {
            assert!(
                candidates.iter().any(|name| name == "ライム"),
                "{candidates:?}"
            );
            assert!(
                candidates.iter().any(|name| name == "プラム"),
                "{candidates:?}"
            );
        }
        RegisterOwnedBodyResult::Registered { .. } => panic!("ambiguous name was registered"),
    }
    let still = require_product(&connection, "424302").unwrap();
    assert_eq!(still.category, before.category);
    assert!(!still.category_locked && !still.genre_locked);
    assert_eq!(
        link_rows(&connection, "424302"),
        vec![
            ("lime".to_string(), "extracted".to_string()),
            ("plum".to_string(), "extracted".to_string()),
        ]
    );

    match register_manual_owned_body(&mut connection, "424303", None).unwrap() {
        RegisterOwnedBodyResult::NeedsName { candidates, .. } => assert!(candidates.is_empty()),
        RegisterOwnedBodyResult::Registered { .. } => panic!("missing name was registered"),
    }
    assert!(register_manual_owned_body(&mut connection, "424303", Some("")).is_err());
    assert!(register_manual_owned_body(&mut connection, "424303", Some("汎用")).is_err());

    let created = register_manual_owned_body(&mut connection, "424303", Some("見本丸")).unwrap();
    let (snapshot, avatar_id) = match created {
        RegisterOwnedBodyResult::Registered {
            snapshot,
            avatar_id,
            display_name,
        } => {
            assert_eq!(display_name, "見本丸");
            (snapshot, avatar_id)
        }
        RegisterOwnedBodyResult::NeedsName { .. } => panic!("name was required"),
    };
    assert_eq!(avatar_id, "owned-424303");
    let body = require_product(&connection, "424303").unwrap();
    assert_eq!(body.category, Some(Category::Avatar));
    assert_eq!(body.genre.as_deref(), Some("other"));
    assert!(body.category_locked && body.genre_locked && body.links_locked);
    assert!(body.user_confirmed && !body.needs_review);
    assert_eq!(
        link_rows(&connection, "424303"),
        vec![("owned-424303".to_string(), "owned".to_string())]
    );
    assert_eq!(owned_flag(&connection, "owned-424303"), 1);
    assert!(
        !product_ids_by_genre(&connection, "asset", Some("other"))
            .unwrap()
            .contains(&"424303".into())
    );
    assert!(
        avatar_cards(&load_snapshot(&connection).unwrap())
            .iter()
            .any(|card| card.id == "owned-424303" && card.owned)
    );

    apply_extraction(
        &mut connection,
        "424303",
        &extraction(Category::Hair, &["ライム"], false, 0.99),
        0.6,
    )
    .unwrap();
    let resent = parse_export(
            r#"[{"boothId":"424303","title":"名前の無い小物","shop":"見本","description":"説明を変えたダミーです。"}]"#,
        )
        .unwrap();
    import_products(&mut connection, &resent).unwrap();
    connection
        .execute(
            "UPDATE products SET classifier_version = 1 WHERE booth_id = '424303'",
            [],
        )
        .unwrap();
    reresolve_all(&mut connection).unwrap();
    let kept = require_product(&connection, "424303").unwrap();
    assert_eq!(kept.category, Some(Category::Avatar));
    assert!(kept.category_locked && kept.genre_locked && kept.links_locked);
    assert!(kept.user_confirmed);
    assert_eq!(
        link_rows(&connection, "424303"),
        vec![("owned-424303".to_string(), "owned".to_string())]
    );
    for target in [
        ExtractTarget::Unclassified,
        ExtractTarget::Stale,
        ExtractTarget::All,
    ] {
        assert!(
            !products_for_extract(&connection, target)
                .unwrap()
                .iter()
                .any(|job| job.booth_id == "424303"),
            "{target:?}"
        );
    }

    restore_product_classification(&mut connection, &snapshot).unwrap();
    let restored = require_product(&connection, "424303").unwrap();
    assert_eq!(restored.category, Some(Category::Other));
    assert!(!restored.category_locked && !restored.genre_locked && !restored.links_locked);
    assert!(link_rows(&connection, "424303").is_empty());
    assert!(restored.compatible_avatars.is_empty());
    assert!(restored.unresolved_names.is_empty());
    assert!(!avatar_exists(&connection, "owned-424303").unwrap());
    match register_manual_owned_body(&mut connection, "424303", None).unwrap() {
        RegisterOwnedBodyResult::NeedsName { candidates, .. } => assert!(candidates.is_empty()),
        RegisterOwnedBodyResult::Registered { .. } => panic!("undone name was reused"),
    }

    let extracted = register_manual_owned_body(&mut connection, "424301", None).unwrap();
    let extracted_snapshot = match extracted {
        RegisterOwnedBodyResult::Registered {
            snapshot,
            avatar_id,
            display_name,
        } => {
            assert_eq!(display_name, "架空丸");
            assert_eq!(avatar_id, "owned-424301");
            snapshot
        }
        RegisterOwnedBodyResult::NeedsName { .. } => {
            panic!("one extracted name should register")
        }
    };
    assert!(
        register_manual_owned_body(&mut connection, "424301", None)
            .unwrap_err()
            .contains("すでにアバター")
    );
    let released = unpin_product_classification(&mut connection, "424301").unwrap();
    let open = require_product(&connection, "424301").unwrap();
    assert_eq!(open.category, Some(Category::Other));
    assert_eq!(open.genre.as_deref(), Some("other"));
    assert!(!open.category_locked && !open.genre_locked && !open.links_locked);
    assert!(link_rows(&connection, "424301").is_empty());
    assert_eq!(owned_flag(&connection, "owned-424301"), 0);
    assert!(avatar_exists(&connection, "owned-424301").unwrap());
    restore_product_classification(&mut connection, &released).unwrap();
    let again = require_product(&connection, "424301").unwrap();
    assert_eq!(again.category, Some(Category::Avatar));
    assert!(again.category_locked && again.genre_locked && again.links_locked);
    assert_eq!(owned_flag(&connection, "owned-424301"), 1);
    assert_eq!(
        link_rows(&connection, "424301"),
        vec![("owned-424301".to_string(), "owned".to_string())]
    );
    restore_product_classification(&mut connection, &extracted_snapshot).unwrap();
    let extracted_back = require_product(&connection, "424301").unwrap();
    assert_eq!(extracted_back.category, Some(Category::Other));
    assert!(!extracted_back.category_locked && !extracted_back.links_locked);
    assert!(!avatar_exists(&connection, "owned-424301").unwrap());

    let known = register_manual_owned_body(&mut connection, "424302", Some("ライム")).unwrap();
    match known {
        RegisterOwnedBodyResult::Registered { avatar_id, .. } => assert_eq!(avatar_id, "lime"),
        RegisterOwnedBodyResult::NeedsName { .. } => panic!("supplied name should register"),
    }
    assert_eq!(owned_flag(&connection, "lime"), 1);
    assert_eq!(
        link_rows(&connection, "424302"),
        vec![("lime".to_string(), "owned".to_string())]
    );
    connection
        .execute(
            "INSERT INTO avatar_sources (booth_id, avatar_id) VALUES ('424304', 'plum')",
            [],
        )
        .unwrap();
    match register_manual_owned_body(&mut connection, "424304", None).unwrap() {
        RegisterOwnedBodyResult::Registered {
            avatar_id,
            display_name,
            ..
        } => {
            assert_eq!(avatar_id, "plum");
            assert_eq!(display_name, "プラム");
        }
        RegisterOwnedBodyResult::NeedsName { .. } => panic!("catalog body should register"),
    }
    assert_eq!(
        link_rows(&connection, "424304"),
        vec![("plum".to_string(), "catalog".to_string())]
    );
    unpin_product_classification(&mut connection, "424302").unwrap();
    let lime_back = require_product(&connection, "424302").unwrap();
    assert_eq!(lime_back.category, Some(Category::Other));
    assert_eq!(owned_flag(&connection, "lime"), 0);
    assert!(avatar_exists(&connection, "lime").unwrap());
    let plum_still = require_product(&connection, "424304").unwrap();
    assert_eq!(plum_still.category, Some(Category::Avatar));
    assert_eq!(owned_flag(&connection, "plum"), 1);

    connection
        .execute(
            "UPDATE products SET title = ?1 WHERE booth_id = '424305'",
            params!["オリジナル3Dモデル「見本子」"],
        )
        .unwrap();
    match register_manual_owned_body(&mut connection, "424305", None).unwrap() {
        RegisterOwnedBodyResult::Registered {
            avatar_id,
            display_name,
            ..
        } => {
            assert_eq!(display_name, "見本子");
            assert_eq!(avatar_id, "owned-424305");
        }
        RegisterOwnedBodyResult::NeedsName { .. } => panic!("quoted name should register"),
    }
    let quoted = require_product(&connection, "424305").unwrap();
    assert_eq!(quoted.category, Some(Category::Avatar));
    assert!(quoted.genre.is_none());
    assert!(quoted.category_locked && quoted.genre_locked);
    unpin_product_classification(&mut connection, "424305").unwrap();
    let quoted_back = require_product(&connection, "424305").unwrap();
    assert!(quoted_back.category.is_none());
    assert!(!quoted_back.category_locked && !quoted_back.genre_locked);

    set_product_excluded(&mut connection, "424306", true).unwrap();
    assert!(
        register_manual_owned_body(&mut connection, "424306", Some("見本丸"))
            .unwrap_err()
            .contains("除外")
    );
}

fn owned_flag(connection: &Connection, id: &str) -> i64 {
    connection
        .query_row(
            "SELECT owned FROM avatars WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn manual_avatar_attach_locks_the_link_without_pinning_classification() {
    let mut connection = open_memory().unwrap();
    let id = add_free_product(&mut connection, "https://booth.pm/ja/items/515151").unwrap();
    apply_extraction(
        &mut connection,
        &id,
        &extraction(Category::Gimmick, &[], true, 0.9),
        0.6,
    )
    .unwrap();
    connection
            .execute(
                "INSERT INTO product_avatars (product_id, avatar_id, source) VALUES ('515151', 'plum', 'extracted')",
                [],
            )
            .unwrap();
    let before = require_product(&connection, "515151").unwrap();
    assert_eq!(before.category, Some(Category::Gimmick));
    assert!(!before.category_locked && !before.genre_locked && !before.links_locked);
    assert!(!before.user_confirmed);

    attach_product_to_avatar(&mut connection, "lime", "515151").unwrap();
    attach_product_to_avatar(&mut connection, "lime", "515151").unwrap();
    let attached = require_product(&connection, "515151").unwrap();
    assert_eq!(attached.category, Some(Category::Gimmick));
    assert_eq!(attached.genre, before.genre);
    assert!(!attached.category_locked && !attached.genre_locked);
    assert!(attached.links_locked);
    assert!(!attached.user_confirmed);
    assert_eq!(
        link_rows(&connection, "515151"),
        vec![
            ("lime".to_string(), "manual".to_string()),
            ("plum".to_string(), "extracted".to_string()),
        ]
    );

    reresolve_all(&mut connection).unwrap();
    assert_eq!(
        link_rows(&connection, "515151"),
        vec![
            ("lime".to_string(), "manual".to_string()),
            ("plum".to_string(), "extracted".to_string()),
        ]
    );

    let resent = parse_export(
            r#"[{"boothId":"515151","title":"見本のギミック","shop":"見本","description":"対応アバター\nライム_ver1.00 https://booth.pm/ja/items/1001\n基本仕様\n布です。"}]"#,
        )
        .unwrap();
    import_products(&mut connection, &resent).unwrap();
    let reimported = require_product(&connection, "515151").unwrap();
    assert!(reimported.category.is_none());
    assert!(!reimported.category_locked && !reimported.genre_locked);
    assert!(reimported.links_locked);
    assert!(!reimported.user_confirmed);
    assert!(reimported.needs_review);
    assert_eq!(
        link_rows(&connection, "515151"),
        vec![
            ("lime".to_string(), "manual".to_string()),
            ("plum".to_string(), "extracted".to_string()),
        ]
    );
    let detail = avatar_detail(&load_snapshot(&connection).unwrap(), "lime").unwrap();
    assert!(
        detail
            .unset_edition_items
            .iter()
            .any(|item| item.booth_id == "515151")
    );

    apply_extraction(
        &mut connection,
        "515151",
        &extraction(Category::Hair, &["ライム"], false, 0.99),
        0.6,
    )
    .unwrap();
    let reclassified = require_product(&connection, "515151").unwrap();
    assert_eq!(reclassified.category, Some(Category::Hair));
    assert!(!reclassified.category_locked && !reclassified.genre_locked);
    assert!(reclassified.links_locked);
    assert!(
        link_rows(&connection, "515151")
            .iter()
            .any(|(avatar, source)| avatar == "lime" && source == "manual")
    );

    let world = add_free_product(&mut connection, "https://booth.pm/ja/items/525252").unwrap();
    apply_extraction(
        &mut connection,
        &world,
        &extraction(Category::World, &[], false, 0.9),
        0.6,
    )
    .unwrap();
    assert!(
        attach_product_to_avatar(&mut connection, "lime", "525252")
            .unwrap_err()
            .contains("ワールド")
    );
    assert!(link_rows(&connection, "525252").is_empty());

    let body = add_free_product(&mut connection, "https://booth.pm/ja/items/535353").unwrap();
    apply_extraction(
        &mut connection,
        &body,
        &extraction(Category::Avatar, &["ライム"], false, 0.9),
        0.6,
    )
    .unwrap();
    assert!(
        attach_product_to_avatar(&mut connection, "plum", "535353")
            .unwrap_err()
            .contains("アバター本体")
    );
    assert!(
        !link_rows(&connection, "535353")
            .iter()
            .any(|(avatar, _)| avatar == "plum")
    );

    let hidden = add_free_product(&mut connection, "https://booth.pm/ja/items/545454").unwrap();
    set_product_excluded(&mut connection, &hidden, true).unwrap();
    assert!(
        attach_product_to_avatar(&mut connection, "lime", "545454")
            .unwrap_err()
            .contains("除外")
    );
    assert!(attach_product_to_avatar(&mut connection, "missing", "515151").is_err());
    assert!(attach_product_to_avatar(&mut connection, "lime", "999999").is_err());
}

#[test]
fn classification_feedback_exports_targets_and_records_manual_moves() {
    let mut connection = open_memory().unwrap();
    let head = "用途の説明がありません。";
    let description = format!("{head}{}末尾マーカー", "あ".repeat(450));
    let parsed = parse_export(&format!(
            r#"[
                {{"boothId":"8800201","title":"分類できなかった小物","shop":"サンプル工房","description":{description}}},
                {{"boothId":"8800203","title":"手動でその他のままにした小物","shop":"サンプル工房","description":"小物のまま使っています。"}},
                {{"boothId":"8800204","title":"まだ分類していない小物","shop":"サンプル工房","description":"説明は短いです。"}},
                {{"boothId":"8800205","title":"普通の衣装","shop":"サンプル工房","description":"衣装です。"}},
                {{"boothId":"8800299","title":"除外した小物","shop":"サンプル工房","description":"管理対象から外しました。"}}
            ]"#,
            description = serde_json::to_string(&description).unwrap(),
        ))
        .unwrap();
    import_products(&mut connection, &parsed).unwrap();
    connection
            .execute(
                "UPDATE products SET category = 'other', genre = 'shader', category_locked = 0, genre_locked = 0,
                 needs_review = 0, thumbnail_url = 'https://pximg.net/secret-thumb', thumbnail_path = 'C:\\Secret\\thumb.png'
                 WHERE booth_id = '8800201'",
                [],
            )
            .unwrap();
    connection
            .execute(
                "UPDATE products SET category = 'other', genre = 'other', category_locked = 1, genre_locked = 1,
                 needs_review = 0 WHERE booth_id = '8800203'",
                [],
            )
            .unwrap();
    connection
            .execute(
                "UPDATE products SET category = 'outfit', genre = 'outfit', category_locked = 0, genre_locked = 0,
                 needs_review = 0 WHERE booth_id = '8800205'",
                [],
            )
            .unwrap();
    connection
            .execute(
                "UPDATE products SET category = 'other', genre = 'other', needs_review = 1 WHERE booth_id = '8800299'",
                [],
            )
            .unwrap();
    connection
            .execute(
                "INSERT INTO local_files (path, filename, linked_product_id, scanned_at)
                 VALUES ('/home/secret/library/file.unitypackage', 'file.unitypackage', '8800201', 't')",
                [],
            )
            .unwrap();
    set_setting(&connection, "library_folder", "/home/secret/library").unwrap();
    set_setting(&connection, "extension_token", "SECRETTOKEN").unwrap();
    set_product_excluded(&mut connection, "8800299", true).unwrap();

    assert!(classification_feedback_json(&connection, "avatars").is_err());

    let before = move_product_genre(&mut connection, "8800204", "asset", "hair").unwrap();
    assert_eq!(before.category, None);
    move_product_genre(&mut connection, "8800204", "asset", "hair").unwrap();
    let pinned = unpin_product_classification(&mut connection, "8800204").unwrap();
    assert_eq!(pinned.category.as_deref(), Some("hair"));
    restore_product_classification(&mut connection, &before).unwrap();
    let same = classification_snapshot(&require_product(&connection, "8800204").unwrap());
    restore_product_classification(&mut connection, &same).unwrap();

    let moves: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM classification_moves WHERE booth_id = '8800204'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(moves, 3);

    let both: serde_json::Value =
        serde_json::from_str(&classification_feedback_json(&connection, "both").unwrap()).unwrap();
    assert_eq!(both["format"], "vrclibman.classification-feedback");
    assert_eq!(both["version"], 1);
    assert_eq!(both["scope"], "both");
    assert_eq!(both["descriptionHeadChars"], 400);
    assert!(both["purpose"].as_str().unwrap().contains("トークン"));
    let ids: Vec<&str> = both["products"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["boothId"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["8800201", "8800203", "8800204"]);
    let long = both["products"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["boothId"] == "8800201")
        .unwrap();
    assert_eq!(
        long["descriptionHead"].as_str().unwrap().chars().count(),
        400
    );
    assert!(
        !long["descriptionHead"]
            .as_str()
            .unwrap()
            .contains("末尾マーカー")
    );
    assert_eq!(long["classificationStatus"], "classified");
    assert_eq!(long["category"], "other");
    assert_eq!(long["genre"], "shader");
    assert_eq!(long["shop"], "サンプル工房");
    let pinned_other = both["products"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["boothId"] == "8800203")
        .unwrap();
    assert_eq!(pinned_other["classificationStatus"], "manual");
    let open = both["products"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["boothId"] == "8800204")
        .unwrap();
    assert_eq!(open["classificationStatus"], "unclassified");
    assert!(open["category"].is_null());
    let actions: Vec<&str> = both["corrections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["action"].as_str().unwrap())
        .collect();
    assert_eq!(actions, vec!["move", "unpin", "restore"]);
    let correction = &both["corrections"][0];
    assert_eq!(correction["boothId"], "8800204");
    assert!(correction["from"]["category"].is_null());
    assert_eq!(correction["to"]["category"], "hair");
    assert_eq!(correction["to"]["genre"], "hair");
    assert!(both["corrections"][1]["to"].is_null());
    assert!(correction["at"].as_str().unwrap().contains('T'));
    let manual: Vec<&str> = both["manualClassifications"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["boothId"].as_str().unwrap())
        .collect();
    assert_eq!(manual, vec!["8800203"]);

    let other: serde_json::Value =
        serde_json::from_str(&classification_feedback_json(&connection, "other").unwrap()).unwrap();
    let other_ids: Vec<&str> = other["products"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["boothId"].as_str().unwrap())
        .collect();
    assert_eq!(other_ids, vec!["8800201", "8800203"]);
    assert_eq!(other["manualClassifications"].as_array().unwrap().len(), 1);
    assert_eq!(other["corrections"].as_array().unwrap().len(), 3);

    let unclassified: serde_json::Value =
        serde_json::from_str(&classification_feedback_json(&connection, "unclassified").unwrap())
            .unwrap();
    let unclassified_ids: Vec<&str> = unclassified["products"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["boothId"].as_str().unwrap())
        .collect();
    assert_eq!(unclassified_ids, vec!["8800204"]);

    let raw = classification_feedback_json(&connection, "both").unwrap();
    for secret in [
        "booth.pm",
        "pximg",
        "SECRETTOKEN",
        "/home/secret",
        "unitypackage",
        "thumbnail",
        "8800205",
        "8800299",
        "普通の衣装",
        "除外した小物",
        "末尾マーカー",
    ] {
        assert!(!raw.contains(secret), "{secret}");
    }
    fn keys(value: &serde_json::Value, found: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, child) in map {
                    found.push(key.clone());
                    keys(child, found);
                }
            }
            serde_json::Value::Array(items) => {
                for child in items {
                    keys(child, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    keys(&both, &mut found);
    for key in [
        "url",
        "thumbnailUrl",
        "thumbnailPath",
        "path",
        "token",
        "localPath",
        "filename",
    ] {
        assert!(!found.iter().any(|item| item == key), "{key}");
    }

    let dir = std::env::temp_dir().join(format!(
        "vrclibman-feedback-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let destination = dir.join("vrclibman-classification-feedback.json");
    save_classification_feedback(&connection, "both", &destination).unwrap();
    let saved = std::fs::read_to_string(&destination).unwrap();
    assert!(!saved.contains(&dir.display().to_string()));
    assert!(saved.contains("8800201"));
    assert!(save_classification_feedback(&connection, "both", &dir.join("notes.txt")).is_err());
    assert!(save_classification_feedback(&connection, "nope", &destination).is_err());
    assert!(
        save_classification_feedback(&connection, "both", &dir.join("missing").join("out.json"))
            .is_err()
    );
    #[cfg(unix)]
    {
        let secret = dir.join("secret.json");
        std::fs::write(&secret, b"keep").unwrap();
        let link = dir.join("link.json");
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        assert!(save_classification_feedback(&connection, "both", &link).is_err());
        assert_eq!(std::fs::read(&secret).unwrap(), b"keep");
    }
    let _ = std::fs::remove_dir_all(&dir);

    move_product_genre(&mut connection, "8800201", "asset", "accessory").unwrap();
    set_product_excluded(&mut connection, "8800201", true).unwrap();
    let hidden = classification_feedback_json(&connection, "both").unwrap();
    assert!(!hidden.contains("8800201"), "excluded product");
    assert!(!hidden.contains("分類できなかった小物"));
    let still: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM classification_moves WHERE booth_id = '8800201'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(still >= 1);
}

#[test]
fn title_keywords_override_a_vague_model_and_leave_pins() {
    let mut connection = open_memory().unwrap();
    insert_dummy_product(&connection, "7702001", "サンプルこたつセット", "");
    apply_extraction(
        &mut connection,
        "7702001",
        &extraction(Category::Gimmick, &[], false, 0.9),
        0.6,
    )
    .unwrap();
    let kotatsu = require_product(&connection, "7702001").unwrap();
    assert_eq!(kotatsu.category, Some(Category::World));
    assert_eq!(kotatsu.genre.as_deref(), Some("furniture_props"));
    assert!(!kotatsu.category_locked && !kotatsu.genre_locked);

    insert_dummy_product(&connection, "7702002", "サンプル Fullpack", "");
    apply_extraction(
        &mut connection,
        "7702002",
        &extraction(Category::Other, &[], true, 0.4),
        0.6,
    )
    .unwrap();
    let pack = require_product(&connection, "7702002").unwrap();
    assert_eq!(pack.category, Some(Category::Outfit));
    assert_eq!(pack.genre.as_deref(), Some("outfit"));

    insert_dummy_product(&connection, "7702003", "サンプル磁気ディスク", "");
    apply_extraction(
        &mut connection,
        "7702003",
        &extraction(Category::Gimmick, &[], false, 0.9),
        0.6,
    )
    .unwrap();
    let disk = require_product(&connection, "7702003").unwrap();
    assert_eq!(disk.category, Some(Category::Accessory));
    assert_eq!(disk.genre.as_deref(), Some("accessory"));

    move_product_genre(&mut connection, "7702001", "asset", "gimmick").unwrap();
    apply_extraction(
        &mut connection,
        "7702001",
        &extraction(Category::Outfit, &[], false, 0.99),
        0.6,
    )
    .unwrap();
    let pinned = require_product(&connection, "7702001").unwrap();
    assert_eq!(pinned.category, Some(Category::Gimmick));
    assert_eq!(pinned.genre.as_deref(), Some("gimmick"));
    assert!(pinned.category_locked && pinned.genre_locked);
    assert!(
        !products_for_extract(&connection, ExtractTarget::Stale)
            .unwrap()
            .iter()
            .any(|job| job.booth_id == "7702001")
    );

    connection
        .execute(
            "UPDATE products SET classifier_version = 2 WHERE booth_id = '7702002'",
            [],
        )
        .unwrap();
    assert!(
        products_for_extract(&connection, ExtractTarget::Stale)
            .unwrap()
            .iter()
            .any(|job| job.booth_id == "7702002")
    );
}

fn link_rows(connection: &Connection, booth_id: &str) -> Vec<(String, String)> {
    let mut statement = connection
            .prepare("SELECT avatar_id, source FROM product_avatars WHERE product_id = ?1 ORDER BY avatar_id")
            .unwrap();
    statement
        .query_map(params![booth_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn base_body_of(connection: &Connection, id: &str) -> Option<String> {
    connection
        .query_row(
            "SELECT base_body FROM avatars WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .unwrap()
}

fn avatars_named(connection: &Connection, name: &str) -> Vec<String> {
    let key = crate::normalize::normalize_label(name);
    load_avatars(connection)
        .unwrap()
        .into_iter()
        .filter(|avatar| {
            std::iter::once(&avatar.display_name)
                .chain(avatar.aliases.iter())
                .any(|label| crate::normalize::normalize_label(label) == key)
        })
        .map(|avatar| avatar.id)
        .collect()
}

#[test]
fn seed_base_bodies_have_sources_and_unique_members() {
    let seed = seed_file().unwrap();
    let bodies: HashSet<&str> = seed
        .base_bodies
        .iter()
        .map(|body| body.id.as_str())
        .collect();
    assert_eq!(bodies.len(), seed.base_bodies.len());
    for body in &seed.base_bodies {
        assert!(valid_id(&body.id), "{}", body.id);
        assert!(body.revision <= seed.revision, "{}", body.id);
        assert!(body.note.chars().count() <= 400, "{}", body.id);
        assert!(
            body.note.contains("https://booth.pm/"),
            "{} の note に出典が無い",
            body.id
        );
        let members = seed
            .avatars
            .iter()
            .filter(|avatar| avatar.base_body.as_deref() == Some(body.id.as_str()))
            .count();
        assert!(members >= 2, "{} は2体以上で組む", body.id);
    }
    let mut keys: HashMap<String, &str> = HashMap::new();
    let mut booths: HashSet<&str> = HashSet::new();
    for avatar in &seed.avatars {
        if let Some(body) = avatar.base_body.as_deref() {
            assert!(bodies.contains(body), "{} の素体 {body} が無い", avatar.id);
            assert!(
                avatar.booth_id.is_some(),
                "{} に Booth ID が無い",
                avatar.id
            );
        }
        if let Some(booth) = avatar.booth_id.as_deref() {
            assert!(booths.insert(booth), "Booth ID {booth} が重複");
        }
        for name in std::iter::once(&avatar.display_name).chain(avatar.aliases.iter()) {
            let key = crate::normalize::normalize_label(name);
            if let Some(other) = keys.insert(key.clone(), &avatar.id) {
                assert_eq!(
                    other, avatar.id,
                    "名前 {name} が {other} と {} に当たる",
                    avatar.id
                );
            }
        }
    }
    let komado: Vec<&str> = seed
        .avatars
        .iter()
        .filter(|avatar| avatar.base_body.as_deref() == Some("komado"))
        .map(|avatar| avatar.id.as_str())
        .collect();
    assert_eq!(
        komado,
        vec!["lime", "plum", "chiffon", "chocolat", "mf-8962830"]
    );
    for id in ["manuka", "shinano"] {
        let avatar = seed.avatars.iter().find(|avatar| avatar.id == id).unwrap();
        assert_eq!(avatar.base_body, None);
    }
    assert!(
        seed.avatars
            .iter()
            .all(|avatar| avatar.aliases.iter().all(|alias| alias != "Chocolate"))
    );
}

#[test]
fn fresh_library_groups_catalog_avatars_by_seed_base_body() {
    let connection = open_memory().unwrap();
    for name in [
        "まめひなた",
        "キプフェル",
        "ルキフェル",
        "ライム",
        "リーファ",
    ] {
        assert_eq!(avatars_named(&connection, name).len(), 1, "{name}");
    }
    for avatar in seed_file().unwrap().avatars {
        let Some(booth_id) = avatar.booth_id.as_deref() else {
            continue;
        };
        assert_eq!(
            avatar_id_for_booth(&connection, booth_id)
                .unwrap()
                .as_deref(),
            Some(avatar.id.as_str()),
            "{booth_id}"
        );
        assert_eq!(
            base_body_of(&connection, &avatar.id),
            avatar.base_body,
            "{}",
            avatar.id
        );
    }
    assert_eq!(
        base_body_of(&connection, "mf-4340548").as_deref(),
        Some("mamefriends")
    );
    assert_eq!(
        base_body_of(&connection, "mf-5813187").as_deref(),
        Some("mamefriends")
    );
    assert_eq!(
        base_body_of(&connection, "mf-2040716").as_deref(),
        Some("ym-body-women-m")
    );
    assert_eq!(
        base_body_of(&connection, "mf-2040691").as_deref(),
        Some("ym-body-women-s")
    );
    assert_eq!(base_body_of(&connection, "lime").as_deref(), Some("komado"));
    assert_eq!(base_body_of(&connection, "manuka"), None);
    assert_eq!(base_body_of(&connection, "shinano"), None);
    // 同じショップでも共通素体と書かれていないカリンは結ばない。
    let karin = avatars_named(&connection, "カリン");
    assert_eq!(karin.len(), 1);
    assert_eq!(base_body_of(&connection, &karin[0]), None);
    assert_eq!(
        avatar_id_for_booth(&connection, "4340548")
            .unwrap()
            .as_deref(),
        Some("mf-4340548")
    );
    assert_eq!(
        avatar_id_for_booth(&connection, "4876459")
            .unwrap()
            .as_deref(),
        Some("lime")
    );
    assert_eq!(
        setting(&connection, "seed_revision").unwrap(),
        Some(seed_file().unwrap().revision.to_string())
    );
}

#[test]
fn existing_library_receives_new_base_bodies_once() {
    let mut connection = open_memory().unwrap();
    // v0.1.19 までのライブラリを再現する: 素体はこまどだけ、カタログ由来のアバターは素体なし。
    connection
            .execute_batch(
                "UPDATE avatars SET base_body = NULL WHERE base_body IS NOT NULL AND base_body <> 'komado';
                 DELETE FROM base_bodies WHERE id <> 'komado';
                 UPDATE avatars SET base_body = NULL WHERE id = 'chiffon';
                 DELETE FROM settings WHERE key = 'seed_revision';",
            )
            .unwrap();
    let before = load_avatars(&connection).unwrap().len();
    let inserted = ensure_ready(&connection).unwrap();
    assert_eq!(inserted, 0);
    assert_eq!(load_avatars(&connection).unwrap().len(), before);
    assert_eq!(
        base_body_of(&connection, "mf-4340548").as_deref(),
        Some("mamefriends")
    );
    assert_eq!(
        base_body_of(&connection, "mf-7163384").as_deref(),
        Some("chiwado-palfy")
    );
    // こまどは以前からある素体なので、ユーザーが外したシフォンは戻さない。
    assert_eq!(base_body_of(&connection, "chiffon"), None);

    // 消した素体は次の起動で戻らない。
    delete_base_body(&connection, "mamefriends").unwrap();
    ensure_ready(&connection).unwrap();
    assert_eq!(base_body_of(&connection, "mf-4340548"), None);
    // 「初期データを追加」は消した素体と、その素体の空欄を戻す。
    let added = restore_seed(&mut connection).unwrap();
    assert!(added >= 15, "{added}");
    assert_eq!(
        base_body_of(&connection, "mf-4340548").as_deref(),
        Some("mamefriends")
    );
    assert_eq!(base_body_of(&connection, "chiffon"), None);
    assert_eq!(avatars_named(&connection, "まめひなた").len(), 1);
}

#[test]
fn existing_library_receives_an_avatar_added_to_an_older_group_once() {
    let connection = open_memory().unwrap();
    // v0.1.19 にはソルトが無い。こまどは以前からある素体。
    let forget_salt = "DELETE FROM avatar_sources WHERE booth_id = '8962830';
             DELETE FROM avatars WHERE id = 'mf-8962830';
             UPDATE settings SET value = '1' WHERE key = 'seed_revision';";
    connection.execute_batch(forget_salt).unwrap();
    assert_eq!(ensure_ready(&connection).unwrap(), 1);
    assert_eq!(
        base_body_of(&connection, "mf-8962830").as_deref(),
        Some("komado")
    );
    assert_eq!(
        avatar_id_for_booth(&connection, "8962830")
            .unwrap()
            .as_deref(),
        Some("mf-8962830")
    );
    assert_eq!(
        avatars_named(&connection, "Salt"),
        vec!["mf-8962830".to_string()]
    );

    // 二度目の起動では足さない。
    assert_eq!(ensure_ready(&connection).unwrap(), 0);
    assert_eq!(
        avatars_named(&connection, "ソルト"),
        vec!["mf-8962830".to_string()]
    );

    // こまどを消してあれば、ソルトは素体なしで足す。
    connection.execute_batch(forget_salt).unwrap();
    delete_base_body(&connection, "komado").unwrap();
    assert_eq!(ensure_ready(&connection).unwrap(), 1);
    assert_eq!(base_body_of(&connection, "mf-8962830"), None);
    assert_eq!(base_body_of(&connection, "lime"), None);

    // 自分で作った同名のアバターには、残っているこまどを埋める。
    let connection = open_memory().unwrap();
    connection
            .execute_batch(
                "DELETE FROM avatar_sources WHERE booth_id = '8962830';
                 UPDATE avatars SET id = 'my-salt', is_builtin = 0, base_body = NULL WHERE id = 'mf-8962830';
                 UPDATE settings SET value = '1' WHERE key = 'seed_revision';",
            )
            .unwrap();
    assert_eq!(ensure_ready(&connection).unwrap(), 0);
    assert_eq!(
        base_body_of(&connection, "my-salt").as_deref(),
        Some("komado")
    );
    assert_eq!(
        avatars_named(&connection, "ソルト"),
        vec!["my-salt".to_string()]
    );
}

#[test]
fn seed_upgrade_reuses_an_unbound_avatar_with_the_same_name() {
    let connection = open_memory().unwrap();
    connection
            .execute_batch(
                "DELETE FROM avatar_sources WHERE booth_id = '5986971';
                 UPDATE avatars SET id = 'nemesis-user', is_builtin = 0, base_body = NULL WHERE id = 'mf-5986971';
                 UPDATE avatars SET base_body = NULL WHERE id = 'mf-6663666';
                 DELETE FROM base_bodies WHERE id = 'komainu-nemesis';
                 UPDATE settings SET value = '1' WHERE key = 'seed_revision';",
            )
            .unwrap();
    assert_eq!(ensure_ready(&connection).unwrap(), 0);
    assert_eq!(
        avatars_named(&connection, "ネメシス"),
        vec!["nemesis-user".to_string()]
    );
    assert_eq!(
        base_body_of(&connection, "nemesis-user").as_deref(),
        Some("komainu-nemesis")
    );
    assert_eq!(
        base_body_of(&connection, "mf-6663666").as_deref(),
        Some("komainu-nemesis")
    );
    assert_eq!(
        avatar_id_for_booth(&connection, "5986971")
            .unwrap()
            .as_deref(),
        Some("nemesis-user")
    );
}

#[test]
fn seed_upgrade_skips_a_same_name_avatar_bound_to_another_item() {
    let connection = open_memory().unwrap();
    connection
            .execute_batch(
                "DELETE FROM avatar_sources WHERE booth_id = '5986971';
                 UPDATE avatars SET id = 'other-nemesis', is_builtin = 0, base_body = NULL WHERE id = 'mf-5986971';
                 INSERT INTO avatar_sources (booth_id, avatar_id) VALUES ('8800999', 'other-nemesis');
                 DELETE FROM base_bodies WHERE id = 'komainu-nemesis';
                 UPDATE settings SET value = '1' WHERE key = 'seed_revision';",
            )
            .unwrap();
    assert_eq!(ensure_ready(&connection).unwrap(), 1);
    assert_eq!(base_body_of(&connection, "other-nemesis"), None);
    assert_eq!(
        base_body_of(&connection, "mf-5986971").as_deref(),
        Some("komainu-nemesis")
    );
}

#[test]
fn outfits_for_one_mamefriends_avatar_show_on_another() {
    let mut connection = open_memory().unwrap();
    let json = r#"{
            "products": [
                {"boothId":"4340548","title":"まめひなた Mamehinata / オリジナル3Dモデル","shop":"もち山金魚","description":"このアバターはまめふれんず共通素体を使用しています。"},
                {"boothId":"5813187","title":"キプフェル Kipfel / オリジナル3Dモデル","shop":"もち山金魚","description":"このアバターはまめふれんず共通素体を使用しています。"},
                {"boothId":"8800501","title":"【まめひなた対応】架空のワンピース","shop":"s","description":"衣装です。"}
            ]
        }"#;
    import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
    apply_extraction(
        &mut connection,
        "8800501",
        &extraction(Category::Outfit, &["まめひなた"], false, 0.9),
        0.6,
    )
    .unwrap();
    // This compatibility fixture owns all editions; metadata alone is not ownership.
    connection.execute_batch("INSERT OR REPLACE INTO purchased_editions SELECT booth_id, '{\"mode\":\"all\",\"avatarIds\":[]}' FROM products").unwrap();
    let snapshot = load_snapshot(&mut connection).unwrap();
    let kipfel = avatar_detail(&snapshot, "mf-5813187").unwrap();
    assert!(
        kipfel
            .siblings
            .iter()
            .any(|sibling| sibling.id == "mf-4340548")
    );
    assert_eq!(
        section_items(&kipfel, "outfit"),
        vec![("8800501".to_string(), true)]
    );
    let hinata = avatar_detail(&snapshot, "mf-4340548").unwrap();
    assert_eq!(
        section_items(&hinata, "outfit"),
        vec![("8800501".to_string(), false)]
    );
}

fn section_items(detail: &crate::view::AvatarDetail, category: &str) -> Vec<(String, bool)> {
    detail
        .sections
        .iter()
        .filter(|section| section.category == category)
        .flat_map(|section| {
            section
                .items
                .iter()
                .map(|item| (item.booth_id.clone(), item.via_base_body))
        })
        .collect()
}
