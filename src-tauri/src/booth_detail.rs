//! One public `items/{id}.json` read per product.
//! No session header, no age-gate bypass, and nothing from an adult or age response is kept.

use crate::library_walk;
use crate::model::UNAVAILABLE_DESCRIPTION;
use reqwest::header::{ACCEPT, CONTENT_TYPE, LOCATION, USER_AGENT};
use reqwest::StatusCode;
use serde_json::Value;
use std::time::Duration;
use url::Url;

pub const BODY_LIMIT: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicItem {
    pub description: String,
    pub shop: String,
    pub category_id: Option<i64>,
    pub category_name: String,
    pub category_parent: String,
    pub tags: Vec<String>,
    pub is_free: Option<bool>,
    pub is_adult: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailOutcome {
    Public(PublicItem),
    /// `is_adult` was true. The rest of the payload is discarded.
    Adult,
    /// Age check, login wall, or a non-JSON page. Nothing from the body is kept.
    AgeGate,
    Missing,
    /// 429 / 503 / transport. Leave the product unchecked and stop the batch.
    RetryLater,
    /// This item did not yield a usable document. Try it again next time.
    Again,
}

pub fn should_request_detail(is_adult: Option<bool>, already_checked: bool) -> bool {
    !already_checked && is_adult != Some(true)
}

pub fn item_json_url(booth_id: &str) -> Result<Url, String> {
    if !(4..=12).contains(&booth_id.len()) || !booth_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("商品IDを特定できません".into());
    }
    let url = Url::parse(&format!("https://booth.pm/ja/items/{booth_id}.json"))
        .map_err(|_| "商品情報のURLを作れません".to_string())?;
    if !is_item_json_url(&url) || age_gate_url(&url) {
        return Err("このURLへは移動しません".into());
    }
    Ok(url)
}

pub fn age_gate_url(url: &Url) -> bool {
    if url.scheme() != "https" {
        return true;
    }
    let path = url.path().to_ascii_lowercase();
    let query = url.query().unwrap_or("").to_ascii_lowercase();
    let haystack = format!("{path}?{query}");
    [
        "age_verification",
        "age-verification",
        "age_check",
        "r18",
        "r-18",
    ]
    .iter()
    .any(|token| haystack.contains(token))
}

pub fn is_item_json_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("booth.pm")
        && url
            .path()
            .trim_end_matches('/')
            .to_ascii_lowercase()
            .ends_with(".json")
        && url.path().contains("/items/")
}

pub fn classify_http(status: u16, final_url: &str, content_type: &str, body: &str) -> DetailOutcome {
    let url = Url::parse(final_url).ok();
    if url.as_ref().is_none_or(age_gate_url) {
        return DetailOutcome::AgeGate;
    }
    if status == 404 {
        return DetailOutcome::Missing;
    }
    if status == 401 || status == 403 {
        return DetailOutcome::AgeGate;
    }
    if status == 429 || status == 503 || !(200..300).contains(&status) {
        return if status == 429 || status == 503 {
            DetailOutcome::RetryLater
        } else {
            DetailOutcome::Again
        };
    }
    let url = url.expect("url");
    if !is_item_json_url(&url) {
        return DetailOutcome::Again;
    }
    let ctype = content_type.to_ascii_lowercase();
    let trimmed = body.trim_start();
    if ctype.contains("html") || trimmed.starts_with('<') {
        return DetailOutcome::AgeGate;
    }
    if body.len() > BODY_LIMIT {
        return DetailOutcome::Again;
    }
    interpret_json(body)
}

