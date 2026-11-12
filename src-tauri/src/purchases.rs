//! 購入履歴の保存、検索、ラベル、アバター別の集計。
//! 保存するのは注文番号、注文日時、金額、商品 ID・名前・ショップ、ギフトかどうかだけ。
//! 支払方法、住所、氏名、ギフト用 URL は受け取らず、保存もしない。
//! 確定した注文だけを残し、キャンセルと支払い待ちは行ごと消す。

use crate::db::{set_setting, setting};
use crate::model::now_iso;
use crate::normalize::nfkc;
use crate::purchase_walk::{
    DetailApplied, ListApplied, ListOrder, OrderDetail, OrderState, is_order_id,
};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub(crate) const TABLES: &str = "
CREATE TABLE IF NOT EXISTS purchase_orders (
    order_id TEXT PRIMARY KEY,
    ordered_at TEXT,
    total_yen INTEGER,
    gift_detected INTEGER NOT NULL DEFAULT 0,
    gift_override INTEGER,
    list_title TEXT NOT NULL DEFAULT '',
    detail_read_at TEXT,
    source TEXT NOT NULL,
    recorded_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS purchase_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    order_id TEXT NOT NULL REFERENCES purchase_orders(order_id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    booth_id TEXT NOT NULL,
    name TEXT NOT NULL,
    shop TEXT NOT NULL DEFAULT '',
    price_yen INTEGER,
    boost_yen INTEGER,
    quantity INTEGER NOT NULL DEFAULT 1,
    UNIQUE (order_id, booth_id, name)
);
CREATE INDEX IF NOT EXISTS idx_purchase_items_product ON purchase_items(booth_id);
CREATE TABLE IF NOT EXISTS purchase_labels (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS purchase_item_labels (
    item_id INTEGER NOT NULL REFERENCES purchase_items(id) ON DELETE CASCADE,
    label_id INTEGER NOT NULL REFERENCES purchase_labels(id) ON DELETE CASCADE,
    PRIMARY KEY (item_id, label_id)
);
";

/// 一覧を一度最後まで読んだ印。以後の読み込みは新しい注文の無いページで止める。
const COMPLETE_KEY: &str = "purchase_history_complete";
const READ_AT_KEY: &str = "purchase_history_read_at";
pub(crate) const SHOW_PRICES_KEY: &str = "purchase_show_prices";
pub const PAGE_SIZE: u32 = 50;
const MAX_PAGE_SIZE: u32 = 200;
const LABEL_MAX_CHARS: usize = 30;
const LABEL_LIMIT: i64 = 100;
pub const GIFT_LABEL: &str = "ギフト（贈った）";
const GENERIC_KEY: &str = "generic";
const UNCLASSIFIED_KEY: &str = "unclassified";

fn sql<T>(result: rusqlite::Result<T>) -> Result<T, String> {
    result.map_err(|err| err.to_string())
}

/// 日本時間の今。BOOTH の注文日時と同じ形にして、並べ替えで混ぜられるようにする。
pub(crate) fn jst_now() -> String {
    let jst = chrono::FixedOffset::east_opt(9 * 3600).expect("JST offset");
    chrono::Utc::now()
        .with_timezone(&jst)
        .format("%Y-%m-%dT%H:%M:%S+09:00")
        .to_string()
}

fn delete_order(connection: &Connection, order_id: &str) -> Result<bool, String> {
    sql(connection.execute(
        "DELETE FROM purchase_item_labels WHERE item_id IN (SELECT id FROM purchase_items WHERE order_id = ?1)",
        [order_id],
    ))?;
    sql(connection.execute("DELETE FROM purchase_items WHERE order_id = ?1", [order_id]))?;
    Ok(sql(connection.execute("DELETE FROM purchase_orders WHERE order_id = ?1", [order_id]))? > 0)
}

fn order_exists(connection: &Connection, order_id: &str) -> Result<bool, String> {
    sql(connection.query_row(
        "SELECT EXISTS (SELECT 1 FROM purchase_orders WHERE order_id = ?1)",
        [order_id],
        |row| row.get(0),
    ))
}

/// 一覧 1 ページ分。確定した注文を足し、キャンセルと支払い待ちになった注文を消す。
/// 詳細を読み終えた注文の日時や商品は、一覧の値で上書きしない。
pub(crate) fn apply_list_page(connection: &mut Connection, orders: &[ListOrder]) -> Result<ListApplied, String> {
    let tx = sql(connection.transaction())?;
    let mut applied = ListApplied::default();
    let now = now_iso();
    let recorded = jst_now();
    for order in orders {
        if !is_order_id(&order.order_id) {
            continue;
        }
        let exists = order_exists(&tx, &order.order_id)?;
        match order.state {
            OrderState::Cancelled | OrderState::Pending => {
                if exists && delete_order(&tx, &order.order_id)? {
                    applied.removed += 1;
                    applied.news = true;
                }
            }
            OrderState::Confirmed if exists => {
                sql(tx.execute(
                    "UPDATE purchase_orders
                     SET ordered_at = COALESCE(ordered_at, ?2),
                         list_title = CASE WHEN list_title = '' THEN ?3 ELSE list_title END
                     WHERE order_id = ?1",
                    params![order.order_id, order.ordered_at, order.title],
                ))?;
            }
            OrderState::Confirmed => {
                sql(tx.execute(
                    "INSERT INTO purchase_orders (order_id, ordered_at, list_title, source, recorded_at, updated_at)
                     VALUES (?1, ?2, ?3, 'history', ?4, ?5)",
                    params![order.order_id, order.ordered_at, order.title, recorded, now],
                ))?;
                applied.added += 1;
                applied.news = true;
            }
        }
    }
    sql(tx.commit())?;
    Ok(applied)
}

/// 注文 1 件の詳細。日時、金額、ギフトの印、商品を詳細の値にそろえる。
/// 同じ商品の行は ID を保ち、名前が変わった（購入完了ページでバリエーションが無かった）行のラベルは引き継ぐ。
pub(crate) fn apply_detail(connection: &mut Connection, detail: &OrderDetail) -> Result<DetailApplied, String> {
    if !is_order_id(&detail.order_id) {
        return Err("注文番号が正しくありません".into());
    }
    let tx = sql(connection.transaction())?;
    if matches!(detail.state, OrderState::Cancelled | OrderState::Pending) {
        let removed = delete_order(&tx, &detail.order_id)?;
        sql(tx.commit())?;
        return Ok(DetailApplied { removed });
    }
    let now = now_iso();
    let title = detail.items.first().map(|item| item.name.clone()).unwrap_or_default();
    sql(tx.execute(
        "INSERT INTO purchase_orders
            (order_id, ordered_at, total_yen, gift_detected, list_title, detail_read_at, source, recorded_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'history', ?7, ?6)
         ON CONFLICT(order_id) DO UPDATE SET
            ordered_at = COALESCE(excluded.ordered_at, purchase_orders.ordered_at),
            total_yen = COALESCE(excluded.total_yen, purchase_orders.total_yen),
            gift_detected = excluded.gift_detected,
            list_title = CASE WHEN purchase_orders.list_title = '' THEN excluded.list_title ELSE purchase_orders.list_title END,
            detail_read_at = excluded.detail_read_at,
            updated_at = excluded.updated_at",
        params![
            detail.order_id,
            detail.ordered_at,
            detail.total_yen,
            detail.gift,
            title,
            now,
            jst_now()
        ],
    ))?;
    if !detail.items.is_empty() {
        let mut kept: Vec<(i64, String)> = Vec::new();
        for (position, item) in detail.items.iter().enumerate() {
            sql(tx.execute(
                "INSERT INTO purchase_items (order_id, position, booth_id, name, shop, price_yen, boost_yen, quantity)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(order_id, booth_id, name) DO UPDATE SET
                    position = excluded.position,
                    shop = excluded.shop,
                    price_yen = excluded.price_yen,
                    boost_yen = excluded.boost_yen,
                    quantity = excluded.quantity",
                params![
                    detail.order_id,
                    position as i64,
                    item.booth_id,
                    item.name,
                    item.shop,
                    item.price_yen,
                    item.boost_yen,
                    item.quantity
                ],
            ))?;
            let id: i64 = sql(tx.query_row(
                "SELECT id FROM purchase_items WHERE order_id = ?1 AND booth_id = ?2 AND name = ?3",
                params![detail.order_id, item.booth_id, item.name],
                |row| row.get(0),
            ))?;
            kept.push((id, item.booth_id.clone()));
        }
        let stale: Vec<(i64, String)> = {
            let mut statement = sql(tx.prepare("SELECT id, booth_id FROM purchase_items WHERE order_id = ?1"))?;
            let rows = sql(statement.query_map([&detail.order_id], |row| Ok((row.get(0)?, row.get(1)?))))?;
            let all: Vec<(i64, String)> = sql(rows.collect())?;
            all.into_iter()
                .filter(|(id, _)| !kept.iter().any(|(kept_id, _)| kept_id == id))
                .collect()
        };
        for (old, booth_id) in stale {
            if let Some((new, _)) = kept.iter().find(|(_, kept_booth)| *kept_booth == booth_id) {
                sql(tx.execute(
                    "INSERT OR IGNORE INTO purchase_item_labels (item_id, label_id)
                     SELECT ?1, label_id FROM purchase_item_labels WHERE item_id = ?2",
                    params![new, old],
                ))?;
            }
            sql(tx.execute("DELETE FROM purchase_item_labels WHERE item_id = ?1", [old]))?;
            sql(tx.execute("DELETE FROM purchase_items WHERE id = ?1", [old]))?;
        }
    }
    sql(tx.commit())?;
    Ok(DetailApplied::default())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutProduct {
    #[serde(default, alias = "booth_id")]
    pub booth_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub shop: String,
}

/// 内蔵ブラウザで本人が開いた購入完了ページの注文。日時と金額は、詳細を読むまで空のまま。
pub(crate) fn record_checkout(
    connection: &mut Connection,
    order_id: &str,
    products: &[CheckoutProduct],
) -> Result<bool, String> {
    if !is_order_id(order_id) {
        return Err("注文番号が正しくありません".into());
    }
    let products: Vec<&CheckoutProduct> = products
        .iter()
        .filter(|product| is_order_id(&product.booth_id) && !product.title.trim().is_empty())
        .collect();
    if products.is_empty() {
        return Ok(false);
    }
    let tx = sql(connection.transaction())?;
    if order_exists(&tx, order_id)? {
        return Ok(false);
    }
    let now = now_iso();
    sql(tx.execute(
        "INSERT INTO purchase_orders (order_id, list_title, source, recorded_at, updated_at)
         VALUES (?1, ?2, 'checkout', ?3, ?4)",
        params![order_id, products[0].title.trim(), jst_now(), now],
    ))?;
    for (position, product) in products.iter().enumerate() {
        sql(tx.execute(
            "INSERT OR IGNORE INTO purchase_items (order_id, position, booth_id, name, shop)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![order_id, position as i64, product.booth_id, product.title.trim(), product.shop.trim()],
        ))?;
    }
    sql(tx.commit())?;
    Ok(true)
}

/// 詳細をまだ読んでいない注文。新しい順。
pub(crate) fn pending_detail_ids(connection: &Connection) -> Result<Vec<String>, String> {
    let mut statement = sql(connection.prepare(
        "SELECT order_id FROM purchase_orders WHERE detail_read_at IS NULL
         ORDER BY COALESCE(ordered_at, recorded_at) DESC, order_id DESC",
    ))?;
    let rows = sql(statement.query_map([], |row| row.get(0)))?;
    sql(rows.collect())
}

pub(crate) fn history_complete(connection: &Connection) -> Result<bool, String> {
    Ok(setting(connection, COMPLETE_KEY)?.as_deref() == Some("1"))
}

pub(crate) fn finish_walk(connection: &Connection, list_complete: bool) -> Result<(), String> {
    if list_complete {
        set_setting(connection, COMPLETE_KEY, "1")?;
    }
    set_setting(connection, READ_AT_KEY, &now_iso())
}

pub(crate) fn show_prices(connection: &Connection) -> Result<bool, String> {
    Ok(setting(connection, SHOW_PRICES_KEY)?.as_deref() == Some("1"))
}

pub(crate) fn set_show_prices(connection: &Connection, show: bool) -> Result<(), String> {
    set_setting(connection, SHOW_PRICES_KEY, if show { "1" } else { "0" })
}

struct StoredItem {
    id: i64,
    booth_id: String,
    name: String,
    shop: String,
    price_yen: Option<i64>,
    boost_yen: Option<i64>,
    quantity: i64,
    labels: Vec<i64>,
}

struct StoredOrder {
    order_id: String,
    ordered_at: Option<String>,
    recorded_at: String,
    total_yen: Option<i64>,
    gift_detected: bool,
    gift_override: Option<bool>,
    list_title: String,
    detail_read: bool,
    source: String,
    items: Vec<StoredItem>,
}

impl StoredOrder {
    fn gift(&self) -> bool {
        self.gift_override.unwrap_or(self.gift_detected)
    }

    fn sort_key(&self) -> &str {
        self.ordered_at.as_deref().unwrap_or(&self.recorded_at)
    }

    fn year(&self) -> Option<i32> {
        self.sort_key().get(0..4)?.parse().ok()
    }
}

fn load_orders(connection: &Connection) -> Result<Vec<StoredOrder>, String> {
    let mut labels: HashMap<i64, Vec<i64>> = HashMap::new();
    {
        let mut statement = sql(connection.prepare(
            "SELECT item_id, label_id FROM purchase_item_labels ORDER BY item_id, label_id",
        ))?;
        let rows = sql(statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))))?;
        for row in rows {
            let (item, label) = sql(row)?;
            labels.entry(item).or_default().push(label);
        }
    }
    let mut items: HashMap<String, Vec<StoredItem>> = HashMap::new();
    {
        let mut statement = sql(connection.prepare(
            "SELECT id, order_id, booth_id, name, shop, price_yen, boost_yen, quantity
             FROM purchase_items ORDER BY order_id, position, id",
        ))?;
        let rows = sql(statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                StoredItem {
                    id: row.get(0)?,
                    booth_id: row.get(2)?,
                    name: row.get(3)?,
                    shop: row.get(4)?,
                    price_yen: row.get(5)?,
                    boost_yen: row.get(6)?,
                    quantity: row.get(7)?,
                    labels: Vec::new(),
                },
            ))
        }))?;
        for row in rows {
            let (order, mut item) = sql(row)?;
            item.labels = labels.remove(&item.id).unwrap_or_default();
            items.entry(order).or_default().push(item);
        }
    }
    let mut statement = sql(connection.prepare(
        "SELECT order_id, ordered_at, recorded_at, total_yen, gift_detected, gift_override, list_title,
                detail_read_at IS NOT NULL, source
         FROM purchase_orders",
    ))?;
    let rows = sql(statement.query_map([], |row| {
        Ok(StoredOrder {
            order_id: row.get(0)?,
            ordered_at: row.get(1)?,
            recorded_at: row.get(2)?,
            total_yen: row.get(3)?,
            gift_detected: row.get(4)?,
            gift_override: row.get(5)?,
            list_title: row.get(6)?,
            detail_read: row.get(7)?,
            source: row.get(8)?,
            items: Vec::new(),
        })
    }))?;
    let mut orders: Vec<StoredOrder> = sql(rows.collect())?;
    for order in &mut orders {
        order.items = items.remove(&order.order_id).unwrap_or_default();
    }
    orders.sort_by(|a, b| {
        b.sort_key()
            .cmp(a.sort_key())
            .then_with(|| b.order_id.len().cmp(&a.order_id.len()))
            .then_with(|| b.order_id.cmp(&a.order_id))
    });
    Ok(orders)
}

