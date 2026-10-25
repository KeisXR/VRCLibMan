use serde::{Deserialize, Serialize};
use url::Url;

pub const TOOLBAR_LABEL: &str = "booth-toolbar";
pub const DEFAULT_TOOLBAR_HEIGHT: f64 = 136.0;
pub const MAX_TABS: usize = 20;
pub const MAX_BOOKMARKS: usize = 200;

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTab {
    pub id: String,
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserState {
    pub open: bool,
    pub tabs: Vec<BrowserTab>,
    pub active_id: Option<String>,
    pub bookmarks: Vec<Bookmark>,
    pub busy: bool,
}

pub struct TabSession {
    pub tabs: Vec<BrowserTab>,
    pub active_id: Option<String>,
    pub toolbar_height: f64,
    next_id: u64,
}

impl TabSession {
    pub const fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_id: None,
            toolbar_height: DEFAULT_TOOLBAR_HEIGHT,
            next_id: 1,
        }
    }
    pub fn add(&mut self, url: &Url) -> Result<String, String> {
        if self.tabs.len() >= MAX_TABS {
            return Err(format!("タブは{MAX_TABS}個まで開けます"));
        }
        let id = format!("booth-tab-{}", self.next_id);
        self.next_id += 1;
        self.tabs.push(BrowserTab {
            id: id.clone(),
            url: url.to_string(),
            title: url.host_str().unwrap_or("新しいタブ").into(),
            loading: true,
            can_go_back: false,
            can_go_forward: false,
        });
        self.active_id = Some(id.clone());
        Ok(id)
    }
    pub fn select(&mut self, id: &str) -> Result<(), String> {
        if !self.tabs.iter().any(|tab| tab.id == id) {
            return Err("タブが見つかりません".into());
        }
        self.active_id = Some(id.into());
        Ok(())
    }
    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        let index = self
            .tabs
            .iter()
            .position(|tab| tab.id == id)
            .ok_or("タブが見つかりません")?;
        self.tabs.remove(index);
        if self.active_id.as_deref() == Some(id) {
            self.active_id = self
                .tabs
                .get(index.min(self.tabs.len().saturating_sub(1)))
                .map(|tab| tab.id.clone());
        }
        Ok(())
    }
    pub fn clear(&mut self) {
        self.tabs.clear();
        self.active_id = None;
        self.toolbar_height = DEFAULT_TOOLBAR_HEIGHT;
    }
    pub fn update(&mut self, id: &str, work: impl FnOnce(&mut BrowserTab)) -> bool {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            work(tab);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    #[test]
    fn tabs_keep_independent_metadata_and_select_a_neighbor_on_close() {
        let mut tabs = TabSession::new();
        let a = tabs.add(&navigation_url("one.example").unwrap()).unwrap();
        let b = tabs.add(&navigation_url("two.example").unwrap()).unwrap();
        tabs.select(&a).unwrap();
        tabs.update(&b, |tab| {
            tab.title = "Second".into();
            tab.can_go_back = true;
        });
        assert_eq!(tabs.active_id.as_deref(), Some(a.as_str()));
        assert!(!tabs.tabs[0].can_go_back);
        assert!(tabs.tabs[1].can_go_back);
        tabs.remove(&a).unwrap();
        assert_eq!(tabs.active_id.as_deref(), Some(b.as_str()));
        tabs.remove(&b).unwrap();
        assert!(tabs.active_id.is_none());
        assert!(!tabs.update(&b, |_| panic!("closed tab must ignore late events")));
        tabs.clear();
        assert_ne!(
            tabs.add(&navigation_url("three.example").unwrap()).unwrap(),
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
        let mut tabs = TabSession::new();
        for _ in 0..MAX_TABS {
            tabs.add(&navigation_url("example.com").unwrap()).unwrap();
        }
        let before = tabs.active_id.clone();
        assert!(tabs.add(&navigation_url("other.example").unwrap()).is_err());
        assert_eq!(tabs.active_id, before);
        assert_eq!(tabs.tabs.len(), MAX_TABS);
    }
}