pub fn interpret_json(body: &str) -> DetailOutcome {
    let value: Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) => return DetailOutcome::Again,
    };
    let Some(map) = value.as_object() else {
        return DetailOutcome::Again;
    };
    if flag(map, &["is_adult", "isAdult"]) == Some(true) {
        return DetailOutcome::Adult;
    }
    let category = map.get("category").and_then(Value::as_object);
    let (category_id, category_name, category_parent) = if let Some(category) = category {
        let id = category.get("id").and_then(json_i64);
        let name = clip(&raw_text(category.get("name")), 80);
        let parent = match category.get("parent") {
            Some(Value::String(text)) => clip(text.trim(), 80),
            Some(Value::Object(parent)) => clip(&raw_text(parent.get("name")), 80),
            _ => String::new(),
        };
        (id, name, parent)
    } else {
        (None, String::new(), String::new())
    };
    DetailOutcome::Public(PublicItem {
        description: clip(&raw_text(map.get("description")), 8000),
        shop: shop_name(map.get("shop")),
        category_id,
        category_name,
        category_parent,
        tags: tag_names(map.get("tags")),
        is_free: price_is_free(map.get("price")),
        is_adult: flag(map, &["is_adult", "isAdult"]),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchReport {
    pub http_status: Option<u16>,
    pub outcome: DetailOutcome,
}

pub async fn fetch_public_item(booth_id: &str) -> Result<FetchReport, String> {
    let mut current = item_json_url(booth_id)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| "商品情報を確認できません".to_string())?;
    let mut last_status = None;
    for _ in 0..3 {
        if age_gate_url(&current) || !is_item_json_url(&current) {
            return Ok(FetchReport {
                http_status: last_status,
                outcome: DetailOutcome::AgeGate,
            });
        }
        let response = client
            .get(current.clone())
            .header(ACCEPT, "application/json")
            .header(USER_AGENT, "VRCLibMan")
            .send()
            .await
            .map_err(|_| "商品情報を確認できません".to_string())?;
        let status = response.status();
        last_status = Some(status.as_u16());
        if is_redirect(status) {
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| "商品情報を確認できません".to_string())?;
            let joined = current
                .join(location)
                .map_err(|_| "商品情報を確認できません".to_string())?;
            if age_gate_url(&joined) {
                return Ok(FetchReport {
                    http_status: last_status,
                    outcome: DetailOutcome::AgeGate,
                });
            }
            if !is_item_json_url(&joined) {
                return Ok(FetchReport {
                    http_status: last_status,
                    outcome: DetailOutcome::Again,
                });
            }
            current = joined;
            continue;
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        let final_url = response.url().to_string();
        let body = response
            .text()
            .await
            .map_err(|_| "商品情報を確認できません".to_string())?;
        return Ok(FetchReport {
            http_status: last_status,
            outcome: classify_http(status.as_u16(), &final_url, &content_type, &body),
        });
    }
    Ok(FetchReport {
        http_status: last_status,
        outcome: DetailOutcome::Again,
    })
}

/// Last four digits only. The line never includes the title, description, tags, or shop.
pub fn id_tail(booth_id: &str) -> &str {
    let len = booth_id.len();
    if len <= 4 { booth_id } else { &booth_id[len - 4..] }
}

pub fn detail_log_line(
    booth_id: &str,
    http_status: Option<u16>,
    outcome: &DetailOutcome,
    interval_ms: u64,
) -> String {
    let status = http_status
        .map(|code| code.to_string())
        .unwrap_or_else(|| "-".into());
    let kind = match outcome {
        DetailOutcome::Public(_) => "成功",
        DetailOutcome::Adult | DetailOutcome::AgeGate => "年齢制限",
        DetailOutcome::Missing => "公開終了",
        DetailOutcome::RetryLater => "再試行",
        DetailOutcome::Again => "失敗",
    };
    format!(
        "商品情報 {} HTTP{status} {kind} 間隔{interval_ms}ms",
        id_tail(booth_id)
    )
}

pub fn unavailable_description() -> &'static str {
    UNAVAILABLE_DESCRIPTION
}

fn is_redirect(status: StatusCode) -> bool {
    matches!(
        status.as_u16(),
        301 | 302 | 303 | 307 | 308
    )
}

fn shop_name(value: Option<&Value>) -> String {
    match value {
        Some(Value::Object(shop)) => clip(&raw_text(shop.get("name")), 120),
        Some(Value::String(text)) => clip(text.trim(), 120),
        _ => String::new(),
    }
}

fn tag_names(value: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    let mut tags = Vec::new();
    for item in items {
        let name = match item {
            Value::String(text) => clip(text.trim(), 40),
            Value::Object(map) => clip(&raw_text(map.get("name")), 40),
            _ => continue,
        };
        if name.is_empty() || tags.iter().any(|existing| existing == &name) {
            continue;
        }
        tags.push(name);
        if tags.len() == 40 {
            break;
        }
    }
    tags
}

fn flag(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<bool> {
    for key in keys {
        if let Some(value) = map.get(*key) {
            return match value {
                Value::Bool(bit) => Some(*bit),
                Value::Number(number) => number.as_u64().map(|number| number != 0),
                _ => None,
            };
        }
    }
    None
}

fn price_is_free(value: Option<&Value>) -> Option<bool> {
    match value {
        Some(Value::Number(number)) => number.as_f64().map(|price| price == 0.0),
        Some(Value::String(text)) if text.contains("無料") => Some(true),
        Some(Value::String(text)) => {
            let digits: String = text.chars().filter(|ch| ch.is_ascii_digit()).collect();
            if digits.is_empty() {
                None
            } else {
                digits.parse::<u64>().ok().map(|price| price == 0)
            }
        }
        _ => None,
    }
}

fn raw_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.trim().to_string(),
        _ => String::new(),
    }
}

fn json_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
    .filter(|id| (1..1_000_000).contains(id))
}