#[derive(Clone, Debug)]
struct AvatarRef {
    id: String,
    name: String,
    owned: bool,
}

struct LibraryProduct {
    title: String,
    thumbnail_path: Option<String>,
    thumbnail_url: Option<String>,
    is_generic: bool,
    category: Option<String>,
    excluded: bool,
}

/// 分類の今の値。購入履歴は分類を写さず、表示のたびにここから引く。
struct Library {
    avatars: HashMap<String, AvatarRef>,
    editions: crate::purchased_editions::Catalog,
    products: HashMap<String, LibraryProduct>,
    links: HashMap<String, Vec<AvatarRef>>,
    bodies: HashMap<String, AvatarRef>,
}

fn load_library(connection: &Connection) -> Result<Library, String> {
    let mut products = HashMap::new();
    {
        let mut statement = sql(connection.prepare(
            "SELECT booth_id, title, thumbnail_path, thumbnail_url, is_generic, category,
                    booth_id IN (SELECT booth_id FROM product_exclusions)
             FROM products
             WHERE booth_id IN (SELECT booth_id FROM purchase_items)",
        ))?;
        let rows = sql(statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                LibraryProduct {
                    title: row.get(1)?,
                    thumbnail_path: row.get(2)?,
                    thumbnail_url: row.get(3)?,
                    is_generic: row.get(4)?,
                    category: row.get(5)?,
                    excluded: row.get(6)?,
                },
            ))
        }))?;
        for row in rows {
            let (id, product) = sql(row)?;
            products.insert(id, product);
        }
    }
    let mut links: HashMap<String, Vec<AvatarRef>> = HashMap::new();
    {
        let mut statement = sql(connection.prepare(
            "SELECT pa.product_id, a.id, a.display_name, a.owned
             FROM product_avatars pa JOIN avatars a ON a.id = pa.avatar_id
             WHERE pa.product_id IN (SELECT booth_id FROM purchase_items)
             ORDER BY pa.product_id, a.display_name, a.id",
        ))?;
        let rows = sql(statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                AvatarRef {
                    id: row.get(1)?,
                    name: row.get(2)?,
                    owned: row.get(3)?,
                },
            ))
        }))?;
        for row in rows {
            let (product, avatar) = sql(row)?;
            links.entry(product).or_default().push(avatar);
        }
    }
    let mut bodies = HashMap::new();
    {
        let mut statement = sql(connection.prepare(
            "SELECT s.booth_id, a.id, a.display_name, a.owned
             FROM avatar_sources s JOIN avatars a ON a.id = s.avatar_id
             WHERE s.booth_id IN (SELECT booth_id FROM purchase_items)",
        ))?;
        let rows = sql(statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                AvatarRef {
                    id: row.get(1)?,
                    name: row.get(2)?,
                    owned: row.get(3)?,
                },
            ))
        }))?;
        for row in rows {
            let (product, avatar) = sql(row)?;
            bodies.insert(product, avatar);
        }
    }
    Ok(Library {
        avatars: crate::db::load_avatars(connection)?.into_iter().map(|a| (a.id.clone(), AvatarRef { id: a.id, name: a.display_name, owned: a.owned })).collect(),
        editions: crate::purchased_editions::Catalog::load(connection)?,
        products,
        links,
        bodies,
    })
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseGroup {
    /// アバター・購入版または分類別の枠のキー。
    pub key: String,
    pub kind: &'static str,
    pub label: String,
    pub avatar_id: Option<String>,
    /// アバター本体の購入。
    pub body: bool,
}

