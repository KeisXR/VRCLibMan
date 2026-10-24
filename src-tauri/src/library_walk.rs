//! 拡張の「ライブラリをすべて送る」と同じページの進み方。
//! ライブラリを `?page=` で 1 ページずつ読み、そのあとギフトを同じ間隔で読む。
//! BOOTH へのページ遷移は少なくとも 1 秒間隔にする。ファイルは取りに行かない。

use serde_json::Value;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use url::Url;

/// `extension/popup.js` がページのあいだに待つミリ秒。
pub const PAGE_DELAY_MS: u64 = 1000;
pub const PAGE_DELAY: Duration = Duration::from_millis(PAGE_DELAY_MS);
const MAX_PAGE: u32 = 200;

pub const LIBRARY_PAGE: &str = "https://accounts.booth.pm/library";
pub const GIFT_PAGE: &str = "https://accounts.booth.pm/library/gifts";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalkSection {
    Library,
    Gifts,
}

pub const WALK_SECTIONS: [WalkSection; 2] = [WalkSection::Library, WalkSection::Gifts];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WalkStop {
    Finished,
    Cancelled,
    Login,
    AgeGate,
    Failed,
}

#[derive(Clone, Debug)]
pub struct PageSnapshot {
    pub href: String,
    pub products: Vec<Value>,
    pub last_page: u32,
}

#[derive(Clone, Debug)]
pub struct PlannedPage {
    pub url: Url,
    pub pause: bool,
    pub section: WalkSection,
    pub page: u32,
    pub last_page: u32,
}

#[derive(Clone, Debug)]
pub struct CollectedWalk {
    pub products: Vec<Value>,
    pub pages: u32,
    pub files: u32,
    pub named_files: u32,
    pub unnamed_files: u32,
    pub stop: WalkStop,
    pub error: String,
}

impl CollectedWalk {
    pub fn product_count(&self) -> u32 {
        self.products.len() as u32
    }
}

pub fn section_label(section: WalkSection) -> &'static str {
    match section {
        WalkSection::Library => "ライブラリ",
        WalkSection::Gifts => "ギフト",
    }
}

pub fn progress_line(
    section: WalkSection,
    page: u32,
    last_page: u32,
    products: u32,
    files: u32,
    named_files: u32,
    unnamed_files: u32,
) -> String {
    format!(
        "{} {page}/{last_page} ページ（商品 {products}、ファイル {files}、名前あり {named_files}、名前なし {unnamed_files}）",
        section_label(section)
    )
}

pub fn final_counts_line(
    products: u32,
    files: u32,
    named_files: u32,
    unnamed_files: u32,
) -> String {
    format!("商品 {products}、ファイル {files}、名前あり {named_files}、名前なし {unnamed_files}")
}

/// 拡張の `libraryPageUrl` と同じく `page` だけを付ける。
/// 年齢確認やログイン、ライブラリとギフト以外は拒否する。結果に商品名は入らない。
pub fn library_page_url(href: &str, page: u32) -> Result<String, String> {
    if !(1..=MAX_PAGE).contains(&page) {
        return Err("ページ番号が範囲外です".into());
    }
    if walk_block_reason(href).is_some() {
        return Err("このURLへは移動しません".into());
    }
    let url = Url::parse(href).map_err(|_| "ページのURLを読めません".to_string())?;
    if url.scheme() != "https" || url.host_str() != Some("accounts.booth.pm") {
        return Err("ライブラリとギフト以外のページへは移動しません".into());
    }
    let section = match url.path().trim_end_matches('/') {
        "/library" => WalkSection::Library,
        "/library/gifts" => WalkSection::Gifts,
        _ => return Err("ライブラリとギフト以外のページへは移動しません".into()),
    };
    Ok(library_walk_url(section, page)?.to_string())
}

