//! The compatibility table: avatars, base bodies, merging, the seed, and the catalog.
use super::*;

pub(crate) fn knowledge(connection: &Connection) -> Result<Knowledge, String> {
    Ok(Knowledge {
        avatars: load_avatars(connection)?,
        base_bodies: load_bases(connection)?,
    })
}

pub(crate) fn save_avatar(
    connection: &mut Connection,
    input: &AvatarInput,
) -> Result<String, String> {
    let display = input.display_name.trim();
    if display.is_empty() || display.chars().count() > 80 {
        return Err("表示名は1〜80文字にしてください".into());
    }
    let aliases = clean_aliases(&input.aliases, display);
    let notes: String = input.notes.chars().take(400).collect();
    let base = normalize_base(connection, input.base_body.as_deref())?;
    let requested = input
        .id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    let id = if let Some(id) = requested {
        if !valid_id(id) {
            return Err("IDは英小文字、数字、_、- で64文字以内にしてください".into());
        }
        if id.starts_with("owned-") && !avatar_exists(connection, id)? {
            return Err("このIDは自動作成用に予約されています".into());
        }
        id.to_string()
    } else {
        generate_id(connection, display)?
    };
    let builtin = avatar_by_id(connection, &id)?
        .map(|avatar| avatar.is_builtin)
        .unwrap_or(false);
    connection
        .execute(
            "INSERT INTO avatars (id, display_name, base_body, aliases_json, is_builtin, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
               display_name = excluded.display_name,
               base_body = excluded.base_body,
               aliases_json = excluded.aliases_json,
               notes = excluded.notes",
            params![id, display, base, to_json(&aliases)?, builtin, notes],
        )
        .sql()?;
    crate::sync_meta::touch_avatar(connection, &id)?;
    reresolve_after_edit(connection)?;
    Ok(id)
}

pub(crate) fn set_avatar_hero_path(
    connection: &Connection,
    id: &str,
    path: Option<&str>,
) -> Result<(), String> {
    let rows = connection
        .execute(
            "UPDATE avatars SET hero_image_path = ?1 WHERE id = ?2",
            params![path, id],
        )
        .sql()?;
    if rows == 0 {
        return Err("アバターが見つかりません".into());
    }
    Ok(())
}

pub(crate) fn avatar_hero_paths(connection: &Connection) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare("SELECT hero_image_path FROM avatars WHERE hero_image_path IS NOT NULL")
        .sql()?;
    let rows = statement.query_map([], |row| row.get(0)).sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

/// The links go before the re-resolve, so the log has to start before they do.
pub(crate) fn delete_avatar(connection: &mut Connection, id: &str) -> Result<(), String> {
    let mut run = ClassificationRun::new(KIND_EDIT, CLASSIFIER_CATALOG);
    observe_all(connection, &mut run, |connection| {
        delete_avatar_unlogged(connection, id)
    })
}

pub(super) fn delete_avatar_unlogged(connection: &mut Connection, id: &str) -> Result<(), String> {
    let avatar = avatar_by_id(connection, id)?.ok_or("アバターが見つかりません")?;
    if avatar.is_builtin {
        return Err("初期データのアバターは削除できません".into());
    }
    connection
        .execute(
            "DELETE FROM product_avatars WHERE avatar_id = ?1",
            params![id],
        )
        .sql()?;
    connection
        .execute("DELETE FROM avatars WHERE id = ?1", params![id])
        .sql()?;
    reresolve_products(connection)?;
    Ok(())
}

pub(crate) fn save_base_body(
    connection: &Connection,
    input: &BaseBodyInput,
) -> Result<String, String> {
    let display = input.display_name.trim();
    if display.is_empty() || display.chars().count() > 80 {
        return Err("素体名は1〜80文字にしてください".into());
    }
    let note: String = input.note.chars().take(400).collect();
    let requested = input
        .id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    let id = if let Some(id) = requested {
        if !valid_id(id) {
            return Err("IDは英小文字、数字、_、- で64文字以内にしてください".into());
        }
        id.to_string()
    } else {
        generate_id(connection, display)?
    };
    connection
        .execute(
            "INSERT INTO base_bodies (id, display_name, note) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET display_name = excluded.display_name, note = excluded.note",
            params![id, display, note],
        )
        .sql()?;
    Ok(id)
}

