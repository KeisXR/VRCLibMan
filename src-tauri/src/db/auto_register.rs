//! Products waiting to be classified after the browser or the extension registered them.
use super::*;

// Sparse checkout metadata must not erase descriptions, links, or manual classification.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserAutoItem {
    pub booth_id: String,
    pub title: String,
}

pub(crate) fn browser_auto_register(
    connection: &mut Connection,
    product: &serde_json::Value,
) -> Result<Option<String>, String> {
    let saved = settings(connection)?;
    if !saved.builtin_browser || !saved.browser_auto_register {
        return Ok(None);
    }
    auto_register(connection, product, "browser_auto_pending")
}

/// 拡張が本人の開いた購入完了ページから送った商品。内蔵ブラウザの設定によらず分類待ちに入れる
/// （拡張側の「購入完了ページを自動で登録する」で止められる）。
pub(crate) fn extension_auto_register(
    connection: &mut Connection,
    product: &serde_json::Value,
) -> Result<Option<String>, String> {
    auto_register(connection, product, "extension_import_pending")
}

pub(super) fn auto_register(
    connection: &mut Connection,
    product: &serde_json::Value,
    pending_prefix: &str,
) -> Result<Option<String>, String> {
    let id = product
        .get("boothId")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !crate::library_files::valid_booth_id(id) {
        return Err("商品IDを特定できません".into());
    }
    if product_excluded(connection, id)? {
        return Ok(None);
    }
    let exists = connection
        .query_row(
            "SELECT COUNT(*) FROM products WHERE booth_id = ?1",
            [id],
            |r| r.get::<_, i64>(0),
        )
        .sql()?
        != 0;
    if !exists {
        import_export_json(
            connection,
            &crate::browser::export_json(serde_json::json!([product])),
        )?;
    }
    let seen = format!("browser_auto_seen:{id}");
    if setting(connection, &seen)?.is_none() {
        let p = require_product(connection, id)?;
        if !p.user_confirmed && !classification_pinned(&p) {
            set_setting(connection, &format!("{pending_prefix}:{id}"), "1")?;
            connection
                .execute(
                    "UPDATE products SET needs_review = 1 WHERE booth_id = ?1",
                    [id],
                )
                .sql()?;
            crate::sync_meta::touch_product(connection, id)?;
        }
        set_setting(connection, &seen, "1")?;
    }
    Ok(Some(id.to_string()))
}

pub(crate) fn browser_auto_pending(
    connection: &Connection,
) -> Result<Vec<BrowserAutoItem>, String> {
    let saved = settings(connection)?;
    let browser_enabled = saved.builtin_browser && saved.browser_auto_register;
    let mut query = connection.prepare(
        "SELECT booth_id, title FROM products p WHERE EXISTS
         (SELECT 1 FROM settings s WHERE (s.key = 'extension_import_pending:' || p.booth_id OR (?1 AND s.key = 'browser_auto_pending:' || p.booth_id)) AND s.value = '1') AND user_confirmed = 0 AND NOT (category_locked = 1 AND genre_locked = 1) ORDER BY booth_id"
    ).sql()?;
    let rows = query
        .query_map([browser_enabled], |r| {
            Ok(BrowserAutoItem {
                booth_id: r.get(0)?,
                title: r.get(1)?,
            })
        })
        .sql()?;
    let mut items = vec![];
    for row in rows {
        let item = row.sql()?;
        if !product_excluded(connection, &item.booth_id)? {
            items.push(item);
        }
    }
    Ok(items)
}

pub(crate) fn browser_auto_finish(connection: &mut Connection, id: &str) -> Result<(), String> {
    let p = require_product(connection, id)?;
    if !p.user_confirmed && !classification_pinned(&p) {
        return Err("分類先を選んでから確定してください".into());
    }
    connection
        .execute(
            "DELETE FROM settings WHERE key IN (?1, ?2)",
            [
                format!("browser_auto_pending:{id}"),
                format!("extension_import_pending:{id}"),
            ],
        )
        .sql()?;
    Ok(())
}

