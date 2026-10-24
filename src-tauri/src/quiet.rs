use crate::library_files::{
    choose_download_name, file_looks_like_login_page, looks_like_login_page, safe_filename,
    without_verbatim_prefix,
};
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

/// 画面に返すアカウント。クッキーもトークンも持たない。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoothAccount {
    pub logged_in: bool,
    pub display_name: String,
    pub pixiv_id: String,
    pub avatar_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuietError {
    Login,
    AgeGate,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hop {
    Follow,
    Login,
    AgeGate,
    Reject,
}

/// ヘッダの dataset だけを読む。クッキー文字列は読まない。
pub fn account_script() -> &'static str {
    r##"(function () {
  try {
    var nav = document.querySelector("#js-navigation");
    var sign = document.querySelector("#sign_in");
    var signed = nav ? String(nav.getAttribute("data-signed-in") || "") : "";
    var name = nav ? String(nav.getAttribute("data-user-name") || "") : "";
    var thumb = nav ? String(nav.getAttribute("data-user-thumbnail-url") || "") : "";
    var pixiv = "";
    if (nav) {
      pixiv = nav.getAttribute("data-user-id")
        || nav.getAttribute("data-pixiv-id")
        || nav.getAttribute("data-user-pixiv-id")
        || "";
    }
    return {
      ready: !!(nav || sign),
      loggedIn: signed === "true",
      displayName: name,
      pixivId: String(pixiv || ""),
      avatarUrl: String(thumb || "")
    };
  } catch (error) {
    return { ready: false, loggedIn: false, displayName: "", pixivId: "", avatarUrl: "" };
  }
})()"##
}

pub fn parse_account_eval(raw: &str) -> Result<BoothAccount, String> {
    let mut value: serde_json::Value =
        serde_json::from_str(raw).map_err(|_| "ページの結果を読めません".to_string())?;
    if let serde_json::Value::String(inner) = value {
        value = serde_json::from_str(&inner).map_err(|_| "ページの結果を読めません".to_string())?;
    }
    let logged_in = value
        .get("loggedIn")
        .and_then(|item| item.as_bool())
        .unwrap_or(false);
    let display_name = value
        .get("displayName")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    let pixiv_id = value
        .get("pixivId")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    let avatar_url = value
        .get("avatarUrl")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    Ok(finish_account(
        logged_in,
        display_name,
        pixiv_id,
        avatar_url,
    ))
}

pub fn parse_account_html(html: &str) -> BoothAccount {
    let tag = element_with_id(html, "js-navigation").unwrap_or("");
    let signed = attr(tag, "data-signed-in").unwrap_or("");
    if signed != "true" {
        return finish_account(false, "", "", "");
    }
    let name = attr(tag, "data-user-name").unwrap_or("");
    let avatar = attr(tag, "data-user-thumbnail-url").unwrap_or("");
    let pixiv = attr(tag, "data-user-id")
        .or_else(|| attr(tag, "data-pixiv-id"))
        .or_else(|| attr(tag, "data-user-pixiv-id"))
        .unwrap_or("");
    finish_account(true, name, pixiv, avatar)
}

fn finish_account(
    logged_in: bool,
    display_name: &str,
    pixiv_id: &str,
    avatar_url: &str,
) -> BoothAccount {
    if !logged_in {
        return BoothAccount {
            logged_in: false,
            display_name: String::new(),
            pixiv_id: String::new(),
            avatar_url: String::new(),
        };
    }
    let avatar_url = sanitize_avatar_url(avatar_url);
    let mut pixiv_id = clean_pixiv_id(pixiv_id);
    if pixiv_id.is_empty() {
        pixiv_id = pixiv_id_in_path(
            Url::parse(&avatar_url)
                .map(|url| url.path().to_string())
                .unwrap_or_default()
                .as_str(),
        )
        .unwrap_or_default();
    }
    BoothAccount {
        logged_in: true,
        display_name: clean_display_name(display_name),
        pixiv_id,
        avatar_url,
    }
}

pub fn sanitize_avatar_url(raw: &str) -> String {
    let raw = decode_entities(raw.trim());
    let Ok(mut url) = Url::parse(raw.trim()) else {
        return String::new();
    };
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return String::new();
    }
    let host = url.host_str().unwrap_or("");
    let allowed = host == "booth.pm"
        || host.ends_with(".booth.pm")
        || host == "pximg.net"
        || host.ends_with(".pximg.net");
    if !allowed {
        return String::new();
    }
    let path = url.path().to_ascii_lowercase();
    if path.contains("..") || path.contains("placeholder") {
        return String::new();
    }
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

