use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

const USER_AGENT: &str = "VRCLibMan/0.1 (local asset organizer)";
const MAX_BYTES: usize = 4_000_000;

pub fn download_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|err| format!("HTTPクライアントを初期化できません: {err}"))
}

pub async fn download_thumbnail(
    client: &reqwest::Client,
    raw_url: &str,
    directory: &Path,
    booth_id: &str,
) -> Result<PathBuf, String> {
    if !booth_id.chars().all(|ch| ch.is_ascii_digit()) {
        return Err("商品IDが不正です".into());
    }
    let url = card_image_url(raw_url)?;
    let response = client
        .get(url.clone())
        .header("User-Agent", USER_AGENT)
        .header("Referer", "https://booth.pm/")
        .send()
        .await
        .map_err(|err| format!("サムネイルを取得できません: {err}"))?;
    let status = response.status();
    let final_url = response.url().clone();
    ensure_allowed_host(&final_url)?;
    if !status.is_success() {
        return Err(format!("サムネイルの取得に失敗しました ({status})"));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let bytes = response
        .bytes()
        .await
        .map_err(|err| format!("サムネイルを読めません: {err}"))?;
    if bytes.len() > MAX_BYTES {
        return Err("サムネイルが大きすぎます".into());
    }
    if bytes.is_empty() {
        return Err("サムネイルが空です".into());
    }
    let extension = extension_for(content_type.as_deref(), &final_url);
    std::fs::create_dir_all(directory)
        .map_err(|err| format!("サムネイルフォルダを作れません: {err}"))?;
    for old in ["jpg", "jpeg", "png", "webp", "gif"] {
        let stale = directory.join(format!("{booth_id}.{old}"));
        if stale.extension().and_then(|ext| ext.to_str()) != Some(extension) {
            let _ = std::fs::remove_file(stale);
        }
    }
    let destination = directory.join(format!("{booth_id}.{extension}"));
    std::fs::write(&destination, &bytes)
        .map_err(|err| format!("サムネイルを保存できません: {err}"))?;
    Ok(destination)
}

fn card_image_url(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw.trim()).map_err(|_| "サムネイルURLが不正です".to_string())?;
    ensure_allowed_host(&url)?;
    if is_pximg(&url) {
        Ok(resize_pximg(&url))
    } else {
        Ok(url)
    }
}

fn ensure_allowed_host(url: &Url) -> Result<(), String> {
    if url.scheme() != "https" {
        return Err("サムネイルは https だけを保存します".into());
    }
    let host = url.host_str().unwrap_or("");
    let allowed = host == "booth.pximg.net"
        || host.ends_with(".pximg.net")
        || host == "booth.pm"
        || host.ends_with(".booth.pm");
    if allowed {
        Ok(())
    } else {
        Err("サムネイルの配布元がBoothではありません".into())
    }
}

fn is_pximg(url: &Url) -> bool {
    let host = url.host_str().unwrap_or("");
    host == "booth.pximg.net" || host.ends_with(".pximg.net")
}

fn resize_pximg(url: &Url) -> Url {
    let path = url.path();
    // 360x360 と素の 300x300 は pximg が 403 を返す。150x150 は jpeg も png も返る。
    let resized = if let Some(rest) = path.strip_prefix("/c/") {
        match rest.find('/') {
            Some(index) => format!("/c/150x150{}", &rest[index..]),
            None => path.to_string(),
        }
    } else {
        format!("/c/150x150{path}")
    };
    let mut updated = url.clone();
    updated.set_path(&resized);
    updated
}

fn extension_for(content_type: Option<&str>, url: &Url) -> &'static str {
    let content_type = content_type.unwrap_or("").to_ascii_lowercase();
    if content_type.contains("png") {
        return "png";
    }
    if content_type.contains("webp") {
        return "webp";
    }
    if content_type.contains("gif") {
        return "gif";
    }
    let path = url.path().to_ascii_lowercase();
    if path.ends_with(".png") {
        return "png";
    }
    if path.ends_with(".webp") {
        return "webp";
    }
    if path.ends_with(".gif") {
        return "gif";
    }
    "jpg"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_pximg_to_card_size_and_rejects_other_hosts() {
        let rewritten =
            card_image_url("https://booth.pximg.net/61a3b2d7/i/4876459/image_base_resized.jpg")
                .unwrap();
        assert_eq!(
            rewritten.path(),
            "/c/150x150/61a3b2d7/i/4876459/image_base_resized.jpg"
        );
        let already =
            card_image_url("https://booth.pximg.net/c/72x72_a2_g5/61a3b2d7/i/4876459/image.jpg")
                .unwrap();
        assert_eq!(already.path(), "/c/150x150/61a3b2d7/i/4876459/image.jpg");
        assert!(card_image_url("https://example.com/image.jpg").is_err());
        assert!(card_image_url("http://booth.pximg.net/a.jpg").is_err());
    }
}