pub(crate) fn delete_base_body(connection: &Connection, id: &str) -> Result<(), String> {
    connection
        .execute(
            "UPDATE avatars SET base_body = NULL WHERE base_body = ?1",
            params![id],
        )
        .sql()?;
    let rows = connection
        .execute("DELETE FROM base_bodies WHERE id = ?1", params![id])
        .sql()?;
    if rows == 0 {
        return Err("素体が見つかりません".into());
    }
    crate::sync_meta::reconcile_all(connection)
}

/// Links move before the re-resolve, so the log has to start before they do.
pub(crate) fn merge_avatars(
    connection: &mut Connection,
    from_id: &str,
    into_id: &str,
) -> Result<(), String> {
    let mut run = ClassificationRun::new(KIND_EDIT, CLASSIFIER_CATALOG);
    observe_all(connection, &mut run, |connection| {
        merge_avatars_unlogged(connection, from_id, into_id)
    })
}

pub(super) fn merge_avatars_unlogged(
    connection: &mut Connection,
    from_id: &str,
    into_id: &str,
) -> Result<(), String> {
    if from_id == into_id {
        return Err("同じアバターには統合できません".into());
    }
    let source = avatar_by_id(connection, from_id)?.ok_or("統合元のアバターが見つかりません")?;
    let mut target =
        avatar_by_id(connection, into_id)?.ok_or("統合先のアバターが見つかりません")?;
    if source.is_builtin {
        return Err("初期データのアバターは統合で消せません。別名を編集してください".into());
    }
    let mut aliases = target.aliases.clone();
    for alias in std::iter::once(source.display_name.clone()).chain(source.aliases) {
        if alias != target.display_name && !aliases.iter().any(|existing| existing == &alias) {
            aliases.push(alias);
        }
    }
    aliases.truncate(32);
    target.aliases = aliases;
    let tx = connection.transaction().sql()?;
    tx.execute(
        "UPDATE avatars SET aliases_json = ?1 WHERE id = ?2",
        params![to_json(&target.aliases)?, into_id],
    )
    .sql()?;
    let mut statement = tx
        .prepare(
            "SELECT l.product_id FROM product_avatars l
             JOIN products p ON p.booth_id = l.product_id
             WHERE l.avatar_id = ?1 AND p.links_locked = 1",
        )
        .sql()?;
    let locked: Vec<String> = statement
        .query_map(params![from_id], |row| row.get(0))
        .sql()?
        .collect::<Result<Vec<_>, _>>()
        .sql()?;
    drop(statement);
    tx.execute(
        "DELETE FROM product_avatars WHERE avatar_id = ?1",
        params![from_id],
    )
    .sql()?;
    for product_id in locked {
        tx.execute(
            "INSERT OR IGNORE INTO product_avatars (product_id, avatar_id, source) VALUES (?1, ?2, 'manual')",
            params![product_id, into_id],
        )
        .sql()?;
    }
    crate::purchased_editions::merge_avatar(&tx, from_id, into_id)?;
    tx.execute("DELETE FROM avatars WHERE id = ?1", params![from_id])
        .sql()?;
    tx.commit().sql()?;
    reresolve_products(connection)?;
    Ok(())
}

pub(crate) fn restore_seed(connection: &mut Connection) -> Result<u32, String> {
    let merged = merge_seed(connection, 0)?;
    reapply_catalog(connection)?;
    Ok(merged.inserted + merged.filled)
}

pub(super) fn seed_file() -> Result<SeedFile, String> {
    serde_json::from_str(include_str!("../../knowledge/seed.json"))
        .map_err(|err| format!("初期データを読めません: {err}"))
}