fn body_for<'a>(library: &'a Library, booth_id: &str) -> Option<&'a AvatarRef> {
    library.bodies.get(booth_id).or_else(|| {
        if library.products.get(booth_id)?.category.as_deref() != Some("avatar") {
            return None;
        }
        let [avatar] = library.links.get(booth_id)?.as_slice() else { return None };
        Some(avatar)
    })
}

/// 購入版か、アバター別に分けない商品の分類で集計する。
/// 同じ素体のほかのアバターには広げない。
fn groups_for(library: &Library, booth_id: &str, name: &str) -> (Vec<PurchaseGroup>, Vec<bool>) {
    let special = |key: &'static str, label: &str| {
        (vec![PurchaseGroup {
            key: key.into(), kind: key, label: label.into(), avatar_id: None, body: false,
        }], vec![false])
    };
    let product = library.products.get(booth_id);
    if product.is_some_and(|p| p.category.as_deref() == Some("world")) {
        return special("world", "ワールド");
    }
    if let Some(avatar) = body_for(library, booth_id) {
        return (
            vec![PurchaseGroup {
                key: format!("avatar:{}", avatar.id),
                kind: "avatar",
                label: avatar.name.clone(),
                avatar_id: Some(avatar.id.clone()),
                body: true,
            }],
            vec![avatar.owned],
        );
    }
    let hint = product.and_then(|p| variation_of(name, &p.title));
    let title_only = product.is_some_and(|p| {
        name.trim() == p.title.trim()
            || nfkc(name).trim() == nfkc(&p.title).trim()
    });
    let scope = library.editions.order_scope(booth_id, hint.as_deref(), title_only);
    if !scope.avatar_ids.is_empty() || library.links.contains_key(booth_id) {
        let avatars: Vec<_> = scope.avatar_ids.iter().filter_map(|id| library.avatars.get(id)).collect();
        if avatars.len() != 1 {
            let (key, label) = if avatars.len() > 1 { ("multiple", "複数アバター対応") } else { ("unset", "対応版未設定") };
            return special(key, label);
        }
        return (
            avatars
                .iter()
                .map(|avatar| PurchaseGroup {
                    key: format!("avatar:{}", avatar.id),
                    kind: "avatar",
                    label: avatar.name.clone(),
                    avatar_id: Some(avatar.id.clone()),
                    body: false,
                })
                .collect(),
            avatars.iter().map(|avatar| avatar.owned).collect(),
        );
    }
    if product.is_some_and(|product| product.is_generic) {
        special(GENERIC_KEY, "汎用")
    } else {
        special(UNCLASSIFIED_KEY, "分類なし")
    }
}

/// 商品名が「商品名 (バリエーション)」の形なら、括弧の中をバリエーションとして返す。
fn variation_of(name: &str, title: &str) -> Option<String> {
    crate::purchased_editions::variation_of(name, title)
}

fn amount(item: &StoredItem) -> Option<i64> {
    item.price_yen.map(|price| price + item.boost_yen.unwrap_or(0))
}

/// アプリの中でその商品を開く先。アセットとワールドは一覧、アバター本体はアバターの画面。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryLink {
    /// `asset`、`world`、`avatar`。
    pub kind: &'static str,
    pub avatar_id: Option<String>,
}