pub fn library_walk_url(section: WalkSection, page: u32) -> Result<Url, String> {
    if !(1..=MAX_PAGE).contains(&page) {
        return Err("ページ番号が範囲外です".into());
    }
    let base = match section {
        WalkSection::Library => LIBRARY_PAGE,
        WalkSection::Gifts => GIFT_PAGE,
    };
    let url = Url::parse(&format!("{base}?page={page}")).map_err(|err| err.to_string())?;
    if walk_block_reason(url.as_str()).is_some() {
        return Err("このURLへは移動しません".into());
    }
    Ok(url)
}

/// 取り込みを止めるページ。年齢確認を先に見る。ライブラリとギフト以外はログイン切れとして止める。
pub fn walk_block_reason(href: &str) -> Option<WalkStop> {
    let url = Url::parse(href).ok()?;
    let path = url.path().to_ascii_lowercase();
    let query = url.query().unwrap_or("").to_ascii_lowercase();
    if is_age_gate(&path, &query) {
        return Some(WalkStop::AgeGate);
    }
    if url.scheme() != "https" {
        return Some(WalkStop::Login);
    }
    let host = url.host_str().unwrap_or("");
    if !is_booth_host(host) || is_login_path(&path) || !is_library_or_gift_path(&path) {
        return Some(WalkStop::Login);
    }
    None
}

/// `about:blank` のように、まだ移動先が決まっていない URL では止めない。
pub fn settled_stop(href: &str) -> Option<WalkStop> {
    let url = Url::parse(href).ok()?;
    if url.scheme() != "https" && url.scheme() != "http" {
        return None;
    }
    walk_block_reason(href)
}

pub fn same_library_page(landed: &str, target: &Url) -> bool {
    let Ok(url) = Url::parse(landed) else {
        return false;
    };
    if url.scheme() != "https" || !is_booth_host(url.host_str().unwrap_or("")) {
        return false;
    }
    if url.path().trim_end_matches('/') != target.path().trim_end_matches('/') {
        return false;
    }
    page_number(&url) == page_number(target)
}

fn page_number(url: &Url) -> u32 {
    url.query_pairs()
        .find(|(key, _)| key == "page")
        .and_then(|(_, value)| value.parse::<u32>().ok())
        .filter(|page| (1..=MAX_PAGE).contains(page))
        .unwrap_or(1)
}

fn is_booth_host(host: &str) -> bool {
    host == "booth.pm" || host.ends_with(".booth.pm")
}

fn is_login_path(path: &str) -> bool {
    path.contains("sign_in") || path.contains("signin") || path.contains("/login")
}

fn is_library_or_gift_path(path: &str) -> bool {
    let path = path.trim_end_matches('/');
    path == "/library" || path == "/library/gifts"
}