pub(super) struct SeedMerge {
    /// New avatar rows.
    pub(super) inserted: u32,
    /// Existing avatars that received the base body of a group added now.
    pub(super) filled: u32,
}

/// Adds seed rows that are missing. Only base bodies with a revision above
/// `after` are considered, together with the avatars that belong to them,
/// plus avatars whose own revision is above `after` (`after = 0` merges
/// everything). Such an avatar joins its older group only when that group is
/// still in the library.
///
/// An avatar already in the library, found by id, by its catalog Booth id or
/// by one unambiguous name, is not inserted again. It gets the seed base body
/// only when its base body is empty and the group was inserted by this call,
/// so a base body the user cleared on an existing group stays cleared.
pub(super) fn merge_seed(connection: &Connection, after: u32) -> Result<SeedMerge, String> {
    let seed = seed_file()?;
    let mut inserted_bodies: HashSet<String> = HashSet::new();
    let mut considered: HashSet<String> = HashSet::new();
    for body in &seed.base_bodies {
        if body.revision <= after {
            continue;
        }
        considered.insert(body.id.clone());
        let rows = connection
            .execute(
                "INSERT OR IGNORE INTO base_bodies (id, display_name, note) VALUES (?1, ?2, ?3)",
                params![body.id, body.display_name, body.note],
            )
            .sql()?;
        if rows > 0 {
            inserted_bodies.insert(body.id.clone());
        }
    }
    let mut merged = SeedMerge {
        inserted: 0,
        filled: 0,
    };
    for avatar in &seed.avatars {
        let seed_body = avatar.base_body.as_deref().filter(|body| !body.is_empty());
        let new_avatar = after > 0 && avatar.revision > after;
        let in_scope = match seed_body {
            Some(body) => considered.contains(body) || new_avatar,
            None => after == 0 || new_avatar,
        };
        if !in_scope {
            continue;
        }
        // A new avatar in an older group joins it only if the user kept that group.
        let joins_kept_group = match seed_body {
            Some(body) if !considered.contains(body) => base_body_exists(connection, body)?,
            _ => false,
        };
        let body = seed_body.filter(|body| considered.contains(*body) || joins_kept_group);
        let existing = match existing_seed_avatar(connection, avatar)? {
            SeedMatch::Found(id) => Some(id),
            SeedMatch::Ambiguous => continue,
            SeedMatch::Missing => None,
        };
        if let Some(id) = existing {
            if let Some(body) =
                body.filter(|body| inserted_bodies.contains(*body) || joins_kept_group)
            {
                let rows = connection
                    .execute(
                        "UPDATE avatars SET base_body = ?1
                         WHERE id = ?2 AND (base_body IS NULL OR base_body = '')",
                        params![body, id],
                    )
                    .sql()?;
                merged.filled += rows as u32;
            }
            continue;
        }
        let rows = connection
            .execute(
                "INSERT OR IGNORE INTO avatars (id, display_name, base_body, aliases_json, is_builtin, notes)
                 VALUES (?1, ?2, ?3, ?4, 1, '')",
                params![avatar.id, avatar.display_name, body, to_json(&avatar.aliases)?],
            )
            .sql()?;
        merged.inserted += rows as u32;
        if let Some(booth_id) = avatar.booth_id.as_deref().filter(|id| !id.is_empty())
            && rows > 0
            && avatar_id_for_booth(connection, booth_id)?.is_none()
        {
            bind_booth(connection, &avatar.id, booth_id)?;
        }
    }
    Ok(merged)
}

pub(super) enum SeedMatch {
    Found(String),
    Missing,
    Ambiguous,
}

