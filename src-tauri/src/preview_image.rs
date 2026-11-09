//! 選択した商品の大きい画像を1件取得する。
//! Cookie は送らない。失敗したときは 150px のサムネイルのままにする。

use crate::thumb::{self, MAX_BYTES};
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Debug, PartialEq, Eq)]
pub enum PreviewSave {
    Saved(PathBuf),
    Fallback,
}

pub fn valid_preview_id(booth_id: &str) -> bool {
    (4..=12).contains(&booth_id.len()) && booth_id.bytes().all(|byte| byte.is_ascii_digit())
}

/// 年齢制限の商品は、表示するまで大きい画像を返さない。
pub fn may_show_large(is_adult: bool, reveal: bool) -> bool {
    !is_adult || reveal
}

pub fn original_image_url(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw.trim()).map_err(|_| "プレビューURLが不正です".to_string())?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("プレビューURLが不正です".into());
    }
    thumb::ensure_allowed_host(&url)?;
    if !thumb::is_pximg(&url) {
        return Ok(url);
    }
    let path = url.path();
    let stripped = if let Some(rest) = path.strip_prefix("/c/") {
        match rest.find('/') {
            Some(index) => rest[index..].to_string(),
            None => path.to_string(),
        }
    } else {
        path.to_string()
    };
    let mut updated = url;
    updated.set_path(&stripped);
    Ok(updated)
}