/// ライブラリにあり、除外しておらず、分類が決まっている商品だけ。贈っただけの商品はライブラリに無いので出ない。
fn library_link(library: &Library, booth_id: &str) -> Option<LibraryLink> {
    let product = library.products.get(booth_id).filter(|product| !product.excluded)?;
    match product.category.as_deref()? {
        "avatar" => body_for(library, booth_id).map(|avatar| LibraryLink {
            kind: "avatar",
            avatar_id: Some(avatar.id.clone()),
        }),
        "world" => Some(LibraryLink {
            kind: "world",
            avatar_id: None,
        }),
        _ => Some(LibraryLink {
            kind: "asset",
            avatar_id: None,
        }),
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseItemView {
    pub id: i64,
    pub booth_id: String,
    pub name: String,
    pub variation: Option<String>,
    pub shop: String,
    pub price_yen: Option<i64>,
    pub boost_yen: Option<i64>,
    pub amount_yen: Option<i64>,
    pub quantity: i64,
    pub in_library: bool,
    pub library_link: Option<LibraryLink>,
    pub thumbnail_path: Option<String>,
    pub thumbnail_url: Option<String>,
    pub groups: Vec<PurchaseGroup>,
    pub label_ids: Vec<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseOrderView {
    pub order_id: String,
    pub ordered_at: Option<String>,
    pub recorded_at: String,
    pub total_yen: Option<i64>,
    pub gift: bool,
    pub gift_detected: bool,
    pub gift_override: Option<bool>,
    pub detail_read: bool,
    pub source: String,
    pub title: String,
    pub items: Vec<PurchaseItemView>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseLabel {
    pub id: i64,
    pub name: String,
    pub items: u32,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GroupOption {
    pub key: String,
    pub kind: &'static str,
    pub label: String,
    pub items: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseStatus {
    pub orders: u32,
    pub items: u32,
    pub pending_details: u32,
    pub complete: bool,
    pub last_read_at: Option<String>,
    pub show_prices: bool,
    pub running: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchasePage {
    pub orders: Vec<PurchaseOrderView>,
    pub total: u32,
    pub offset: u32,
    pub limit: u32,
    pub labels: Vec<PurchaseLabel>,
    pub groups: Vec<GroupOption>,
    pub years: Vec<i32>,
    pub status: PurchaseStatus,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseQuery {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub label_id: Option<i64>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub gift: Option<bool>,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub offset: u32,
    #[serde(default)]
    pub limit: Option<u32>,
}

fn search_text(value: &str) -> String {
    nfkc(value).to_lowercase()
}

fn tokens(query: &str) -> Vec<String> {
    search_text(query)
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) fn labels(connection: &Connection) -> Result<Vec<PurchaseLabel>, String> {
    let mut statement = sql(connection.prepare(
        "SELECT l.id, l.name, COUNT(il.item_id)
         FROM purchase_labels l LEFT JOIN purchase_item_labels il ON il.label_id = l.id
         GROUP BY l.id ORDER BY l.name, l.id",
    ))?;
    let rows = sql(statement.query_map([], |row| {
        Ok(PurchaseLabel {
            id: row.get(0)?,
            name: row.get(1)?,
            items: row.get::<_, i64>(2)? as u32,
        })
    }))?;
    sql(rows.collect())
}

pub(crate) fn status(connection: &Connection, running: bool) -> Result<PurchaseStatus, String> {
    let (orders, items, pending): (i64, i64, i64) = sql(connection.query_row(
        "SELECT (SELECT COUNT(*) FROM purchase_orders),
                (SELECT COUNT(*) FROM purchase_items),
                (SELECT COUNT(*) FROM purchase_orders WHERE detail_read_at IS NULL)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ))?;
    Ok(PurchaseStatus {
        orders: orders as u32,
        items: items as u32,
        pending_details: pending as u32,
        complete: history_complete(connection)?,
        last_read_at: setting(connection, READ_AT_KEY)?,
        show_prices: show_prices(connection)?,
        running,
    })
}

fn item_view(library: &Library, item: &StoredItem) -> PurchaseItemView {
    let product = library.products.get(&item.booth_id);
    let (groups, _) = groups_for(library, &item.booth_id, &item.name);
    let variation = product.and_then(|product| variation_of(&item.name, &product.title));
    let name = product
        .map(|p| match (&variation, crate::purchased_editions::trailing_variation(&p.title)) {
            (Some(_), Some((base, _))) => base.to_string(),
            _ => p.title.clone(),
        })
        .unwrap_or_else(|| item.name.clone());
    PurchaseItemView {
        id: item.id,
        booth_id: item.booth_id.clone(),
        name,
        variation,
        shop: item.shop.clone(),
        price_yen: item.price_yen,
        boost_yen: item.boost_yen,
        amount_yen: amount(item),
        quantity: item.quantity,
        in_library: product.is_some(),
        library_link: library_link(library, &item.booth_id),
        thumbnail_path: product.and_then(|product| product.thumbnail_path.clone()),
        thumbnail_url: product.and_then(|product| product.thumbnail_url.clone()),
        groups,
        label_ids: item.labels.clone(),
    }
}

fn order_view(library: &Library, order: &StoredOrder) -> PurchaseOrderView {
    PurchaseOrderView {
        order_id: order.order_id.clone(),
        ordered_at: order.ordered_at.clone(),
        recorded_at: order.recorded_at.clone(),
        total_yen: order.total_yen,
        gift: order.gift(),
        gift_detected: order.gift_detected,
        gift_override: order.gift_override,
        detail_read: order.detail_read,
        source: order.source.clone(),
        title: order.list_title.clone(),
        items: order.items.iter().map(|item| item_view(library, item)).collect(),
    }
}

fn haystack(view: &PurchaseOrderView, label_names: &HashMap<i64, String>) -> String {
    let mut parts = vec![view.order_id.clone(), view.title.clone()];
    if view.gift {
        parts.push(GIFT_LABEL.to_string());
    }
    for item in &view.items {
        parts.push(item.name.clone());
        parts.extend(item.variation.clone());
        parts.push(item.shop.clone());
        parts.push(item.booth_id.clone());
        parts.extend(item.groups.iter().map(|group| group.label.clone()));
        parts.extend(item.label_ids.iter().filter_map(|id| label_names.get(id).cloned()));
    }
    search_text(&parts.join("\n"))
}

fn matches(view: &PurchaseOrderView, query: &PurchaseQuery, tokens: &[String], label_names: &HashMap<i64, String>) -> bool {
    if let Some(gift) = query.gift {
        if view.gift != gift {
            return false;
        }
    }
    if let Some(year) = query.year {
        let at = view.ordered_at.as_deref().unwrap_or(&view.recorded_at);
        if at.get(0..4).and_then(|value| value.parse::<i32>().ok()) != Some(year) {
            return false;
        }
    }
    if let Some(label) = query.label_id {
        if !view.items.iter().any(|item| item.label_ids.contains(&label)) {
            return false;
        }
    }
    if let Some(group) = query.group.as_deref().filter(|group| !group.is_empty()) {
        if !view.items.iter().any(|item| item.groups.iter().any(|candidate| candidate.key == group)) {
            return false;
        }
    }
    if tokens.is_empty() {
        return true;
    }
    let text = haystack(view, label_names);
    tokens.iter().all(|token| text.contains(token.as_str()))
}

/// 購入履歴の 1 ページ。検索と絞り込みは全注文に対して行い、結果をページに分ける。
pub(crate) fn page(connection: &Connection, query: &PurchaseQuery, running: bool) -> Result<PurchasePage, String> {
    let library = load_library(connection)?;
    let orders = load_orders(connection)?;
    let labels = labels(connection)?;
    let label_names: HashMap<i64, String> = labels.iter().map(|label| (label.id, label.name.clone())).collect();
    let views: Vec<PurchaseOrderView> = orders.iter().map(|order| order_view(&library, order)).collect();

    let mut group_counts: BTreeMap<String, GroupOption> = BTreeMap::new();
    let mut years = BTreeSet::new();
    for (order, view) in orders.iter().zip(&views) {
        if let Some(year) = order.year() {
            years.insert(year);
        }
        for item in &view.items {
            for group in &item.groups {
                group_counts
                    .entry(group.key.clone())
                    .or_insert_with(|| GroupOption {
                        key: group.key.clone(),
                        kind: group.kind,
                        label: group.label.clone(),
                        items: 0,
                    })
                    .items += 1;
            }
        }
    }
    let mut groups: Vec<GroupOption> = group_counts.into_values().collect();
    groups.sort_by(|a, b| group_rank(a.kind).cmp(&group_rank(b.kind)).then_with(|| a.label.cmp(&b.label)));

    let tokens = tokens(&query.query);
    let matched: Vec<PurchaseOrderView> = views
        .into_iter()
        .filter(|view| matches(view, query, &tokens, &label_names))
        .collect();
    let limit = query.limit.unwrap_or(PAGE_SIZE).clamp(1, MAX_PAGE_SIZE);
    let total = matched.len() as u32;
    let offset = if query.offset >= total { (total.saturating_sub(1) / limit) * limit } else { query.offset - query.offset % limit };
    Ok(PurchasePage {
        orders: matched.into_iter().skip(offset as usize).take(limit as usize).collect(),
        total,
        offset,
        limit,
        labels,
        groups,
        years: years.into_iter().rev().collect(),
        status: status(connection, running)?,
    })
}

fn group_rank(kind: &str) -> u8 {
    match kind {
        "avatar" => 0,
        "generic" => 1,
        "world" => 2,
        _ => 3,
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SummaryGroup {
    pub key: String,
    pub kind: &'static str,
    pub label: String,
    pub avatar_id: Option<String>,
    pub owned: bool,
    pub items: u32,
    pub body_items: u32,
    pub amount_yen: i64,
    pub unknown_amount_items: u32,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseSummary {
    pub year: Option<i32>,
    pub years: Vec<i32>,
    pub orders: u32,
    pub items: u32,
    /// 商品代金と BOOST の合計。送料と手数料は入れない。贈ったギフトも含む。
    pub total_yen: i64,
    pub unknown_amount_items: u32,
    pub gift_items: u32,
    pub gift_yen: i64,
    pub groups: Vec<SummaryGroup>,
    pub show_prices: bool,
}

/// 購入した対応版ごとの集計。複数アバター対応は専用の枠に一度だけ数える。
/// 贈ったギフトは枠に入れない。
pub(crate) fn summary(connection: &Connection, year: Option<i32>) -> Result<PurchaseSummary, String> {
    let library = load_library(connection)?;
    let orders = load_orders(connection)?;
    let years: BTreeSet<i32> = orders.iter().filter_map(StoredOrder::year).collect();
    let mut summary = PurchaseSummary {
        year,
        years: years.into_iter().rev().collect(),
        orders: 0,
        items: 0,
        total_yen: 0,
        unknown_amount_items: 0,
        gift_items: 0,
        gift_yen: 0,
        groups: Vec::new(),
        show_prices: show_prices(connection)?,
    };
    let mut groups: HashMap<String, SummaryGroup> = HashMap::new();
    for order in orders.iter().filter(|order| year.is_none() || order.year() == year) {
        summary.orders += 1;
        for item in &order.items {
            summary.items += 1;
            let value = amount(item);
            match value {
                Some(value) => summary.total_yen += value,
                None => summary.unknown_amount_items += 1,
            }
            if order.gift() {
                summary.gift_items += 1;
                summary.gift_yen += value.unwrap_or(0);
                continue;
            }
            let (item_groups, owned) = groups_for(&library, &item.booth_id, &item.name);
            for (group, owned) in item_groups.into_iter().zip(owned) {
                let entry = groups.entry(group.key.clone()).or_insert_with(|| SummaryGroup {
                    key: group.key.clone(),
                    kind: group.kind,
                    label: group.label.clone(),
                    avatar_id: group.avatar_id.clone(),
                    owned,
                    items: 0,
                    body_items: 0,
                    amount_yen: 0,
                    unknown_amount_items: 0,
                });
                entry.items += 1;
                if group.body {
                    entry.body_items += 1;
                }
                match value {
                    Some(value) => entry.amount_yen += value,
                    None => entry.unknown_amount_items += 1,
                }
            }
        }
    }
    let mut groups: Vec<SummaryGroup> = groups.into_values().collect();
    groups.sort_by(|a, b| {
        group_rank(a.kind)
            .cmp(&group_rank(b.kind))
            .then_with(|| b.amount_yen.cmp(&a.amount_yen))
            .then_with(|| b.items.cmp(&a.items))
            .then_with(|| a.label.cmp(&b.label))
    });
    summary.groups = groups;
    Ok(summary)
}

fn label_name(name: &str) -> Result<String, String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err("ラベルの名前を入力してください".into());
    }
    if name.chars().count() > LABEL_MAX_CHARS {
        return Err(format!("ラベルの名前は{LABEL_MAX_CHARS}文字までです"));
    }
    if name == GIFT_LABEL {
        return Err("その名前はギフトの印で使っています".into());
    }
    Ok(name)
}

fn unique_name(result: rusqlite::Result<usize>) -> Result<usize, String> {
    result.map_err(|err| match err {
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            "同じ名前のラベルがあります".to_string()
        }
        other => other.to_string(),
    })
}

pub(crate) fn create_label(connection: &Connection, name: &str) -> Result<PurchaseLabel, String> {
    let name = label_name(name)?;
    let count: i64 = sql(connection.query_row("SELECT COUNT(*) FROM purchase_labels", [], |row| row.get(0)))?;
    if count >= LABEL_LIMIT {
        return Err(format!("ラベルは{LABEL_LIMIT}個までです"));
    }
    unique_name(connection.execute(
        "INSERT INTO purchase_labels (name, created_at) VALUES (?1, ?2)",
        params![name, now_iso()],
    ))?;
    Ok(PurchaseLabel {
        id: connection.last_insert_rowid(),
        name,
        items: 0,
    })
}

pub(crate) fn rename_label(connection: &Connection, id: i64, name: &str) -> Result<(), String> {
    let name = label_name(name)?;
    if unique_name(connection.execute("UPDATE purchase_labels SET name = ?2 WHERE id = ?1", params![id, name]))? == 0 {
        return Err("ラベルが見つかりません".into());
    }
    Ok(())
}

pub(crate) fn delete_label(connection: &mut Connection, id: i64) -> Result<(), String> {
    let tx = sql(connection.transaction())?;
    sql(tx.execute("DELETE FROM purchase_item_labels WHERE label_id = ?1", [id]))?;
    if sql(tx.execute("DELETE FROM purchase_labels WHERE id = ?1", [id]))? == 0 {
        return Err("ラベルが見つかりません".into());
    }
    sql(tx.commit())
}

pub(crate) fn set_item_labels(connection: &mut Connection, item_id: i64, label_ids: &[i64]) -> Result<(), String> {
    let tx = sql(connection.transaction())?;
    let exists: bool = sql(tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM purchase_items WHERE id = ?1)",
        [item_id],
        |row| row.get(0),
    ))?;
    if !exists {
        return Err("購入履歴の商品が見つかりません".into());
    }
    let wanted: HashSet<i64> = label_ids.iter().copied().collect();
    for label in &wanted {
        let known: bool = sql(tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM purchase_labels WHERE id = ?1)",
            [label],
            |row| row.get(0),
        ))?;
        if !known {
            return Err("ラベルが見つかりません".into());
        }
    }
    sql(tx.execute("DELETE FROM purchase_item_labels WHERE item_id = ?1", [item_id]))?;
    for label in wanted {
        sql(tx.execute(
            "INSERT INTO purchase_item_labels (item_id, label_id) VALUES (?1, ?2)",
            params![item_id, label],
        ))?;
    }
    sql(tx.commit())
}

/// ギフトの印を手で決める。`None` は詳細ページの判定に戻す。
pub(crate) fn set_gift_override(connection: &Connection, order_id: &str, gift: Option<bool>) -> Result<(), String> {
    if sql(connection.execute(
        "UPDATE purchase_orders SET gift_override = ?2, updated_at = ?3 WHERE order_id = ?1",
        params![order_id, gift, now_iso()],
    ))? == 0
    {
        return Err("注文が見つかりません".into());
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncPurchaseItem {
    pub booth_id: String,
    pub name: String,
    pub shop: String,
    pub price_yen: Option<i64>,
    pub boost_yen: Option<i64>,
    pub quantity: i64,
    pub labels: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncPurchaseOrder {
    pub order_id: String,
    pub ordered_at: Option<String>,
    pub total_yen: Option<i64>,
    pub gift: bool,
    pub gift_detected: bool,
    pub gift_override: Option<bool>,
    pub detail_read: bool,
    pub items: Vec<SyncPurchaseItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncPurchases {
    pub show_prices: bool,
    pub labels: Vec<String>,
    pub orders: Vec<SyncPurchaseOrder>,
}

impl SyncPurchases {
    pub fn item_count(&self) -> usize {
        self.orders.iter().map(|order| order.items.len()).sum()
    }
}

/// データ移行の `purchases`。注文番号の順に並べ、同じ内容なら同じ JSON になる。
pub(crate) fn export(connection: &Connection) -> Result<SyncPurchases, String> {
    let label_list = labels(connection)?;
    let names: HashMap<i64, String> = label_list.iter().map(|label| (label.id, label.name.clone())).collect();
    let mut orders = load_orders(connection)?;
    orders.sort_by(|a, b| a.order_id.len().cmp(&b.order_id.len()).then_with(|| a.order_id.cmp(&b.order_id)));
    Ok(SyncPurchases {
        show_prices: show_prices(connection)?,
        labels: label_list.into_iter().map(|label| label.name).collect(),
        orders: orders
            .into_iter()
            .map(|order| SyncPurchaseOrder {
                gift: order.gift(),
                order_id: order.order_id,
                ordered_at: order.ordered_at,
                total_yen: order.total_yen,
                gift_detected: order.gift_detected,
                gift_override: order.gift_override,
                detail_read: order.detail_read,
                items: order
                    .items
                    .into_iter()
                    .map(|item| {
                        let mut labels: Vec<String> = item.labels.iter().filter_map(|id| names.get(id).cloned()).collect();
                        labels.sort();
                        SyncPurchaseItem {
                            booth_id: item.booth_id,
                            name: item.name,
                            shop: item.shop,
                            price_yen: item.price_yen,
                            boost_yen: item.boost_yen,
                            quantity: item.quantity,
                            labels,
                        }
                    })
                    .collect(),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::purchase_walk::DetailItem;

    fn db() -> Connection {
        crate::db::open_memory().unwrap()
    }

    fn listed(id: &str, state: OrderState, at: &str) -> ListOrder {
        ListOrder {
            order_id: id.into(),
            state,
            ordered_at: Some(at.into()),
            title: format!("一覧の名前 {id}"),
        }
    }

    fn item(booth_id: &str, name: &str, price: Option<i64>, boost: Option<i64>) -> DetailItem {
        DetailItem {
            booth_id: booth_id.into(),
            name: name.into(),
            shop: "見本ショップ".into(),
            price_yen: price,
            boost_yen: boost,
            quantity: 1,
        }
    }

    fn detail(id: &str, at: &str, gift: bool, items: Vec<DetailItem>) -> OrderDetail {
        OrderDetail {
            order_id: id.into(),
            state: OrderState::Confirmed,
            ordered_at: Some(at.into()),
            total_yen: Some(items.iter().filter_map(|item| item.price_yen).sum()),
            gift,
            items,
        }
    }

    fn product(connection: &Connection, booth_id: &str, title: &str, generic: bool) {
        connection
            .execute(
                "INSERT INTO products (booth_id, title, imported_at, updated_at, is_generic) VALUES (?1, ?2, '2026-01-01', '2026-01-01', ?3)",
                params![booth_id, title, generic],
            )
            .unwrap();
    }

    fn link(connection: &Connection, booth_id: &str, avatar: &str) {
        connection
            .execute(
                "INSERT INTO product_avatars (product_id, avatar_id, source) VALUES (?1, ?2, 'manual')",
                params![booth_id, avatar],
            )
            .unwrap();
    }

    #[test]
    fn the_list_adds_confirmed_orders_and_drops_cancelled_ones() {
        let mut connection = db();
        let applied = apply_list_page(
            &mut connection,
            &[
                listed("1001", OrderState::Confirmed, "2026-11-02T09:30:20+09:00"),
                listed("1002", OrderState::Cancelled, "2026-11-01T09:30:20+09:00"),
                listed("1003", OrderState::Pending, "2026-10-01T09:30:20+09:00"),
            ],
        )
        .unwrap();
        assert_eq!(applied, ListApplied { added: 1, removed: 0, news: true });
        let again = apply_list_page(&mut connection, &[listed("1001", OrderState::Confirmed, "2026-11-02T09:30:21+09:00")]).unwrap();
        assert_eq!(again, ListApplied::default());
        assert_eq!(pending_detail_ids(&connection).unwrap(), vec!["1001".to_string()]);

        apply_detail(
            &mut connection,
            &detail("1001", "2026-11-02T09:30:19+09:00", false, vec![item("2001", "見本の衣装 (フル)", Some(2000), Some(300))]),
        )
        .unwrap();
        assert!(pending_detail_ids(&connection).unwrap().is_empty());
        // 一覧と詳細で秒がずれることがある。詳細の値を残す。
        apply_list_page(&mut connection, &[listed("1001", OrderState::Confirmed, "2026-11-02T09:30:20+09:00")]).unwrap();
        let page = page(&connection, &PurchaseQuery::default(), false).unwrap();
        assert_eq!(page.orders[0].ordered_at.as_deref(), Some("2026-11-02T09:30:19+09:00"));
        assert_eq!(page.orders[0].items[0].amount_yen, Some(2300));

        let removed = apply_list_page(&mut connection, &[listed("1001", OrderState::Cancelled, "2026-11-02T09:30:20+09:00")]).unwrap();
        assert_eq!(removed, ListApplied { added: 0, removed: 1, news: true });
        assert_eq!(status(&connection, false).unwrap().orders, 0);
    }

    #[test]
    fn a_checkout_record_is_completed_by_its_detail_and_keeps_labels() {
        let mut connection = db();
        let products = vec![CheckoutProduct {
            booth_id: "2001".into(),
            title: "見本の衣装".into(),
            shop: "見本ショップ".into(),
        }];
        assert!(record_checkout(&mut connection, "1001", &products).unwrap());
        assert!(!record_checkout(&mut connection, "1001", &products).unwrap());
        assert!(record_checkout(&mut connection, "../1", &products).is_err());
        assert_eq!(pending_detail_ids(&connection).unwrap(), vec!["1001".to_string()]);
        let label = create_label(&connection, "お気に入り").unwrap();
        let item_id = page(&connection, &PurchaseQuery::default(), false).unwrap().orders[0].items[0].id;
        set_item_labels(&mut connection, item_id, &[label.id]).unwrap();

        apply_detail(
            &mut connection,
            &detail("1001", "2026-11-02T09:30:19+09:00", false, vec![item("2001", "見本の衣装 (フルセット)", Some(2000), None)]),
        )
        .unwrap();
        let view = page(&connection, &PurchaseQuery::default(), false).unwrap();
        let only = &view.orders[0];
        assert!(only.detail_read);
        assert_eq!(only.items.len(), 1);
        assert_eq!(only.items[0].name, "見本の衣装 (フルセット)");
        assert_eq!(only.items[0].label_ids, vec![label.id]);

        let mut cancelled = detail("1001", "2026-11-02T09:30:19+09:00", false, Vec::new());
        cancelled.state = OrderState::Cancelled;
        assert!(apply_detail(&mut connection, &cancelled).unwrap().removed);
        assert_eq!(labels(&connection).unwrap()[0].items, 0);
    }

    #[test]
    fn search_spans_every_page_and_filters_by_label_group_gift_and_year() {
        let mut connection = db();
        product(&connection, "3000", "プラム用の髪", false);
        link(&connection, "3000", "plum");
        let mut list = Vec::new();
        for index in 0..120 {
            let id = format!("{}", 5000 + index);
            let at = format!("2025-{:02}-01T10:00:00+09:00", 1 + index % 12);
            list.push(listed(&id, OrderState::Confirmed, &at));
        }
        apply_list_page(&mut connection, &list).unwrap();
        for index in 0..120 {
            let id = format!("{}", 5000 + index);
            let booth = if index == 7 { "3000" } else { "3001" };
            let name = if index == 7 { "プラム用の髪 (黒)" } else { "別の小物" };
            apply_detail(
                &mut connection,
                &detail(&id, &format!("2025-{:02}-01T10:00:00+09:00", 1 + index % 12), index == 9, vec![item(booth, name, Some(500), Some(0))]),
            )
            .unwrap();
        }
        apply_detail(&mut connection, &detail("9000", "2026-01-05T10:00:00+09:00", false, vec![item("3001", "別の小物", Some(100), None)])).unwrap();

        let first = page(&connection, &PurchaseQuery::default(), false).unwrap();
        assert_eq!(first.total, 121);
        assert_eq!(first.orders.len(), 50);
        assert_eq!(first.orders[0].order_id, "9000");
        assert_eq!(first.years, vec![2026, 2025]);
        let last = page(&connection, &PurchaseQuery { offset: 100, ..Default::default() }, false).unwrap();
        assert_eq!(last.orders.len(), 21);

        let hair = page(&connection, &PurchaseQuery { query: "ﾌﾟﾗﾑ 黒".into(), ..Default::default() }, false).unwrap();
        assert_eq!(hair.total, 1);
        assert_eq!(hair.orders[0].items[0].variation.as_deref(), Some("黒"));
        // A color of a single-avatar product still belongs to that avatar.
        assert_eq!(hair.orders[0].items[0].groups[0].key, "avatar:plum");
        let by_group = page(&connection, &PurchaseQuery { group: Some("avatar:plum".into()), ..Default::default() }, false).unwrap();
        assert_eq!(by_group.total, 1);
        let gifts = page(&connection, &PurchaseQuery { gift: Some(true), ..Default::default() }, false).unwrap();
        assert_eq!(gifts.total, 1);
        let by_word = page(&connection, &PurchaseQuery { query: "ギフト".into(), ..Default::default() }, false).unwrap();
        assert_eq!(by_word.total, 1);
        let in_2026 = page(&connection, &PurchaseQuery { year: Some(2026), ..Default::default() }, false).unwrap();
        assert_eq!(in_2026.total, 1);

        let label = create_label(&connection, "あとで使う").unwrap();
        let target = last.orders[3].items[0].id;
        set_item_labels(&mut connection, target, &[label.id]).unwrap();
        let labelled = page(&connection, &PurchaseQuery { label_id: Some(label.id), ..Default::default() }, false).unwrap();
        assert_eq!(labelled.total, 1);
        let by_label_name = page(&connection, &PurchaseQuery { query: "あとで".into(), ..Default::default() }, false).unwrap();
        assert_eq!(by_label_name.total, 1);
    }

    #[test]
    fn totals_do_not_assign_unresolved_editions_to_compatible_avatars() {
        let mut connection = db();
        product(&connection, "4000", "プラム本体", false);
        connection
            .execute("INSERT INTO avatar_sources (booth_id, avatar_id) VALUES ('4000', 'plum')", [])
            .unwrap();
        product(&connection, "4001", "二人対応の衣装", false);
        link(&connection, "4001", "plum");
        link(&connection, "4001", "lime");
        product(&connection, "4002", "汎用の小物", true);
        for (id, at, gift, items) in [
            ("1001", "2025-03-01T10:00:00+09:00", false, vec![item("4000", "プラム本体", Some(5000), Some(0))]),
            ("1002", "2026-03-01T10:00:00+09:00", false, vec![item("4001", "二人対応の衣装", Some(2000), Some(500)), item("4002", "汎用の小物", Some(300), None)]),
            ("1003", "2026-04-01T10:00:00+09:00", true, vec![item("4001", "二人対応の衣装", Some(2000), None)]),
            ("1004", "2026-05-01T10:00:00+09:00", false, vec![item("4999", "グッズ", None, None)]),
        ] {
            apply_detail(&mut connection, &detail(id, at, gift, items)).unwrap();
        }
        let all = summary(&connection, None).unwrap();
        assert_eq!(all.orders, 4);
        assert_eq!(all.items, 5);
        assert_eq!(all.total_yen, 5000 + 2500 + 300 + 2000);
        assert_eq!(all.unknown_amount_items, 1);
        assert_eq!((all.gift_items, all.gift_yen), (1, 2000));
        let get = |key: &str| all.groups.iter().find(|group| group.key == key).unwrap().clone();
        let plum = get("avatar:plum");
        assert_eq!((plum.items, plum.body_items, plum.amount_yen), (1, 1, 5000));
        assert_eq!(get("unset").amount_yen, 2500);
        assert!(!all.groups.iter().any(|g| g.key == "avatar:lime"));
        assert_eq!(get("generic").amount_yen, 300);
        let unclassified = get("unclassified");
        assert_eq!((unclassified.items, unclassified.unknown_amount_items), (1, 1));
        assert_eq!(all.groups.last().unwrap().kind, "unclassified");

        let this_year = summary(&connection, Some(2026)).unwrap();
        assert_eq!(this_year.orders, 3);
        assert!(this_year.groups.iter().all(|group| group.body_items == 0));
        assert_eq!(this_year.years, vec![2026, 2025]);

        set_gift_override(&connection, "1003", Some(false)).unwrap();
        assert_eq!(summary(&connection, None).unwrap().gift_items, 0);
        set_gift_override(&connection, "1003", None).unwrap();
        assert_eq!(summary(&connection, None).unwrap().gift_items, 1);
        assert!(set_gift_override(&connection, "9999", Some(true)).is_err());
    }

    #[test]
    fn title_only_purchases_use_a_single_known_edition_and_worlds_have_their_own_group() {
        let mut c = db();
        for (id, title, category) in [
            ("6001", "専用の衣装", "outfit"),
            ("6002", "二人対応の衣装", "outfit"),
            ("6003", "見本ワールド", "world"),
            ("6004", "ライム本体", "avatar"),
            ("6005", "対応に未解決名がある髪", "hair"),
            ("6006", "【17アバター対応】ミルキーチル (フルセット)", "outfit"),
            ("6007", "【14アバター対応】アイテクスチャ (ショコラ -Chocolat-)", "texture"),
            ("6008", "22アバター対応 衣装 (ライム・プラム対応)", "outfit"),
        ] {
            product(&c, id, title, false);
            c.execute("UPDATE products SET category=?2 WHERE booth_id=?1", params![id, category]).unwrap();
            if id != "6003" { link(&c, id, "lime"); }
        }
        link(&c, "6002", "plum");
        link(&c, "6006", "plum");
        link(&c, "6007", "chocolat");
        link(&c, "6008", "plum");
        c.execute("UPDATE products SET unresolved_names_json='[\"見本ちゃん\"]' WHERE booth_id='6005'", []).unwrap();
        let items = [
            ("6001", "専用の衣装", 100),
            ("6002", "二人対応の衣装", 200),
            ("6002", "二人対応の衣装 （プラム版）", 300),
            ("6003", "見本ワールド", 400),
            ("6004", "ライム本体", 500),
            ("6005", "対応に未解決名がある髪", 600),
            ("6006", "【17アバター対応】ミルキーチル (フルセット)", 700),
            ("6007", "【14アバター対応】アイテクスチャ (ショコラ -Chocolat-)", 800),
            ("6008", "22アバター対応 衣装 (ライム・プラム対応)", 900),
        ].into_iter().map(|(id, name, price)| item(id, name, Some(price), None)).collect();
        apply_detail(&mut c, &detail("1001", "2026-01-01T00:00:00+09:00", false, items)).unwrap();

        let totals = summary(&c, None).unwrap();
        let group = |key| totals.groups.iter().find(|g| g.key == key).unwrap();
        assert_eq!((group("avatar:lime").items, group("avatar:lime").body_items, group("avatar:lime").amount_yen), (2, 1, 600));
        assert_eq!(group("avatar:plum").amount_yen, 300);
        assert_eq!(group("avatar:chocolat").amount_yen, 800);
        assert_eq!(group("multiple").amount_yen, 1600);
        assert_eq!(group("unset").amount_yen, 800);
        assert_eq!(group("world").amount_yen, 400);
        assert_eq!(totals.groups.iter().map(|g| g.amount_yen).sum::<i64>(), totals.total_yen);
        let worlds = page(&c, &PurchaseQuery { group: Some("world".into()), ..Default::default() }, false).unwrap();
        assert_eq!(worlds.total, 1);
        assert!(worlds.groups.iter().any(|g| g.key == "world" && g.items == 1));

        let catalog = crate::purchased_editions::Catalog::load(&c).unwrap();
        assert_eq!(catalog.scope("6001").avatar_ids, ["lime"]);
        assert_eq!(catalog.scope("6005").mode, "unknown");
        crate::purchased_editions::save(&c, "6001", &crate::purchased_editions::Choice::unknown()).unwrap();
        let updated = summary(&c, None).unwrap();
        assert_eq!(updated.groups.iter().find(|g| g.key == "unset").unwrap().amount_yen, 900);
    }

    #[test]
    fn one_package_for_every_avatar_counts_once_and_per_avatar_files_do_not() {
        let mut c = db();
        for (id, title, other) in [("7001", "ふたり対応の髪", "plum"), ("7002", "ふたり対応の衣装", "plum"), ("7003", "ふたり対応の目", "chocolat")] {
            product(&c, id, title, false);
            link(&c, id, "lime");
            link(&c, id, other);
        }
        c.execute_batch(
            "INSERT INTO product_files(booth_id,variation,filename) VALUES
                ('7001','','Calm_Hair.zip'),
                ('7002','','Outfit_Lime.zip'),('7002','','Outfit_Plum.zip'),
                ('7003','','Eye_Lime.zip'),('7003','','Eye_Chocolate.zip');",
        )
        .unwrap();
        let items = [("7001", "ふたり対応の髪", 100), ("7002", "ふたり対応の衣装", 200), ("7003", "ふたり対応の目 (chocolate)", 400)]
            .into_iter()
            .map(|(id, name, price)| item(id, name, Some(price), None))
            .collect();
        apply_detail(&mut c, &detail("1001", "2026-01-01T00:00:00+09:00", false, items)).unwrap();
        let totals = summary(&c, None).unwrap();
        let amount = |key| totals.groups.iter().find(|g| g.key == key).map(|g| g.amount_yen);
        // One shared package covers every compatible avatar.
        assert_eq!(amount("multiple"), Some(100));
        // Files split by avatar and an unknown spelling leave the edition to the user.
        assert_eq!(amount("unset"), Some(200 + 400));
        assert_eq!(amount("avatar:chocolat"), None);
    }

    #[test]
    fn individual_orders_and_full_sets_have_disjoint_nonduplicating_spend_groups() {
        let mut c = db();
        product(&c, "6001", "衣装A", false);
        for id in ["lime", "plum", "manuka"] { link(&c, "6001", id); }
        for (id, variant, price) in [("1001", "Lime", 1500), ("1002", "Plum", 1600), ("1003", "フルセット", 5000), ("1004", "黒", 100)] {
            apply_detail(&mut c, &detail(id, "2026-01-01T00:00:00+09:00", false, vec![item("6001", &format!("衣装A ({variant})"), Some(price), Some(0))])).unwrap();
        }
        let summary = summary(&c, None).unwrap();
        assert_eq!(summary.total_yen, 8200);
        assert_eq!(summary.groups.iter().map(|g| g.amount_yen).sum::<i64>(), 8200);
        for (key, expected) in [("avatar:lime",1500),("avatar:plum",1600),("multiple",5000),("unset",100)] {
            assert_eq!(summary.groups.iter().find(|g| g.key==key).unwrap().amount_yen, expected);
        }
        assert!(!summary.groups.iter().any(|g| g.key=="avatar:manuka"));
        let lime = page(&c, &PurchaseQuery { group: Some("avatar:lime".into()), ..Default::default() }, false).unwrap();
        assert_eq!(lime.total, 1);
        assert_eq!(lime.orders[0].items[0].variation.as_deref(), Some("Lime"));
    }

    #[test]
    fn items_in_the_library_open_their_asset_world_or_avatar() {
        let mut connection = db();
        for (id, category) in [("5001", Some("outfit")), ("5002", Some("world")), ("5003", Some("avatar")), ("5004", Some("hair")), ("5005", None)] {
            product(&connection, id, "見本", false);
            connection
                .execute("UPDATE products SET category = ?2 WHERE booth_id = ?1", params![id, category])
                .unwrap();
        }
        connection
            .execute_batch(
                "INSERT INTO avatar_sources (booth_id, avatar_id) VALUES ('5003', 'plum');
                 INSERT INTO product_exclusions (booth_id, excluded_at) VALUES ('5004', 't');",
            )
            .unwrap();
        let items = ["5001", "5002", "5003", "5004", "5005", "5006"]
            .iter()
            .map(|id| item(id, &format!("見本 {id}"), Some(100), None))
            .collect();
        apply_detail(&mut connection, &detail("1001", "2026-01-01T10:00:00+09:00", false, items)).unwrap();
        let view = page(&connection, &PurchaseQuery::default(), false).unwrap();
        let links: Vec<Option<(&str, Option<&str>)>> = view.orders[0]
            .items
            .iter()
            .map(|item| item.library_link.as_ref().map(|link| (link.kind, link.avatar_id.as_deref())))
            .collect();
        assert_eq!(
            links,
            vec![
                Some(("asset", None)),
                Some(("world", None)),
                Some(("avatar", Some("plum"))),
                None,
                None,
                None,
            ]
        );
    }

    #[test]
    fn labels_are_named_once_and_checked() {
        let mut connection = db();
        let label = create_label(&connection, "  あとで   使う ").unwrap();
        assert_eq!(label.name, "あとで 使う");
        assert!(create_label(&connection, "あとで 使う").unwrap_err().contains("同じ名前"));
        assert!(create_label(&connection, "").is_err());
        assert!(create_label(&connection, GIFT_LABEL).is_err());
        assert!(create_label(&connection, &"あ".repeat(31)).is_err());
        let other = create_label(&connection, "別").unwrap();
        assert!(rename_label(&connection, other.id, "あとで 使う").is_err());
        rename_label(&connection, other.id, "使った").unwrap();
        assert!(set_item_labels(&mut connection, 999, &[label.id]).is_err());
        delete_label(&mut connection, other.id).unwrap();
        assert!(delete_label(&mut connection, other.id).is_err());
        assert_eq!(labels(&connection).unwrap().len(), 1);
    }

    #[test]
    fn the_export_lists_orders_amounts_and_labels_in_a_fixed_order() {
        let mut connection = db();
        apply_detail(&mut connection, &detail("2002", "2026-01-02T10:00:00+09:00", true, vec![item("3001", "見本", Some(100), Some(50))])).unwrap();
        apply_detail(&mut connection, &detail("1001", "2026-01-01T10:00:00+09:00", false, vec![item("3002", "見本2", Some(200), None)])).unwrap();
        let label = create_label(&connection, "見本ラベル").unwrap();
        let id = page(&connection, &PurchaseQuery::default(), false).unwrap().orders[0].items[0].id;
        set_item_labels(&mut connection, id, &[label.id]).unwrap();
        set_show_prices(&connection, true).unwrap();
        let exported = export(&connection).unwrap();
        assert!(exported.show_prices);
        assert_eq!(exported.labels, vec!["見本ラベル".to_string()]);
        assert_eq!(exported.orders.iter().map(|order| order.order_id.as_str()).collect::<Vec<_>>(), vec!["1001", "2002"]);
        assert!(exported.orders[1].gift);
        assert_eq!(exported.orders[1].items[0].labels, vec!["見本ラベル".to_string()]);
        assert_eq!(exported.item_count(), 2);
        let json = serde_json::to_string(&exported).unwrap();
        assert!(!json.contains("recordedAt"));
    }
}