pub(super) fn existing_seed_avatar(
    connection: &Connection,
    avatar: &SeedAvatar,
) -> Result<SeedMatch, String> {
    if avatar_exists(connection, &avatar.id)? {
        return Ok(SeedMatch::Found(avatar.id.clone()));
    }
    if let Some(booth_id) = avatar.booth_id.as_deref().filter(|id| !id.is_empty())
        && let Some(id) = avatar_id_for_booth(connection, booth_id)?
    {
        return Ok(SeedMatch::Found(id));
    }
    let keys: HashSet<String> = std::iter::once(&avatar.display_name)
        .chain(avatar.aliases.iter())
        .map(|name| crate::normalize::normalize_label(name))
        .filter(|key| !key.is_empty())
        .collect();
    let booth = avatar.booth_id.as_deref().filter(|id| !id.is_empty());
    let mut hits: Vec<String> = Vec::new();
    for other in load_avatars(connection)? {
        // Same name but already tied to another Booth item: a different avatar.
        if booth.is_some() && bound_to_any_booth(connection, &other.id)? {
            continue;
        }
        let hit = std::iter::once(&other.display_name)
            .chain(other.aliases.iter())
            .any(|name| keys.contains(&crate::normalize::normalize_label(name)));
        if hit && !hits.contains(&other.id) {
            hits.push(other.id);
        }
    }
    Ok(match hits.len() {
        0 => SeedMatch::Missing,
        1 => {
            let id = hits.remove(0);
            if let Some(booth_id) = booth {
                bind_booth(connection, &id, booth_id)?;
            }
            SeedMatch::Found(id)
        }
        _ => SeedMatch::Ambiguous,
    })
}

pub(super) fn bound_to_any_booth(connection: &Connection, avatar_id: &str) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM avatar_sources WHERE avatar_id = ?1)",
            params![avatar_id],
            |row| row.get(0),
        )
        .sql()
}

pub(super) fn merge_catalog(connection: &Connection) -> Result<u32, String> {
    let parsed = crate::catalog::bundled()?;
    let added = merge_catalog_entries(connection, &parsed.entries)?;
    if setting(connection, "catalog_fetched")?.is_none() {
        set_setting(connection, "catalog_fetched", &parsed.fetched_on)?;
        set_setting(connection, "catalog_upstream", &parsed.upstream_updated)?;
        set_setting(
            connection,
            "catalog_count",
            &parsed.entries.len().to_string(),
        )?;
    }
    Ok(added)
}

pub(crate) fn install_catalog(
    connection: &mut Connection,
    parsed: &crate::catalog::ParsedCatalog,
) -> Result<CatalogRefresh, String> {
    let added = merge_catalog_entries(connection, &parsed.entries)?;
    let fetched = tokyo_date();
    let upstream = if parsed.upstream_updated.is_empty() {
        setting(connection, "catalog_upstream")?.unwrap_or_default()
    } else {
        parsed.upstream_updated.clone()
    };
    set_setting(connection, "catalog_fetched", &fetched)?;
    set_setting(connection, "catalog_upstream", &upstream)?;
    set_setting(
        connection,
        "catalog_count",
        &parsed.entries.len().to_string(),
    )?;
    let mut run = ClassificationRun::new(KIND_CATALOG, CLASSIFIER_CATALOG);
    observe_all(connection, &mut run, |connection| {
        settle_dictionary(connection)?;
        reresolve_products(connection)
    })?;
    Ok(CatalogRefresh {
        added,
        total: parsed.entries.len() as u32,
        upstream_updated: upstream,
    })
}