fn is_age_gate(path: &str, query: &str) -> bool {
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

/// 番号だけ、空、downloadable id と同じ名前は「名前なし」。
pub fn file_has_name(filename: &str, downloadable_id: &str) -> bool {
    let name = filename.trim();
    if name.is_empty() {
        return false;
    }
    let id = downloadable_id.trim();
    if !id.is_empty() && name == id {
        return false;
    }
    let bytes = name.as_bytes();
    !((4..=12).contains(&bytes.len()) && bytes.iter().all(|byte| byte.is_ascii_digit()))
}

fn count_files(product: &Value) -> (u32, u32, u32) {
    let Some(downloads) = product.get("downloads").and_then(|value| value.as_array()) else {
        return (0, 0, 0);
    };
    let mut files = 0;
    let mut named = 0;
    let mut unnamed = 0;
    for download in downloads {
        let filename = download
            .get("filename")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let id = download
            .get("downloadableId")
            .or_else(|| download.get("downloadable_id"))
            .and_then(|value| value.as_str())
            .unwrap_or("");
        files += 1;
        if file_has_name(filename, id) {
            named += 1;
        } else {
            unnamed += 1;
        }
    }
    (files, named, unnamed)
}

fn booth_id_of(product: &Value) -> String {
    match product.get("boothId").or_else(|| product.get("booth_id")) {
        Some(Value::String(id)) => id.trim().to_string(),
        Some(Value::Number(number)) => number
            .as_u64()
            .map(|number| number.to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

pub struct WalkMachine {
    section_index: usize,
    page: u32,
    last_page: u32,
    products: Vec<Value>,
    seen: HashSet<String>,
    pages: u32,
    files: u32,
    named_files: u32,
    unnamed_files: u32,
    stop: Option<WalkStop>,
    error: String,
}

impl WalkMachine {
    pub fn new() -> Self {
        Self {
            section_index: 0,
            page: 1,
            last_page: 1,
            products: Vec::new(),
            seen: HashSet::new(),
            pages: 0,
            files: 0,
            named_files: 0,
            unnamed_files: 0,
            stop: None,
            error: String::new(),
        }
    }

    pub fn plan(&mut self) -> Option<PlannedPage> {
        if self.stop.is_some() {
            return None;
        }
        if self.section_index >= WALK_SECTIONS.len() {
            self.stop = Some(WalkStop::Finished);
            return None;
        }
        let section = WALK_SECTIONS[self.section_index];
        let url = match library_walk_url(section, self.page) {
            Ok(url) => url,
            Err(_) => {
                self.stop = Some(WalkStop::AgeGate);
                return None;
            }
        };
        if let Some(reason) = walk_block_reason(url.as_str()) {
            self.stop = Some(reason);
            return None;
        }
        Some(PlannedPage {
            url,
            pause: self.pages > 0,
            section,
            page: self.page,
            last_page: self.last_page.max(1),
        })
    }

    pub fn progress_line_for(&self, planned: &PlannedPage) -> String {
        progress_line(
            planned.section,
            planned.page,
            planned.last_page,
            self.products.len() as u32,
            self.files,
            self.named_files,
            self.unnamed_files,
        )
    }

    pub fn accept(&mut self, snapshot: PageSnapshot) {
        if let Some(reason) = walk_block_reason(&snapshot.href) {
            self.stop = Some(reason);
            return;
        }
        let incoming = snapshot.products;
        if self.page >= 2 && incoming.is_empty() {
            self.advance_section();
            return;
        }
        for product in incoming {
            let id = booth_id_of(&product);
            if id.is_empty() || !self.seen.insert(id) {
                continue;
            }
            let (files, named, unnamed) = count_files(&product);
            self.files += files;
            self.named_files += named;
            self.unnamed_files += unnamed;
            self.products.push(product);
        }
        self.pages += 1;
        if self.page == 1 {
            self.last_page = snapshot.last_page.clamp(1, MAX_PAGE);
        }
        if self.page >= self.last_page {
            self.advance_section();
        } else {
            self.page += 1;
        }
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
        if self.stop.is_none() {
            self.stop = Some(WalkStop::Failed);
        }
    }

    pub fn stopped(&self) -> bool {
        self.stop.is_some()
    }

    pub fn collected(&self) -> CollectedWalk {
        CollectedWalk {
            products: self.products.clone(),
            pages: self.pages,
            files: self.files,
            named_files: self.named_files,
            unnamed_files: self.unnamed_files,
            stop: self.stop.clone().unwrap_or(WalkStop::Finished),
            error: self.error.clone(),
        }
    }

    fn advance_section(&mut self) {
        self.section_index += 1;
        self.page = 1;
        self.last_page = 1;
    }
}

pub fn drive_library_walk<R, S, P>(
    mut read: R,
    mut sleep_between: S,
    mut on_progress: P,
    cancel: &AtomicBool,
) -> CollectedWalk
where
    R: FnMut(&Url) -> Result<PageSnapshot, String>,
    S: FnMut(Duration),
    P: FnMut(&str),
{
    let mut machine = WalkMachine::new();
    loop {
        if cancel.load(Ordering::SeqCst) {
            machine.cancel();
            break;
        }
        let Some(planned) = machine.plan() else {
            break;
        };
        let line = machine.progress_line_for(&planned);
        on_progress(&line);
        if planned.pause {
            if cancel.load(Ordering::SeqCst) {
                machine.cancel();
                break;
            }
            sleep_between(PAGE_DELAY);
            if cancel.load(Ordering::SeqCst) {
                machine.cancel();
                break;
            }
        }
        if let Some(reason) = walk_block_reason(planned.url.as_str()) {
            machine.stop_for(reason);
            break;
        }
        match read(&planned.url) {
            Ok(snapshot) => machine.accept(snapshot),
            Err(err) => {
                machine.fail(err);
                break;
            }
        }
        if machine.stopped() {
            break;
        }
    }
    machine.collected()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn snapshot(href: &str, products: Value, last_page: u32) -> PageSnapshot {
        PageSnapshot {
            href: href.to_string(),
            products: products.as_array().cloned().unwrap_or_default(),
            last_page,
        }
    }

    fn sample_product() -> Value {
        serde_json::json!({
            "boothId": "424242",
            "title": "サンプル商品",
            "downloads": [
                {"filename": "sample_body.unitypackage", "downloadableId": "2336025", "variation": ""},
                {"filename": "2336028", "downloadableId": "2336028", "variation": ""}
            ]
        })
    }

    #[test]
    fn page_urls_match_the_extension_and_reject_age_gates() {
        assert_eq!(PAGE_DELAY_MS, 1000);
        assert_eq!(PAGE_DELAY, Duration::from_millis(1000));
        assert_eq!(
            library_page_url("https://accounts.booth.pm/library?page=4", 1).unwrap(),
            "https://accounts.booth.pm/library?page=1"
        );
        assert_eq!(
            library_page_url(LIBRARY_PAGE, 2).unwrap(),
            "https://accounts.booth.pm/library?page=2"
        );
        assert_eq!(
            library_page_url("https://accounts.booth.pm/library/gifts", 3).unwrap(),
            "https://accounts.booth.pm/library/gifts?page=3"
        );
        for page in [1_u32, 2, 12] {
            for section in WALK_SECTIONS {
                let url = library_walk_url(section, page).unwrap();
                assert!(
                    url.as_str()
                        .starts_with("https://accounts.booth.pm/library")
                );
                assert_eq!(url.query(), Some(format!("page={page}").as_str()));
                assert!(!url.as_str().contains("サンプル"));
                assert!(!url.as_str().contains("sample_body"));
                assert!(walk_block_reason(url.as_str()).is_none());
            }
        }
        assert!(library_page_url("https://accounts.booth.pm/library?r18=1", 1).is_err());
        assert!(library_page_url("https://accounts.booth.pm/age_verification", 1).is_err());
        assert!(
            library_page_url("https://accounts.booth.pm/library/gifts?age_check=1", 2).is_err()
        );
        assert!(library_page_url("https://accounts.pixiv.net/login", 1).is_err());
        assert!(library_page_url("https://accounts.booth.pm/users/sign_in", 1).is_err());
        assert!(library_page_url("https://booth.pm/ja/items/424242", 1).is_err());
        assert!(library_walk_url(WalkSection::Library, 0).is_err());
        assert!(library_walk_url(WalkSection::Gifts, 201).is_err());
        assert_eq!(
            walk_block_reason("https://accounts.booth.pm/age_verification?next=/library"),
            Some(WalkStop::AgeGate)
        );
        assert_eq!(
            walk_block_reason("https://accounts.pixiv.net/login?return_to=secret"),
            Some(WalkStop::Login)
        );
        assert_eq!(
            walk_block_reason("https://accounts.booth.pm/users/sign_in"),
            Some(WalkStop::Login)
        );
        assert!(settled_stop("about:blank").is_none());
        assert_eq!(
            settled_stop("https://accounts.booth.pm/library?r18=1"),
            Some(WalkStop::AgeGate)
        );
        let page2 = library_walk_url(WalkSection::Library, 2).unwrap();
        assert!(same_library_page(
            "https://accounts.booth.pm/library?page=2",
            &page2
        ));
        assert!(!same_library_page(
            "https://accounts.booth.pm/library",
            &page2
        ));
        let page1 = library_walk_url(WalkSection::Library, 1).unwrap();
        assert!(same_library_page(
            "https://accounts.booth.pm/library",
            &page1
        ));
        let gift = library_walk_url(WalkSection::Gifts, 1).unwrap();
        assert!(!same_library_page(
            "https://accounts.booth.pm/library?page=1",
            &gift
        ));
    }

    #[test]
    fn delay_matches_the_extension_without_sleeping_it() {
        let cancel = AtomicBool::new(false);
        let mut delays = Vec::new();
        let reads = AtomicUsize::new(0);
        let collected = drive_library_walk(
            |_| {
                let n = reads.fetch_add(1, Ordering::SeqCst);
                let href = if n == 0 { LIBRARY_PAGE } else { GIFT_PAGE };
                Ok(snapshot(href, serde_json::json!([]), 1))
            },
            |delay| delays.push(delay),
            |_| {},
            &cancel,
        );
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        assert_eq!(delays, vec![PAGE_DELAY]);
        assert_eq!(delays[0].as_millis(), 1000);
        assert_eq!(collected.stop, WalkStop::Finished);
        assert_eq!(collected.pages, 2);
    }

    #[test]
    fn cancel_during_the_pause_does_not_open_the_next_page() {
        let cancel = AtomicBool::new(false);
        let mut opened = Vec::new();
        let collected = drive_library_walk(
            |url| {
                opened.push(url.to_string());
                Ok(snapshot(
                    LIBRARY_PAGE,
                    serde_json::json!([sample_product()]),
                    4,
                ))
            },
            |_| cancel.store(true, Ordering::SeqCst),
            |_| {},
            &cancel,
        );
        assert_eq!(
            opened,
            vec!["https://accounts.booth.pm/library?page=1".to_string()]
        );
        assert_eq!(collected.stop, WalkStop::Cancelled);
        assert_eq!(collected.product_count(), 1);
        assert!(opened.iter().all(|url| !url.contains("/gifts")));
        assert!(opened.iter().all(|url| walk_block_reason(url).is_none()));
    }

    #[test]
    fn an_age_gate_or_login_page_stops_the_walk_before_later_pages() {
        for landed in [
            "https://accounts.booth.pm/age_verification",
            "https://accounts.pixiv.net/login",
            "https://accounts.booth.pm/users/sign_in",
        ] {
            let cancel = AtomicBool::new(false);
            let mut opened = Vec::new();
            let expected = if landed.contains("age_") {
                WalkStop::AgeGate
            } else {
                WalkStop::Login
            };
            let collected = drive_library_walk(
                |url| {
                    opened.push(url.to_string());
                    if opened.len() == 1 {
                        Ok(snapshot(
                            LIBRARY_PAGE,
                            serde_json::json!([sample_product()]),
                            3,
                        ))
                    } else {
                        Ok(snapshot(landed, serde_json::json!([]), 1))
                    }
                },
                |_| {},
                |line| {
                    assert!(!line.contains('?'));
                    assert!(!line.contains("サンプル商品"));
                    assert!(!line.contains("sample_body"));
                },
                &cancel,
            );
            assert_eq!(collected.stop, expected, "{landed}");
            assert_eq!(
                opened,
                vec![
                    "https://accounts.booth.pm/library?page=1".to_string(),
                    "https://accounts.booth.pm/library?page=2".to_string(),
                ],
                "{landed}"
            );
            assert!(opened.iter().all(|url| walk_block_reason(url).is_none()));
            assert!(collected.products.iter().all(|product| {
                product.get("title").and_then(|value| value.as_str()) == Some("サンプル商品")
            }));
        }
    }

    #[test]
    fn fixture_html_matches_extract_js_and_counts_named_files() {
        let html = r#"<!doctype html><body>
          <div class="mb-16 bg-white">
            <div class="gap-8 border-b pb-16">
              <a href="https://booth.pm/ja/items/424242">
                <img class="l-library-item-thumbnail" alt="" src="https://booth.pximg.net/c/72x72/a.jpg" />
              </a>
              <div>
                <a class="text-text-default" href="https://booth.pm/items/424242">サンプル商品</a>
              </div>
            </div>
            <div class="mt-16">
              <div class="flex items-center justify-between">
                <div>
                  <div>通常</div>
                  <div class="flex min-w-0">
                    <div class="truncate"><span>sample_body</span></div>
                    <div class="shrink-0">.unitypackage</div>
                  </div>
                </div>
                <a href="https://booth.pm/downloadables/2336025">ダウンロード</a>
              </div>
              <div class="flex items-center justify-between">
                <a href="https://booth.pm/downloadables/2336028">ダウンロード</a>
              </div>
            </div>
          </div>
          <a href="?page=3">3</a>
        </body>"#;
        let parsed = extract_fixture(html, LIBRARY_PAGE);
        let products = parsed
            .get("products")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap();
        assert_eq!(products.len(), 1);
        let downloads = products[0]
            .get("downloads")
            .and_then(|value| value.as_array())
            .unwrap();
        assert_eq!(downloads.len(), 2);
        let named = downloads
            .iter()
            .filter(|row| {
                file_has_name(
                    row.get("filename")
                        .and_then(|value| value.as_str())
                        .unwrap_or(""),
                    row.get("downloadableId")
                        .and_then(|value| value.as_str())
                        .unwrap_or(""),
                )
            })
            .count();
        let unnamed = downloads.len() - named;
        assert_eq!(named, 1);
        assert_eq!(unnamed, 1);
        assert_eq!(
            downloads[0]
                .get("filename")
                .and_then(|value| value.as_str()),
            Some("sample_body.unitypackage")
        );
        assert_eq!(
            downloads[1]
                .get("filename")
                .and_then(|value| value.as_str()),
            Some("2336028")
        );
        assert_eq!(
            parsed.get("lastPage").and_then(|value| value.as_u64()),
            Some(3)
        );
        let cancel = AtomicBool::new(false);
        let mut opened = Vec::new();
        let mut lines = Vec::new();
        let body = Value::Array(products);
        let collected = drive_library_walk(
            |url| {
                opened.push(url.to_string());
                if opened.len() == 1 {
                    Ok(snapshot(LIBRARY_PAGE, body.clone(), 3))
                } else {
                    Ok(snapshot(
                        "https://accounts.booth.pm/age_verification",
                        serde_json::json!([]),
                        1,
                    ))
                }
            },
            |_| {},
            |line| lines.push(line.to_string()),
            &cancel,
        );
        assert_eq!(collected.stop, WalkStop::AgeGate);
        assert_eq!(collected.product_count(), 1);
        assert_eq!(collected.files, 2);
        assert_eq!(collected.named_files, 1);
        assert_eq!(collected.unnamed_files, 1);
        assert_eq!(opened.len(), 2);
        assert!(opened.iter().all(|url| !url.contains("age_")));
        let summary = final_counts_line(
            collected.product_count(),
            collected.files,
            collected.named_files,
            collected.unnamed_files,
        );
        assert_eq!(summary, "商品 1、ファイル 2、名前あり 1、名前なし 1");
        for line in lines.iter().chain(std::iter::once(&summary)) {
            assert!(!line.contains("サンプル商品"));
            assert!(!line.contains("sample_body"));
            assert!(!line.contains('?'));
        }
    }

    fn extract_fixture(html: &str, href: &str) -> Value {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let dir = std::env::temp_dir().join(format!("vrclibman-extract-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("read-fixture.cjs");
        let html_path = dir.join("fixture.html");
        std::fs::write(
            &script,
            r#"
const { parseHTML } = require(process.argv[2]);
const api = require(process.argv[3]);
const fs = require("fs");
const html = fs.readFileSync(process.argv[4], "utf8");
const href = process.argv[5];
const { document } = parseHTML(html);
const products = api.extractPage(document, href);
const lastPage = api.libraryLastPage(document, href);
process.stdout.write(JSON.stringify({ products, lastPage }));
"#,
        )
        .unwrap();
        std::fs::write(&html_path, html).unwrap();
        let output = std::process::Command::new("node")
            .arg(&script)
            .arg(root.join("node_modules/linkedom"))
            .arg(root.join("extension/extract.js"))
            .arg(&html_path)
            .arg(href)
            .output()
            .expect("node が必要です");
        assert!(
            output.status.success(),
            "extract.js を読めません: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("extract.js の結果")
    }
}