/// Explicit extension download import; independent of native browser preferences.
pub(crate) fn extension_import_product(
    connection: &mut Connection,
    text: &str,
) -> Result<ImportReport, String> {
    let product: serde_json::Value =
        serde_json::from_str(text).map_err(|_| "商品情報を読めません".to_string())?;
    let id = product
        .get("boothId")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !crate::library_files::valid_booth_id(id) {
        return Err("商品IDを特定できません".into());
    }
    if product_excluded(connection, id)? {
        return Err("除外中の商品はインポートできません".into());
    }
    let report = import_export_json(
        connection,
        &crate::browser::export_json(serde_json::json!([product])),
    )?;
    let p = require_product(connection, id)?;
    if !p.user_confirmed && !classification_pinned(&p) {
        set_setting(connection, &format!("extension_import_pending:{id}"), "1")?;
    }
    Ok(report)
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AvatarSuggestions {
    pub avatar_ids: Vec<String>,
    pub unresolved_names: Vec<String>,
}

/// Read only: candidates come from explicit page evidence and the editable dictionary.
pub(crate) fn product_avatar_suggestions(
    connection: &Connection,
    id: &str,
) -> Result<AvatarSuggestions, String> {
    let product = require_product(connection, id)?;
    let avatars = load_avatars(connection)?;
    let names = crate::compat::names_for_extraction(
        &product.title,
        &product.description_snippet,
        &[],
        &avatars,
    );
    let mut result = AvatarSuggestions {
        avatar_ids: vec![],
        unresolved_names: vec![],
    };
    for name in names {
        if let Some(avatar) = crate::resolve::resolve_name(&name, &avatars) {
            if !result.avatar_ids.contains(&avatar.id) {
                result.avatar_ids.push(avatar.id.clone());
            }
        } else if !result.unresolved_names.contains(&name) {
            result.unresolved_names.push(name);
        }
    }
    Ok(result)
}

/// 「取得した商品の分類」で最初に選んでおく候補。保存はしない（確定は本人が押したときの手動分類）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenreSuggestion {
    /// `asset` / `world` / `avatar`。候補が無いときは None（本人が選ぶまで確定できない）。
    pub kind: Option<String>,
    /// `kind` が `asset` / `world` のときのジャンル。
    pub genre: Option<String>,
    /// 候補の出どころ。`classified`（自動の分類済み）、`booth`（BOOTH のカテゴリとタグ）、`rules`（タイトルと説明）。
    pub source: Option<String>,
}

/// 分類と同じ順（自動の分類の結果、BOOTH のカテゴリとタグ、タイトルと説明の規則）で候補を決める。
/// 規則で何も手がかりが無い「その他」は候補にしない。見本の「衣装」のような決め打ちの初期値を確定させないため。
pub(crate) fn product_genre_suggestion(
    connection: &Connection,
    id: &str,
) -> Result<GenreSuggestion, String> {
    let product = require_product(connection, id)?;
    let pick =
        |category: Category, genre: Option<String>, source: &str| -> Option<GenreSuggestion> {
            let kind = match category {
                Category::Avatar => {
                    return Some(GenreSuggestion {
                        kind: Some("avatar".into()),
                        genre: None,
                        source: Some(source.into()),
                    });
                }
                Category::World => "world",
                _ => "asset",
            };
            let genre = genre.filter(|value| {
                crate::genre::kind_genres(kind)
                    .is_some_and(|list| list.iter().any(|(key, _)| key == value))
            })?;
            Some(GenreSuggestion {
                kind: Some(kind.into()),
                genre: Some(genre),
                source: Some(source.into()),
            })
        };
    if product.extracted_at.is_some()
        && let Some(category) = product.category
        && let Some(found) = pick(category, product.genre.clone(), "classified")
    {
        return Ok(found);
    }
    let meta = booth_meta(connection, id)?;
    if let Some(mut settlement) = crate::booth_category::settle(meta.id, &meta.name, &meta.tags) {
        if crate::booth_category::is_coarse_tool(meta.id, &meta.name)
            && let Some((category, genre)) = crate::classifier::coarse_tool_override(
                &product.title,
                &product.description_snippet,
            )
        {
            settlement.category = category;
            settlement.genre = Some(genre);
        }
        let genre = settlement
            .genre
            .clone()
            .or_else(|| Some(crate::genre::validate(settlement.category, None)));
        if let Some(found) = pick(settlement.category, genre, "booth") {
            return Ok(found);
        }
    }
    if product.description_snippet != crate::model::UNAVAILABLE_DESCRIPTION {
        let extraction =
            crate::classifier::rule_extraction(&product.title, &product.description_snippet);
        let no_signal =
            extraction.category == Category::Other && extraction.genre.as_deref() == Some("other");
        if !no_signal && let Some(found) = pick(extraction.category, extraction.genre, "rules") {
            return Ok(found);
        }
    }
    Ok(GenreSuggestion {
        kind: None,
        genre: None,
        source: None,
    })
}

