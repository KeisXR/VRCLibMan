//! 購入履歴の読み込みの進み方。
//! 一覧（`accounts.booth.pm/orders?page=N`）を 1 ページずつ読み、そのあと詳細が未取得の注文を
//! 1 件ずつ読む。ギフトの印は詳細ページにしか出ないことがあるので、詳細は全注文で一度ずつ読む。
//! 並行には読まず、間隔はライブラリ全取得と同じ設定を使う。ファイルは取りに行かない。

use crate::library_walk::{WalkStop, is_age_gate};
use serde::Deserialize;
use url::Url;

pub const ORDERS_PAGE: &str = "https://accounts.booth.pm/orders";
const MAX_PAGE: u32 = 200;
const MAX_NAME_CHARS: usize = 1000;
const MAX_YEN: i64 = 100_000_000;
pub const LOGIN_NOTICE: &str =
    "ログインの期限が切れているか、購入履歴のページではありません。内蔵ブラウザでログインしてから、もう一度読み込んでください";
pub const DETAIL_UNREADABLE: &str =
    "注文の詳細を読めませんでした。BOOTH のページの形が変わった可能性があります";

pub fn is_order_id(id: &str) -> bool {
    (4..=12).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_booth_id(id: &str) -> bool {
    is_order_id(id)
}

pub fn orders_page_url(page: u32) -> Result<Url, String> {
    if !(1..=MAX_PAGE).contains(&page) {
        return Err("ページ番号が範囲外です".into());
    }
    let text = if page == 1 {
        ORDERS_PAGE.to_string()
    } else {
        format!("{ORDERS_PAGE}?page={page}")
    };
    Url::parse(&text).map_err(|err| err.to_string())
}

pub fn order_detail_url(order_id: &str) -> Result<Url, String> {
    if !is_order_id(order_id) {
        return Err("注文番号が正しくありません".into());
    }
    Url::parse(&format!("{ORDERS_PAGE}/{order_id}")).map_err(|err| err.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrdersPath {
    List,
    Detail(String),
}

/// 購入履歴の一覧か注文詳細なら、その種類。それ以外の BOOTH のページは `None`。
pub fn orders_path(url: &Url) -> Option<OrdersPath> {
    if url.scheme() != "https"
        || url.host_str() != Some("accounts.booth.pm")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    let path = url.path().trim_end_matches('/');
    if path == "/orders" {
        return Some(OrdersPath::List);
    }
    let id = path.strip_prefix("/orders/")?;
    is_order_id(id).then(|| OrdersPath::Detail(id.to_string()))
}

/// 読み込みを止めるページ。年齢確認を先に見る。購入履歴と注文詳細以外はログイン切れとして止める。
pub fn purchase_block_reason(href: &str) -> Option<WalkStop> {
    let url = Url::parse(href).ok()?;
    let path = url.path().to_ascii_lowercase();
    let query = url.query().unwrap_or("").to_ascii_lowercase();
    if is_age_gate(&path, &query) {
        return Some(WalkStop::AgeGate);
    }
    if orders_path(&url).is_none() {
        return Some(WalkStop::Login);
    }
    None
}

/// `about:blank` のように、まだ移動先が決まっていない URL では止めない。
pub fn purchase_settled_stop(href: &str) -> Option<WalkStop> {
    let url = Url::parse(href).ok()?;
    if url.scheme() != "https" && url.scheme() != "http" {
        return None;
    }
    purchase_block_reason(href)
}

fn page_number(url: &Url) -> u32 {
    url.query_pairs()
        .find(|(key, _)| key == "page")
        .and_then(|(_, value)| value.parse::<u32>().ok())
        .filter(|page| (1..=MAX_PAGE).contains(page))
        .unwrap_or(1)
}

pub fn same_orders_page(landed: &str, target: &Url) -> bool {
    let Ok(url) = Url::parse(landed) else {
        return false;
    };
    match (orders_path(&url), orders_path(target)) {
        (Some(OrdersPath::List), Some(OrdersPath::List)) => page_number(&url) == page_number(target),
        (Some(OrdersPath::Detail(a)), Some(OrdersPath::Detail(b))) => a == b,
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderState {
    Confirmed,
    Cancelled,
    Pending,
}

fn state_from(value: &str) -> OrderState {
    match value {
        "cancelled" => OrderState::Cancelled,
        "pending" => OrderState::Pending,
        _ => OrderState::Confirmed,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListOrder {
    pub order_id: String,
    pub state: OrderState,
    pub ordered_at: Option<String>,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetailItem {
    pub booth_id: String,
    pub name: String,
    pub shop: String,
    pub price_yen: Option<i64>,
    pub boost_yen: Option<i64>,
    pub quantity: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderDetail {
    pub order_id: String,
    pub state: OrderState,
    pub ordered_at: Option<String>,
    pub total_yen: Option<i64>,
    pub gift: bool,
    pub items: Vec<DetailItem>,
}

#[derive(Clone, Debug, Default)]
pub struct ListSnapshot {
    pub href: String,
    pub orders: Vec<ListOrder>,
    pub last_page: u32,
}

#[derive(Clone, Debug, Default)]
pub struct DetailSnapshot {
    pub href: String,
    pub detail: Option<OrderDetail>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawListOrder {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    ordered_at: Option<String>,
    #[serde(default)]
    title: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawItem {
    #[serde(default)]
    booth_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    shop: String,
    #[serde(default)]
    price_yen: Option<f64>,
    #[serde(default)]
    boost_yen: Option<f64>,
    #[serde(default)]
    quantity: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDetail {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    ordered_at: Option<String>,
    #[serde(default)]
    total_yen: Option<f64>,
    #[serde(default)]
    gift: bool,
    #[serde(default)]
    items: Vec<RawItem>,
}

fn clean_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_NAME_CHARS)
        .collect()
}

/// ページから来た日時は `YYYY-MM-DDTHH:MM:SS+09:00` だけを受ける。
pub fn valid_ordered_at(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    let parsed = chrono::DateTime::parse_from_rfc3339(value).ok()?;
    let jst = chrono::FixedOffset::east_opt(9 * 3600)?;
    Some(
        parsed
            .with_timezone(&jst)
            .format("%Y-%m-%dT%H:%M:%S+09:00")
            .to_string(),
    )
}

fn yen(value: Option<f64>) -> Option<i64> {
    let value = value?;
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > MAX_YEN as f64 {
        return None;
    }
    Some(value as i64)
}

pub fn parse_list_orders(value: &serde_json::Value) -> Vec<ListOrder> {
    let Ok(raw) = serde_json::from_value::<Vec<RawListOrder>>(value.clone()) else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    raw.into_iter()
        .filter(|order| is_order_id(&order.order_id) && seen.insert(order.order_id.clone()))
        .map(|order| ListOrder {
            state: state_from(&order.state),
            ordered_at: valid_ordered_at(order.ordered_at.as_deref()),
            title: clean_text(&order.title),
            order_id: order.order_id,
        })
        .collect()
}

pub fn parse_detail(value: &serde_json::Value) -> Option<OrderDetail> {
    if value.is_null() {
        return None;
    }
    let raw: RawDetail = serde_json::from_value(value.clone()).ok()?;
    if !is_order_id(&raw.order_id) {
        return None;
    }
    let mut seen = std::collections::HashSet::new();
    let items = raw
        .items
        .into_iter()
        .filter_map(|item| {
            let name = clean_text(&item.name);
            if !is_booth_id(&item.booth_id) || name.is_empty() {
                return None;
            }
            if !seen.insert((item.booth_id.clone(), name.clone())) {
                return None;
            }
            let quantity = item
                .quantity
                .filter(|value| value.is_finite() && value.fract() == 0.0 && (1.0..=9999.0).contains(value))
                .map(|value| value as i64)
                .unwrap_or(1);
            Some(DetailItem {
                booth_id: item.booth_id,
                name,
                shop: clean_text(&item.shop),
                price_yen: yen(item.price_yen),
                boost_yen: yen(item.boost_yen),
                quantity,
            })
        })
        .collect();
    Some(OrderDetail {
        state: state_from(&raw.state),
        ordered_at: valid_ordered_at(raw.ordered_at.as_deref()),
        total_yen: yen(raw.total_yen),
        gift: raw.gift,
        items,
        order_id: raw.order_id,
    })
}

/// 一覧 1 ページを DB に入れた結果。`news` は新しい注文か、状態の変わった注文があったこと。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListApplied {
    pub added: u32,
    pub removed: u32,
    pub news: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DetailApplied {
    pub removed: bool,
}

#[derive(Clone, Debug)]
pub enum PlannedRead {
    List { url: Url, page: u32, last_page: u32 },
    Detail { url: Url, order_id: String, index: u32, total: u32 },
}

#[derive(Clone, Debug)]
pub struct Planned {
    pub read: PlannedRead,
    pub pause: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Phase {
    List { page: u32, last_page: u32 },
    NeedDetails,
    Details { queue: Vec<String>, index: usize },
    Done,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseWalkReport {
    pub pages: u32,
    pub added: u32,
    pub removed: u32,
    pub details: u32,
    pub cancelled: bool,
    pub notice: String,
}

/// 一覧を読み、そのあと詳細を読む。一度最後まで一覧を読んだあとは、新しい注文の無いページで一覧を止める。
pub struct PurchaseWalk {
    known_complete: bool,
    phase: Phase,
    reads: u32,
    pages: u32,
    added: u32,
    removed: u32,
    details: u32,
    list_complete: bool,
    stop: Option<WalkStop>,
    error: String,
}

impl PurchaseWalk {
    pub fn new(known_complete: bool) -> Self {
        Self {
            known_complete,
            phase: Phase::List { page: 1, last_page: 1 },
            reads: 0,
            pages: 0,
            added: 0,
            removed: 0,
            details: 0,
            list_complete: false,
            stop: None,
            error: String::new(),
        }
    }

    /// 一覧を読み終え、詳細の読み残しを DB から受け取る番。
    pub fn needs_detail_queue(&self) -> bool {
        self.stop.is_none() && self.phase == Phase::NeedDetails
    }

    pub fn start_details(&mut self, queue: Vec<String>) {
        let queue: Vec<String> = queue.into_iter().filter(|id| is_order_id(id)).collect();
        self.phase = if queue.is_empty() {
            Phase::Done
        } else {
            Phase::Details { queue, index: 0 }
        };
    }

    pub fn plan(&mut self) -> Option<Planned> {
        if self.stop.is_some() {
            return None;
        }
        let pause = self.reads > 0;
        let read = match &self.phase {
            Phase::List { page, last_page } => match orders_page_url(*page) {
                Ok(url) => PlannedRead::List {
                    url,
                    page: *page,
                    last_page: (*last_page).max(1),
                },
                Err(err) => {
                    self.fail(err);
                    return None;
                }
            },
            Phase::Details { queue, index } => {
                let order_id = queue.get(*index)?.clone();
                match order_detail_url(&order_id) {
                    Ok(url) => PlannedRead::Detail {
                        url,
                        order_id,
                        index: *index as u32 + 1,
                        total: queue.len() as u32,
                    },
                    Err(err) => {
                        self.fail(err);
                        return None;
                    }
                }
            }
            Phase::NeedDetails | Phase::Done => {
                if self.phase == Phase::Done {
                    self.stop = Some(WalkStop::Finished);
                }
                return None;
            }
        };
        Some(Planned { read, pause })
    }

    pub fn progress_line(&self, planned: &Planned) -> String {
        match &planned.read {
            PlannedRead::List { page, last_page, .. } => {
                format!("購入履歴 {page}/{last_page} ページ（新しい注文 {} 件）", self.added)
            }
            PlannedRead::Detail { index, total, .. } => format!("注文の詳細 {index}/{total}"),
        }
    }

    pub fn accept_list(&mut self, snapshot: &ListSnapshot, applied: ListApplied) {
        if let Some(reason) = purchase_block_reason(&snapshot.href) {
            self.stop_for(reason);
            return;
        }
        let Phase::List { page, last_page } = self.phase.clone() else {
            return;
        };
        self.reads += 1;
        self.pages += 1;
        self.added += applied.added;
        self.removed += applied.removed;
        let last_page = if page == 1 {
            snapshot.last_page.clamp(1, MAX_PAGE)
        } else {
            last_page
        };
        if snapshot.orders.is_empty() || page >= last_page {
            self.list_complete = true;
            self.phase = Phase::NeedDetails;
        } else if self.known_complete && !applied.news {
            // 前回までに最後まで読んでいて、このページに新しい注文が無い。古いページは読み直さない。
            self.list_complete = true;
            self.phase = Phase::NeedDetails;
        } else {
            self.phase = Phase::List {
                page: page + 1,
                last_page,
            };
        }
    }

    pub fn accept_detail(&mut self, snapshot: &DetailSnapshot, applied: DetailApplied) {
        if let Some(reason) = purchase_block_reason(&snapshot.href) {
            self.stop_for(reason);
            return;
        }
        let Phase::Details { queue, index } = &mut self.phase else {
            return;
        };
        self.reads += 1;
        self.details += 1;
        if applied.removed {
            self.removed += 1;
        }
        *index += 1;
        if *index >= queue.len() {
            self.phase = Phase::Done;
        }
    }

    /// 一覧を最後まで読んだ（または前回の続きで新しい注文が尽きた）。DB の完了印に使う。
    pub fn list_complete(&self) -> bool {
        self.list_complete
    }

    pub fn cancel(&mut self) {
        self.stop_for(WalkStop::Cancelled);
    }

    pub fn stop_for(&mut self, reason: WalkStop) {
        if self.stop.is_none() {
            self.stop = Some(reason);
        }
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        self.error = message.into();
        self.stop_for(WalkStop::Failed);
    }

    pub fn stopped(&self) -> bool {
        self.stop.is_some()
    }

    pub fn report(&self, age_message: &str) -> PurchaseWalkReport {
        let stop = self.stop.clone().unwrap_or(WalkStop::Finished);
        PurchaseWalkReport {
            pages: self.pages,
            added: self.added,
            removed: self.removed,
            details: self.details,
            cancelled: stop == WalkStop::Cancelled,
            notice: match stop {
                WalkStop::Login => LOGIN_NOTICE.to_string(),
                WalkStop::AgeGate => age_message.to_string(),
                WalkStop::Failed => self.error.clone(),
                WalkStop::Cancelled | WalkStop::Finished => String::new(),
            },
        }
    }
}

pub fn summary_line(report: &PurchaseWalkReport) -> String {
    format!(
        "購入履歴 {} ページ、新しい注文 {} 件、詳細 {} 件",
        report.pages, report.added, report.details
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(id: &str, state: OrderState) -> ListOrder {
        ListOrder {
            order_id: id.to_string(),
            state,
            ordered_at: Some("2026-11-02T09:30:20+09:00".to_string()),
            title: "見本".to_string(),
        }
    }

    fn list(page: u32, last_page: u32, ids: &[&str]) -> ListSnapshot {
        ListSnapshot {
            href: orders_page_url(page).unwrap().to_string(),
            orders: ids.iter().map(|id| order(id, OrderState::Confirmed)).collect(),
            last_page,
        }
    }

    fn detail(id: &str) -> DetailSnapshot {
        DetailSnapshot {
            href: order_detail_url(id).unwrap().to_string(),
            detail: None,
        }
    }

    #[test]
    fn only_the_order_history_and_one_order_are_reachable() {
        assert_eq!(orders_page_url(1).unwrap().as_str(), "https://accounts.booth.pm/orders");
        assert_eq!(orders_page_url(3).unwrap().as_str(), "https://accounts.booth.pm/orders?page=3");
        assert!(orders_page_url(0).is_err());
        assert!(orders_page_url(201).is_err());
        assert_eq!(
            order_detail_url("90221765").unwrap().as_str(),
            "https://accounts.booth.pm/orders/90221765"
        );
        for bad in ["", "123", "1234567890123", "12a456", "../1234"] {
            assert!(order_detail_url(bad).is_err(), "{bad}");
        }
        for (href, stop) in [
            ("https://accounts.booth.pm/orders?page=2", None),
            ("https://accounts.booth.pm/orders/90221765", None),
            ("https://accounts.booth.pm/library", Some(WalkStop::Login)),
            ("https://accounts.booth.pm/orders/90221765/order_qualified_invoices", Some(WalkStop::Login)),
            ("https://accounts.booth.pm/users/sign_in", Some(WalkStop::Login)),
            ("https://booth.pm/orders", Some(WalkStop::Login)),
            ("http://accounts.booth.pm/orders", Some(WalkStop::Login)),
            ("https://accounts.booth.pm/age_verification?return=/orders", Some(WalkStop::AgeGate)),
        ] {
            assert_eq!(purchase_block_reason(href), stop, "{href}");
        }
        assert_eq!(purchase_settled_stop("about:blank"), None);
    }

    #[test]
    fn landed_pages_match_by_page_number_or_order() {
        let second = orders_page_url(2).unwrap();
        assert!(same_orders_page("https://accounts.booth.pm/orders?page=2", &second));
        assert!(!same_orders_page("https://accounts.booth.pm/orders", &second));
        let first = orders_page_url(1).unwrap();
        assert!(same_orders_page("https://accounts.booth.pm/orders?page=1", &first));
        let one = order_detail_url("90221765").unwrap();
        assert!(same_orders_page("https://accounts.booth.pm/orders/90221765/", &one));
        assert!(!same_orders_page("https://accounts.booth.pm/orders/90221766", &one));
    }

    #[test]
    fn the_first_walk_reads_every_list_page_then_every_detail() {
        let mut walk = PurchaseWalk::new(false);
        let planned = walk.plan().unwrap();
        assert!(!planned.pause);
        walk.accept_list(&list(1, 3, &["1001", "1002"]), ListApplied { added: 2, removed: 0, news: true });
        let planned = walk.plan().unwrap();
        assert!(planned.pause);
        assert!(matches!(planned.read, PlannedRead::List { page: 2, last_page: 3, .. }));
        // 前回の完了印が無いので、新しい注文が無いページでも最後まで読む。
        walk.accept_list(&list(2, 3, &["1003"]), ListApplied::default());
        walk.accept_list(&list(3, 3, &["1004"]), ListApplied::default());
        assert!(walk.list_complete());
        assert!(walk.plan().is_none());
        assert!(walk.needs_detail_queue());
        walk.start_details(vec!["1001".into(), "bad".into(), "1002".into()]);
        let planned = walk.plan().unwrap();
        assert!(matches!(&planned.read, PlannedRead::Detail { order_id, index: 1, total: 2, .. } if order_id == "1001"));
        assert_eq!(walk.progress_line(&planned), "注文の詳細 1/2");
        walk.accept_detail(&detail("1001"), DetailApplied::default());
        walk.accept_detail(&detail("1002"), DetailApplied { removed: true });
        assert!(walk.plan().is_none());
        let report = walk.report("年齢確認");
        assert_eq!((report.pages, report.added, report.removed, report.details), (3, 2, 1, 2));
        assert!(!report.cancelled);
        assert!(report.notice.is_empty());
    }

    #[test]
    fn a_later_walk_stops_the_list_at_a_page_without_new_orders() {
        let mut walk = PurchaseWalk::new(true);
        walk.plan().unwrap();
        walk.accept_list(&list(1, 7, &["1001"]), ListApplied { added: 1, removed: 0, news: true });
        walk.plan().unwrap();
        walk.accept_list(&list(2, 7, &["1002"]), ListApplied::default());
        assert!(walk.list_complete());
        assert!(walk.needs_detail_queue());
        walk.start_details(Vec::new());
        assert!(walk.plan().is_none());
        assert_eq!(walk.report("").pages, 2);
    }

    #[test]
    fn a_cancelled_list_is_not_marked_complete() {
        let mut walk = PurchaseWalk::new(false);
        walk.plan().unwrap();
        walk.accept_list(&list(1, 3, &["1001"]), ListApplied { added: 1, removed: 0, news: true });
        walk.cancel();
        assert!(walk.plan().is_none());
        assert!(!walk.list_complete());
        assert!(walk.report("").cancelled);
    }

    #[test]
    fn landing_on_login_or_age_check_stops_without_reading_more() {
        let mut walk = PurchaseWalk::new(false);
        walk.plan().unwrap();
        walk.accept_list(
            &ListSnapshot {
                href: "https://accounts.booth.pm/users/sign_in".into(),
                orders: Vec::new(),
                last_page: 1,
            },
            ListApplied::default(),
        );
        assert!(walk.plan().is_none());
        assert_eq!(walk.report("").notice, LOGIN_NOTICE);
        assert!(!walk.list_complete());

        let mut walk = PurchaseWalk::new(false);
        walk.plan().unwrap();
        walk.accept_list(&list(1, 1, &["1001"]), ListApplied::default());
        walk.start_details(vec!["1001".into()]);
        walk.plan().unwrap();
        walk.accept_detail(
            &DetailSnapshot {
                href: "https://accounts.booth.pm/age_verification".into(),
                detail: None,
            },
            DetailApplied::default(),
        );
        assert_eq!(walk.report("年齢確認は自動で通過しません").notice, "年齢確認は自動で通過しません");
    }

    #[test]
    fn page_values_are_validated_before_they_reach_the_database() {
        let orders = parse_list_orders(&serde_json::json!([
            { "orderId": "90221765", "state": "confirmed", "orderedAt": "2026-11-02T09:30:20+09:00", "title": " 見本\n の衣装 " },
            { "orderId": "90221765", "state": "confirmed", "orderedAt": null, "title": "重複" },
            { "orderId": "../1", "state": "confirmed", "orderedAt": null, "title": "不正" },
            { "orderId": "90221766", "state": "cancelled", "orderedAt": "not a date", "title": "" }
        ]));
        assert_eq!(orders.len(), 2);
        assert_eq!(orders[0].title, "見本 の衣装");
        assert_eq!(orders[1].state, OrderState::Cancelled);
        assert_eq!(orders[1].ordered_at, None);

        let detail = parse_detail(&serde_json::json!({
            "orderId": "90221765", "state": "confirmed", "orderedAt": "2026-11-02T00:30:20Z",
            "totalYen": 2300.0, "gift": true,
            "items": [
                { "boothId": "1234567", "name": "見本", "shop": "店", "priceYen": 2000, "boostYen": 300, "quantity": 1 },
                { "boothId": "1234567", "name": "見本", "shop": "店", "priceYen": 2000, "boostYen": 300, "quantity": 1 },
                { "boothId": "x", "name": "不正", "shop": "", "priceYen": 1, "boostYen": null, "quantity": 1 },
                { "boothId": "7654321", "name": "負の金額", "shop": "", "priceYen": -5, "boostYen": 0.5, "quantity": 0 }
            ]
        }))
        .unwrap();
        assert_eq!(detail.ordered_at.as_deref(), Some("2026-11-02T09:30:20+09:00"));
        assert_eq!(detail.total_yen, Some(2300));
        assert!(detail.gift);
        assert_eq!(detail.items.len(), 2);
        assert_eq!(detail.items[1].price_yen, None);
        assert_eq!(detail.items[1].boost_yen, None);
        assert_eq!(detail.items[1].quantity, 1);
        assert!(parse_detail(&serde_json::Value::Null).is_none());
        assert!(parse_detail(&serde_json::json!({ "orderId": "x" })).is_none());
    }
}
