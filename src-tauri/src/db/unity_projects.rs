//! Unity プロジェクトの手で足した行・隠した行・アバターとの結び付け（DB版16）。
//! ALCOM の一覧は写さない。端末のパスなので、データ移行の ZIP には入れない。
use super::*;
use crate::unity_projects::{ProjectAvatar, StoredProject};

pub(crate) const UNITY_PROJECT_TABLES: &str = "
CREATE TABLE IF NOT EXISTS unity_projects (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL,
    manual INTEGER NOT NULL DEFAULT 0,
    hidden INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS unity_project_avatars (
    project_id TEXT NOT NULL REFERENCES unity_projects(id) ON DELETE CASCADE,
    avatar_id TEXT NOT NULL REFERENCES avatars(id) ON DELETE CASCADE,
    PRIMARY KEY (project_id, avatar_id)
);
CREATE INDEX IF NOT EXISTS unity_project_avatars_by_avatar ON unity_project_avatars(avatar_id);
";

pub(crate) fn stored_unity_projects(connection: &Connection) -> Result<Vec<StoredProject>, String> {
    let mut statement = connection
        .prepare("SELECT id, path, manual, hidden FROM unity_projects ORDER BY rowid")
        .sql()?;
    statement
        .query_map([], |row| {
            Ok(StoredProject {
                id: row.get(0)?,
                path: row.get(1)?,
                manual: row.get::<_, i64>(2)? != 0,
                hidden: row.get::<_, i64>(3)? != 0,
            })
        })
        .sql()?
        .collect::<Result<Vec<_>, _>>()
        .sql()
}

/// プロジェクトごとの結び付けたアバター（表示名の順）。
pub(crate) fn unity_project_links(
    connection: &Connection,
) -> Result<HashMap<String, Vec<ProjectAvatar>>, String> {
    let mut statement = connection
        .prepare(
            "SELECT l.project_id, a.id, a.display_name FROM unity_project_avatars l
             JOIN avatars a ON a.id = l.avatar_id
             ORDER BY a.display_name, a.id",
        )
        .sql()?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                ProjectAvatar {
                    id: row.get(1)?,
                    name: row.get(2)?,
                },
            ))
        })
        .sql()?
        .collect::<Result<Vec<_>, _>>()
        .sql()?;
    let mut links: HashMap<String, Vec<ProjectAvatar>> = HashMap::new();
    for (project, avatar) in rows {
        links.entry(project).or_default().push(avatar);
    }
    Ok(links)
}

/// 行を作るか、パスだけ新しくする。印は変えない。
fn ensure_row(connection: &Connection, id: &str, path: &str) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO unity_projects (id, path) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET path = excluded.path",
            params![id, path],
        )
        .sql()?;
    Ok(())
}

/// 手で足したのでも隠したのでもなく、結び付けも無い行は残さない。
fn prune_row(connection: &Connection, id: &str) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM unity_projects WHERE id = ?1 AND manual = 0 AND hidden = 0
             AND NOT EXISTS (SELECT 1 FROM unity_project_avatars WHERE project_id = ?1)",
            params![id],
        )
        .sql()?;
    Ok(())
}

pub(crate) fn add_manual_unity_project(
    connection: &Connection,
    id: &str,
    path: &str,
) -> Result<(), String> {
    ensure_row(connection, id, path)?;
    connection
        .execute(
            "UPDATE unity_projects SET manual = 1, hidden = 0 WHERE id = ?1",
            params![id],
        )
        .sql()?;
    Ok(())
}

/// 行ごと消す（結び付けも消える）。ALCOM の一覧にあるものは、呼ぶ側が隠すだけにする。
pub(crate) fn forget_unity_project(connection: &Connection, id: &str) -> Result<(), String> {
    connection
        .execute("DELETE FROM unity_projects WHERE id = ?1", params![id])
        .sql()?;
    Ok(())
}

pub(crate) fn set_unity_project_hidden(
    connection: &mut Connection,
    id: &str,
    path: &str,
    hidden: bool,
) -> Result<(), String> {
    let tx = connection.transaction().sql()?;
    ensure_row(&tx, id, path)?;
    tx.execute(
        "UPDATE unity_projects SET hidden = ?2 WHERE id = ?1",
        params![id, i64::from(hidden)],
    )
    .sql()?;
    prune_row(&tx, id)?;
    tx.commit().sql()
}

pub(crate) fn link_unity_project_avatar(
    connection: &mut Connection,
    id: &str,
    path: &str,
    avatar_id: &str,
    linked: bool,
) -> Result<(), String> {
    if linked && avatar_by_id(connection, avatar_id)?.is_none() {
        return Err("アバターが見つかりません".into());
    }
    let tx = connection.transaction().sql()?;
    if linked {
        ensure_row(&tx, id, path)?;
        tx.execute(
            "INSERT OR IGNORE INTO unity_project_avatars (project_id, avatar_id) VALUES (?1, ?2)",
            params![id, avatar_id],
        )
        .sql()?;
    } else {
        tx.execute(
            "DELETE FROM unity_project_avatars WHERE project_id = ?1 AND avatar_id = ?2",
            params![id, avatar_id],
        )
        .sql()?;
        prune_row(&tx, id)?;
    }
    tx.commit().sql()
}

