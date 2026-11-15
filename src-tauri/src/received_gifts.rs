//! もらったギフト（BOOTH のライブラリの「ギフト」に出る商品）の現在価格。
//! 贈り主が払った額は分からないので、公開の items/{id}.json をセッションなしで 1 件ずつ読み、
//! バリエーションの名前と今の価格だけを残す。説明、カテゴリ、タグはここでは保存しない。
//! 年齢制限の商品は読まず、応答の is_adult が真なら何も残さない。

use crate::booth_detail::{self, DetailOutcome, ItemDocument};
use crate::model::now_iso;
use crate::normalize::nfkc;
use crate::progress::ProgressGuard;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub(crate) const TABLES: &str = "
CREATE TABLE IF NOT EXISTS product_prices (
    booth_id TEXT PRIMARY KEY REFERENCES products(booth_id) ON DELETE CASCADE,
    outcome TEXT NOT NULL,
    variations_json TEXT NOT NULL DEFAULT '[]',
    checked_at TEXT NOT NULL
);
";

pub(crate) const JOB: &str = "gift-prices";
const OUTCOME_OK: &str = "ok";
const OUTCOME_ADULT: &str = "adult";
const OUTCOME_MISSING: &str = "missing";
const MAX_VARIATIONS: usize = 200;

static RUNNING: AtomicBool = AtomicBool::new(false);

fn sql<T>(result: rusqlite::Result<T>) -> Result<T, String> {
    result.map_err(|err| err.to_string())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VariationPrice {
    pub name: String,
    pub price_yen: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PriceOutcome {
    Prices(Vec<VariationPrice>),
    /// 年齢制限、年齢確認、ログインの壁。本文から何も残さない。
    Restricted,
    Missing,
    RetryLater,
    Again,
}

fn yen(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().filter(|v| v.fract() == 0.0).map(|v| v as i64))
            .filter(|price| *price >= 0),
        Value::String(text) => {
            let digits: String = text.chars().filter(char::is_ascii_digit).collect();
            let plain = text.chars().all(|ch| ch.is_ascii_digit() || matches!(ch, '¥' | '￥' | ',' | ' ' | '円'));
            if digits.is_empty() || !plain {
                None
            } else {
                digits.parse().ok()
            }
        }
        _ => None,
    }
}

/// 商品 JSON の本文から、バリエーションごとの今の価格を読む。
pub fn interpret_prices(body: &str) -> PriceOutcome {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return PriceOutcome::Again;
    };
    let Some(map) = value.as_object() else {
        return PriceOutcome::Again;
    };
    if booth_detail::flag(map, &["is_adult", "isAdult"]) == Some(true) {
        return PriceOutcome::Restricted;
    }
    let mut prices = Vec::new();
    if let Some(Value::Array(rows)) = map.get("variations") {
        for row in rows.iter().take(MAX_VARIATIONS) {
            let Some(row) = row.as_object() else { continue };
            let Some(price) = yen(row.get("price")) else { continue };
            let name = row
                .get("name")
                .and_then(Value::as_str)
                .and_then(crate::library_files::clean_variation)
                .unwrap_or_default();
            prices.push(VariationPrice { name, price_yen: price });
        }
    }
    if prices.is_empty() {
        if let Some(price) = yen(map.get("price")) {
            prices.push(VariationPrice { name: String::new(), price_yen: price });
        }
    }
    PriceOutcome::Prices(prices)
}

pub(crate) fn classify(document: ItemDocument) -> PriceOutcome {
    match document {
        ItemDocument::Body { status, final_url, content_type, body } => {
            match booth_detail::gate_http(status, &final_url, &content_type, &body) {
                None => interpret_prices(&body),
                Some(DetailOutcome::Missing) => PriceOutcome::Missing,
                Some(DetailOutcome::Adult | DetailOutcome::AgeGate) => PriceOutcome::Restricted,
                Some(DetailOutcome::RetryLater) => PriceOutcome::RetryLater,
                Some(DetailOutcome::Again | DetailOutcome::Public(_)) => PriceOutcome::Again,
            }
        }
        ItemDocument::AgeGate => PriceOutcome::Restricted,
        ItemDocument::Again => PriceOutcome::Again,
    }
}

