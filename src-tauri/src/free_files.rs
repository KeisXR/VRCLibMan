//! 「ファイルを取得」: one public `items/{id}.json` read, started by a click on one product.
//! Only variations BOOTH marks `free_download` become download rows. No session is sent here;
//! the later download of one row uses the existing built-in browser path.
//! Nothing but file names, sizes, downloadable numbers, and the free / adult flags is kept.
//! Description, category, and tags are not read here, so an adult item keeps no text either.

use crate::booth_detail::{self, DetailOutcome, ItemDocument};
use crate::model::IncomingDownload;
use serde_json::Value;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreeFilesOutcome {
    Free {
        files: Vec<IncomingDownload>,
        is_free: Option<bool>,
        is_adult: Option<bool>,
    },
    /// No variation is a free download (paid, sold out, or supporter only).
    NotFree,
    /// A free variation exists but lists no downloadable file.
    NoFiles,
    /// Age gate, 404, rate limit, or an unreadable document. Nothing is stored.
    Unavailable(DetailOutcome),
}

pub fn classify(status: u16, final_url: &str, content_type: &str, body: &str) -> FreeFilesOutcome {
    if let Some(outcome) = booth_detail::gate_http(status, final_url, content_type, body) {
        return FreeFilesOutcome::Unavailable(outcome);
    }
    interpret(body)
}

pub fn interpret(body: &str) -> FreeFilesOutcome {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return FreeFilesOutcome::Unavailable(DetailOutcome::Again);
    };
    let Some(map) = value.as_object() else {
        return FreeFilesOutcome::Unavailable(DetailOutcome::Again);
    };
    let rows = map
        .get("variations")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut free_rows = 0;
    let mut files: Vec<IncomingDownload> = Vec::new();
    for row in rows {
        let Some(row) = row.as_object() else {
            continue;
        };
        if row.get("status").and_then(Value::as_str) != Some("free_download") {
            continue;
        }
        if matches!(row.get("price"), Some(Value::Number(price)) if price.as_f64().is_some_and(|price| price > 0.0))
        {
            continue;
        }
        free_rows += 1;
        let variation = row
            .get("name")
            .and_then(Value::as_str)
            .and_then(crate::library_files::clean_variation)
            .unwrap_or_default();
        for file in listed_files(row.get("downloadable")) {
            let Some(downloadable_id) = file
                .get("url")
                .and_then(Value::as_str)
                .and_then(downloadable_id_from)
            else {
                continue;
            };
            if files
                .iter()
                .any(|row| row.downloadable_id == downloadable_id)
            {
                continue;
            }
            let filename = file_name(file).unwrap_or_else(|| downloadable_id.clone());
            files.push(IncomingDownload {
                filename,
                size_bytes: None,
                variation: variation.clone(),
                downloadable_id,
            });
        }
    }
    if free_rows == 0 {
        return FreeFilesOutcome::NotFree;
    }
    if files.is_empty() {
        return FreeFilesOutcome::NoFiles;
    }
    FreeFilesOutcome::Free {
        files,
        is_free: booth_detail::price_is_free(map.get("price")).or(Some(true)),
        is_adult: booth_detail::flag(map, &["is_adult", "isAdult"]),
    }
}

fn listed_files(value: Option<&Value>) -> Vec<&serde_json::Map<String, Value>> {
    let mut lists: Vec<&Vec<Value>> = Vec::new();
    match value {
        Some(Value::Array(list)) => lists.push(list),
        Some(Value::Object(map)) => {
            for key in ["no_musics", "noMusics", "musics", "files"] {
                if let Some(Value::Array(list)) = map.get(key) {
                    lists.push(list);
                }
            }
        }
        _ => {}
    }
    lists
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .collect()
}