fn clean_display_name(value: &str) -> String {
    let decoded = decode_entities(value);
    let collapsed = decoded.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for ch in collapsed.chars() {
        if ch.is_control() {
            continue;
        }
        out.push(ch);
        if out.chars().count() >= 80 {
            break;
        }
    }
    out
}

fn clean_pixiv_id(value: &str) -> String {
    let value = decode_entities(value).trim().to_string();
    if value.is_empty() || value.len() > 16 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return String::new();
    }
    value
}

fn pixiv_id_in_path(path: &str) -> Option<String> {
    let mut start = 0;
    while let Some(rel) = path[start..].find("/users/") {
        let index = start + rel + "/users/".len();
        let rest = &path[index..];
        if rest.starts_with("sign_in") || rest.starts_with("sign_out") {
            start = index;
            continue;
        }
        let id: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
        if (1..=16).contains(&id.len()) {
            return Some(id);
        }
        start = index;
    }
    None
}

fn decode_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn element_with_id<'a>(html: &'a str, id: &str) -> Option<&'a str> {
    let double = format!("id=\"{id}\"");
    let single = format!("id='{id}'");
    let index = html.find(&double).or_else(|| html.find(&single))?;
    let start = html[..index].rfind('<')?;
    let end = html[index..].find('>')? + index;
    Some(&html[start..=end])
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let double = format!("{name}=\"");
    if let Some(index) = tag.find(&double) {
        let rest = &tag[index + double.len()..];
        let end = rest.find('"')?;
        return Some(&rest[..end]);
    }
    let single = format!("{name}='");
    let index = tag.find(&single)?;
    let rest = &tag[index + single.len()..];
    let end = rest.find('\'')?;
    Some(&rest[..end])
}

pub fn classify_download_url(url: &Url) -> Hop {
    if url.scheme() != "https" {
        return Hop::Reject;
    }
    let path = url.path().to_ascii_lowercase();
    let query = url.query().unwrap_or("").to_ascii_lowercase();
    if is_age_gate(&path, &query) {
        return Hop::AgeGate;
    }
    if url_is_login(url) {
        return Hop::Login;
    }
    Hop::Follow
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

fn url_is_login(url: &Url) -> bool {
    let host = url.host_str().unwrap_or("");
    let path = url.path().to_ascii_lowercase();
    let pixiv = (host == "pixiv.net" || host.ends_with(".pixiv.net"))
        && (path.contains("login") || path.contains("sign_in") || path.contains("signin"));
    let booth = (host == "booth.pm" || host.ends_with(".booth.pm"))
        && (path.contains("sign_in") || path.contains("signin") || path.contains("/login"));
    pixiv || booth
}

fn booth_host(url: &Url) -> bool {
    let host = url.host_str().unwrap_or("");
    host == "booth.pm" || host.ends_with(".booth.pm")
}

/// The WebView supplied cookies for the original download URL, not for a
/// redirect target. Send them only on the initial request, even if a redirect
/// stays within booth.pm: the target can have different cookie domain/path rules.
fn initial_download_cookie<'a>(
    cookie_header: &'a str,
    initial_url: &Url,
    current_url: &Url,
    redirect_index: usize,
) -> Option<&'a str> {
    if redirect_index == 0
        && current_url == initial_url
        && initial_url.scheme() == "https"
        && booth_host(initial_url)
        && !cookie_header.is_empty()
    {
        Some(cookie_header)
    } else {
        None
    }
}

/// ワーカー側から呼ぶ。UI スレッドや dispatch_ui の中では呼ばない。
pub fn session_header(
    window: &tauri::webview::WebviewWindow<tauri::Wry>,
    url: &Url,
) -> Result<String, String> {
    let cookies = window
        .cookies_for_url(url.clone())
        .map_err(|_| "ログイン状態を確認できません".to_string())?;
    Ok(header_from_cookies(&cookies))
}