#[cfg(test)]
mod browser_auto_tests {
    use super::*;
    fn enabled_db() -> Connection {
        let c = open_memory().unwrap();
        set_setting(&c, "builtin_browser", "1").unwrap();
        c
    }
    fn product(id: &str) -> serde_json::Value {
        serde_json::json!({"boothId": id, "title": "見本の衣装", "description": "対応する衣装の見本"})
    }
    #[test]
    fn genre_suggestion_is_a_candidate_and_never_a_blind_default() {
        let mut c = open_memory().unwrap();
        let json = r#"{"products":[
            {"boothId":"1001","title":"【複数アバター対応】見本のワンピース","shop":"s","description":"衣装です"},
            {"boothId":"1002","title":"見本","shop":"s","description":""},
            {"boothId":"1003","title":"ワールド用 見本の家具セット","shop":"s","description":"ワールドに置ける家具"}
        ]}"#;
        import_products(&mut c, &crate::import_data::parse_export(json).unwrap()).unwrap();
        let outfit = product_genre_suggestion(&c, "1001").unwrap();
        assert_eq!(
            (
                outfit.kind.as_deref(),
                outfit.genre.as_deref(),
                outfit.source.as_deref()
            ),
            (Some("asset"), Some("outfit"), Some("rules"))
        );
        let blank = product_genre_suggestion(&c, "1002").unwrap();
        assert_eq!(
            blank,
            GenreSuggestion {
                kind: None,
                genre: None,
                source: None
            },
            "no evidence: the person chooses"
        );
        let world = product_genre_suggestion(&c, "1003").unwrap();
        assert_eq!(world.kind.as_deref(), Some("world"));
        // BOOTH のカテゴリが決まっていれば、それを先に使う。
        c.execute("UPDATE products SET booth_category_id = 209, booth_category_name = '3D衣装' WHERE booth_id = '1002'", []).unwrap();
        let booth = product_genre_suggestion(&c, "1002").unwrap();
        assert_eq!(
            (booth.kind.as_deref(), booth.source.as_deref()),
            (Some("asset"), Some("booth"))
        );
        // 読むだけで、分類も固定もしない。
        let product = require_product(&c, "1001").unwrap();
        assert!(product.category.is_none() && !product.category_locked && !product.genre_locked);
    }

    #[test]
    fn extension_import_prompts_without_builtin_browser_and_keeps_manual_edits() {
        let mut c = open_memory().unwrap();
        assert!(!settings(&c).unwrap().builtin_browser);
        let text = product("123456").to_string();
        extension_import_product(&mut c, &text).unwrap();
        extension_import_product(&mut c, &text).unwrap();
        assert_eq!(browser_auto_pending(&c).unwrap().len(), 1);
        assert!(browser_auto_finish(&mut c, "123456").is_err());
        move_product_genre_with_avatars(
            &mut c,
            "123456",
            "asset",
            "hair",
            Some(vec!["lime".into()]),
        )
        .unwrap();
        browser_auto_finish(&mut c, "123456").unwrap();
        extension_import_product(&mut c, &text).unwrap();
        assert!(browser_auto_pending(&c).unwrap().is_empty());
        assert_eq!(
            require_product(&c, "123456").unwrap().genre.as_deref(),
            Some("hair")
        );
        assert_eq!(link_ids(&c, "123456").unwrap(), vec!["lime".to_string()]);
    }
    #[test]
    fn page_avatar_extraction_is_read_only_and_does_not_expand_shared_bodies() {
        let mut c = open_memory().unwrap();
        extension_import_product(
            &mut c,
            &serde_json::json!({
                "boothId":"123456", "title":"【ライム対応】見本の衣装",
                "description":"対応アバター\nライム\nマヌカ\n架空ちゃん\n利用規約\nテスト用の文章"
            })
            .to_string(),
        )
        .unwrap();
        let before = link_ids(&c, "123456").unwrap();
        let result = product_avatar_suggestions(&c, "123456").unwrap();
        assert!(result.avatar_ids.contains(&"lime".to_string()));
        assert!(result.avatar_ids.contains(&"manuka".to_string()));
        assert!(!result.avatar_ids.contains(&"plum".to_string()));
        assert!(result.unresolved_names.contains(&"架空ちゃん".to_string()));
        assert_eq!(link_ids(&c, "123456").unwrap(), before);
    }
    #[test]
    fn invalid_avatar_does_not_partially_save_classification() {
        let mut c = open_memory().unwrap();
        extension_import_product(&mut c, &product("123456").to_string()).unwrap();
        let before = require_product(&c, "123456").unwrap();
        assert!(
            move_product_genre_with_avatars(
                &mut c,
                "123456",
                "asset",
                "hair",
                Some(vec!["missing-avatar".into()])
            )
            .is_err()
        );
        let after = require_product(&c, "123456").unwrap();
        assert_eq!(before.category, after.category);
        assert_eq!(before.genre, after.genre);
        assert_eq!(browser_auto_pending(&c).unwrap().len(), 1);
        set_product_excluded(&mut c, "123456", true).unwrap();
        assert!(extension_import_product(&mut c, &product("123456").to_string()).is_err());
    }
    #[test]
    fn default_enabled_and_off_prevents_registration() {
        let mut c = enabled_db();
        assert!(settings(&c).unwrap().browser_auto_register);
        update_settings(
            &mut c,
            &SettingsPatch {
                browser_auto_register: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            browser_auto_register(&mut c, &product("123456")).unwrap(),
            None
        );
        assert!(require_product(&c, "123456").is_err());
        update_settings(
            &mut c,
            &SettingsPatch {
                browser_auto_register: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            browser_auto_register(&mut c, &product("123456")).unwrap(),
            Some("123456".into())
        );
        assert_eq!(browser_auto_pending(&c).unwrap().len(), 1);
    }
    #[test]
    fn duplicate_checkout_and_redownload_keep_one_pending_item() {
        let mut c = enabled_db();
        browser_auto_register(&mut c, &product("123456")).unwrap();
        browser_auto_register(&mut c, &product("123456")).unwrap();
        assert_eq!(browser_auto_pending(&c).unwrap().len(), 1);
        assert!(browser_auto_finish(&mut c, "123456").is_err());
        move_product_genre(&mut c, "123456", "asset", "outfit").unwrap();
        browser_auto_finish(&mut c, "123456").unwrap();
        browser_auto_register(&mut c, &product("123456")).unwrap();
        assert!(browser_auto_pending(&c).unwrap().is_empty());
    }
    #[test]
    fn sparse_metadata_preserves_existing_details_and_manual_classification() {
        let mut c = enabled_db();
        import_export_json(
            &mut c,
            &crate::browser::export_json(serde_json::json!([product("123456")])),
        )
        .unwrap();
        move_product_genre(&mut c, "123456", "asset", "hair").unwrap();
        browser_auto_register(
            &mut c,
            &serde_json::json!({"boothId":"123456", "title":"短い名前"}),
        )
        .unwrap();
        let p = require_product(&c, "123456").unwrap();
        assert_eq!(p.description_snippet, "対応する衣装の見本");
        assert_eq!(p.genre.as_deref(), Some("hair"));
        assert!(browser_auto_pending(&c).unwrap().is_empty());
    }
    #[test]
    fn pending_survives_reopening_and_disabled_settings_hide_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.sqlite");
        {
            let mut c = open(&path).unwrap();
            set_setting(&c, "builtin_browser", "1").unwrap();
            browser_auto_register(&mut c, &product("123456")).unwrap();
        }
        let c = open(&path).unwrap();
        assert_eq!(browser_auto_pending(&c).unwrap().len(), 1);
        set_setting(&c, "browser_auto_register", "0").unwrap();
        assert!(browser_auto_pending(&c).unwrap().is_empty());
        set_setting(&c, "browser_auto_register", "1").unwrap();
        assert_eq!(browser_auto_pending(&c).unwrap().len(), 1);
    }
}