fn file_name(file: &serde_json::Map<String, Value>) -> Option<String> {
    let text = |key: &str| {
        file.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    if let Some(name) = crate::library_files::safe_filename(&text("name")) {
        return Some(name);
    }
    let stem = text("file_name");
    let extension = text("file_extension");
    if stem.is_empty() {
        return None;
    }
    let joined = if extension.is_empty() || stem.ends_with(&extension) {
        stem
    } else if extension.starts_with('.') {
        format!("{stem}{extension}")
    } else {
        format!("{stem}.{extension}")
    };
    crate::library_files::safe_filename(&joined)
}

/// `https://booth.pm/downloadables/{number}` only. The query (variation id) is dropped.
pub fn downloadable_id_from(value: &str) -> Option<String> {
    let url = Url::parse(value.trim()).ok()?;
    if url.scheme() != "https" || url.host_str() != Some("booth.pm") {
        return None;
    }
    let mut parts = url.path_segments()?;
    if parts.next()? != "downloadables" {
        return None;
    }
    let id = parts.next()?;
    if parts.any(|part| !part.is_empty()) {
        return None;
    }
    crate::library_files::valid_booth_id(id).then(|| id.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreeFilesFetch {
    pub http_status: Option<u16>,
    pub outcome: FreeFilesOutcome,
}

pub async fn fetch(booth_id: &str) -> Result<FreeFilesFetch, String> {
    let (http_status, document) = booth_detail::fetch_item_document(booth_id).await?;
    let outcome = match document {
        ItemDocument::Body {
            status,
            final_url,
            content_type,
            body,
        } => classify(status, &final_url, &content_type, &body),
        ItemDocument::AgeGate => FreeFilesOutcome::Unavailable(DetailOutcome::AgeGate),
        ItemDocument::Again => FreeFilesOutcome::Unavailable(DetailOutcome::Again),
    };
    Ok(FreeFilesFetch {
        http_status,
        outcome,
    })
}

/// The message the card shows when nothing was stored.
pub fn refusal(outcome: &FreeFilesOutcome) -> Option<&'static str> {
    match outcome {
        FreeFilesOutcome::Free { .. } => None,
        FreeFilesOutcome::NotFree => Some(
            "無料でダウンロードできるファイルはありません。有料の商品は、購入後に BOOTH のライブラリから読み込んでください",
        ),
        FreeFilesOutcome::NoFiles => Some(
            "無料の配布ですが、ファイルの一覧がありません。BOOTH の商品ページで確認してください",
        ),
        FreeFilesOutcome::Unavailable(DetailOutcome::Missing) => {
            Some("BOOTH で商品が見つかりません。公開が終了した可能性があります")
        }
        FreeFilesOutcome::Unavailable(DetailOutcome::RetryLater) => {
            Some("BOOTH が混み合っています。少し待ってから、もう一度押してください")
        }
        FreeFilesOutcome::Unavailable(_) => {
            Some("商品情報を確認できません。BOOTH の商品ページで確認してください")
        }
    }
}

/// Last four digits, the status, the kind, and the count. No title, shop, or file name.
pub fn log_line(booth_id: &str, http_status: Option<u16>, outcome: &FreeFilesOutcome) -> String {
    let status = http_status
        .map(|code| code.to_string())
        .unwrap_or_else(|| "-".into());
    let kind = match outcome {
        FreeFilesOutcome::Free { files, .. } => format!("無料 {}件", files.len()),
        FreeFilesOutcome::NotFree => "無料なし".into(),
        FreeFilesOutcome::NoFiles => "ファイルなし".into(),
        FreeFilesOutcome::Unavailable(DetailOutcome::Missing) => "公開終了".into(),
        FreeFilesOutcome::Unavailable(DetailOutcome::RetryLater) => "再試行".into(),
        FreeFilesOutcome::Unavailable(DetailOutcome::AgeGate) => "年齢制限".into(),
        FreeFilesOutcome::Unavailable(_) => "失敗".into(),
    };
    format!(
        "無料ファイル確認 {} HTTP{status} {kind}",
        booth_detail::id_tail(booth_id)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const FREE: &str = r#"{"id":424242,"is_adult":false,"name":"ダミー","price":"¥ 0","description":"保存しない説明",
        "variations":[{"name":null,"price":0,"status":"free_download","type":"digital",
          "downloadable":{"musics":[],"no_musics":[
            {"file_name":"導入動画","file_extension":".mp4","file_size":"10.3 MB","name":"導入動画.mp4","url":"https://booth.pm/downloadables/9400001?variation_id=1"},
            {"file_name":"dummy_gimmick_v1","file_extension":".zip","file_size":"11.2 KB","name":"","url":"https://booth.pm/downloadables/9400002?variation_id=1"},
            {"file_name":"no_url","file_extension":".zip","name":"no_url.zip"},
            {"name":"other.zip","url":"https://example.test/downloadables/9400003"}
          ]}}]}"#;

    #[test]
    fn free_variations_become_rows_with_numbers() {
        match interpret(FREE) {
            FreeFilesOutcome::Free {
                files,
                is_free,
                is_adult,
            } => {
                assert_eq!(is_free, Some(true));
                assert_eq!(is_adult, Some(false));
                let names: Vec<_> = files.iter().map(|row| row.filename.as_str()).collect();
                assert_eq!(names, vec!["導入動画.mp4", "dummy_gimmick_v1.zip"]);
                let ids: Vec<_> = files
                    .iter()
                    .map(|row| row.downloadable_id.as_str())
                    .collect();
                assert_eq!(ids, vec!["9400001", "9400002"]);
                assert!(files.iter().all(|row| row.variation.is_empty()));
            }
            other => panic!("expected free files, got {other:?}"),
        }
    }

    #[test]
    fn paid_and_mixed_items_keep_only_free_variations() {
        let paid = r#"{"price":"¥ 500","variations":[{"name":"通常","price":500,"status":"on_sale","downloadable":null}]}"#;
        assert_eq!(interpret(paid), FreeFilesOutcome::NotFree);
        let unknown = r#"{"variations":[]}"#;
        assert_eq!(interpret(unknown), FreeFilesOutcome::NotFree);
        let mixed = r#"{"price":"¥ 0〜¥ 500","variations":[
            {"name":"無料版","price":0,"status":"free_download","downloadable":{"no_musics":[{"name":"free.zip","url":"https://booth.pm/downloadables/5000001"}]}},
            {"name":"支援版","price":500,"status":"on_sale","downloadable":{"no_musics":[{"name":"paid.zip","url":"https://booth.pm/downloadables/5000002"}]}}]}"#;
        match interpret(mixed) {
            FreeFilesOutcome::Free { files, .. } => {
                assert_eq!(files.len(), 1);
                assert_eq!(files[0].filename, "free.zip");
                assert_eq!(files[0].variation, "無料版");
            }
            other => panic!("expected free files, got {other:?}"),
        }
        let empty = r#"{"price":0,"variations":[{"status":"free_download","downloadable":{"no_musics":[]}}]}"#;
        assert_eq!(interpret(empty), FreeFilesOutcome::NoFiles);
    }

    #[test]
    fn an_adult_item_keeps_files_and_the_flag_but_no_text() {
        let adult = r#"{"is_adult":true,"price":"¥ 0","description":"残してはいけない説明","tags":[{"name":"ダミー"}],
            "variations":[{"status":"free_download","downloadable":{"no_musics":[{"name":"a.zip","url":"https://booth.pm/downloadables/6000001"}]}}]}"#;
        match interpret(adult) {
            FreeFilesOutcome::Free {
                files, is_adult, ..
            } => {
                assert_eq!(is_adult, Some(true));
                assert_eq!(files.len(), 1);
                assert!(!format!("{files:?}").contains("残してはいけない"));
            }
            other => panic!("expected free files, got {other:?}"),
        }
    }

    #[test]
    fn gates_and_errors_store_nothing() {
        let url = "https://booth.pm/ja/items/424242.json";
        assert_eq!(
            classify(404, url, "application/json", ""),
            FreeFilesOutcome::Unavailable(DetailOutcome::Missing)
        );
        assert_eq!(
            classify(
                200,
                "https://booth.pm/age_verification",
                "text/html",
                "<html></html>"
            ),
            FreeFilesOutcome::Unavailable(DetailOutcome::AgeGate)
        );
        assert_eq!(
            classify(429, url, "application/json", ""),
            FreeFilesOutcome::Unavailable(DetailOutcome::RetryLater)
        );
        assert!(matches!(
            classify(200, url, "application/json", FREE),
            FreeFilesOutcome::Free { .. }
        ));
        assert!(
            refusal(&FreeFilesOutcome::NotFree)
                .unwrap()
                .contains("有料")
        );
        assert!(refusal(&classify(200, url, "application/json", FREE)).is_none());
    }

    #[test]
    fn only_booth_downloadable_numbers_are_accepted() {
        assert_eq!(
            downloadable_id_from("https://booth.pm/downloadables/9472559?variation_id=1")
                .as_deref(),
            Some("9472559")
        );
        assert!(downloadable_id_from("http://booth.pm/downloadables/9472559").is_none());
        assert!(downloadable_id_from("https://evil.test/downloadables/9472559").is_none());
        assert!(downloadable_id_from("https://booth.pm/downloadables/../1234").is_none());
        assert!(downloadable_id_from("https://booth.pm/downloadables/12").is_none());
        assert!(downloadable_id_from("https://booth.pm/items/9472559").is_none());
    }

    #[test]
    fn log_lines_have_no_names() {
        let outcome = interpret(FREE);
        let line = log_line("8722239", Some(200), &outcome);
        assert_eq!(line, "無料ファイル確認 2239 HTTP200 無料 2件");
        assert!(!line.contains("導入動画"));
        assert!(!line.contains("8722239"));
    }

    #[test]
    fn this_module_sends_no_session() {
        let source = include_str!("free_files.rs");
        let production = source
            .split("mod tests")
            .next()
            .unwrap()
            .to_ascii_lowercase();
        assert!(!production.contains("cookie"));
        assert!(!production.contains("credentials"));
    }
}
