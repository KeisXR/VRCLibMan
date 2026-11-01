use serde::{Deserialize, Serialize};
use url::Url;

pub const TOOLBAR_LABEL: &str = "booth-toolbar";
/// The right panel is the only docked host. Window hosts are `window-N`.
pub const PANEL_HOST_ID: &str = "panel";
pub const DEFAULT_TOOLBAR_HEIGHT: f64 = 112.0;
pub const MAX_TABS: usize = 20;
pub const MAX_BOOKMARKS: usize = 200;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ZoomChange {
    In,
    Out,
    Reset,
}

/// Page zoom is kept on each native view, independently of its host window.
pub fn next_zoom(current: f64, change: ZoomChange) -> f64 {
    const LEVELS: &[f64] = &[
        0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5,
        3.0, 4.0, 5.0,
    ];
    let current = if current.is_finite() { current } else { 1.0 };
    match change {
        ZoomChange::In => LEVELS.iter().copied().find(|level| *level > current + 0.001).unwrap_or(5.0),
        ZoomChange::Out => LEVELS.iter().rev().copied().find(|level| *level < current - 0.001).unwrap_or(0.25),
        ZoomChange::Reset => 1.0,
    }
}

/// App-made toolbars may call browser commands. Page webviews may not.
pub fn is_toolbar_label(label: &str) -> bool {
    label == TOOLBAR_LABEL || label.starts_with("booth-toolbar-")
}

pub fn window_label_for(slot: u64) -> String {
    format!("booth-win-{slot}")
}

pub fn toolbar_label_for(slot: u64) -> String {
    format!("booth-toolbar-{slot}")
}

/// Logical screen position for a detached window, near the pointer.
/// Negative coordinates are other monitors. Non-finite values are rejected.
pub fn detach_position(x: f64, y: f64) -> Result<(f64, f64), String> {
    if !x.is_finite() || !y.is_finite() {
        return Err("分離先の位置が不正です".into());
    }
    const LIMIT: f64 = 100_000.0;
    Ok((x.clamp(-LIMIT, LIMIT), (y - 28.0).clamp(-LIMIT, LIMIT)))
}

/// A late Destroyed event matches only the generation still registered for that window.
pub fn destroy_matches(live_generation: Option<u64>, event_generation: u64) -> bool {
    live_generation == Some(event_generation)
}

/// Preserve BOOTH / pixiv's existing same-tab login flow; normal links get a tab.
pub fn login_popup_in_same_tab(url: &Url) -> bool {
    let host = url.host_str().unwrap_or("");
    let path = url.path().to_ascii_lowercase();
    url.scheme() == "https"
        && (host == "booth.pm"
            || host.ends_with(".booth.pm")
            || host == "pixiv.net"
            || host.ends_with(".pixiv.net"))
        && ["login", "signin", "session", "oauth", "auth", "sign_in"]
            .iter()
            .any(|part| path.contains(part))
}

