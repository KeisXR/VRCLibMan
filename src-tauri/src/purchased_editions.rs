//! Purchased editions are independent of product compatibility and body ownership.
use crate::{db, model::Avatar};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const TABLES: &str = "CREATE TABLE IF NOT EXISTS purchased_editions (product_id TEXT PRIMARY KEY REFERENCES products(booth_id) ON DELETE CASCADE, choice_json TEXT NOT NULL);";
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub mode: String,
    #[serde(default)]
    pub avatar_ids: Vec<String>,
}
impl Default for Choice {
    fn default() -> Self {
        Self {
            mode: "auto".into(),
            avatar_ids: vec![],
        }
    }
}
impl Choice {
    pub fn unknown() -> Self {
        Self {
            mode: "unknown".into(),
            avatar_ids: vec![],
        }
    }
}
pub struct Catalog {
    avatars: Vec<Avatar>,
    links: HashMap<String, Vec<String>>,
    names: HashMap<String, Vec<String>>,
    pub manual: HashMap<String, Choice>,
}
fn error(e: impl std::fmt::Display) -> String {
    format!("購入した対応版を読み込めません：{e}")
}
pub fn variation_of(name: &str, title: &str) -> Option<String> {
    let rest = name.strip_prefix(title.trim())?.trim();
    let inner = rest.strip_prefix('(')?.strip_suffix(')')?.trim();
    (!title.trim().is_empty() && !inner.is_empty()).then(|| inner.to_string())
}
impl Catalog {
    pub fn load(c: &Connection) -> Result<Self, String> {
        let mut result = Self {
            avatars: db::load_avatars(c)?,
            links: HashMap::new(),
            names: HashMap::new(),
            manual: HashMap::new(),
        };
        let mut stmt = c
            .prepare("SELECT product_id, avatar_id FROM product_avatars ORDER BY avatar_id")
            .map_err(error)?;
        for row in stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(error)?
        {
            let (id, avatar) = row.map_err(error)?;
            result.links.entry(id).or_default().push(avatar);
        }
        let mut stmt = c
            .prepare("SELECT booth_id, name FROM product_variations WHERE owned=1")
            .map_err(error)?;
        for row in stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(error)?
        {
            let (id, name) = row.map_err(error)?;
            result.names.entry(id).or_default().push(name);
        }
        let mut stmt = c.prepare("SELECT i.booth_id, i.name, p.title FROM purchase_items i JOIN purchase_orders o ON o.order_id=i.order_id JOIN products p ON p.booth_id=i.booth_id WHERE COALESCE(o.gift_override,o.gift_detected)=0").map_err(error)?;
        for row in stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(error)?
        {
            let (id, name, title) = row.map_err(error)?;
            if let Some(name) = variation_of(&name, &title) {
                result.names.entry(id).or_default().push(name);
            }
        }
        let mut stmt = c
            .prepare("SELECT product_id, choice_json FROM purchased_editions")
            .map_err(error)?;
        for row in stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(error)?
        {
            let (id, json) = row.map_err(error)?;
            result
                .manual
                .insert(id, serde_json::from_str(&json).map_err(error)?);
        }
        Ok(result)
    }
    pub fn scope(&self, id: &str, hint: Option<&str>) -> Choice {
        if let Some(choice) = self.manual.get(id) {
            return if choice.mode == "all" {
                Choice {
                    mode: "all".into(),
                    avatar_ids: self.links.get(id).cloned().unwrap_or_default(),
                }
            } else {
                choice.clone()
            };
        }
        let names = hint
            .map(|s| vec![s.to_string()])
            .unwrap_or_else(|| self.names.get(id).cloned().unwrap_or_default());
        self.infer(id, &names)
    }
    fn infer(&self, id: &str, names: &[String]) -> Choice {
        let mut ids = Vec::new();

        for name in names {
            let normalized = crate::normalize::nfkc(name)
                .to_lowercase()
                .replace([' ', '　', '-', '_'], "");
            if [
                "フルセット",
                "全対応版セット",
                "全アバターセット",
                "fullset",
                "allavatars",
            ]
            .contains(&normalized.as_str())
            {
                return Choice {
                    mode: "all".into(),
                    avatar_ids: self.links.get(id).cloned().unwrap_or_default(),
                };
            }
            if let Some(avatar) = crate::resolve::resolve_name(name, &self.avatars) {
                ids.push(avatar.id.clone());
            }
        }
        ids.sort();
        ids.dedup();
        // Unresolved options (e.g. colors) are not evidence of an avatar edition.
        if ids.is_empty() {
            Choice::unknown()
        } else {
            Choice {
                mode: "specific".into(),
                avatar_ids: ids,
            }
        }
    }
    pub fn scopes(&self) -> HashMap<String, Choice> {
        self.links
            .keys()
            .chain(self.names.keys())
            .chain(self.manual.keys())
            .map(|id| (id.clone(), self.scope(id, None)))
            .collect()
    }
}
pub fn save(c: &Connection, id: &str, choice: &Choice) -> Result<(), String> {
    let mut choice = choice.clone();
    choice.avatar_ids.sort();
    choice.avatar_ids.dedup();
    if !["auto", "all", "specific", "unknown"].contains(&choice.mode.as_str()) {
        return Err("購入した対応版の指定が不正です".into());
    }
    if choice.mode != "specific" && !choice.avatar_ids.is_empty() {
        return Err("アバターは個別の対応版で指定してください".into());
    }
    if choice.mode == "specific" && choice.avatar_ids.is_empty() {
        return Err("購入した対応版を選んでください".into());
    }
    for avatar in &choice.avatar_ids {
        if !c
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM avatars WHERE id=?1)",
                [avatar],
                |r| r.get::<_, bool>(0),
            )
            .map_err(error)?
        {
            return Err("対応版のアバターが見つかりません".into());
        }
    }
    if choice.mode == "auto" {
        c.execute("DELETE FROM purchased_editions WHERE product_id=?1", [id])
            .map_err(error)?;
    } else {
        c.execute("INSERT INTO purchased_editions VALUES (?1,?2) ON CONFLICT(product_id) DO UPDATE SET choice_json=excluded.choice_json", params![id, serde_json::to_string(&choice).map_err(error)?]).map_err(error)?;
    }
    Ok(())
}

