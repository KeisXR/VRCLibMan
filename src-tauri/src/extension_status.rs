//! Minimal local-library facts for visible BOOTH cards. Never returns paths or descriptions.
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Query {
    pub ids: Vec<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Avatar {
    id: String,
    name: String,
    via_base_body: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    booth_id: String,
    registered: bool,
    owned: bool,
    local_recorded: bool,
    files_known: bool,
    avatars: Vec<Avatar>,
}

pub(crate) fn query(connection: &Connection, query: Query) -> Result<Vec<Status>, String> {
    if query.ids.len() > 100
        || query
            .ids
            .iter()
            .any(|id| !(4..=12).contains(&id.len()) || !id.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("商品IDは100件までの4〜12桁で指定してください".into());
    }
    let run = || -> rusqlite::Result<Vec<Status>> {
        let mut result = Vec::new();
        for id in query.ids {
            if result.iter().any(|row: &Status| row.booth_id == id) {
                continue;
            }
            let category: Option<Option<String>> = connection.query_row(
                "SELECT category FROM products p WHERE booth_id=?1 AND NOT EXISTS(SELECT 1 FROM product_exclusions e WHERE e.booth_id=p.booth_id)", [&id], |r| r.get(0)).optional()?;
            let mut row = Status {
                booth_id: id.clone(),
                registered: category.is_some(),
                owned: false,
                local_recorded: false,
                files_known: false,
                avatars: Vec::new(),
            };
            if let Some(category) = category {
                row.owned = connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM product_variations WHERE booth_id=?1 AND owned=1)",
                    [&id],
                    |r| r.get(0),
                )?;
                row.files_known = connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM product_files WHERE booth_id=?1)",
                    [&id],
                    |r| r.get(0),
                )?;
                row.local_recorded = connection.query_row("SELECT EXISTS(SELECT 1 FROM product_files WHERE booth_id=?1 AND local_path IS NOT NULL) OR EXISTS(SELECT 1 FROM local_files WHERE linked_product_id=?1 AND is_dir=0)", [&id], |r| r.get(0))?;
                // Only existing table relationships count. Hair never inherits body compatibility.
                let mut statement = connection.prepare("SELECT a.id,a.display_name,
                    NOT EXISTS(SELECT 1 FROM product_avatars d WHERE d.product_id=?1 AND d.avatar_id=a.id)
                    FROM avatars a WHERE EXISTS(SELECT 1 FROM product_avatars own JOIN products p ON p.booth_id=own.product_id WHERE own.avatar_id=a.id AND p.category='avatar' AND NOT EXISTS(SELECT 1 FROM product_exclusions e WHERE e.booth_id=p.booth_id))
                    AND (EXISTS(SELECT 1 FROM product_avatars d WHERE d.product_id=?1 AND d.avatar_id=a.id)
                    OR (?2 != 'hair' AND a.base_body IS NOT NULL AND a.base_body != '' AND EXISTS(SELECT 1 FROM product_avatars d JOIN avatars b ON b.id=d.avatar_id WHERE d.product_id=?1 AND b.base_body=a.base_body))) ORDER BY a.display_name,a.id")?;
                row.avatars = statement
                    .query_map(params![id, category.unwrap_or_default()], |r| {
                        Ok(Avatar {
                            id: r.get(0)?,
                            name: r.get(1)?,
                            via_base_body: r.get(2)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
            }
            result.push(row);
        }
        Ok(result)
    };
    run().map_err(|_| "ライブラリの状態を読み取れません".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registered_is_not_owned_and_compatibility_uses_explicit_groups() {
        let dir = tempfile::tempdir().unwrap();
        let c = crate::db::open(&dir.path().join("test.sqlite")).unwrap();
        c.execute_batch("DELETE FROM product_avatars; DELETE FROM products; DELETE FROM avatars;
            INSERT INTO avatars(id,display_name,base_body) VALUES ('a','見本A','body'),('b','見本B','body'),('c','見本C',NULL);
            INSERT INTO products(booth_id,title,category,imported_at,updated_at) VALUES ('1000','本体','avatar','',''),('2000','衣装','outfit','',''),('3000','髪','hair','','');
            INSERT INTO product_avatars VALUES ('1000','a','manual'),('2000','b','manual'),('3000','b','manual');
            INSERT INTO product_files(booth_id,filename) VALUES ('2000','test.zip');").unwrap();
        let rows = query(
            &c,
            Query {
                ids: vec!["2000".into(), "3000".into(), "9999".into()],
            },
        )
        .unwrap();
        assert!(
            rows[0].registered && !rows[0].owned && !rows[0].local_recorded && rows[0].files_known
        );
        assert_eq!(rows[0].avatars.len(), 1);
        assert!(rows[0].avatars[0].via_base_body);
        assert!(rows[1].avatars.is_empty());
        assert!(!rows[2].registered);
        c.execute_batch("INSERT INTO product_variations VALUES ('2000','通常',1); INSERT INTO product_exclusions(booth_id,excluded_at) VALUES ('3000','');").unwrap();
        assert!(
            query(
                &c,
                Query {
                    ids: vec!["2000".into()]
                }
            )
            .unwrap()[0]
                .owned
        );
        assert!(
            !query(
                &c,
                Query {
                    ids: vec!["3000".into()]
                }
            )
            .unwrap()[0]
                .registered
        );
        assert!(
            query(
                &c,
                Query {
                    ids: vec!["../1000".into()]
                }
            )
            .is_err()
        );
        assert!(
            query(
                &c,
                Query {
                    ids: vec!["1000".into(); 101]
                }
            )
            .is_err()
        );
        let json = serde_json::to_string(&rows).unwrap();
        assert!(!json.contains("localPath") && !json.contains("description"));
    }
}

#[cfg(test)]
mod download_tests {
    #[test]
    fn renamed_download_retains_original_file_id_and_variation() {
        let dir = tempfile::tempdir().unwrap();
        let c = crate::db::open(&dir.path().join("test.sqlite")).unwrap();
        c.execute_batch("INSERT INTO products(booth_id,title,imported_at,updated_at) VALUES ('123456','見本','','');
            INSERT INTO product_files(booth_id,variation,filename,downloadable_id) VALUES ('123456','A用','sample.zip','555555'),('123456','B用','sample.zip','666666');").unwrap();
        let path = dir.path().join("sample (1).zip");
        std::fs::write(&path, b"test").unwrap();
        crate::db::record_extension_download(&c, "123456", "A用", &path, None, Some("555555"))
            .unwrap();
        let (downloaded, file_id): (bool,String) = c.query_row("SELECT downloaded,downloadable_id FROM product_files WHERE booth_id='123456' AND variation='A用'",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert!(downloaded);
        assert_eq!(file_id, "555555");
        let count: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM product_files WHERE booth_id='123456'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        assert!(
            crate::db::record_extension_download(&c, "123456", "B用", &path, None, Some("555555"))
                .is_err()
        );
    }
}