pub fn cached_preview(directory: &Path, booth_id: &str, source_url: &str) -> Option<PathBuf> {
    if !valid_preview_id(booth_id) {
        return None;
    }
    let recorded = std::fs::read_to_string(directory.join(format!("{booth_id}.source"))).ok()?;
    if recorded.trim() != source_url {
        return None;
    }
    for extension in ["jpg", "jpeg", "png", "webp", "gif"] {
        let path = directory.join(format!("{booth_id}.{extension}"));
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

pub fn store_preview_body(
    directory: &Path,
    booth_id: &str,
    source_url: &str,
    status: u16,
    content_type: Option<&str>,
    body: &[u8],
) -> Result<PreviewSave, String> {
    if !valid_preview_id(booth_id) {
        return Err("商品IDが不正です".into());
    }
    let Some(extension) = accepted_image(status, content_type, body) else {
        return Ok(PreviewSave::Fallback);
    };
    std::fs::create_dir_all(directory)
        .map_err(|err| format!("プレビューを保存できません: {err}"))?;
    for old in ["jpg", "jpeg", "png", "webp", "gif"] {
        if old != extension {
            let _ = std::fs::remove_file(directory.join(format!("{booth_id}.{old}")));
        }
    }
    let destination = directory.join(format!("{booth_id}.{extension}"));
    std::fs::write(&destination, body)
        .map_err(|err| format!("プレビューを保存できません: {err}"))?;
    std::fs::write(directory.join(format!("{booth_id}.source")), source_url)
        .map_err(|err| format!("プレビューを保存できません: {err}"))?;
    Ok(PreviewSave::Saved(destination))
}

pub async fn download_preview(
    client: &reqwest::Client,
    url: &Url,
    directory: &Path,
    booth_id: &str,
) -> Result<PathBuf, String> {
    thumb::ensure_allowed_host(url)?;
    let response = client
        .get(url.clone())
        .header(reqwest::header::USER_AGENT, thumb::USER_AGENT)
        .header(reqwest::header::REFERER, "https://booth.pm/")
        .send()
        .await
        .map_err(|_| "プレビューを取得できません".to_string())?;
    let status = response.status();
    let final_url = response.url().clone();
    thumb::ensure_allowed_host(&final_url)?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let bytes = response
        .bytes()
        .await
        .map_err(|_| "プレビューを読めません".to_string())?;
    match store_preview_body(
        directory,
        booth_id,
        url.as_str(),
        status.as_u16(),
        content_type.as_deref(),
        &bytes,
    )? {
        PreviewSave::Saved(path) => Ok(path),
        PreviewSave::Fallback => Err("プレビューを保存できません".into()),
    }
}

fn accepted_image(status: u16, content_type: Option<&str>, body: &[u8]) -> Option<&'static str> {
    if !(200..300).contains(&status) || body.is_empty() || body.len() > MAX_BYTES {
        return None;
    }
    let content_type = content_type.unwrap_or("").to_ascii_lowercase();
    if content_type.contains("html") || content_type.contains("xml") {
        return None;
    }
    let trimmed = body
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map(|index| &body[index..])
        .unwrap_or(body);
    if trimmed.starts_with(b"<") {
        return None;
    }
    sniff_image(body)
}

fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some("png");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_pximg_drops_the_resize_prefix_and_rejects_other_hosts() {
        let resized =
            original_image_url("https://booth.pximg.net/c/150x150/aa11/i/8800101/sample.jpg?ts=2")
                .unwrap();
        assert_eq!(resized.path(), "/aa11/i/8800101/sample.jpg");
        assert_eq!(resized.query(), Some("ts=2"));
        let variant =
            original_image_url("https://booth.pximg.net/c/72x72_a2_g5/aa11/i/8800101/sample.png")
                .unwrap();
        assert_eq!(variant.path(), "/aa11/i/8800101/sample.png");
        let plain =
            original_image_url("https://booth.pximg.net/aa11/i/8800101/sample.jpg").unwrap();
        assert_eq!(plain.path(), "/aa11/i/8800101/sample.jpg");
        let hosted = original_image_url("https://booth.pm/images/sample.png").unwrap();
        assert_eq!(hosted.path(), "/images/sample.png");
        assert!(original_image_url("https://example.com/sample.jpg").is_err());
        assert!(original_image_url("http://booth.pximg.net/aa11/sample.jpg").is_err());
        assert!(original_image_url("https://user:secret@booth.pximg.net/aa11/sample.jpg").is_err());
    }

    #[test]
    fn preview_cache_saves_an_image_and_other_bodies_fall_back() {
        let directory = tempfile::tempdir().unwrap();
        let source = "https://booth.pximg.net/aa11/i/8800101/sample.jpg";
        let jpeg = [0xFF, 0xD8, 0xFF, 0x00, 0x11];
        let saved = store_preview_body(
            directory.path(),
            "8800101",
            source,
            200,
            Some("image/jpeg"),
            &jpeg,
        )
        .unwrap();
        let PreviewSave::Saved(path) = saved else {
            panic!("image should be stored");
        };
        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("jpg"));
        assert_eq!(std::fs::read(&path).unwrap(), jpeg);
        assert_eq!(
            cached_preview(directory.path(), "8800101", source).as_ref(),
            Some(&path)
        );
        assert!(
            cached_preview(
                directory.path(),
                "8800101",
                "https://booth.pximg.net/aa11/i/8800101/other.jpg"
            )
            .is_none()
        );

        let html = store_preview_body(
            directory.path(),
            "8800102",
            source,
            200,
            Some("text/html"),
            b"<html><title>login</title></html>",
        )
        .unwrap();
        assert_eq!(html, PreviewSave::Fallback);
        assert!(cached_preview(directory.path(), "8800102", source).is_none());
        assert!(!directory.path().join("8800102.jpg").exists());

        let markup = store_preview_body(
            directory.path(),
            "8800103",
            source,
            200,
            Some("image/jpeg"),
            b"  <!DOCTYPE html><html></html>",
        )
        .unwrap();
        assert_eq!(markup, PreviewSave::Fallback);

        let missing = store_preview_body(
            directory.path(),
            "8800104",
            source,
            403,
            Some("image/jpeg"),
            &jpeg,
        )
        .unwrap();
        assert_eq!(missing, PreviewSave::Fallback);
        assert!(!directory.path().join("8800104.jpg").exists());

        let mut huge = vec![0xFF, 0xD8, 0xFF];
        huge.resize(MAX_BYTES + 1, 0);
        let oversized = store_preview_body(
            directory.path(),
            "8800105",
            source,
            200,
            Some("image/jpeg"),
            &huge,
        )
        .unwrap();
        assert_eq!(oversized, PreviewSave::Fallback);

        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A];
        let replaced = store_preview_body(
            directory.path(),
            "8800101",
            source,
            200,
            Some("image/png"),
            &png,
        )
        .unwrap();
        let PreviewSave::Saved(png_path) = replaced else {
            panic!("png should replace jpg");
        };
        assert_eq!(
            png_path.extension().and_then(|ext| ext.to_str()),
            Some("png")
        );
        assert!(!directory.path().join("8800101.jpg").exists());

        assert!(
            store_preview_body(
                directory.path(),
                "../8800",
                source,
                200,
                Some("image/jpeg"),
                &jpeg,
            )
            .unwrap_err()
            .contains("商品ID")
        );
        assert!(!directory.path().join("../8800.jpg").exists());
    }

    #[test]
    fn adult_preview_stays_hidden_until_reveal() {
        assert!(!may_show_large(true, false));
        assert!(may_show_large(true, true));
        assert!(may_show_large(false, false));
        assert!(may_show_large(false, true));
    }
}