fn key(name: &str) -> String {
    nfkc(name).split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// もらったバリエーションの今の価格。名前が合うものを足し、合わなければ価格が 1 つに決まるときだけ返す。
pub fn current_price(prices: &[VariationPrice], owned: &[String]) -> Option<i64> {
    let owned: HashSet<String> = owned.iter().map(|name| key(name)).filter(|name| !name.is_empty()).collect();
    let matched: Vec<i64> = prices
        .iter()
        .filter(|row| !row.name.is_empty() && owned.contains(&key(&row.name)))
        .map(|row| row.price_yen)
        .collect();
    if !matched.is_empty() {
        return Some(matched.iter().sum());
    }
    let first = prices.first()?.price_yen;
    prices.iter().all(|row| row.price_yen == first).then_some(first)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GiftPrice {
    /// まだ価格を確かめていない。
    Unchecked,
    Known(i64),
    /// 確かめたが決まらない（年齢制限、公開終了、バリエーションを決められない）。
    Unknown,
}

#[derive(Clone, Debug)]
pub(crate) struct ReceivedGift {
    pub booth_id: String,
    pub price: GiftPrice,
}

/// ライブラリのギフトから読んだ商品と、その今の価格。
pub(crate) fn load(connection: &Connection) -> Result<Vec<ReceivedGift>, String> {
    let mut owned: HashMap<String, Vec<String>> = HashMap::new();
    {
        let mut statement = sql(connection.prepare(
            "SELECT v.booth_id, v.name FROM product_variations v
             JOIN products p ON p.booth_id = v.booth_id
             WHERE p.received_gift = 1 AND v.owned = 1",
        ))?;
        let rows = sql(statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))))?;
        for row in rows {
            let (id, name) = sql(row)?;
            owned.entry(id).or_default().push(name);
        }
    }
    let mut statement = sql(connection.prepare(
        "SELECT p.booth_id, pp.outcome, pp.variations_json
         FROM products p LEFT JOIN product_prices pp ON pp.booth_id = p.booth_id
         WHERE p.received_gift = 1
         ORDER BY p.booth_id",
    ))?;
    let rows = sql(statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?))
    }))?;
    let mut gifts = Vec::new();
    for row in rows {
        let (booth_id, outcome, json) = sql(row)?;
        let price = match outcome.as_deref() {
            None => GiftPrice::Unchecked,
            Some(OUTCOME_OK) => {
                let prices: Vec<VariationPrice> = json.and_then(|json| serde_json::from_str(&json).ok()).unwrap_or_default();
                let names = owned.get(&booth_id).map(Vec::as_slice).unwrap_or_default();
                current_price(&prices, names).map(GiftPrice::Known).unwrap_or(GiftPrice::Unknown)
            }
            Some(_) => GiftPrice::Unknown,
        };
        gifts.push(ReceivedGift { booth_id, price });
    }
    Ok(gifts)
}

struct Target {
    booth_id: String,
    adult: bool,
}

fn targets(connection: &Connection, refresh: bool) -> Result<Vec<Target>, String> {
    let mut statement = sql(connection.prepare(
        "SELECT booth_id, IFNULL(is_adult, 0) FROM products
         WHERE received_gift = 1
           AND (?1 OR booth_id NOT IN (SELECT booth_id FROM product_prices))
         ORDER BY booth_id",
    ))?;
    let rows = sql(statement.query_map(params![refresh], |row| {
        Ok(Target { booth_id: row.get(0)?, adult: row.get::<_, i64>(1)? != 0 })
    }))?;
    sql(rows.collect())
}