/// User navigation is general web browsing; application and executable schemes stay outside it.
pub fn navigation_url(input: &str) -> Result<Url, String> {
    let input = input.trim();
    if input.is_empty() || input.len() > 4096 || input.chars().any(char::is_control) {
        return Err("URLを入力してください（4096文字以内）".into());
    }
    let candidate = if input.contains("://") {
        input.to_string()
    } else {
        format!("https://{input}")
    };
    let url = Url::parse(&candidate).map_err(|_| "URLが正しくありません".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("http または https のページを開いてください".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("認証情報を含むURLは使えません".into());
    }
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    if host == "tauri.localhost"
        || host.ends_with(".tauri.localhost")
        || host == "asset.localhost"
        || host == "ipc.localhost"
    {
        return Err("アプリ内部のURLはブラウザでは開けません".into());
    }
    Ok(url)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub title: String,
    pub url: String,
}

impl Bookmark {
    pub fn new(title: &str, input: &str) -> Result<Self, String> {
        let url = navigation_url(input)?;
        let title = title.trim();
        Ok(Self {
            title: if title.is_empty() {
                url.host_str().unwrap_or("ページ").into()
            } else {
                title.chars().take(100).collect()
            },
            url: url.to_string(),
        })
    }
}

pub fn save_bookmark(
    bookmarks: &mut Vec<Bookmark>,
    original: Option<&str>,
    title: &str,
    url: &str,
) -> Result<(), String> {
    let bookmark = Bookmark::new(title, url)?;
    if let Some(original) = original {
        let index = bookmarks
            .iter()
            .position(|item| item.url == original)
            .ok_or("編集するブックマークが見つかりません")?;
        bookmarks[index] = bookmark.clone();
        // Editing to an existing URL merges the duplicates using the edited title.
        let mut seen = false;
        bookmarks.retain(|item| {
            if item.url != bookmark.url {
                true
            } else if !seen {
                seen = true;
                true
            } else {
                false
            }
        });
        if let Some(item) = bookmarks.iter_mut().find(|item| item.url == bookmark.url) {
            item.title = bookmark.title;
        }
    } else if let Some(existing) = bookmarks.iter_mut().find(|item| item.url == bookmark.url) {
        existing.title = bookmark.title;
    } else {
        if bookmarks.len() >= MAX_BOOKMARKS {
            return Err(format!("ブックマークは{MAX_BOOKMARKS}件まで保存できます"));
        }
        bookmarks.push(bookmark);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HostKind {
    Panel,
    Window,
}

impl HostKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Panel => "panel",
            Self::Window => "window",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTab {
    pub id: String,
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub zoom_factor: f64,
    pub host_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostState {
    pub id: String,
    pub kind: HostKind,
    pub tab_ids: Vec<String>,
    pub active_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserState {
    pub open: bool,
    pub tabs: Vec<BrowserTab>,
    /// Active tab of the focused host. Each host also has its own `hosts[].activeId`.
    pub active_id: Option<String>,
    pub hosts: Vec<HostState>,
    pub bookmarks: Vec<Bookmark>,
    pub busy: bool,
    /// "panel" when the panel has pages, otherwise "window" when a separate window does.
    pub host: Option<&'static str>,
    /// This build can place pages in the main window's right panel.
    pub panel_supported: bool,
}

#[derive(Debug, Clone)]
pub struct SessionHost {
    pub id: String,
    pub kind: HostKind,
    pub order: Vec<String>,
    pub active_id: Option<String>,
    pub slot: u64,
}

/// Enough to put a tab back where it was when a move fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveToken {
    pub tab_id: String,
    pub from_host: String,
    pub from_index: usize,
    pub from_active: Option<String>,
    pub to_host: String,
    pub to_active: Option<String>,
}

pub struct TabSession {
    pub tabs: Vec<BrowserTab>,
    pub hosts: Vec<SessionHost>,
    next_id: u64,
    next_window: u64,
    focused: Option<String>,
}

impl TabSession {
    pub const fn new() -> Self {
        Self {
            tabs: Vec::new(),
            hosts: Vec::new(),
            next_id: 1,
            next_window: 1,
            focused: None,
        }
    }

    pub fn ensure_panel(&mut self) {
        if self.hosts.iter().any(|host| host.id == PANEL_HOST_ID) {
            return;
        }
        self.hosts.push(SessionHost {
            id: PANEL_HOST_ID.into(),
            kind: HostKind::Panel,
            order: Vec::new(),
            active_id: None,
            slot: 0,
        });
    }

    pub fn add_window(&mut self) -> String {
        let slot = self.next_window;
        self.next_window += 1;
        let id = format!("window-{slot}");
        self.hosts.push(SessionHost {
            id: id.clone(),
            kind: HostKind::Window,
            order: Vec::new(),
            active_id: None,
            slot,
        });
        id
    }

    pub fn add(&mut self, host_id: &str, url: &Url) -> Result<String, String> {
        if self.tabs.len() >= MAX_TABS {
            return Err(format!("タブは{MAX_TABS}個まで開けます"));
        }
        let host = self
            .hosts
            .iter()
            .position(|host| host.id == host_id)
            .ok_or("表示先が見つかりません")?;
        let id = format!("booth-tab-{}", self.next_id);
        self.next_id += 1;
        self.tabs.push(BrowserTab {
            id: id.clone(),
            url: url.to_string(),
            title: url.host_str().unwrap_or("新しいタブ").into(),
            loading: true,
            can_go_back: false,
            can_go_forward: false,
            zoom_factor: 1.0,
            host_id: host_id.into(),
        });
        self.hosts[host].order.push(id.clone());
        self.hosts[host].active_id = Some(id.clone());
        self.focused = Some(host_id.into());
        Ok(id)
    }

    pub fn select(&mut self, id: &str) -> Result<(), String> {
        let host_id = self
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| tab.host_id.clone())
            .ok_or("タブが見つかりません")?;
        let host = self
            .hosts
            .iter_mut()
            .find(|host| host.id == host_id)
            .ok_or("表示先が見つかりません")?;
        if !host.order.iter().any(|tab| tab == id) {
            return Err("タブが見つかりません".into());
        }
        host.active_id = Some(id.into());
        self.focused = Some(host_id);
        Ok(())
    }

    /// Removes one tab. `Some(host)` means that host no longer has tabs.
    pub fn remove(&mut self, id: &str) -> Result<Option<String>, String> {
        let host_id = self
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| tab.host_id.clone())
            .ok_or("タブが見つかりません")?;
        let host = self
            .hosts
            .iter_mut()
            .find(|host| host.id == host_id)
            .ok_or("タブが見つかりません")?;
        let index = host
            .order
            .iter()
            .position(|tab| tab == id)
            .ok_or("タブが見つかりません")?;
        host.order.remove(index);
        if host.active_id.as_deref() == Some(id) {
            let len = host.order.len();
            host.active_id = if len == 0 {
                None
            } else {
                host.order.get(index.min(len - 1)).cloned()
            };
        }
        let empty = host.order.is_empty();
        self.tabs.retain(|tab| tab.id != id);
        if empty {
            self.hosts.retain(|host| host.id != host_id);
            if self.focused.as_deref() == Some(host_id.as_str()) {
                self.focused = self.primary_host().map(str::to_string);
            }
            Ok(Some(host_id))
        } else {
            Ok(None)
        }
    }

    pub fn reorder(&mut self, host_id: &str, ids: &[String]) -> Result<(), String> {
        let host = self
            .hosts
            .iter_mut()
            .find(|host| host.id == host_id)
            .ok_or("表示先が見つかりません")?;
        if ids.len() != host.order.len() || !host.order.iter().all(|id| ids.contains(id)) {
            return Err("タブの並びが一致しません".into());
        }
        host.order = ids.to_vec();
        Ok(())
    }

    /// Moves one tab onto another host. An emptied source host stays until `drop_host_if_empty`.
    pub fn move_tab(&mut self, tab_id: &str, to_host: &str, index: usize) -> Result<MoveToken, String> {
        let from_host = self
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .map(|tab| tab.host_id.clone())
            .ok_or("タブが見つかりません")?;
        if from_host == to_host {
            return Err("タブはすでにその表示先にあります".into());
        }
        let from = self
            .hosts
            .iter()
            .position(|host| host.id == from_host)
            .ok_or("表示先が見つかりません")?;
        let to = self
            .hosts
            .iter()
            .position(|host| host.id == to_host)
            .ok_or("表示先が見つかりません")?;
        let from_pos = self.hosts[from]
            .order
            .iter()
            .position(|id| id == tab_id)
            .ok_or("タブが見つかりません")?;
        let from_active = self.hosts[from].active_id.clone();
        let to_active = self.hosts[to].active_id.clone();
        self.hosts[from].order.remove(from_pos);
        if self.hosts[from].active_id.as_deref() == Some(tab_id) {
            let len = self.hosts[from].order.len();
            self.hosts[from].active_id = if len == 0 {
                None
            } else {
                self.hosts[from].order.get(from_pos.min(len - 1)).cloned()
            };
        }
        let insert = index.min(self.hosts[to].order.len());
        self.hosts[to].order.insert(insert, tab_id.to_string());
        self.hosts[to].active_id = Some(tab_id.to_string());
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            tab.host_id = to_host.to_string();
        }
        self.focused = Some(to_host.to_string());
        Ok(MoveToken {
            tab_id: tab_id.into(),
            from_host,
            from_index: from_pos,
            from_active,
            to_host: to_host.into(),
            to_active,
        })
    }

    pub fn restore(&mut self, token: MoveToken) -> Result<(), String> {
        self.ensure_known_host(&token.from_host);
        if let Some(host) = self.hosts.iter_mut().find(|host| host.id == token.to_host) {
            host.order.retain(|id| id != &token.tab_id);
            host.active_id = token.to_active.clone();
        }
        let host = self
            .hosts
            .iter_mut()
            .find(|host| host.id == token.from_host)
            .ok_or("表示先が見つかりません")?;
        if !host.order.iter().any(|id| id == &token.tab_id) {
            let insert = token.from_index.min(host.order.len());
            host.order.insert(insert, token.tab_id.clone());
        }
        host.active_id = token.from_active.clone();
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == token.tab_id) {
            tab.host_id = token.from_host.clone();
        }
        self.focused = Some(token.from_host.clone());
        self.hosts.retain(|host| !host.order.is_empty() || host.id == token.from_host);
        if self
            .hosts
            .iter()
            .any(|host| host.id == token.from_host && host.order.is_empty())
        {
            self.hosts.retain(|host| host.id != token.from_host || !host.order.is_empty());
        }
        Ok(())
    }

    pub fn drop_host_if_empty(&mut self, host_id: &str) -> bool {
        let empty = self
            .hosts
            .iter()
            .any(|host| host.id == host_id && host.order.is_empty());
        if !empty {
            return false;
        }
        self.hosts.retain(|host| host.id != host_id);
        if self.focused.as_deref() == Some(host_id) {
            self.focused = self.primary_host().map(str::to_string);
        }
        true
    }

    /// Forgets one host and the tabs that still belong to it. Other hosts stay.
    pub fn take_host(&mut self, host_id: &str) -> Vec<String> {
        let Some(index) = self.hosts.iter().position(|host| host.id == host_id) else {
            return Vec::new();
        };
        let ids = self.hosts.remove(index).order;
        self.tabs.retain(|tab| !ids.iter().any(|id| id == &tab.id));
        if self.focused.as_deref() == Some(host_id) {
            self.focused = self.primary_host().map(str::to_string);
        }
        ids
    }

    pub fn clear(&mut self) {
        self.tabs.clear();
        self.hosts.clear();
        self.focused = None;
    }

    pub fn update(&mut self, id: &str, work: impl FnOnce(&mut BrowserTab)) -> bool {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            work(tab);
            true
        } else {
            false
        }
    }

    pub fn has(&self, id: &str) -> bool {
        self.tabs.iter().any(|tab| tab.id == id)
    }

    pub fn active_of(&self, host_id: &str) -> Option<String> {
        self.hosts
            .iter()
            .find(|host| host.id == host_id)
            .and_then(|host| host.active_id.clone())
    }

    pub fn host_of(&self, tab_id: &str) -> Option<String> {
        self.tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .map(|tab| tab.host_id.clone())
    }

    pub fn kind_of(&self, host_id: &str) -> Option<HostKind> {
        self.hosts
            .iter()
            .find(|host| host.id == host_id)
            .map(|host| host.kind)
    }

    pub fn slot_of(&self, host_id: &str) -> Option<u64> {
        self.hosts
            .iter()
            .find(|host| host.id == host_id)
            .map(|host| host.slot)
    }

    pub fn focus_host(&mut self, host_id: &str) {
        if self.hosts.iter().any(|host| host.id == host_id) {
            self.focused = Some(host_id.to_string());
        }
    }

    pub fn focused_host(&self) -> Option<String> {
        self.focused.clone()
    }

    pub fn focused_tab(&self) -> Option<String> {
        if let Some(host) = &self.focused {
            if let Some(tab) = self.active_of(host) {
                return Some(tab);
            }
        }
        self.primary_host().and_then(|host| self.active_of(host))
    }

    pub fn primary_host(&self) -> Option<&str> {
        self.hosts
            .iter()
            .find(|host| host.kind == HostKind::Panel && !host.order.is_empty())
            .or_else(|| self.hosts.iter().find(|host| !host.order.is_empty()))
            .map(|host| host.id.as_str())
    }

    pub fn coarse_host(&self) -> Option<&'static str> {
        if self
            .hosts
            .iter()
            .any(|host| host.kind == HostKind::Panel && !host.order.is_empty())
        {
            Some("panel")
        } else if self
            .hosts
            .iter()
            .any(|host| host.kind == HostKind::Window && !host.order.is_empty())
        {
            Some("window")
        } else {
            None
        }
    }

    pub fn host_states(&self) -> Vec<HostState> {
        self.hosts
            .iter()
            .filter(|host| !host.order.is_empty())
            .map(|host| HostState {
                id: host.id.clone(),
                kind: host.kind,
                tab_ids: host.order.clone(),
                active_id: host.active_id.clone(),
            })
            .collect()
    }

    fn ensure_known_host(&mut self, host_id: &str) {
        if self.hosts.iter().any(|host| host.id == host_id) {
            return;
        }
        if host_id == PANEL_HOST_ID {
            self.ensure_panel();
            return;
        }
        let slot = host_id
            .strip_prefix("window-")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        self.hosts.push(SessionHost {
            id: host_id.into(),
            kind: HostKind::Window,
            order: Vec::new(),
            active_id: None,
            slot,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_zoom_steps_reset_and_clamp_at_both_limits() {
        assert_eq!(next_zoom(1.0, ZoomChange::Out), 0.9);
        assert_eq!(next_zoom(1.0, ZoomChange::In), 1.1);
        assert_eq!(next_zoom(1.18, ZoomChange::In), 1.25);
        assert_eq!(next_zoom(1.18, ZoomChange::Out), 1.1);
        assert_eq!(next_zoom(1.10000001, ZoomChange::In), 1.25);
        assert_eq!(next_zoom(5.0, ZoomChange::In), 5.0);
        assert_eq!(next_zoom(0.25, ZoomChange::Out), 0.25);
        assert_eq!(next_zoom(0.25, ZoomChange::Reset), 1.0);
        assert_eq!(next_zoom(f64::NAN, ZoomChange::Reset), 1.0);
    }

    #[test]
    fn page_zoom_belongs_to_the_tab_when_it_moves_or_rolls_back() {
        let mut tabs = TabSession::new();
        tabs.ensure_panel();
        let page = navigation_url("zoom.example").unwrap();
        let first = tabs.add(PANEL_HOST_ID, &page).unwrap();
        let second = tabs.add(PANEL_HOST_ID, &page).unwrap();
        tabs.update(&first, |tab| tab.zoom_factor = 0.75);
        let window = tabs.add_window();
        let moved = tabs.move_tab(&first, &window, 0).unwrap();
        assert_eq!(tabs.tabs.iter().find(|tab| tab.id == first).unwrap().zoom_factor, 0.75);
        assert_eq!(tabs.tabs.iter().find(|tab| tab.id == second).unwrap().zoom_factor, 1.0);
        tabs.restore(moved).unwrap();
        assert_eq!(tabs.tabs.iter().find(|tab| tab.id == first).unwrap().zoom_factor, 0.75);
    }
    #[test]
    fn login_popups_use_the_source_tab_only_on_known_login_pages() {
        for value in [
            "https://accounts.pixiv.net/login",
            "https://accounts.booth.pm/users/sign_in",
            "https://accounts.booth.pm/session/new",
        ] {
            assert!(login_popup_in_same_tab(&Url::parse(value).unwrap()));
        }
        for value in [
            "https://accounts.google.com/o/oauth2/auth",
            "https://booth.pm/ja/items/1234567",
            "https://accounts.booth.pm/age_verification",
            "https://pixiv.net.evil.example/login",
            "http://accounts.pixiv.net/login",
        ] {
            assert!(!login_popup_in_same_tab(&Url::parse(value).unwrap()));
        }
    }
    #[test]
    fn bookmark_limit_rejects_new_entries_but_allows_edits_and_merges() {
        let mut items = (0..MAX_BOOKMARKS)
            .map(|i| {
                Bookmark::new(&format!("Page {i}"), &format!("https://example.com/{i}")).unwrap()
            })
            .collect::<Vec<_>>();
        let before = items.clone();
        assert!(save_bookmark(&mut items, None, "Extra", "https://example.com/extra").is_err());
        assert_eq!(items, before);
        save_bookmark(
            &mut items,
            Some("https://example.com/1"),
            "Renamed",
            "https://example.com/0",
        )
        .unwrap();
        assert_eq!(items.len(), MAX_BOOKMARKS - 1);
        assert_eq!(items[0].title, "Renamed");
    }
    #[test]
    fn accepts_general_web_urls_but_not_credentials_or_application_urls() {
        assert_eq!(
            navigation_url("example.com/path?q=1#part")
                .unwrap()
                .as_str(),
            "https://example.com/path?q=1#part"
        );
        assert!(navigation_url("http://localhost:3000/").is_ok());
        for value in [
            "",
            "javascript:alert(1)",
            "data:text/html,x",
            "file:///tmp/x",
            "tauri://localhost/",
            "https://tauri.localhost/",
            "https://asset.localhost/",
            "https://user:pass@example.com/",
            "https://example.com/\nnext",
        ] {
            assert!(navigation_url(value).is_err(), "{value}");
        }
    }
    fn session() -> TabSession {
        let mut tabs = TabSession::new();
        tabs.ensure_panel();
        tabs
    }
    #[test]
    fn tabs_keep_independent_metadata_and_select_a_neighbor_on_close() {
        let mut tabs = session();
        let a = tabs
            .add(PANEL_HOST_ID, &navigation_url("one.example").unwrap())
            .unwrap();
        let b = tabs
            .add(PANEL_HOST_ID, &navigation_url("two.example").unwrap())
            .unwrap();
        tabs.select(&a).unwrap();
        tabs.update(&b, |tab| {
            tab.title = "Second".into();
            tab.can_go_back = true;
        });
        assert_eq!(tabs.focused_tab().as_deref(), Some(a.as_str()));
        assert!(!tabs.tabs[0].can_go_back);
        assert!(tabs.tabs[1].can_go_back);
        assert!(tabs.remove(&a).unwrap().is_none());
        assert_eq!(tabs.focused_tab().as_deref(), Some(b.as_str()));
        assert_eq!(tabs.remove(&b).unwrap().as_deref(), Some(PANEL_HOST_ID));
        assert!(tabs.focused_tab().is_none());
        assert!(!tabs.update(&b, |_| panic!("closed tab must ignore late events")));
        tabs.ensure_panel();
        assert_ne!(
            tabs.add(PANEL_HOST_ID, &navigation_url("three.example").unwrap())
                .unwrap(),
            a
        );
    }
    #[test]
    fn bookmark_edit_delete_and_duplicate_urls_are_predictable() {
        let mut items = Vec::new();
        save_bookmark(&mut items, None, "First", "example.com").unwrap();
        save_bookmark(&mut items, None, "Updated", "https://example.com/").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Updated");
        save_bookmark(
            &mut items,
            Some("https://example.com/"),
            "Other",
            "https://other.example/",
        )
        .unwrap();
        assert_eq!(items[0].url, "https://other.example/");
        assert!(save_bookmark(&mut items, Some("gone"), "", "example.com").is_err());
        assert!(save_bookmark(&mut items, None, "Bad", "javascript:alert(1)").is_err());
    }
    #[test]
    fn tab_limit_keeps_existing_tabs_intact() {
        let mut tabs = session();
        for _ in 0..MAX_TABS {
            tabs.add(PANEL_HOST_ID, &navigation_url("example.com").unwrap())
                .unwrap();
        }
        let before = tabs.focused_tab();
        assert!(tabs
            .add(PANEL_HOST_ID, &navigation_url("other.example").unwrap())
            .is_err());
        assert_eq!(tabs.focused_tab(), before);
        assert_eq!(tabs.tabs.len(), MAX_TABS);
    }
    #[test]
    fn hosts_keep_their_own_order_and_selection() {
        let mut tabs = session();
        let panel_a = tabs
            .add(PANEL_HOST_ID, &navigation_url("panel.example/a").unwrap())
            .unwrap();
        let panel_b = tabs
            .add(PANEL_HOST_ID, &navigation_url("panel.example/b").unwrap())
            .unwrap();
        let window = tabs.add_window();
        let popped = tabs
            .add(&window, &navigation_url("window.example").unwrap())
            .unwrap();
        tabs.select(&panel_a).unwrap();
        assert_eq!(tabs.active_of(PANEL_HOST_ID).as_deref(), Some(panel_a.as_str()));
        assert_eq!(tabs.active_of(&window).as_deref(), Some(popped.as_str()));
        assert_eq!(tabs.coarse_host(), Some("panel"));
        tabs.reorder(PANEL_HOST_ID, &[panel_b.clone(), panel_a.clone()])
            .unwrap();
        assert_eq!(
            tabs.hosts.iter().find(|host| host.id == PANEL_HOST_ID).unwrap().order,
            vec![panel_b.clone(), panel_a.clone()]
        );
        assert!(tabs.reorder(PANEL_HOST_ID, &[panel_a.clone()]).is_err());
        let token = tabs.move_tab(&panel_b, &window, 0).unwrap();
        assert_eq!(tabs.host_of(&panel_b).as_deref(), Some(window.as_str()));
        assert_eq!(tabs.active_of(PANEL_HOST_ID).as_deref(), Some(panel_a.as_str()));
        assert_eq!(tabs.active_of(&window).as_deref(), Some(panel_b.as_str()));
        tabs.restore(token).unwrap();
        assert_eq!(tabs.host_of(&panel_b).as_deref(), Some(PANEL_HOST_ID));
        assert_eq!(
            tabs.hosts
                .iter()
                .find(|host| host.id == PANEL_HOST_ID)
                .unwrap()
                .order,
            vec![panel_b.clone(), panel_a.clone()]
        );
        assert_eq!(tabs.active_of(PANEL_HOST_ID).as_deref(), Some(panel_a.as_str()));
        assert_eq!(tabs.active_of(&window).as_deref(), Some(popped.as_str()));
        let gone = tabs.take_host(&window);
        assert_eq!(gone, vec![popped.clone()]);
        assert!(tabs.host_of(&panel_a).is_some());
        assert!(tabs.host_of(&popped).is_none());
        assert_eq!(tabs.remove(&panel_a).unwrap(), None);
        assert!(tabs.has(&panel_b));
    }
    #[test]
    fn detach_position_keeps_other_monitors_and_rejects_non_finite_values() {
        assert_eq!(detach_position(-1920.0, 40.0).unwrap(), (-1920.0, 12.0));
        assert_eq!(detach_position(10.0, 10.0).unwrap(), (10.0, -18.0));
        assert!(detach_position(f64::NAN, 0.0).is_err());
        assert!(detach_position(0.0, f64::INFINITY).is_err());
        assert!(!destroy_matches(None, 3));
        assert!(!destroy_matches(Some(2), 3));
        assert!(destroy_matches(Some(3), 3));
        assert!(is_toolbar_label("booth-toolbar"));
        assert!(is_toolbar_label("booth-toolbar-1"));
        assert!(!is_toolbar_label("booth-tab-1"));
        assert!(!is_toolbar_label("main"));
    }
}
