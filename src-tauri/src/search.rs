//! Library search. The cards on the search page carry only the first 500 characters of a
//! description, so matching runs here over the stored text: title, shop, the whole
//! description (up to the 8000 characters the import keeps), the genre label and the BOOTH id.
//! Tokens are split on whitespace, lower-cased, and every token must appear (AND), like the
//! page's own filter in `src/searchUtils.ts`. Excluded products are not searched.

use rusqlite::Connection;

pub(crate) fn tokens(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn genre_label(genre: Option<&str>) -> &'static str {
    let Some(genre) = genre else { return "" };
    crate::genre::ASSET_GENRES
        .iter()
        .chain(crate::genre::WORLD_GENRES.iter())
        .find(|(key, _)| *key == genre)
        .map(|(_, label)| *label)
        .unwrap_or("")
}

pub(crate) fn matches(fields: &[&str], tokens: &[String]) -> bool {
    // Joined with a newline so that a token cannot run across two fields.
    let haystack = fields.join("\n").to_lowercase();
    tokens.iter().all(|token| haystack.contains(token.as_str()))
}

/// BOOTH ids of the products whose stored text holds every token. An empty query matches nothing.
pub(crate) fn product_ids(connection: &Connection, query: &str) -> Result<Vec<String>, String> {
    let tokens = tokens(query);
    if tokens.is_empty() {
        return Ok(Vec::new());
    }
    let mut statement = connection
        .prepare(
            "SELECT booth_id, title, shop, description_snippet, genre FROM products
             WHERE booth_id NOT IN (SELECT booth_id FROM product_exclusions)
             ORDER BY booth_id",
        )
        .map_err(|err| format!("検索できません: {err}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|err| format!("検索できません: {err}"))?;
    let mut found = Vec::new();
    for row in rows {
        let (booth_id, title, shop, description, genre) =
            row.map_err(|err| format!("検索できません: {err}"))?;
        let label = genre_label(genre.as_deref());
        if matches(&[&title, &shop, &description, label, &booth_id], &tokens) {
            found.push(booth_id);
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_descriptions_are_searched_with_and_tokens() {
        let directory = tempfile::tempdir().unwrap();
        let connection = crate::db::open(&directory.path().join("library.sqlite")).unwrap();
        // The searched word sits after the 500 characters a card shows.
        let long = format!("{}末尾の対応表記 ライム", "あ".repeat(600));
        connection
            .execute(
                "INSERT INTO products (booth_id, title, shop, description_snippet, genre, imported_at, updated_at)
                 VALUES ('100001', 'Sample Outfit', 'Shop', ?1, 'outfit', 't', 't'),
                        ('100002', '別の服', 'Shop', '短い説明', 'hair', 't', 't'),
                        ('100003', '除外した服', 'Shop', 'ライム', NULL, 't', 't')",
                [long],
            )
            .unwrap();
        connection
            .execute("INSERT INTO product_exclusions (booth_id, excluded_at) VALUES ('100003', 't')", [])
            .unwrap();
        assert_eq!(product_ids(&connection, "ライム").unwrap(), ["100001"]);
        assert_eq!(product_ids(&connection, "sample  ライム").unwrap(), ["100001"]);
        assert_eq!(product_ids(&connection, "SHOP").unwrap(), ["100001", "100002"]);
        assert_eq!(product_ids(&connection, "髪型").unwrap(), ["100002"], "genre label");
        assert_eq!(product_ids(&connection, "100002").unwrap(), ["100002"]);
        assert!(product_ids(&connection, "ライム 短い").unwrap().is_empty());
        assert!(product_ids(&connection, "   ").unwrap().is_empty());
        // A token never spans two fields.
        assert!(!matches(&["ab", "cd"], &tokens("bc")));
        // Full-width spaces separate tokens too.
        assert_eq!(tokens("ライム\u{3000}衣装"), ["ライム", "衣装"]);
    }
}

