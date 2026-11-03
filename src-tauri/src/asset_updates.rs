//! Opt-in public metadata checks. Never downloads files or uses a BOOTH session.
use crate::{AppState, booth_detail, db};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const KEY: &str = "asset_update_watch_v1";
static RUNNING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Watch {
    automatic: bool,
    #[serde(default)]
    retry_after: Option<i64>,
    generation: u64,
    rows: Vec<Row>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    booth_id: String,
    enabled: bool,
    generation: u64,
    attempted_at: Option<i64>,
    checked_at: Option<String>,
    error: Option<String>,
    baseline: Option<Snapshot>,
    latest: Option<Snapshot>,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
struct Snapshot {
    // No adult description or tags, no URL, cookies, prices, popularity, or signed links.
    description: String,
    files: Vec<(String, String, String)>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    automatic: bool,
    running: bool,
    rows: Vec<RowStatus>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RowStatus {
    booth_id: String,
    title: String,
    enabled: bool,
    checked_at: Option<String>,
    error: Option<String>,
    changes: Vec<String>,
    revision: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Configure {
    automatic: Option<bool>,
    booth_id: Option<String>,
    enabled: Option<bool>,
    acknowledge: Option<bool>,
}
fn load(c: &Connection) -> Result<Watch, String> {
    match db::setting(c, KEY)? {
        Some(raw) => serde_json::from_str(&raw).map_err(|_| "更新確認の設定を読めません".into()),
        None => Ok(Watch::default()),
    }
}
fn save(c: &mut Connection, w: &Watch) -> Result<(), String> {
    db::set_setting(
        c,
        KEY,
        &serde_json::to_string(w).map_err(|_| "更新確認の設定を保存できません")?,
    )
}
fn active(c: &Connection, id: &str) -> Result<bool, String> {
    let exists = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM products WHERE booth_id = ?1)",
            [id],
            |r| r.get::<_, bool>(0),
        )
        .map_err(|_| "商品を確認できません")?;
    Ok(exists && !db::product_excluded(c, id)?)
}
fn changes(row: &Row) -> Vec<String> {
    let mut changes = Vec::new();
    if let (Some(before), Some(after)) = (&row.baseline, &row.latest) {
        if before.description != after.description {
            changes.push("商品説明の変更".into());
        }
        if before.files != after.files {
            changes.push("無料ファイル一覧の変更".into());
        }
    }
    changes
}
pub fn status(c: &Connection) -> Result<Status, String> {
    let w = load(c)?;
    let mut rows = Vec::new();
    for row in w.rows {
        if !active(c, &row.booth_id)? {
            continue;
        }
        let title = c
            .query_row(
                "SELECT title FROM products WHERE booth_id = ?1",
                [&row.booth_id],
                |r| r.get(0),
            )
            .map_err(|_| "商品を確認できません")?;
        rows.push(RowStatus {
            changes: changes(&row),
            revision: row.generation,
            booth_id: row.booth_id,
            title,
            enabled: row.enabled,
            checked_at: row.checked_at,
            error: row.error,
        });
    }
    Ok(Status {
        automatic: w.automatic,
        running: RUNNING.try_lock().is_err(),
        rows,
    })
}
pub fn configure(c: &mut Connection, input: Configure) -> Result<Status, String> {
    let mut w = load(c)?;
    if let Some(id) = input.booth_id {
        booth_detail::item_json_url(&id)?;
        if !active(c, &id)? {
            return Err("登録済みの商品を選んでください".into());
        }
        if let Some(row) = w.rows.iter_mut().find(|r| r.booth_id == id) {
            row.generation += 1;
            if let Some(enabled) = input.enabled {
                row.enabled = enabled;
            }
            if input.acknowledge == Some(true) {
                row.baseline = row.latest.clone();
            }
        } else if input.enabled == Some(true) {
            if w.rows.len() >= 500 {
                return Err("更新確認の対象は500件までです".into());
            }
            w.rows.push(Row {
                booth_id: id,
                enabled: true,
                generation: 0,
                attempted_at: None,
                checked_at: None,
                error: None,
                baseline: None,
                latest: None,
            });
        }
    } else if input.enabled.is_some() || input.acknowledge.is_some() {
        return Err("商品を選んでください".into());
    }
    if let Some(automatic) = input.automatic {
        w.automatic = automatic;
        w.generation += 1;
    }
    save(c, &w)?;
    status(c)
}
fn interpret(id: &str, body: &str) -> Result<Snapshot, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|_| "商品情報を読めません")?;
    let actual_id = v.get("id").map(|id| {
        id.as_str()
            .map(str::to_string)
            .unwrap_or_else(|| id.to_string())
    });
    if actual_id.as_deref() != Some(id)
        || !v.get("variations").is_some_and(serde_json::Value::is_array)
    {
        return Err("商品情報の形式を確認できません".into());
    }
    let description = if v.get("is_adult").and_then(serde_json::Value::as_bool) == Some(false) {
        v.get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(8000)
            .collect()
    } else {
        String::new()
    };
    let mut files = match crate::free_files::interpret(body) {
        crate::free_files::FreeFilesOutcome::Free { files, .. } => files
            .into_iter()
            .map(|f| (f.downloadable_id, f.variation, f.filename))
            .collect(),
        crate::free_files::FreeFilesOutcome::NotFree
        | crate::free_files::FreeFilesOutcome::NoFiles => Vec::new(),
        _ => return Err("ファイル一覧を確認できません".into()),
    };
    files.sort();
    Ok(Snapshot { description, files })
}
async fn fetch(id: &str) -> Result<Snapshot, String> {
    use booth_detail::ItemDocument;
    match booth_detail::fetch_item_document(id).await?.1 {
        ItemDocument::Body {
            status,
            final_url,
            content_type,
            body,
        } => {
            if let Some(gate) = booth_detail::gate_http(status, &final_url, &content_type, &body) {
                return Err(match gate {
                    booth_detail::DetailOutcome::Missing => "商品が公開されていません",
                    booth_detail::DetailOutcome::RetryLater => {
                        "BOOTH が混み合っています。時間を空けて確認します"
                    }
                    booth_detail::DetailOutcome::AgeGate => "ログインまたは年齢確認が必要です",
                    _ => "商品情報を確認できません",
                }
                .into());
            }
            interpret(id, &body)
        }
        ItemDocument::AgeGate => Err("ログインまたは年齢確認が必要です".into()),
        ItemDocument::Again => Err("商品情報を確認できません".into()),
    }
}
fn due(row: &Row, now: i64) -> bool {
    row.attempted_at.is_none_or(|last| {
        now.saturating_sub(last) >= if row.error.is_some() { 3600 } else { 86400 }
    })
}
// A consent change invalidates an in-flight result, including off -> on changes.
fn allowed(w: &Watch, row: &Row, generation: u64, manual: bool) -> bool {
    row.enabled && (manual || w.automatic) && w.generation == generation
}
fn record(row: &mut Row, result: Result<Snapshot, String>) {
    match result {
        Ok(snapshot) => {
            if row.baseline.is_none() {
                row.baseline = Some(snapshot.clone());
            }
            row.latest = Some(snapshot);
            row.checked_at = Some(crate::model::now_iso());
            row.error = None;
        }
        Err(err) => row.error = Some(err), // Keep the last successful observation and candidate.
    }
}
pub async fn check(app: &AppHandle, manual: bool) -> Result<Status, String> {
    let guard = RUNNING
        .try_lock()
        .map_err(|_| "更新確認はすでに実行中です")?;
    let state = app.state::<AppState>();
    let (ids, generation) = {
        let c = state.db.lock().map_err(|_| "設定を読めません")?;
        let w = load(&c)?;
        (
            w.rows
                .iter()
                .filter(|r| {
                    (manual
                        || w.retry_after
                            .is_none_or(|until| chrono::Utc::now().timestamp() >= until))
                        && r.enabled
                        && (manual || (w.automatic && due(r, chrono::Utc::now().timestamp())))
                })
                .map(|r| r.booth_id.clone())
                .collect::<Vec<_>>(),
            w.generation,
        )
    };
    for (index, id) in ids.iter().enumerate() {
        if index > 0 {
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        let revision = {
            let mut c = state.db.lock().map_err(|_| "設定を読めません")?;
            let mut w = load(&c)?;
            let Some(row) = w.rows.iter().find(|r| &r.booth_id == id) else {
                continue;
            };
            if !allowed(&w, row, generation, manual) {
                if w.generation != generation || (!manual && !w.automatic) {
                    break;
                }
                continue;
            }
            if !active(&c, id)? || (!manual && !due(row, chrono::Utc::now().timestamp())) {
                continue;
            }
            let row = w.rows.iter_mut().find(|r| &r.booth_id == id).unwrap();
            // Persist the attempt before I/O so restarts cannot hammer a failing endpoint.
            row.attempted_at = Some(chrono::Utc::now().timestamp());
            let revision = row.generation;
            save(&mut c, &w)?;
            revision
        };
        let result = fetch(id).await;
        let stop = result.as_ref().err().is_some_and(|e| {
            e.contains("BOOTH が") || e.contains("ログイン") || e == "商品情報を確認できません"
        });
        {
            let mut c = state.db.lock().map_err(|_| "設定を読めません")?;
            let mut w = load(&c)?;
            if let Some(row) = w.rows.iter().find(|r| &r.booth_id == id) {
                if allowed(&w, row, generation, manual)
                    && row.generation == revision
                    && active(&c, id)?
                {
                    let row = w.rows.iter_mut().find(|r| &r.booth_id == id).unwrap();
                    record(row, result);
                    if stop {
                        w.retry_after = Some(chrono::Utc::now().timestamp() + 3600);
                    }
                    save(&mut c, &w)?;
                }
            }
        }
        let _ = app.emit_to("main", "asset-updates-changed", ());
        // A gate or rate limit ends the batch; do not continue requesting other products.
        if stop {
            break;
        }
    }
    drop(guard);
    let c = state.db.lock().map_err(|_| "設定を読めません")?;
    let result = status(&c);
    let _ = app.emit_to("main", "asset-updates-changed", ());
    result
}
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let _ = check(&app, false).await;
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row() -> Row {
        Row {
            booth_id: "424242".into(),
            enabled: true,
            generation: 0,
            attempted_at: None,
            checked_at: None,
            error: None,
            baseline: None,
            latest: None,
        }
    }
    fn snapshot(text: &str) -> Snapshot {
        Snapshot {
            description: text.into(),
            files: vec![],
        }
    }
    #[test]
    fn first_observation_is_baseline_and_candidates_wait_for_acknowledgement() {
        let mut r = row();
        record(&mut r, Ok(snapshot("v1")));
        assert!(changes(&r).is_empty());
        record(&mut r, Ok(snapshot("v2")));
        assert_eq!(changes(&r), vec!["商品説明の変更"]);
        record(&mut r, Ok(snapshot("v2")));
        assert_eq!(changes(&r).len(), 1);
        record(&mut r, Err("通信失敗".into()));
        assert_eq!(changes(&r).len(), 1);
        assert_eq!(r.error.as_deref(), Some("通信失敗"));
        r.baseline = r.latest.clone();
        assert!(changes(&r).is_empty());
    }
    #[test]
    fn consent_revocation_and_reenable_reject_old_results() {
        let mut w = Watch::default();
        let mut r = row();
        assert!(!allowed(&w, &r, 0, false));
        assert!(allowed(&w, &r, 0, true));
        w.automatic = true;
        assert!(allowed(&w, &r, 0, false));
        w.generation += 2;
        assert!(!allowed(&w, &r, 0, false));
        r.enabled = false;
        assert!(!allowed(&w, &r, 2, true));
    }
    #[test]
    fn attempts_are_daily_and_errors_wait_an_hour() {
        let mut r = row();
        assert!(due(&r, 100));
        r.attempted_at = Some(100);
        assert!(!due(&r, 86499));
        assert!(due(&r, 86500));
        r.error = Some("失敗".into());
        assert!(!due(&r, 3699));
        assert!(due(&r, 3700));
        assert!(!due(&r, 0));
    }
    #[test]
    fn paid_prices_and_popularity_do_not_signal_a_file_update() {
        let a = r#"{"id":424242,"is_adult":false,"description":"v1  info","price":500,"wish_lists_count":1,"variations":[{"status":"on_sale","price":500}]}"#;
        let b = r#"{"id":424242,"is_adult":false,"description":"v1\ninfo","price":300,"wish_lists_count":5,"variations":[{"status":"on_sale","price":300}]}"#;
        assert_eq!(
            interpret("424242", a).unwrap(),
            interpret("424242", b).unwrap()
        );
        assert!(interpret("999999", a).is_err());
        assert!(interpret("424242", r#"{"id":424242}"#).is_err());
    }
    #[test]
    fn free_file_order_is_stable_and_new_names_are_candidates() {
        let a = r#"{"id":424242,"is_adult":false,"variations":[{"status":"free_download","downloadable":{"no_musics":[{"name":"v1.zip","url":"https://booth.pm/downloadables/9400001?a=1"},{"name":"readme.txt","url":"https://booth.pm/downloadables/9400002"}]}}]}"#;
        let b = a.replace("v1.zip", "v2.zip");
        let mut r = row();
        record(&mut r, Ok(interpret("424242", a).unwrap()));
        record(
            &mut r,
            Ok(interpret("424242", &a.replace("?a=1", "?a=2")).unwrap()),
        );
        assert!(changes(&r).is_empty());
        record(&mut r, Ok(interpret("424242", &b).unwrap()));
        assert_eq!(changes(&r), vec!["無料ファイル一覧の変更"]);
    }
    #[test]
    fn adult_and_unknown_age_descriptions_are_not_retained() {
        for flag in ["true", "null"] {
            let raw = format!(
                r#"{{"id":424242,"is_adult":{flag},"description":"保存しない本文","variations":[]}}"#
            );
            assert!(interpret("424242", &raw).unwrap().description.is_empty());
        }
    }
    #[test]
    fn watch_settings_persist_without_changing_existing_schema_or_settings() {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE products(booth_id TEXT PRIMARY KEY,title TEXT); CREATE TABLE product_exclusions(booth_id TEXT PRIMARY KEY,excluded_at TEXT); INSERT INTO products VALUES('424242','ダミー'); INSERT INTO settings VALUES('ollama_model','saved'); PRAGMA user_version=7;").unwrap();
        assert!(!status(&c).unwrap().automatic);
        configure(
            &mut c,
            Configure {
                automatic: Some(true),
                booth_id: Some("424242".into()),
                enabled: Some(true),
                acknowledge: None,
            },
        )
        .unwrap();
        let mut w = load(&c).unwrap();
        record(&mut w.rows[0], Ok(snapshot("v1")));
        record(&mut w.rows[0], Ok(snapshot("v2")));
        save(&mut c, &w).unwrap();
        let saved = status(&c).unwrap();
        assert!(saved.automatic);
        assert_eq!(saved.rows[0].changes.len(), 1);
        configure(
            &mut c,
            Configure {
                automatic: None,
                booth_id: Some("424242".into()),
                enabled: None,
                acknowledge: Some(true),
            },
        )
        .unwrap();
        assert!(status(&c).unwrap().rows[0].changes.is_empty());
        assert_eq!(
            db::setting(&c, "ollama_model").unwrap().as_deref(),
            Some("saved")
        );
        assert_eq!(
            c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            7
        );
        c.execute("INSERT INTO product_exclusions VALUES('424242','t')", [])
            .unwrap();
        assert!(status(&c).unwrap().rows.is_empty());
        assert!(!active(&c, "424242").unwrap());
    }
}