fn header_from_cookies(cookies: &[tauri::webview::Cookie<'static>]) -> String {
    let pairs: Vec<(&str, &str)> = cookies
        .iter()
        .map(|cookie| (cookie.name(), cookie.value()))
        .collect();
    header_from_pairs(&pairs)
}

fn header_from_pairs(pairs: &[(&str, &str)]) -> String {
    let mut parts = Vec::new();
    for (name, value) in pairs {
        if !cookie_token(name) || value.is_empty() || !cookie_value(value) {
            continue;
        }
        parts.push(format!("{name}={value}"));
    }
    parts.join("; ")
}

fn cookie_token(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn cookie_value(value: &str) -> bool {
    value.len() <= 4096
        && value
            .bytes()
            .all(|byte| byte.is_ascii() && byte != b';' && byte != b'\r' && byte != b'\n')
}

pub struct StagedFetch {
    pub path: PathBuf,
    pub host: String,
}

/// 署名付きの飛び先へ進み、ステージングへ書く。クエリは残さない。
pub async fn fetch_staged_file(
    cookie_header: &str,
    download_url: &Url,
    dir: &Path,
    row_filename: &str,
) -> Result<StagedFetch, QuietError> {
    if classify_download_url(download_url) != Hop::Follow {
        return Err(match classify_download_url(download_url) {
            Hop::AgeGate => QuietError::AgeGate,
            Hop::Login => QuietError::Login,
            _ => QuietError::Failed,
        });
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20 * 60))
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| QuietError::Failed)?;
    let mut current = download_url.clone();
    let mut response = None;
    for redirect_index in 0..8 {
        let mut request = client
            .get(current.clone())
            .header(reqwest::header::ACCEPT, "*/*")
            .header(reqwest::header::REFERER, "https://booth.pm/")
            .header(reqwest::header::USER_AGENT, download_user_agent());
        if let Some(cookie_header) =
            initial_download_cookie(cookie_header, download_url, &current, redirect_index)
        {
            let mut value = reqwest::header::HeaderValue::from_str(cookie_header)
                .map_err(|_| QuietError::Failed)?;
            value.set_sensitive(true);
            request = request.header(reqwest::header::COOKIE, value);
        }
        let next = request
            .send()
            .await
            .map_err(|err| map_request_error(&err))?;
        let status = next.status();
        if is_redirect(status) {
            let location = next
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(QuietError::Failed)?;
            let joined = current.join(location).map_err(|_| QuietError::Failed)?;
            match classify_download_url(&joined) {
                Hop::Follow => current = joined,
                Hop::AgeGate => return Err(QuietError::AgeGate),
                Hop::Login => return Err(QuietError::Login),
                Hop::Reject => return Err(QuietError::Failed),
            }
            continue;
        }
        response = Some(next);
        break;
    }
    let mut response = response.ok_or(QuietError::Failed)?;
    let final_url = response.url().clone();
    match classify_download_url(&final_url) {
        Hop::Follow => {}
        Hop::AgeGate => return Err(QuietError::AgeGate),
        Hop::Login => return Err(QuietError::Login),
        Hop::Reject => return Err(QuietError::Failed),
    }
    if !response.status().is_success() {
        return Err(QuietError::Failed);
    }
    let host = final_url.host_str().unwrap_or("").to_string();
    let disposition = response
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let suggested = suggested_filename(disposition.as_deref(), &final_url);
    let preferred = choose_download_name(row_filename, &suggested);
    let name = unique_name(dir, &preferred);
    let path = without_verbatim_prefix(&dir.join(name));
    let mut file = std::fs::File::create(&path).map_err(|_| QuietError::Failed)?;
    let mut checked = false;
    loop {
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(_) => {
                drop(file);
                let _ = std::fs::remove_file(&path);
                return Err(QuietError::Failed);
            }
        };
        if !checked {
            checked = true;
            if looks_like_login_page(&chunk) {
                drop(file);
                let _ = std::fs::remove_file(&path);
                return Err(QuietError::Login);
            }
        }
        if file.write_all(&chunk).is_err() {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(QuietError::Failed);
        }
    }
    drop(file);
    if file_looks_like_login_page(&path) {
        let _ = std::fs::remove_file(&path);
        return Err(QuietError::Login);
    }
    if !path.is_file() {
        let _ = std::fs::remove_file(&path);
        return Err(QuietError::Failed);
    }
    Ok(StagedFetch { path, host })
}

fn map_request_error(err: &reqwest::Error) -> QuietError {
    if let Some(url) = err.url() {
        return match classify_download_url(url) {
            Hop::AgeGate => QuietError::AgeGate,
            Hop::Login => QuietError::Login,
            _ => QuietError::Failed,
        };
    }
    let mut source = std::error::Error::source(err);
    while let Some(inner) = source {
        let text = inner.to_string();
        if text.contains("vrclibman-age-gate") {
            return QuietError::AgeGate;
        }
        if text.contains("vrclibman-login") {
            return QuietError::Login;
        }
        source = std::error::Error::source(inner);
    }
    QuietError::Failed
}

fn is_redirect(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
}

fn download_user_agent() -> &'static str {
    if cfg!(target_os = "macos") {
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.2 Safari/605.1.15"
    } else {
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0"
    }
}