/// Move persisted references together with the avatar's compatibility links and deletion.
pub(crate) fn merge_avatar(
    tx: &rusqlite::Transaction<'_>,
    from_id: &str,
    into_id: &str,
) -> Result<(), String> {
    let rows = {
        let mut statement = tx
            .prepare("SELECT product_id, choice_json FROM purchased_editions")
            .map_err(error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(error)?
    };
    // A purchase reference can exist without a current compatibility link to the source.
    for (product_id, json) in rows {
        let mut choice: Choice = serde_json::from_str(&json).map_err(error)?;
        if choice.mode != "specific" || !choice.avatar_ids.iter().any(|id| id == from_id) {
            continue;
        }
        for id in &mut choice.avatar_ids {
            if id == from_id {
                *id = into_id.to_string();
            }
        }
        // save validates the destination and removes duplicate editions after the replacement.
        save(tx, &product_id, &choice)?;
        crate::sync_meta::touch_product(tx, &product_id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Connection {
        let c = db::open_memory().unwrap();
        c.execute_batch("INSERT INTO products(booth_id,title,category,imported_at,updated_at) VALUES ('1234','衣装A','outfit','',''); INSERT INTO product_avatars VALUES ('1234','lime','manual'),('1234','plum','manual');").unwrap();
        c
    }
    #[test]
    fn compatibility_is_not_a_purchased_edition_and_library_needs_no_orders() {
        let c = fixture();
        assert_eq!(
            Catalog::load(&c).unwrap().scope("1234", None),
            Choice::unknown()
        );
        c.execute(
            "INSERT INTO product_variations VALUES ('1234','Lime',1)",
            [],
        )
        .unwrap();
        let catalog = Catalog::load(&c).unwrap();
        assert_eq!(catalog.scope("1234", None).avatar_ids, ["lime"]);
        assert_eq!(catalog.scope("1234", Some("Plum")).avatar_ids, ["plum"]);
        assert_eq!(catalog.scope("1234", Some("黒")), Choice::unknown());
        let s = db::load_snapshot(&c).unwrap();
        let lime = crate::view::avatar_detail(&s, "lime").unwrap();
        let plum = crate::view::avatar_detail(&s, "plum").unwrap();
        assert_eq!(
            lime.sections
                .iter()
                .flat_map(|s| &s.items)
                .filter(|i| i.booth_id == "1234")
                .count(),
            1
        );
        assert!(plum
            .sections
            .iter()
            .flat_map(|s| &s.items)
            .all(|i| i.booth_id != "1234"));
        assert!(!lime.owned);
        assert!(!plum.owned);
    }
    #[test]
    fn twenty_compatible_avatars_do_not_mean_twenty_purchased_editions() {
        let c = fixture();
        for i in 0..18 {
            let id = format!("edition-test-{i}");
            c.execute("INSERT INTO avatars(id,display_name) VALUES (?1,?1)", [&id])
                .unwrap();
            c.execute(
                "INSERT INTO product_avatars VALUES ('1234',?1,'manual')",
                [&id],
            )
            .unwrap();
        }
        c.execute(
            "INSERT INTO product_variations VALUES ('1234','Lime',1)",
            [],
        )
        .unwrap();
        let snapshot = db::load_snapshot(&c).unwrap();
        let cards = crate::view::avatar_cards(&snapshot);
        assert_eq!(cards.iter().filter(|a| a.outfit_count > 0).count(), 1);
        assert_eq!(
            cards.iter().find(|a| a.id == "lime").unwrap().outfit_count,
            1
        );
        assert!(cards
            .iter()
            .filter(|a| a.id != "lime")
            .all(|a| a.outfit_count == 0));
        assert!(cards.iter().all(|a| !a.owned));
    }

    #[test]
    fn manual_full_set_tracks_compatibility_but_individual_editions_do_not() {
        let c = fixture();
        save(
            &c,
            "1234",
            &Choice {
                mode: "all".into(),
                avatar_ids: vec![],
            },
        )
        .unwrap();
        assert_eq!(
            Catalog::load(&c)
                .unwrap()
                .scope("1234", None)
                .avatar_ids
                .len(),
            2
        );
        c.execute(
            "INSERT INTO product_avatars VALUES ('1234','manuka','manual')",
            [],
        )
        .unwrap();
        assert_eq!(
            Catalog::load(&c)
                .unwrap()
                .scope("1234", None)
                .avatar_ids
                .len(),
            3
        );
        save(
            &c,
            "1234",
            &Choice {
                mode: "specific".into(),
                avatar_ids: vec!["lime".into()],
            },
        )
        .unwrap();
        assert_eq!(
            Catalog::load(&c)
                .unwrap()
                .scope("1234", Some("フルセット"))
                .avatar_ids,
            ["lime"]
        );
        save(&c, "1234", &Choice::default()).unwrap();
        assert_eq!(
            Catalog::load(&c)
                .unwrap()
                .scope("1234", Some("フルセット"))
                .avatar_ids
                .len(),
            3
        );
        assert_eq!(
            Catalog::load(&c).unwrap().scope("1234", Some("色違い")),
            Choice::unknown()
        );
    }
    #[test]
    fn unknown_editions_remain_visible_but_do_not_count_as_supported_assets() {
        let c = fixture();
        let snapshot = db::load_snapshot(&c).unwrap();
        let detail = crate::view::avatar_detail(&snapshot, "lime").unwrap();
        assert!(detail.sections.iter().all(|s| s.items.is_empty()));
        assert_eq!(detail.unset_edition_items[0].booth_id, "1234");
        assert_eq!(
            crate::view::avatar_cards(&snapshot)
                .iter()
                .find(|a| a.id == "lime")
                .unwrap()
                .outfit_count,
            0
        );
        assert_eq!(
            crate::view::products_by_ids(&snapshot, &["1234".into()]).len(),
            1
        );
    }
    #[test]
    fn v12_migration_and_reimport_preserve_manual_choices_and_purchase_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.sqlite");
        {
            let c = db::open(&path).unwrap();
            c.execute_batch("DROP TABLE purchased_editions; PRAGMA user_version=12; INSERT INTO products(booth_id,title,imported_at,updated_at) VALUES ('1234','衣装A','',''); INSERT INTO purchase_orders(order_id,total_yen,recorded_at,updated_at,source) VALUES ('1001',1500,'','','history');").unwrap();
        }
        let mut c = db::open(&path).unwrap();
        assert_eq!(
            c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            13
        );
        save(
            &c,
            "1234",
            &Choice {
                mode: "specific".into(),
                avatar_ids: vec!["lime".into()],
            },
        )
        .unwrap();
        db::import_export_json(
            &mut c,
            r#"[{"boothId":"1234","title":"衣装A","description":"対応アバター\nプラム"}]"#,
        )
        .unwrap();
        assert_eq!(
            Catalog::load(&c).unwrap().scope("1234", None).avatar_ids,
            ["lime"]
        );
        assert_eq!(
            c.query_row(
                "SELECT total_yen FROM purchase_orders WHERE order_id='1001'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1500
        );
        let exported = crate::sync_export::build(&mut c, "test").unwrap();
        assert_eq!(exported.version, 4);
        assert_eq!(exported.purchased_editions["1234"].avatar_ids, ["lime"]);
    }
    fn merge_fixture() -> Connection {
        let c = fixture();
        c.execute_batch(
            "INSERT INTO avatars(id,display_name) VALUES ('edition-source','統合前のアバター');
             UPDATE products SET category_locked=1, genre='outfit', genre_locked=1, links_locked=1 WHERE booth_id='1234';
             DELETE FROM product_avatars WHERE product_id='1234';
             INSERT INTO product_avatars VALUES ('1234','edition-source','manual'),('1234','lime','manual');
             INSERT INTO purchase_orders(order_id,total_yen,recorded_at,updated_at,source) VALUES ('1001',1700,'','','history');
             INSERT INTO purchase_items(order_id,position,booth_id,name,price_yen,boost_yen) VALUES ('1001',0,'1234','衣装A',1500,200);"
        ).unwrap();
        c
    }

    #[test]
    fn merging_avatars_preserves_manual_editions_asset_counts_and_purchase_totals() {
        for ids in [vec!["edition-source"], vec!["edition-source", "lime"]] {
            let mut c = merge_fixture();
            save(
                &c,
                "1234",
                &Choice {
                    mode: "specific".into(),
                    avatar_ids: ids.into_iter().map(str::to_owned).collect(),
                },
            )
            .unwrap();
            db::merge_avatars(&mut c, "edition-source", "lime").unwrap();
            let catalog = Catalog::load(&c).unwrap();
            assert_eq!(catalog.manual["1234"].mode, "specific");
            assert_eq!(catalog.scope("1234", None).avatar_ids, ["lime"]);
            let snapshot = db::load_snapshot(&c).unwrap();
            assert!(snapshot.avatars.iter().all(|a| a.id != "edition-source"));
            let detail = crate::view::avatar_detail(&snapshot, "lime").unwrap();
            assert_eq!(
                detail
                    .sections
                    .iter()
                    .flat_map(|s| &s.items)
                    .filter(|i| i.booth_id == "1234")
                    .count(),
                1
            );
            assert!(detail.unset_edition_items.is_empty());
            assert_eq!(
                crate::view::avatar_cards(&snapshot)
                    .iter()
                    .find(|a| a.id == "lime")
                    .unwrap()
                    .outfit_count,
                1
            );
            let summary = crate::purchases::summary(&c, None).unwrap();
            assert_eq!(summary.total_yen, 1700);
            assert_eq!(summary.groups.len(), 1);
            assert_eq!(summary.groups[0].key, "avatar:lime");
            assert_eq!(summary.groups[0].items, 1);
            assert_eq!(summary.groups[0].amount_yen, 1700);
        }
    }

    #[test]
    fn merging_avatars_remaps_all_specific_choices_and_preserves_other_modes() {
        let mut c = merge_fixture();
        for id in ["3456", "4567", "5678", "6789"] {
            c.execute("INSERT INTO products(booth_id,title,imported_at,updated_at) VALUES (?1,'別の商品','','')", [id]).unwrap();
        }
        save(
            &c,
            "1234",
            &Choice {
                mode: "specific".into(),
                avatar_ids: vec!["edition-source".into(), "plum".into()],
            },
        )
        .unwrap();
        save(
            &c,
            "3456",
            &Choice {
                mode: "all".into(),
                avatar_ids: vec![],
            },
        )
        .unwrap();
        save(&c, "4567", &Choice::unknown()).unwrap();
        save(
            &c,
            "5678",
            &Choice {
                mode: "specific".into(),
                avatar_ids: vec!["edition-source".into()],
            },
        )
        .unwrap();
        save(
            &c,
            "6789",
            &Choice {
                mode: "specific".into(),
                avatar_ids: vec!["plum".into()],
            },
        )
        .unwrap();
        let before = Catalog::load(&c).unwrap().manual;
        db::merge_avatars(&mut c, "edition-source", "lime").unwrap();
        let after = Catalog::load(&c).unwrap().manual;
        assert_eq!(after["1234"].avatar_ids, ["lime", "plum"]);
        // References must move even if this product has no compatibility link to the source.
        assert_eq!(after["5678"].avatar_ids, ["lime"]);
        for id in ["3456", "4567", "6789"] {
            assert_eq!(before[id], after[id]);
        }
    }

    #[test]
    fn a_failed_avatar_merge_rolls_back_edition_references_and_compatibility() {
        let mut c = merge_fixture();
        save(
            &c,
            "1234",
            &Choice {
                mode: "specific".into(),
                avatar_ids: vec!["edition-source".into()],
            },
        )
        .unwrap();
        c.execute_batch("CREATE TRIGGER prevent_source_delete BEFORE DELETE ON avatars WHEN OLD.id='edition-source' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
        assert!(db::merge_avatars(&mut c, "edition-source", "lime").is_err());
        assert_eq!(
            Catalog::load(&c).unwrap().manual["1234"].avatar_ids,
            ["edition-source"]
        );
        let snapshot = db::load_snapshot(&c).unwrap();
        assert!(snapshot.avatars.iter().any(|a| a.id == "edition-source"));
        assert!(!snapshot
            .avatars
            .iter()
            .find(|a| a.id == "lime")
            .unwrap()
            .aliases
            .contains(&"統合前のアバター".into()));
        assert!(snapshot
            .links
            .iter()
            .any(|l| l.product_id == "1234" && l.avatar_id == "edition-source"));
        let summary = crate::purchases::summary(&c, None).unwrap();
        assert_eq!(summary.groups[0].key, "avatar:edition-source");
        assert_eq!(summary.total_yen, 1700);
    }

    #[test]
    fn invalid_manual_choices_are_rejected_without_replacing_a_valid_choice() {
        let c = fixture();
        save(
            &c,
            "1234",
            &Choice {
                mode: "specific".into(),
                avatar_ids: vec!["lime".into()],
            },
        )
        .unwrap();
        for choice in [
            Choice {
                mode: "all".into(),
                avatar_ids: vec!["lime".into()],
            },
            Choice {
                mode: "specific".into(),
                avatar_ids: vec!["missing".into()],
            },
            Choice {
                mode: "specific".into(),
                avatar_ids: vec![],
            },
        ] {
            assert!(save(&c, "1234", &choice).is_err());
        }
        assert_eq!(
            Catalog::load(&c).unwrap().scope("1234", None).avatar_ids,
            ["lime"]
        );
    }
}