pub(super) fn merge_catalog_entries(
    connection: &Connection,
    entries: &[crate::catalog::CatalogEntry],
) -> Result<u32, String> {
    let avatars = load_avatars(connection)?;
    let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
    for avatar in &avatars {
        for name in std::iter::once(&avatar.display_name).chain(avatar.aliases.iter()) {
            by_name
                .entry(crate::normalize::normalize_label(name))
                .or_default()
                .push(avatar.id.clone());
        }
    }
    let mut added = 0u32;
    for entry in entries {
        if let Some(existing) = avatar_id_for_booth(connection, &entry.booth_id)? {
            remember_catalog_alias(connection, &existing, entry)?;
            continue;
        }
        let key = crate::normalize::normalize_label(&entry.display_name);
        let hits = by_name.get(&key).cloned().unwrap_or_default();
        let mut unique: Vec<String> = hits;
        unique.sort();
        unique.dedup();
        if unique.len() == 1 && booth_count(connection, &unique[0])? == 0 {
            bind_booth(connection, &unique[0], &entry.booth_id)?;
            remember_catalog_alias(connection, &unique[0], entry)?;
            if avatar_notes(connection, &unique[0])?.trim().is_empty() {
                connection
                    .execute(
                        "UPDATE avatars SET notes = ?1 WHERE id = ?2",
                        params![crate::catalog::catalog_note(entry), unique[0]],
                    )
                    .sql()?;
            }
            continue;
        }
        let id = format!("mf-{}", entry.booth_id);
        let inserted = connection
            .execute(
                "INSERT OR IGNORE INTO avatars (id, display_name, base_body, aliases_json, is_builtin, notes)
                 VALUES (?1, ?2, NULL, ?3, 1, ?4)",
                params![
                    id,
                    entry.display_name,
                    to_json(&entry.aliases)?,
                    crate::catalog::catalog_note(entry)
                ],
            )
            .sql()?;
        bind_booth(connection, &id, &entry.booth_id)?;
        by_name.entry(key).or_default().push(id.clone());
        for alias in &entry.aliases {
            by_name
                .entry(crate::normalize::normalize_label(alias))
                .or_default()
                .push(id.clone());
        }
        if inserted > 0 {
            added += 1;
        }
    }
    Ok(added)
}

pub(super) fn remember_catalog_alias(
    connection: &Connection,
    avatar_id: &str,
    entry: &crate::catalog::CatalogEntry,
) -> Result<(), String> {
    let mut aliases = avatar_aliases(connection, avatar_id)?;
    for alias in std::iter::once(&entry.display_name).chain(entry.aliases.iter()) {
        if alias.is_empty() || aliases.iter().any(|existing| existing == alias) {
            continue;
        }
        let display = avatar_display(connection, avatar_id)?;
        if alias == &display {
            continue;
        }
        aliases.push(alias.clone());
    }
    aliases.truncate(32);
    connection
        .execute(
            "UPDATE avatars SET aliases_json = ?1 WHERE id = ?2",
            params![to_json(&aliases)?, avatar_id],
        )
        .sql()?;
    Ok(())
}

pub(super) fn bind_booth(
    connection: &Connection,
    avatar_id: &str,
    booth_id: &str,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR IGNORE INTO avatar_sources (booth_id, avatar_id) VALUES (?1, ?2)",
            params![booth_id, avatar_id],
        )
        .sql()?;
    Ok(())
}

pub(super) fn avatar_id_for_booth(
    connection: &Connection,
    booth_id: &str,
) -> Result<Option<String>, String> {
    connection
        .query_row(
            "SELECT avatar_id FROM avatar_sources WHERE booth_id = ?1",
            params![booth_id],
            |row| row.get(0),
        )
        .optional()
        .sql()
}

pub(super) fn booth_count(connection: &Connection, avatar_id: &str) -> Result<i64, String> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM avatar_sources WHERE avatar_id = ?1",
            params![avatar_id],
            |row| row.get(0),
        )
        .sql()
}

pub(super) fn avatar_display(connection: &Connection, id: &str) -> Result<String, String> {
    connection
        .query_row(
            "SELECT display_name FROM avatars WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .sql()
}

pub(super) fn avatar_aliases(connection: &Connection, id: &str) -> Result<Vec<String>, String> {
    let json: String = connection
        .query_row(
            "SELECT aliases_json FROM avatars WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .sql()?;
    Ok(parse_list(&json))
}

pub(super) fn avatar_notes(connection: &Connection, id: &str) -> Result<String, String> {
    connection
        .query_row(
            "SELECT notes FROM avatars WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .sql()
}