fn record(connection: &Connection, booth_id: &str, outcome: &str, prices: &[VariationPrice]) -> Result<(), String> {
    let json = serde_json::to_string(prices).map_err(|err| err.to_string())?;
    sql(connection.execute(
        "INSERT INTO product_prices (booth_id, outcome, variations_json, checked_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(booth_id) DO UPDATE SET
           outcome = excluded.outcome,
           variations_json = excluded.variations_json,
           checked_at = excluded.checked_at",
        params![booth_id, outcome, json, now_iso()],
    ))?;
    Ok(())
}

pub(crate) fn running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

struct Running;

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiftPriceReport {
    pub checked: u32,
    pub unknown: u32,
    pub failed: u32,
    pub notice: String,
}

/// もらったギフトの今の価格を 1 件ずつ読む。`refresh` が偽なら未取得の商品だけ。
/// 間隔はライブラリの取得と同じ設定で、並列にしない。429 / 503 ではそこで止める。
pub(crate) async fn fetch(app: &AppHandle, refresh: bool) -> Result<GiftPriceReport, String> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("もらったギフトの価格はすでに取得中です".into());
    }
    let _running = Running;
    let state = app.state::<crate::AppState>();
    let (targets, interval) = {
        let connection = state.db.lock().map_err(|_| "データベースをロックできません".to_string())?;
        (targets(&connection, refresh)?, crate::db::booth_page_interval_ms(&connection)?)
    };
    let mut report = GiftPriceReport::default();
    let progress = ProgressGuard::new(app.clone(), JOB);
    let total = targets.len() as u32;
    let mut requested = false;
    for (index, target) in targets.iter().enumerate() {
        progress.update(index as u32, total, format!("もらったギフトの価格を確認しています ({}/{total})", index + 1));
        let outcome = if target.adult {
            PriceOutcome::Restricted
        } else {
            if requested {
                tokio::time::sleep(Duration::from_millis(interval)).await;
            }
            requested = true;
            match booth_detail::fetch_item_document(&target.booth_id).await {
                Ok((_, document)) => classify(document),
                Err(_) => PriceOutcome::Again,
            }
        };
        let saved = match &outcome {
            PriceOutcome::Prices(prices) => Some((OUTCOME_OK, prices.as_slice())),
            PriceOutcome::Restricted => Some((OUTCOME_ADULT, &[][..])),
            PriceOutcome::Missing => Some((OUTCOME_MISSING, &[][..])),
            PriceOutcome::RetryLater => {
                report.notice = "BOOTH が混み合っているため途中で止めました。時間をおいてもう一度試してください".into();
                break;
            }
            PriceOutcome::Again => None,
        };
        match saved {
            Some((kind, prices)) => {
                let connection = state.db.lock().map_err(|_| "データベースをロックできません".to_string())?;
                record(&connection, &target.booth_id, kind, prices)?;
                report.checked += 1;
                if kind != OUTCOME_OK {
                    report.unknown += 1;
                }
            }
            None => report.failed += 1,
        }
    }
    let line = format!("もらったギフト {} 件の価格を確認しました", report.checked);
    if report.notice.is_empty() {
        progress.done_with(total, line);
    } else {
        progress.fail(format!("{line}。{}", report.notice));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, price: i64) -> VariationPrice {
        VariationPrice { name: name.into(), price_yen: price }
    }

    #[test]
    fn reads_variation_prices_and_drops_adult_payloads() {
        let body = r#"{"price":"¥ 1,500〜¥ 3,000","variations":[
            {"name":"ダミーA対応","price":1500},{"name":"フルセット","price":3000},{"name":null,"price":"¥ 500"}]}"#;
        assert_eq!(
            interpret_prices(body),
            PriceOutcome::Prices(vec![row("ダミーA対応", 1500), row("フルセット", 3000), row("", 500)])
        );
        let single = r#"{"price":"¥ 800","variations":[]}"#;
        assert_eq!(interpret_prices(single), PriceOutcome::Prices(vec![row("", 800)]));
        let range = r#"{"price":"¥ 800〜¥ 900","variations":[]}"#;
        assert_eq!(interpret_prices(range), PriceOutcome::Prices(vec![]));
        let adult = r#"{"is_adult":true,"price":"¥ 800","variations":[{"name":"A","price":800}]}"#;
        assert_eq!(interpret_prices(adult), PriceOutcome::Restricted);
        assert_eq!(interpret_prices("<html>"), PriceOutcome::Again);
    }

    #[test]
    fn price_follows_the_received_variation_or_a_single_price() {
        let prices = [row("ダミーA対応", 1500), row("ダミーB対応", 1500), row("フルセット", 3000)];
        assert_eq!(current_price(&prices, &["フルセット".into()]), Some(3000));
        assert_eq!(current_price(&prices, &["ﾀﾞﾐｰA対応".into()]), Some(1500));
        assert_eq!(current_price(&prices, &[]), None);
        assert_eq!(current_price(&prices[..2], &[]), Some(1500));
        assert_eq!(current_price(&[row("", 800)], &["色違い".into()]), Some(800));
        assert_eq!(current_price(&[], &[]), None);
    }

    #[test]
    fn gate_outcomes_keep_nothing_from_restricted_pages() {
        let page = |status: u16, url: &str, ctype: &str, body: &str| ItemDocument::Body {
            status,
            final_url: url.into(),
            content_type: ctype.into(),
            body: body.into(),
        };
        let url = "https://booth.pm/ja/items/424242.json";
        assert_eq!(classify(page(404, url, "application/json", "{}")), PriceOutcome::Missing);
        assert_eq!(classify(page(429, url, "application/json", "{}")), PriceOutcome::RetryLater);
        assert_eq!(classify(page(200, url, "text/html", "<html>")), PriceOutcome::Restricted);
        assert_eq!(
            classify(page(200, "https://booth.pm/age_verification", "application/json", "{}")),
            PriceOutcome::Restricted
        );
        assert_eq!(
            classify(page(200, url, "application/json", r#"{"variations":[{"name":"A","price":100}]}"#)),
            PriceOutcome::Prices(vec![row("A", 100)])
        );
    }

    #[test]
    fn library_gifts_are_marked_and_priced_from_the_received_variation() {
        let mut connection = crate::db::open_memory().unwrap();
        let json = r#"[
            {"boothId":"424242","title":"ダミー衣装","receivedGift":true,"variations":[{"name":"フルセット","owned":true}]},
            {"boothId":"434343","title":"ダミー髪","receivedGift":true},
            {"boothId":"454545","title":"自分で買った衣装"}
        ]"#;
        crate::db::import_export_json(&mut connection, json).unwrap();
        // 省略された印は消さない。
        crate::db::import_export_json(&mut connection, r#"[{"boothId":"424242","title":"ダミー衣装"}]"#).unwrap();
        let gifts = load(&connection).unwrap();
        assert_eq!(gifts.iter().map(|gift| gift.booth_id.as_str()).collect::<Vec<_>>(), ["424242", "434343"]);
        assert!(gifts.iter().all(|gift| gift.price == GiftPrice::Unchecked));
        assert_eq!(targets(&connection, false).unwrap().len(), 2);
        record(&connection, "424242", OUTCOME_OK, &[row("ダミーA対応", 1500), row("フルセット", 3000)]).unwrap();
        record(&connection, "434343", OUTCOME_MISSING, &[]).unwrap();
        let gifts = load(&connection).unwrap();
        assert_eq!(gifts[0].price, GiftPrice::Known(3000));
        assert_eq!(gifts[1].price, GiftPrice::Unknown);
        assert!(targets(&connection, false).unwrap().is_empty());
        assert_eq!(targets(&connection, true).unwrap().len(), 2);
    }
}
