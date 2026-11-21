//! 拡張のサイドパネルに出すライブラリ。アバターの並び、アバターごと・ジャンルごとの商品、
//! 保存済みのサムネイル、見ている商品の購入の記録を返す。読むだけで、何も書かない。
//! 「アプリで開く」は、登録済みの商品 1 件の画面をアプリに開かせる。本人が押したときだけ届く。
//! 返すのは ID・商品名・ショップ・印・注文番号・日時だけ。パス・説明・URL・金額は返さない。
//! 並べ方と分け方はアプリの画面と同じ `view` の規則を使い、拡張に複製しない。
use crate::{db, purchases, view};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    Avatars,
    Items,
    Thumb,
    Product,
    Open,
}

impl Route {
    pub(crate) fn reads_body(self) -> bool {
        !matches!(self, Route::Avatars)
    }
}

pub(crate) fn route(method: &str, path: &str) -> Option<Route> {
    // 拡張は Origin を付けるために読み取りも POST で送る。
    if !method.eq_ignore_ascii_case("POST") {
        return None;
    }
    match path {
        "/library/avatars" => Some(Route::Avatars),
        "/library/items" => Some(Route::Items),
        "/library/thumb" => Some(Route::Thumb),
        "/library/product" => Some(Route::Product),
        "/library/open" => Some(Route::Open),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Reply {
    Json { status: u16, body: String },
    Image { content_type: &'static str, bytes: Vec<u8> },
    /// アプリのメイン画面をこの場所（`#/assets/item/<ID>` など）へ移して、窓を前に出す。
    Open { route: String },
}

impl Reply {
    fn ok(body: Value) -> Self {
        Reply::Json {
            status: 200,
            body: body.to_string(),
        }
    }

    fn error(status: u16, message: &str) -> Self {
        Reply::Json {
            status,
            body: json!({ "error": message }).to_string(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemsBody {
    /// アバターの ID。無いときは `kind` の一覧。
    #[serde(default)]
    avatar: Option<String>,
    /// `asset`（アセットすべて）か `world`。
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdBody {
    id: String,
}

fn valid_id(id: &str) -> bool {
    (4..=12).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_digit())
}

/// サムネイル以外。DB を読む。
pub(crate) fn handle(connection: &Connection, route: Route, body: &str) -> Result<Reply, String> {
    match route {
        Route::Avatars => {
            let snapshot = db::load_snapshot(connection)?;
            let count = |kind: &str| db::product_ids_by_genre(connection, kind, None).map(|ids| ids.len());
            Ok(Reply::ok(json!({
                "avatars": view::panel_avatars(&snapshot),
                "assetCount": count("asset")?,
                "worldCount": count("world")?,
            })))
        }
        Route::Items => {
            let Ok(body) = serde_json::from_str::<ItemsBody>(body) else {
                return Ok(Reply::error(400, "一覧の指定を読めません"));
            };
            let snapshot = db::load_snapshot(connection)?;
            let sections = match (body.avatar.as_deref(), body.kind.as_deref()) {
                (Some(avatar), None) if avatar.len() <= 200 => {
                    let Some(sections) = view::panel_avatar_sections(&snapshot, avatar) else {
                        return Ok(Reply::error(404, "アバターが見つかりません"));
                    };
                    sections
                }
                (None, Some(kind)) => {
                    let Some(genres) = crate::genre::kind_genres(kind) else {
                        return Ok(Reply::error(400, "一覧の指定を読めません"));
                    };
                    let ids = db::product_ids_by_genre(connection, kind, None)?;
                    view::panel_kind_sections(&snapshot, genres, &ids)
                }
                _ => return Ok(Reply::error(400, "一覧の指定を読めません")),
            };
            Ok(Reply::ok(json!({ "sections": sections })))
        }
        Route::Product => {
            let Some(id) = serde_json::from_str::<IdBody>(body).ok().map(|body| body.id).filter(|id| valid_id(id)) else {
                return Ok(Reply::error(400, "商品IDを読めません"));
            };
            product(connection, &id).map(Reply::ok)
        }
        Route::Open => {
            let Some(id) = serde_json::from_str::<IdBody>(body).ok().map(|body| body.id).filter(|id| valid_id(id)) else {
                return Ok(Reply::error(400, "商品IDを読めません"));
            };
            Ok(match app_route(connection, &id)? {
                Some(route) => Reply::Open { route },
                None => Reply::error(404, "この商品は、まだアプリで開けません。アプリに登録して分類すると開けます"),
            })
        }
        Route::Thumb => Ok(Reply::error(404, "サムネイルがありません")),
    }
}

/// アプリの中でその商品を開く先。購入履歴の画面のリンクと同じ条件と行き先で、登録済み・除外していない・
/// カテゴリの決まった商品だけ。アセットとワールドは一覧の中のその商品、アバター本体はアバターの画面。
pub(crate) fn app_route(connection: &Connection, id: &str) -> Result<Option<String>, String> {
    let failed = |_| "商品を読めません".to_string();
    let category: Option<Option<String>> = connection
        .query_row(
            "SELECT category FROM products p WHERE booth_id = ?1
             AND NOT EXISTS(SELECT 1 FROM product_exclusions e WHERE e.booth_id = p.booth_id)",
            [id],
            |row| row.get(0),
        )
        .optional()
        .map_err(failed)?;
    let Some(category) = category.flatten().filter(|category| !category.is_empty()) else {
        return Ok(None);
    };
    Ok(match category.as_str() {
        "avatar" => {
            let mut statement = connection
                .prepare("SELECT avatar_id FROM product_avatars WHERE product_id = ?1 LIMIT 2")
                .map_err(failed)?;
            let avatars = statement
                .query_map([id], |row| row.get::<_, String>(0))
                .map_err(failed)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(failed)?;
            match avatars.as_slice() {
                [avatar] => Some(format!("#/avatar/{}", encode_component(avatar))),
                _ => None,
            }
        }
        "world" => Some(format!("#/worlds/item/{id}")),
        _ => Some(format!("#/assets/item/{id}")),
    })
}

/// JavaScript の `encodeURIComponent` と同じ文字を残す。画面は `decodeURIComponent` で読む。
fn encode_component(text: &str) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// 見ている 1 商品の購入の記録と、購入した対応版。金額は返さない。
fn product(connection: &Connection, id: &str) -> Result<Value, String> {
    let failed = |_| "購入の記録を読めません".to_string();
    let mut statement = connection
        .prepare(
            "SELECT o.order_id, o.ordered_at, COALESCE(o.gift_override, o.gift_detected)
             FROM purchase_orders o
             WHERE EXISTS(SELECT 1 FROM purchase_items i WHERE i.order_id = o.order_id AND i.booth_id = ?1)
             ORDER BY o.ordered_at IS NULL, o.ordered_at DESC, o.order_id DESC",
        )
        .map_err(failed)?;
    let orders = statement
        .query_map([id], |row| {
            Ok(json!({
                "orderId": row.get::<_, String>(0)?,
                "orderedAt": row.get::<_, Option<String>>(1)?,
                "gift": row.get::<_, Option<i64>>(2)?.unwrap_or(0) != 0,
            }))
        })
        .map_err(failed)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(failed)?;
    let order_count = orders.len();
    Ok(json!({
        "boothId": id,
        "historyRead": purchases::history_read(connection)?,
        "orderCount": order_count,
        "orders": orders.into_iter().take(3).collect::<Vec<_>>(),
        "appLink": app_route(connection, id)?.is_some(),
        "edition": edition(connection, id)?,
    }))
}

/// 購入した対応版。対応先のアバターがある、登録済みのアバター本体以外の商品だけに返す。
fn edition(connection: &Connection, id: &str) -> Result<Value, String> {
    let failed = |_| "購入した対応版を読めません".to_string();
    let category: Option<Option<String>> = connection
        .query_row(
            "SELECT category FROM products p WHERE booth_id = ?1
             AND NOT EXISTS(SELECT 1 FROM product_exclusions e WHERE e.booth_id = p.booth_id)",
            [id],
            |row| row.get(0),
        )
        .optional()
        .map_err(failed)?;
    let linked: bool = connection
        .query_row("SELECT EXISTS(SELECT 1 FROM product_avatars WHERE product_id = ?1)", [id], |row| row.get(0))
        .map_err(failed)?;
    let Some(category) = category else {
        return Ok(Value::Null);
    };
    if !linked || matches!(category.as_deref(), Some("avatar" | "world")) {
        return Ok(Value::Null);
    }
    let choice = crate::purchased_editions::Catalog::load(connection)?.scope(id);
    let mut names = Vec::new();
    for avatar in &choice.avatar_ids {
        let name: Option<String> = connection
            .query_row("SELECT display_name FROM avatars WHERE id = ?1", params![avatar], |row| row.get(0))
            .optional()
            .map_err(failed)?;
        names.extend(name);
    }
    Ok(json!({ "mode": choice.mode, "avatars": names }))
}

const THUMB_TYPES: &[(&str, &str)] = &[
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("png", "image/png"),
    ("webp", "image/webp"),
    ("gif", "image/gif"),
];
/// 保存するサムネイルの上限（`thumb.rs`）と同じ。
const THUMB_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// 保存済みのサムネイル。名前は商品 ID（数字だけ）から組み立て、受け取ったパスは使わない。
/// リンクはたどらない。DB は読まない。
pub(crate) fn thumbnail(thumbs: &Path, body: &str) -> Reply {
    let Some(id) = serde_json::from_str::<IdBody>(body).ok().map(|body| body.id).filter(|id| valid_id(id)) else {
        return Reply::error(400, "商品IDを読めません");
    };
    for (extension, content_type) in THUMB_TYPES {
        let path = thumbs.join(format!("{id}.{extension}"));
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > THUMB_MAX_BYTES {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&path) {
            return Reply::Image { content_type, bytes };
        }
    }
    Reply::error(404, "サムネイルがありません")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let connection = db::open(&dir.path().join("test.sqlite")).unwrap();
        connection
            .execute_batch(
                "DELETE FROM product_avatars; DELETE FROM products; DELETE FROM avatars;
                 INSERT INTO avatars(id,display_name,base_body,owned) VALUES ('a','見本A',NULL,1),('b','見本B',NULL,0);
                 INSERT INTO products(booth_id,title,shop,category,genre,imported_at,updated_at) VALUES
                   ('1000','見本A本体','店','avatar',NULL,'',''),
                   ('2000','見本の衣装','店','outfit','outfit','',''),
                   ('3000','見本の家具','店','world','furniture_props','',''),
                   ('4000','除外した衣装','店','outfit','outfit','','');
                 INSERT INTO product_avatars VALUES ('1000','a','manual'),('2000','a','manual');
                 INSERT INTO product_exclusions(booth_id,excluded_at) VALUES ('4000','');",
            )
            .unwrap();
        (dir, connection)
    }

    fn json_of(reply: Reply) -> (u16, Value) {
        match reply {
            Reply::Json { status, body } => (status, serde_json::from_str(&body).unwrap()),
            Reply::Image { .. } | Reply::Open { .. } => panic!("expected JSON"),
        }
    }

    #[test]
    fn routes_are_post_only_and_only_the_avatar_list_has_no_body() {
        assert_eq!(route("POST", "/library/avatars"), Some(Route::Avatars));
        assert_eq!(route("post", "/library/items"), Some(Route::Items));
        assert_eq!(route("POST", "/library/thumb"), Some(Route::Thumb));
        assert_eq!(route("POST", "/library/product"), Some(Route::Product));
        assert_eq!(route("POST", "/library/open"), Some(Route::Open));
        assert_eq!(route("GET", "/library/avatars"), None);
        assert_eq!(route("POST", "/library"), None);
        assert!(!Route::Avatars.reads_body() && Route::Items.reads_body());
    }

    #[test]
    fn avatars_and_items_come_from_the_app_views_without_paths_or_excluded_products() {
        let (_dir, connection) = open();
        let (status, avatars) = json_of(handle(&connection, Route::Avatars, "").unwrap());
        assert_eq!(status, 200);
        assert_eq!(avatars["avatars"].as_array().unwrap().len(), 1);
        assert_eq!(avatars["avatars"][0]["displayName"], "見本A");
        assert_eq!(avatars["assetCount"], 1);
        assert_eq!(avatars["worldCount"], 1);

        let (_, world) = json_of(handle(&connection, Route::Items, r#"{"kind":"world"}"#).unwrap());
        assert_eq!(world["sections"][0]["key"], "furniture_props");
        assert_eq!(world["sections"][0]["items"][0]["boothId"], "3000");
        let (_, assets) = json_of(handle(&connection, Route::Items, r#"{"kind":"asset"}"#).unwrap());
        let text = assets.to_string();
        assert!(text.contains("見本の衣装") && !text.contains("除外した衣装"));
        let item = &assets["sections"][0]["items"][0];
        let mut keys: Vec<&str> = item.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["boothId", "hasThumbnail", "isAdult", "shop", "title", "unpublished", "viaBaseBody"]);

        for body in ["{}", r#"{"kind":"avatar"}"#, r#"{"avatar":"a","kind":"asset"}"#, r#"{"path":"/"}"#, "x"] {
            assert_eq!(json_of(handle(&connection, Route::Items, body).unwrap()).0, 400, "{body}");
        }
        assert_eq!(json_of(handle(&connection, Route::Items, r#"{"avatar":"nobody"}"#).unwrap()).0, 404);
    }

    #[test]
    fn a_product_reports_its_orders_without_prices_and_its_purchased_edition() {
        let (_dir, connection) = open();
        let (_, before) = json_of(handle(&connection, Route::Product, r#"{"id":"2000"}"#).unwrap());
        assert_eq!(before["historyRead"], false);
        assert_eq!(before["orderCount"], 0);
        connection
            .execute_batch(
                "INSERT INTO purchase_orders(order_id,ordered_at,total_yen,gift_detected,gift_override,source,recorded_at,updated_at) VALUES
                   ('11111111','2026-01-05T10:00:00+09:00',1500,0,NULL,'browser','',''),
                   ('22222222','2026-03-14T21:30:00+09:00',3000,0,1,'browser','',''),
                   ('33333333',NULL,NULL,0,NULL,'browser','','');
                 INSERT INTO purchase_items(order_id,position,booth_id,name,price_yen) VALUES
                   ('11111111',0,'2000','見本の衣装',1500),('22222222',0,'2000','見本の衣装',3000),('33333333',0,'9999','別の商品',100);",
            )
            .unwrap();
        let (status, detail) = json_of(handle(&connection, Route::Product, r#"{"id":"2000"}"#).unwrap());
        assert_eq!(status, 200);
        assert_eq!(detail["historyRead"], true);
        assert_eq!(detail["orderCount"], 2);
        assert_eq!(detail["orders"][0]["orderId"], "22222222");
        assert_eq!(detail["orders"][0]["gift"], true);
        assert_eq!(detail["orders"][1]["gift"], false);
        assert_eq!(detail["edition"]["avatars"][0], "見本A");
        let text = detail.to_string();
        assert!(!text.contains("1500") && !text.contains("3000") && !text.contains("yen"), "no price leaves the app");
        // 本体・未登録・除外した商品には対応版を返さない。
        for id in ["1000", "9999", "4000"] {
            assert!(json_of(handle(&connection, Route::Product, &format!(r#"{{"id":"{id}"}}"#)).unwrap()).1["edition"].is_null(), "{id}");
        }
        for body in [r#"{"id":"12"}"#, r#"{"id":"../1000"}"#, r#"{"id":"1000","x":1}"#, ""] {
            assert_eq!(json_of(handle(&connection, Route::Product, body).unwrap()).0, 400, "{body}");
        }
    }

    #[test]
    fn open_in_app_goes_to_the_same_places_as_the_purchase_history_links() {
        let (_dir, connection) = open();
        connection
            .execute_batch(
                "INSERT INTO products(booth_id,title,shop,category,imported_at,updated_at) VALUES ('5000','未分類','店',NULL,'',''),('6000','本体2','店','avatar','','');
                 INSERT INTO avatars(id,display_name,owned) VALUES ('owned 6000/あ','仮',1);
                 INSERT INTO product_avatars VALUES ('6000','owned 6000/あ','manual');",
            )
            .unwrap();
        let open = |id: &str| handle(&connection, Route::Open, &format!(r#"{{"id":"{id}"}}"#)).unwrap();
        assert_eq!(open("2000"), Reply::Open { route: "#/assets/item/2000".into() });
        assert_eq!(open("3000"), Reply::Open { route: "#/worlds/item/3000".into() });
        assert_eq!(open("1000"), Reply::Open { route: "#/avatar/a".into() });
        assert_eq!(open("6000"), Reply::Open { route: "#/avatar/owned%206000%2F%E3%81%82".into() });
        // 未登録・除外・未分類の商品は開かない。
        for id in ["9999", "4000", "5000"] {
            assert_eq!(json_of(open(id)).0, 404, "{id}");
        }
        for body in [r#"{"id":"12"}"#, r##"{"id":"#/settings"}"##, r#"{"route":"/settings"}"#, ""] {
            assert_eq!(json_of(handle(&connection, Route::Open, body).unwrap()).0, 400, "{body}");
        }
        let (_, detail) = json_of(handle(&connection, Route::Product, r#"{"id":"2000"}"#).unwrap());
        assert_eq!(detail["appLink"], true);
        let (_, missing) = json_of(handle(&connection, Route::Product, r#"{"id":"5000"}"#).unwrap());
        assert_eq!(missing["appLink"], false);
    }

    #[test]
    fn a_thumbnail_is_read_by_product_id_only_and_never_through_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let thumbs = dir.path().join("thumbs");
        std::fs::create_dir_all(&thumbs).unwrap();
        std::fs::write(thumbs.join("123456.png"), b"png-bytes").unwrap();
        std::fs::write(dir.path().join("secret.jpg"), b"secret").unwrap();
        assert_eq!(
            thumbnail(&thumbs, r#"{"id":"123456"}"#),
            Reply::Image { content_type: "image/png", bytes: b"png-bytes".to_vec() }
        );
        assert_eq!(json_of(thumbnail(&thumbs, r#"{"id":"654321"}"#)).0, 404);
        for body in [r#"{"id":"../secret"}"#, r#"{"id":"123456.png"}"#, r#"{"id":""}"#, r#"{"path":"x"}"#, "nope"] {
            assert_eq!(json_of(thumbnail(&thumbs, body)).0, 400, "{body}");
        }
        std::fs::write(thumbs.join("222222.jpg"), b"").unwrap();
        assert_eq!(json_of(thumbnail(&thumbs, r#"{"id":"222222"}"#)).0, 404, "an empty file is not an image");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path().join("secret.jpg"), thumbs.join("333333.jpg")).unwrap();
            assert_eq!(json_of(thumbnail(&thumbs, r#"{"id":"333333"}"#)).0, 404);
        }
    }
}