/// アバターの統合。統合元の結び付けを統合先へ移す（重複は統合元の削除で消える）。
pub(super) fn merge_unity_project_avatar(
    tx: &rusqlite::Transaction<'_>,
    from_id: &str,
    into_id: &str,
) -> Result<(), String> {
    tx.execute(
        "UPDATE OR IGNORE unity_project_avatars SET avatar_id = ?2 WHERE avatar_id = ?1",
        params![from_id, into_id],
    )
    .sql()?;
    tx.execute(
        "DELETE FROM unity_project_avatars WHERE avatar_id = ?1",
        params![from_id],
    )
    .sql()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str = "00000000000000aa";
    const PATH: &str = "D:\\ALCOM\\Plum";

    fn connection() -> Connection {
        let connection = open_memory().unwrap();
        connection
            .execute_batch(
                "INSERT INTO avatars (id, display_name) VALUES ('uproj-a', '見本A'), ('uproj-b', '見本B');",
            )
            .unwrap();
        connection
    }

    fn linked(connection: &Connection) -> Vec<String> {
        unity_project_links(connection)
            .unwrap()
            .get(PROJECT)
            .map(|avatars| avatars.iter().map(|avatar| avatar.id.clone()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn rows_exist_only_while_needed() {
        let mut connection = connection();
        link_unity_project_avatar(&mut connection, PROJECT, PATH, "uproj-a", true).unwrap();
        link_unity_project_avatar(&mut connection, PROJECT, PATH, "uproj-a", true).unwrap();
        assert_eq!(linked(&connection), ["uproj-a"]);
        assert_eq!(stored_unity_projects(&connection).unwrap()[0].path, PATH);
        link_unity_project_avatar(&mut connection, PROJECT, PATH, "uproj-a", false).unwrap();
        assert!(stored_unity_projects(&connection).unwrap().is_empty());
        set_unity_project_hidden(&mut connection, PROJECT, PATH, true).unwrap();
        assert!(stored_unity_projects(&connection).unwrap()[0].hidden);
        set_unity_project_hidden(&mut connection, PROJECT, PATH, false).unwrap();
        assert!(stored_unity_projects(&connection).unwrap().is_empty());
        add_manual_unity_project(&connection, PROJECT, PATH).unwrap();
        set_unity_project_hidden(&mut connection, PROJECT, PATH, false).unwrap();
        assert!(stored_unity_projects(&connection).unwrap()[0].manual);
        assert!(link_unity_project_avatar(&mut connection, PROJECT, PATH, "missing", true).is_err());
        link_unity_project_avatar(&mut connection, PROJECT, PATH, "uproj-b", true).unwrap();
        forget_unity_project(&connection, PROJECT).unwrap();
        assert!(linked(&connection).is_empty());
    }

    #[test]
    fn merging_and_deleting_avatars_moves_links() {
        let mut connection = connection();
        link_unity_project_avatar(&mut connection, PROJECT, PATH, "uproj-a", true).unwrap();
        link_unity_project_avatar(&mut connection, PROJECT, PATH, "uproj-b", true).unwrap();
        link_unity_project_avatar(&mut connection, "00000000000000bb", "D:\\Other", "uproj-a", true)
            .unwrap();
        merge_avatars(&mut connection, "uproj-a", "uproj-b").unwrap();
        assert_eq!(linked(&connection), ["uproj-b"]);
        assert_eq!(
            unity_project_links(&connection).unwrap()["00000000000000bb"][0].id,
            "uproj-b"
        );
        delete_avatar(&mut connection, "uproj-b").unwrap();
        assert!(unity_project_links(&connection).unwrap().is_empty());
    }
}

/// ワールドの商品か（「Unityで開く」で、ワールドのプロジェクトを候補に出すため）。
pub(crate) fn product_is_world(connection: &Connection, booth_id: &str) -> Result<bool, String> {
    Ok(connection
        .query_row(
            "SELECT category FROM products WHERE booth_id = ?1",
            params![booth_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .sql()?
        .flatten()
        .as_deref()
        == Some("world"))
}

/// 商品の対応アバター（「Unityで開く」で、開くプロジェクトの候補を出すため）。
pub(crate) fn product_avatar_ids(
    connection: &Connection,
    booth_id: &str,
) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare("SELECT avatar_id FROM product_avatars WHERE product_id = ?1 ORDER BY avatar_id")
        .sql()?;
    statement
        .query_map(params![booth_id], |row| row.get(0))
        .sql()?
        .collect::<Result<Vec<_>, _>>()
        .sql()
}