fn clip(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

pub fn detail_pause_ms(page_delay_ms: u64) -> u64 {
    library_walk::detail_pause(Duration::from_millis(page_delay_ms)).as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adult_json_keeps_nothing() {
        let body = r#"{"id":424242,"is_adult":true,"name":"ダミー","description":"残してはいけない説明","category":{"id":209,"name":"3D衣装","parent":{"name":"3Dモデル"}},"tags":[{"name":"ダミー"}]}"#;
        assert_eq!(interpret_json(body), DetailOutcome::Adult);
        let html = classify_http(
            200,
            "https://booth.pm/age_verification",
            "text/html",
            "<html>年齢確認</html>",
        );
        assert_eq!(html, DetailOutcome::AgeGate);
        assert_eq!(
            classify_http(401, "https://booth.pm/ja/items/424242.json", "text/html", ""),
            DetailOutcome::AgeGate
        );
        assert_eq!(
            classify_http(404, "https://booth.pm/ja/items/424242.json", "application/json", ""),
            DetailOutcome::Missing
        );
        assert_eq!(
            classify_http(429, "https://booth.pm/ja/items/424242.json", "application/json", ""),
            DetailOutcome::RetryLater
        );
    }

    #[test]
    fn public_json_keeps_category_and_tags() {
        let body = r#"{"id":424242,"is_adult":false,"name":"ダミー衣装","description":"説明です","price":0,"shop":{"name":"ダミー工房"},"category":{"id":209,"name":"3D衣装","parent":{"name":"3Dモデル"}},"tags":[{"name":"VRChat"},"ダミー"]}"#;
        match interpret_json(body) {
            DetailOutcome::Public(item) => {
                assert_eq!(item.category_id, Some(209));
                assert_eq!(item.category_name, "3D衣装");
                assert_eq!(item.category_parent, "3Dモデル");
                assert_eq!(item.tags, vec!["VRChat".to_string(), "ダミー".to_string()]);
                assert_eq!(item.description, "説明です");
                assert_eq!(item.shop, "ダミー工房");
                assert_eq!(item.is_free, Some(true));
                assert_eq!(item.is_adult, Some(false));
            }
            other => panic!("expected public item, got {other:?}"),
        }
    }

    #[test]
    fn html_and_age_redirects_are_not_parsed_as_items() {
        assert_eq!(
            classify_http(
                200,
                "https://booth.pm/ja/items/424242.json",
                "text/html",
                "<html><title>年齢確認</title></html>"
            ),
            DetailOutcome::AgeGate
        );
        assert!(age_gate_url(
            &Url::parse("https://www.pixiv.net/age_verification.php").unwrap()
        ));
        assert!(!should_request_detail(Some(true), false));
        assert!(!should_request_detail(Some(false), true));
        assert!(should_request_detail(None, false));
        assert!(item_json_url("424242").unwrap().path().ends_with(".json"));
        assert!(item_json_url("../secret").is_err());
        assert_eq!(detail_pause_ms(250), 3000);
        assert_eq!(detail_pause_ms(1000), 3000);
    }

    #[test]
    fn detail_log_lines_keep_only_the_tail_status_and_kind() {
        let public = DetailOutcome::Public(PublicItem {
            description: "残してはいけない説明".to_string(),
            shop: "架空工房".to_string(),
            category_id: Some(209),
            category_name: "3D衣装".into(),
            category_parent: "3Dモデル".into(),
            tags: vec!["衣装".into()],
            is_free: Some(false),
            is_adult: Some(false),
        });
        let line = detail_log_line("42424299", Some(200), &public, 3000);
        assert_eq!(line, "商品情報 4299 HTTP200 成功 間隔3000ms");
        assert!(!line.contains("42424299"));
        assert!(!line.contains("残してはいけない"));
        assert!(!line.contains("架空工房"));
        assert!(!line.contains("衣装"));
        assert_eq!(
            detail_log_line("8800881", Some(200), &DetailOutcome::Adult, 3000),
            "商品情報 0881 HTTP200 年齢制限 間隔3000ms"
        );
        assert_eq!(
            detail_log_line("8800881", Some(404), &DetailOutcome::Missing, 1000),
            "商品情報 0881 HTTP404 公開終了 間隔1000ms"
        );
        assert_eq!(
            detail_log_line("12", None, &DetailOutcome::Again, 3000),
            "商品情報 12 HTTP- 失敗 間隔3000ms"
        );
    }

    #[test]
    fn the_fetcher_does_not_attach_a_browser_session() {
        let source = include_str!("booth_detail.rs");
        let production = source.split("mod tests").next().unwrap().to_ascii_lowercase();
        assert!(!production.contains("cookie"));
        assert!(!production.contains("credentials"));
        assert!(production.contains("policy::none"));
    }
}
