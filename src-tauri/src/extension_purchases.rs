//! 拡張から届く購入履歴と購入完了。内蔵ブラウザと同じ `purchase_walk::parse_*` で値を確かめてから
//! `purchases` に入れる。ページの移動とアクセス間隔は拡張が受け持ち、ここでは 1 ページ・1 注文ずつ受ける。
//! 支払方法、住所、ギフト用 URL、ダウンロード URL は受け取っても保存しない（`purchase_walk` が読まない）。
use crate::purchase_walk::{self, OrdersPath};
use crate::purchases::{self, CheckoutProduct};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    State,
    List,
    Pending,
    Detail,
    Finish,
    Checkout,
}

impl Route {
    pub(crate) fn reads_body(self) -> bool {
        !matches!(self, Route::State | Route::Pending)
    }
}

pub(crate) fn route(method: &str, path: &str) -> Option<Route> {
    let get = method.eq_ignore_ascii_case("GET");
    let post = method.eq_ignore_ascii_case("POST");
    match path {
        "/purchases/state" if get => Some(Route::State),
        "/purchases/pending" if get => Some(Route::Pending),
        "/purchases/list" if post => Some(Route::List),
        "/purchases/detail" if post => Some(Route::Detail),
        "/purchases/finish" if post => Some(Route::Finish),
        "/purchases/checkout" if post => Some(Route::Checkout),
        _ => None,
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub status: u16,
    pub body: String,
    pub purchases_changed: bool,
    pub library_changed: bool,
    /// 分類待ちが増えた。アプリの確認画面を開く。
    pub prompt: bool,
}

impl Outcome {
    fn ok(body: Value) -> Self {
        Self {
            status: 200,
            body: body.to_string(),
            ..Default::default()
        }
    }

    fn bad(message: &str) -> Self {
        Self {
            status: 400,
            body: json!({ "error": message }).to_string(),
            ..Default::default()
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListBody {
    #[serde(default)]
    href: String,
    #[serde(default)]
    orders: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DetailBody {
    #[serde(default)]
    href: String,
    #[serde(default)]
    detail: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FinishBody {
    #[serde(default)]
    list_complete: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheckoutBody {
    #[serde(default)]
    href: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    products: Vec<Value>,
}

fn orders_path(href: &str) -> Option<OrdersPath> {
    Url::parse(href).ok().as_ref().and_then(purchase_walk::orders_path)
}

pub(crate) fn handle(connection: &mut Connection, route: Route, body: &str) -> Result<Outcome, String> {
    match route {
        Route::State => Ok(Outcome::ok(json!({
            "complete": purchases::history_complete(connection)?,
        }))),
        Route::Pending => Ok(Outcome::ok(json!({
            "ids": purchases::pending_detail_ids(connection)?,
        }))),
        Route::List => {
            let Ok(body) = serde_json::from_str::<ListBody>(body) else {
                return Ok(Outcome::bad("購入履歴を読めません"));
            };
            if orders_path(&body.href) != Some(OrdersPath::List) {
                return Ok(Outcome::bad(purchase_walk::LOGIN_NOTICE));
            }
            let orders = purchase_walk::parse_list_orders(&body.orders);
            let applied = purchases::apply_list_page(connection, &orders)?;
            Ok(Outcome {
                purchases_changed: true,
                ..Outcome::ok(json!({
                    "orders": orders.len(),
                    "added": applied.added,
                    "removed": applied.removed,
                    "news": applied.news,
                }))
            })
        }
        Route::Detail => {
            let Ok(body) = serde_json::from_str::<DetailBody>(body) else {
                return Ok(Outcome::bad("注文の詳細を読めません"));
            };
            let Some(OrdersPath::Detail(order_id)) = orders_path(&body.href) else {
                return Ok(Outcome::bad(purchase_walk::LOGIN_NOTICE));
            };
            match purchase_walk::parse_detail(&body.detail) {
                Some(detail) if detail.order_id == order_id => {
                    let applied = purchases::apply_detail(connection, &detail)?;
                    Ok(Outcome {
                        purchases_changed: true,
                        ..Outcome::ok(json!({ "removed": applied.removed }))
                    })
                }
                _ => Ok(Outcome::bad(purchase_walk::DETAIL_UNREADABLE)),
            }
        }
        Route::Finish => {
            let Ok(body) = serde_json::from_str::<FinishBody>(body) else {
                return Ok(Outcome::bad("購入履歴の読み込み結果を読めません"));
            };
            purchases::finish_walk(connection, body.list_complete)?;
            Ok(Outcome {
                purchases_changed: true,
                ..Outcome::ok(json!({ "ok": true }))
            })
        }
        Route::Checkout => checkout(connection, body),
    }
}

/// 本人が開いた購入完了ページ。購入履歴への記録と、商品の登録・分類待ちを別々に行う。
fn checkout(connection: &mut Connection, body: &str) -> Result<Outcome, String> {
    let Ok(body) = serde_json::from_str::<CheckoutBody>(body) else {
        return Ok(Outcome::bad("購入完了ページを読めません"));
    };
    let Ok(url) = Url::parse(&body.href) else {
        return Ok(Outcome::bad("購入完了ページではありません"));
    };
    let path_id = url.path().strip_prefix("/orders/").unwrap_or_default();
    if !crate::browser_auto::checkout_url(&url) || path_id != body.order_id || !purchase_walk::is_order_id(path_id) {
        return Ok(Outcome::bad("購入完了ページではありません"));
    }
    let products: Vec<Value> = body
        .products
        .into_iter()
        .filter(|product| {
            product
                .get("boothId")
                .and_then(Value::as_str)
                .is_some_and(purchase_walk::is_order_id)
                && product
                    .get("title")
                    .and_then(Value::as_str)
                    .is_some_and(|title| !title.trim().is_empty() && title.chars().count() <= 1000)
        })
        .map(|product| {
            // 購入完了ページの専用データだけを使う。説明やファイルは受け取らない。
            json!({
                "boothId": product.get("boothId").cloned().unwrap_or(Value::Null),
                "title": product.get("title").cloned().unwrap_or(Value::Null),
                "shop": product.get("shop").and_then(Value::as_str).unwrap_or_default(),
                "url": format!(
                    "https://booth.pm/ja/items/{}",
                    product.get("boothId").and_then(Value::as_str).unwrap_or_default()
                ),
            })
        })
        .collect();
    if products.is_empty() {
        return Ok(Outcome::bad("購入した商品を読めません"));
    }
    let checkout_products: Vec<CheckoutProduct> = products
        .iter()
        .filter_map(|product| serde_json::from_value(product.clone()).ok())
        .collect();
    let recorded = purchases::record_checkout(connection, path_id, &checkout_products)?;
    let before = crate::db::browser_auto_pending(connection)?.len();
    let mut registered = 0u32;
    for product in &products {
        if crate::db::extension_auto_register(connection, product)?.is_some() {
            registered += 1;
        }
    }
    let prompt = crate::db::browser_auto_pending(connection)?.len() > before;
    Ok(Outcome {
        purchases_changed: recorded,
        library_changed: registered > 0,
        prompt,
        ..Outcome::ok(json!({
            "recorded": recorded,
            "registered": registered,
            "needsDetail": recorded,
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let connection = crate::db::open(&dir.path().join("test.sqlite")).unwrap();
        (dir, connection)
    }

    fn json_of(outcome: &Outcome) -> Value {
        serde_json::from_str(&outcome.body).unwrap()
    }

    #[test]
    fn only_the_purchase_paths_are_routed() {
        assert_eq!(route("GET", "/purchases/state"), Some(Route::State));
        assert_eq!(route("POST", "/purchases/list"), Some(Route::List));
        assert_eq!(route("GET", "/purchases/list"), None);
        assert_eq!(route("POST", "/purchases/state"), None);
        assert_eq!(route("POST", "/purchases/checkout"), Some(Route::Checkout));
        assert_eq!(route("POST", "/purchases/../import"), None);
        assert!(!Route::Pending.reads_body());
        assert!(Route::Detail.reads_body());
    }

    #[test]
    fn a_list_page_from_the_history_is_validated_and_stored() {
        let (_dir, mut c) = db();
        let body = json!({
            "href": "https://accounts.booth.pm/orders?page=2",
            "orders": [
                {"orderId": "90000001", "state": "completed", "orderedAt": "2026-11-02T09:30:20+09:00", "title": "見本"},
                {"orderId": "../1", "state": "completed"},
                {"orderId": "90000002", "state": "cancelled"},
            ],
        })
        .to_string();
        let outcome = handle(&mut c, Route::List, &body).unwrap();
        assert_eq!(outcome.status, 200);
        let value = json_of(&outcome);
        assert_eq!(value["added"], 1);
        assert_eq!(value["news"], true);
        assert_eq!(json_of(&handle(&mut c, Route::Pending, "").unwrap())["ids"], json!(["90000001"]));
        let again = handle(&mut c, Route::List, &body).unwrap();
        assert_eq!(json_of(&again)["news"], false);
        for href in ["https://accounts.booth.pm/library", "https://evil.example/orders", "https://accounts.booth.pm/orders/90000001"] {
            let outcome = handle(&mut c, Route::List, &json!({"href": href, "orders": []}).to_string()).unwrap();
            assert_eq!(outcome.status, 400, "{href}");
        }
    }

    #[test]
    fn a_detail_must_match_its_page_and_finishing_marks_the_list_complete() {
        let (_dir, mut c) = db();
        handle(
            &mut c,
            Route::List,
            &json!({"href": "https://accounts.booth.pm/orders", "orders": [{"orderId": "90000001", "state": "completed"}]}).to_string(),
        )
        .unwrap();
        let detail = json!({
            "orderId": "90000001",
            "state": "completed",
            "orderedAt": "2026-11-02T09:30:20+09:00",
            "totalYen": 1500,
            "gift": false,
            "items": [{"boothId": "123456", "name": "見本の衣装", "shop": "見本の店", "priceYen": 1500, "quantity": 1}],
        });
        let wrong = handle(
            &mut c,
            Route::Detail,
            &json!({"href": "https://accounts.booth.pm/orders/90000002", "detail": detail}).to_string(),
        )
        .unwrap();
        assert_eq!(wrong.status, 400);
        let right = handle(
            &mut c,
            Route::Detail,
            &json!({"href": "https://accounts.booth.pm/orders/90000001", "detail": detail}).to_string(),
        )
        .unwrap();
        assert_eq!(right.status, 200);
        assert_eq!(json_of(&handle(&mut c, Route::Pending, "").unwrap())["ids"], json!([]));
        assert_eq!(json_of(&handle(&mut c, Route::State, "").unwrap())["complete"], false);
        handle(&mut c, Route::Finish, r#"{"listComplete":true}"#).unwrap();
        assert_eq!(json_of(&handle(&mut c, Route::State, "").unwrap())["complete"], true);
    }

    #[test]
    fn a_checkout_records_the_order_and_queues_classification_without_the_builtin_browser() {
        let (_dir, mut c) = db();
        assert!(!crate::db::settings(&c).unwrap().builtin_browser);
        let body = json!({
            "href": "https://checkout.booth.pm/orders/90000003",
            "orderId": "90000003",
            "products": [
                {"boothId": "123456", "title": "見本の衣装", "shop": "見本の店", "description": "受け取らない説明"},
                {"boothId": "abc", "title": "壊れた商品"},
            ],
        })
        .to_string();
        let outcome = handle(&mut c, Route::Checkout, &body).unwrap();
        assert_eq!(outcome.status, 200);
        assert!(outcome.purchases_changed && outcome.library_changed && outcome.prompt);
        assert_eq!(json_of(&outcome)["registered"], 1);
        assert_eq!(crate::db::browser_auto_pending(&c).unwrap().len(), 1);
        let description: String = c
            .query_row("SELECT description_snippet FROM products WHERE booth_id='123456'", [], |r| r.get(0))
            .unwrap();
        assert!(!description.contains("受け取らない"));
        assert_eq!(json_of(&handle(&mut c, Route::Pending, "").unwrap())["ids"], json!(["90000003"]));
        let again = handle(&mut c, Route::Checkout, &body).unwrap();
        assert!(!again.purchases_changed && !again.prompt);
        for (href, id) in [
            ("https://checkout.booth.pm/orders/90000004", "90000003"),
            ("https://booth.pm/orders/90000003", "90000003"),
            ("https://checkout.booth.pm/orders/90000003/other", "90000003"),
        ] {
            let outcome = handle(
                &mut c,
                Route::Checkout,
                &json!({"href": href, "orderId": id, "products": [{"boothId": "123456", "title": "見本"}]}).to_string(),
            )
            .unwrap();
            assert_eq!(outcome.status, 400, "{href}");
        }
    }
}