fn suggested_filename(disposition: Option<&str>, url: &Url) -> String {
    if let Some(header) = disposition {
        if let Some(name) = filename_from_content_disposition(header) {
            return name;
        }
    }
    url.path().rsplit('/').next().unwrap_or("").to_string()
}

fn filename_from_content_disposition(header: &str) -> Option<String> {
    let lower = header.to_ascii_lowercase();
    if let Some(index) = lower.find("filename*=") {
        let rest = &header[index + "filename*=".len()..];
        let value = rest
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches('"');
        let encoded = value.split("''").nth(1).unwrap_or(value);
        return decode_percent(encoded);
    }
    let index = lower.find("filename=")?;
    let rest = &header[index + "filename=".len()..];
    let value = rest
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('"');
    Some(value.to_string())
}

fn decode_percent(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn unique_name(dir: &Path, preferred: &str) -> String {
    let preferred = safe_filename(preferred).unwrap_or_else(|| "received.bin".to_string());
    if std::fs::symlink_metadata(dir.join(&preferred)).is_err() {
        return preferred;
    }
    let path = Path::new(&preferred);
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let extension = path.extension().and_then(|name| name.to_str());
    for index in 2..50 {
        let name = match extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        if std::fs::symlink_metadata(dir.join(&name)).is_err() {
            return name;
        }
    }
    match extension {
        Some(extension) => format!("{stem}-copy.{extension}"),
        None => format!("{stem}-copy"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGGED_OUT: &str = r#"<header><div id="js-navigation" data-signed-in="false" data-signin-url="/users/sign_in"></div><a id="sign_in" href="/users/sign_in">ログイン</a></header>"#;

    const LOGGED_IN: &str = r#"<header><div id="js-navigation" data-signed-in="true" data-user-name="テスト利用者" data-user-id="424242" data-user-thumbnail-url="https://booth.pximg.net/users/424242/icon.jpg?token=secret"></div></header>"#;

    #[test]
    fn account_html_parser_drops_the_query_and_keeps_no_secrets() {
        let logged_out = parse_account_html(LOGGED_OUT);
        assert!(!logged_out.logged_in);
        assert!(logged_out.display_name.is_empty());
        assert!(logged_out.pixiv_id.is_empty());
        assert!(logged_out.avatar_url.is_empty());

        let logged_in = parse_account_html(LOGGED_IN);
        assert!(logged_in.logged_in);
        assert_eq!(logged_in.display_name, "テスト利用者");
        assert_eq!(logged_in.pixiv_id, "424242");
        assert_eq!(
            logged_in.avatar_url,
            "https://booth.pximg.net/users/424242/icon.jpg"
        );
        assert!(!logged_in.avatar_url.contains('?'));
        let json = serde_json::to_string(&logged_in).unwrap();
        let lower = json.to_ascii_lowercase();
        assert!(!lower.contains("cookie"));
        assert!(!lower.contains("token"));
        assert!(!json.contains("secret"));
        assert!(!json.contains("document.cookie"));

        let id_only = parse_account_html(
            r#"<div id="js-navigation" data-signed-in="true" data-user-thumbnail-url="https://booth.pximg.net/c/48x48/users/515151/icon.png?x=1"></div>"#,
        );
        assert!(id_only.logged_in);
        assert!(id_only.display_name.is_empty());
        assert_eq!(id_only.pixiv_id, "515151");
        assert!(!id_only.avatar_url.contains('?'));

        let foreign = parse_account_html(
            r#"<div id="js-navigation" data-signed-in="true" data-user-name="テスト利用者" data-user-thumbnail-url="https://evil.example/icon.jpg"></div>"#,
        );
        assert!(foreign.avatar_url.is_empty());
        let insecure = sanitize_avatar_url("http://booth.pximg.net/users/424242/icon.jpg");
        assert!(insecure.is_empty());
        let placeholder =
            sanitize_avatar_url("https://booth.pm/static-images/thumbnail-placeholder-square.png");
        assert!(placeholder.is_empty());

        let script = account_script();
        let script_lower = script.to_ascii_lowercase();
        assert!(!script_lower.contains("document.cookie"));
        assert!(!script_lower.contains("localstorage"));
        assert!(!script.contains("fetch("));
        assert!(script.contains("data-signed-in"));
        assert!(script.contains("data-user-name"));
        assert!(script.contains("data-user-thumbnail-url"));
    }

    #[test]
    fn cookies_are_only_attached_to_the_initial_download_request() {
        let original = Url::parse("https://booth.pm/downloadables/2336025").unwrap();
        let same_host_redirect = Url::parse("https://booth.pm/downloadables/redirected").unwrap();
        let subdomain_redirect = Url::parse("https://accounts.booth.pm/library").unwrap();
        let external_redirect = Url::parse("https://cdn.example.test/file.zip?signature=abc").unwrap();
        let lookalike_host = Url::parse("https://evil-booth.pm/file.zip").unwrap();
        let insecure_url = Url::parse("http://booth.pm/downloadables/2336025").unwrap();
        let session_cookie = "session_id=secret";

        assert_eq!(
            initial_download_cookie(session_cookie, &original, &original, 0),
            Some(session_cookie)
        );
        assert_eq!(initial_download_cookie("", &original, &original, 0), None);
        assert_eq!(
            initial_download_cookie(session_cookie, &original, &original, 1),
            None,
            "cookies must not be reused after an initial redirect"
        );
        for redirected in [
            &same_host_redirect,
            &subdomain_redirect,
            &external_redirect,
            &lookalike_host,
        ] {
            assert_eq!(
                initial_download_cookie(session_cookie, &original, redirected, 1),
                None,
                "a redirected request must not receive the initial URL's cookies"
            );
            assert_eq!(
                initial_download_cookie(session_cookie, &original, redirected, 0),
                None,
                "a different URL must not receive the initial URL's cookies"
            );
        }
        assert_eq!(
            initial_download_cookie(session_cookie, &insecure_url, &insecure_url, 0),
            None
        );
        // Check that the actual fetch path consults this guard before attaching Cookie.
        let production = include_str!("quiet.rs").split("mod tests").next().unwrap();
        let fetch = function_body(production, "fn fetch_staged_file");
        assert!(fetch.contains(
            "initial_download_cookie(cookie_header, download_url, &current, redirect_index)"
        ));
        assert!(!fetch.contains("booth_host(&current) && !cookie_header.is_empty()"));
    }

    #[test]
    fn download_hops_reject_age_gates_and_do_not_fetch() {
        let file = Url::parse("https://booth.pm/downloadables/2336025").unwrap();
        assert_eq!(classify_download_url(&file), Hop::Follow);
        let signed = Url::parse("https://cdn.example.test/files/a?token=secret").unwrap();
        assert_eq!(classify_download_url(&signed), Hop::Follow);
        assert_eq!(signed.host_str(), Some("cdn.example.test"));
        let gate = Url::parse("https://booth.pm/age_verification?r18=1").unwrap();
        assert_eq!(classify_download_url(&gate), Hop::AgeGate);
        let login = Url::parse("https://accounts.pixiv.net/login?return_to=secret").unwrap();
        assert_eq!(classify_download_url(&login), Hop::Login);
        assert_eq!(
            classify_download_url(&Url::parse("http://booth.pm/downloadables/2336025").unwrap()),
            Hop::Reject
        );
        let name = filename_from_content_disposition(
            "attachment; filename=\"body.unitypackage?token=secret\"",
        )
        .unwrap();
        assert_eq!(choose_download_name("2336025", &name), "body.unitypackage");
        let star = filename_from_content_disposition(
            "attachment; filename*=UTF-8''sample%20body.unitypackage",
        )
        .unwrap();
        assert_eq!(star, "sample body.unitypackage");
        let header = header_from_pairs(&[("session_id", "alpha"), ("bad\nname", "beta")]);
        assert!(header.contains("session_id=alpha"));
        assert!(!header.contains('\n'));
        assert!(!header.contains("beta"));
        let source = include_str!("quiet.rs");
        let production = source.split("mod tests").next().unwrap();
        assert!(production.contains("cookies_for_url"));
        assert!(production.contains("fn session_header"));
        assert!(!production.contains(".show("));
        assert!(!production.contains("set_focus"));
        assert!(!production.contains("unminimize"));
        assert!(!production.contains("document.cookie"));
        let fetch = function_body(production, "fn fetch_staged_file");
        let send_at = fetch.find(".send()").expect("send");
        let classify_at = fetch.find("classify_download_url").expect("classify");
        assert!(classify_at < send_at);
        assert!(!fetch.contains("println"));
        assert!(!fetch.contains("eprintln"));
    }

    fn function_body<'a>(source: &'a str, name: &str) -> &'a str {
        let start = source
            .find(name)
            .unwrap_or_else(|| panic!("missing {name}"));
        let rest = &source[start..];
        let after = rest.find('\n').unwrap() + 1;
        let tail = &rest[after..];
        let end = tail
            .find("\nfn ")
            .or_else(|| tail.find("\npub fn "))
            .or_else(|| tail.find("\npub async fn "))
            .or_else(|| tail.find("\nasync fn "))
            .or_else(|| tail.find("\n#[cfg"))
            .unwrap_or(tail.len());
        &rest[..after + end]
    }
}
