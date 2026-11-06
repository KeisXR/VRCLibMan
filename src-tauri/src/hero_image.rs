//! Per-avatar hero images, kept as copies in `app_data_dir/hero_images`.
//! The files travel in the data migration package under these same safe names. Their paths do not.
//! Paths and file names stay out of error text and logs: age-restricted products are in play.

use crate::db;
use crate::view::AvatarDetail;
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use rusqlite::Connection;
use std::collections::HashSet;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

pub(crate) const MAX_SOURCE_BYTES: u64 = 20 * 1024 * 1024;
pub(crate) const MAX_WIDTH: u32 = 2400;
/// Checked from the header. A small compressed file can inflate to several gigabytes.
const MAX_PIXELS: u64 = 64_000_000;
const JPEG_QUALITY: u8 = 90;
const SUPPORTED: [&str; 4] = ["png", "jpg", "jpeg", "webp"];
const WRONG_TYPE: &str = "PNG / JPEG / WebP の画像を選んでください";
const UNREADABLE: &str = "画像を読み込めませんでした。ファイルが壊れている可能性があります";
const SAVE_FAILED: &str = "画像を保存できませんでした";

pub(crate) struct Prepared {
    bytes: Vec<u8>,
    extension: &'static str,
}

/// Validates by decoding, then shrinks if needed. Takes no database lock, so slow sources and
/// large images do not hold up other commands.
pub(crate) fn prepare(source: &Path) -> Result<Prepared, String> {
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !SUPPORTED.contains(&extension.as_str()) {
        return Err(WRONG_TYPE.into());
    }
    let bytes = read_limited(source)?;
    let reader = ImageReader::new(Cursor::new(bytes.as_slice()))
        .with_guessed_format()
        .map_err(|_| UNREADABLE.to_string())?;
    // The extension is only a first filter. A renamed file is stored as what it really is.
    let (format, stored_extension) = match reader.format() {
        Some(ImageFormat::Png) => (ImageFormat::Png, "png"),
        Some(ImageFormat::Jpeg) => (ImageFormat::Jpeg, "jpg"),
        Some(ImageFormat::WebP) => (ImageFormat::WebP, "webp"),
        _ => return Err(WRONG_TYPE.into()),
    };
    let mut decoder = reader.into_decoder().map_err(|_| UNREADABLE.to_string())?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("画像の画素数が大きすぎます。小さくしてから選んでください".into());
    }
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).map_err(|_| UNREADABLE.to_string())?;
    image.apply_orientation(orientation);
    if image.width() <= MAX_WIDTH {
        return Ok(Prepared {
            bytes,
            extension: stored_extension,
        });
    }
    Ok(Prepared {
        bytes: shrink(&image, format)?,
        extension: stored_extension,
    })
}

fn read_limited(source: &Path) -> Result<Vec<u8>, String> {
    let too_large = || "画像が大きすぎます。20MB 以下の画像を選んでください".to_string();
    let unreadable = || "画像ファイルを開けませんでした".to_string();
    let file = std::fs::File::open(source).map_err(|_| unreadable())?;
    let metadata = file.metadata().map_err(|_| unreadable())?;
    if !metadata.is_file() {
        return Err(unreadable());
    }
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err(too_large());
    }
    // The file can grow after the size check, so the read has the same limit.
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unreadable())?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(too_large());
    }
    Ok(bytes)
}

fn shrink(image: &DynamicImage, format: ImageFormat) -> Result<Vec<u8>, String> {
    let height = (f64::from(image.height()) * f64::from(MAX_WIDTH) / f64::from(image.width()))
        .round()
        .max(1.0) as u32;
    let resized = image.resize_exact(MAX_WIDTH, height, FilterType::Lanczos3);
    let failed = |_| "画像を縮小できませんでした".to_string();
    let mut out = Cursor::new(Vec::new());
    match format {
        ImageFormat::Jpeg => {
            // JPEG has no alpha channel, and the original was opaque anyway.
            DynamicImage::ImageRgb8(resized.to_rgb8())
                .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
                    &mut out,
                    JPEG_QUALITY,
                ))
                .map_err(failed)?;
        }
        // The WebP encoder is lossless and takes 8-bit data only.
        ImageFormat::WebP => DynamicImage::ImageRgba8(resized.to_rgba8())
            .write_to(&mut out, ImageFormat::WebP)
            .map_err(failed)?,
        _ => resized
            .write_to(&mut out, ImageFormat::Png)
            .map_err(failed)?,
    }
    Ok(out.into_inner())
}

/// Avatar ids are `[a-z0-9_-]` today, but rows may predate that check. Anything that needed
/// changing gets a hash, so two ids never share a file. Windows reserves names like `con`.
pub(crate) fn file_stem(avatar_id: &str) -> String {
    let safe: String = avatar_id
        .chars()
        .map(|ch| match ch {
            'a'..='z' | '0'..='9' | '_' | '-' => ch,
            _ => '_',
        })
        .take(64)
        .collect();
    let reserved = matches!(safe.as_str(), "con" | "prn" | "aux" | "nul")
        || ((safe.starts_with("com") || safe.starts_with("lpt"))
            && safe.len() == 4
            && safe.ends_with(|ch: char| ch.is_ascii_digit()));
    if safe == avatar_id && !reserved {
        safe
    } else {
        format!("{safe}-{:08x}", db::fnv(avatar_id))
    }
}

fn remove_variants(directory: &Path, stem: &str, keep: Option<&Path>) {
    for extension in SUPPORTED {
        let stale = directory.join(format!("{stem}.{extension}"));
        if keep != Some(stale.as_path()) {
            let _ = std::fs::remove_file(stale);
        }
    }
}

/// Call with the database lock held, so this cannot race `prune` or another change.
pub(crate) fn store(
    connection: &mut Connection,
    directory: &Path,
    avatar_id: &str,
    prepared: &Prepared,
) -> Result<PathBuf, String> {
    let failed = |_: std::io::Error| SAVE_FAILED.to_string();
    std::fs::create_dir_all(directory).map_err(failed)?;
    let stem = file_stem(avatar_id);
    let destination = directory.join(format!("{stem}.{}", prepared.extension));
    let path = destination
        .to_str()
        .ok_or_else(|| SAVE_FAILED.to_string())?;
    // Written beside the target first: a failed save keeps the current image.
    let mut temporary = tempfile::NamedTempFile::new_in(directory).map_err(failed)?;
    temporary.write_all(&prepared.bytes).map_err(failed)?;
    // An unknown avatar leaves no file, and a failed rename leaves the row on the old image.
    let transaction = connection
        .transaction()
        .map_err(|_| SAVE_FAILED.to_string())?;
    db::set_avatar_hero_path(&transaction, avatar_id, Some(path))?;
    temporary
        .persist(&destination)
        .map_err(|_| SAVE_FAILED.to_string())?;
    transaction.commit().map_err(|_| SAVE_FAILED.to_string())?;
    remove_variants(directory, &stem, Some(&destination));
    Ok(destination)
}

/// Call with the database lock held. Removes only names built from the id, never a stored path.
pub(crate) fn clear(
    connection: &Connection,
    directory: &Path,
    avatar_id: &str,
) -> Result<(), String> {
    db::set_avatar_hero_path(connection, avatar_id, None)?;
    remove_variants(directory, &file_stem(avatar_id), None);
    Ok(())
}

/// Removes files no row refers to: deleted and merged avatars, unlinked `owned-*` rows that
/// `reresolve_all` dropped, and temporary files left by a crash. Call with the database lock held.
pub(crate) fn prune(connection: &Connection, directory: &Path) -> Result<(), String> {
    let keep: HashSet<std::ffi::OsString> = db::avatar_hero_paths(connection)?
        .iter()
        .filter_map(|path| Path::new(path).file_name().map(ToOwned::to_owned))
        .collect();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let is_file = entry.file_type().is_ok_and(|kind| kind.is_file());
        if is_file && !keep.contains(&entry.file_name()) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(())
}

/// A stored path whose file is gone falls back to the thumbnail instead of a blank hero.
/// The file keeps its name when replaced by the same format, so the modified time is what tells
/// the WebView that the image changed.
pub(crate) fn fill_display(detail: &mut AvatarDetail) {
    let Some(path) = detail.hero_image_path.as_deref() else {
        return;
    };
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {
            detail.hero_image_version = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|age| age.as_millis() as u64);
        }
        _ => detail.hero_image_path = None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GenericImageView, Rgb, RgbImage};

    fn avatar(connection: &Connection, id: &str) {
        connection
            .execute(
                "INSERT INTO avatars (id, display_name) VALUES (?1, ?1)",
                [id],
            )
            .unwrap();
    }

    fn hero_path(connection: &Connection, id: &str) -> Option<String> {
        connection
            .query_row(
                "SELECT hero_image_path FROM avatars WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn write_png(path: &Path, width: u32, height: u32) {
        RgbImage::from_pixel(width, height, Rgb([200, 40, 40]))
            .save(path)
            .unwrap();
    }

    fn names(directory: &Path) -> Vec<String> {
        let mut found: Vec<String> = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        found.sort();
        found
    }

    #[test]
    fn stores_a_copy_and_records_the_path() {
        let source = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let heroes = data.path().join("hero_images");
        let picked = source.path().join("picked.png");
        write_png(&picked, 40, 20);
        let mut connection = db::open_memory().unwrap();
        avatar(&connection, "my-avatar");

        let prepared = prepare(&picked).unwrap();
        let stored = store(&mut connection, &heroes, "my-avatar", &prepared).unwrap();

        assert_eq!(stored, heroes.join("my-avatar.png"));
        assert_eq!(
            hero_path(&connection, "my-avatar").as_deref(),
            stored.to_str()
        );
        // The copy outlives the original.
        std::fs::remove_file(&picked).unwrap();
        assert!(stored.is_file());
        assert_eq!(names(&heroes), ["my-avatar.png"]);
    }

    #[test]
    fn clearing_removes_the_file_and_the_path() {
        let source = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let heroes = data.path().join("hero_images");
        let picked = source.path().join("picked.png");
        write_png(&picked, 40, 20);
        let mut connection = db::open_memory().unwrap();
        avatar(&connection, "my-avatar");
        store(
            &mut connection,
            &heroes,
            "my-avatar",
            &prepare(&picked).unwrap(),
        )
        .unwrap();

        clear(&connection, &heroes, "my-avatar").unwrap();

        assert_eq!(hero_path(&connection, "my-avatar"), None);
        assert!(names(&heroes).is_empty());
        // Clearing when nothing is set is not an error.
        clear(&connection, &heroes, "my-avatar").unwrap();
    }

    #[test]
    fn replacing_with_another_format_removes_the_old_file() {
        let source = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let heroes = data.path().join("hero_images");
        let png = source.path().join("first.png");
        let jpg = source.path().join("second.JPEG");
        write_png(&png, 40, 20);
        RgbImage::from_pixel(40, 20, Rgb([0, 0, 200]))
            .save_with_format(&jpg, ImageFormat::Jpeg)
            .unwrap();
        let mut connection = db::open_memory().unwrap();
        avatar(&connection, "my-avatar");

        store(
            &mut connection,
            &heroes,
            "my-avatar",
            &prepare(&png).unwrap(),
        )
        .unwrap();
        let stored = store(
            &mut connection,
            &heroes,
            "my-avatar",
            &prepare(&jpg).unwrap(),
        )
        .unwrap();

        assert_eq!(stored, heroes.join("my-avatar.jpg"));
        assert_eq!(names(&heroes), ["my-avatar.jpg"]);
    }

    #[test]
    fn unknown_avatar_keeps_nothing() {
        let source = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let heroes = data.path().join("hero_images");
        let picked = source.path().join("picked.png");
        write_png(&picked, 40, 20);
        let mut connection = db::open_memory().unwrap();

        let error = store(
            &mut connection,
            &heroes,
            "nobody",
            &prepare(&picked).unwrap(),
        )
        .unwrap_err();

        assert_eq!(error, "アバターが見つかりません");
        assert!(names(&heroes).is_empty());
    }

    #[test]
    fn rejects_unsupported_extensions_and_contents() {
        let source = tempfile::tempdir().unwrap();
        let gif = source.path().join("anim.gif");
        std::fs::write(&gif, b"GIF89a").unwrap();
        assert_eq!(prepare(&gif).err().as_deref(), Some(WRONG_TYPE));
        let none = source.path().join("noext");
        std::fs::write(&none, b"x").unwrap();
        assert_eq!(prepare(&none).err().as_deref(), Some(WRONG_TYPE));
        // Accepted extension, other contents.
        let html = source.path().join("page.png");
        std::fs::write(&html, b"<!doctype html><html></html>").unwrap();
        assert_eq!(prepare(&html).err().as_deref(), Some(WRONG_TYPE));
    }

    #[test]
    fn rejects_truncated_images() {
        let source = tempfile::tempdir().unwrap();
        let path = source.path().join("cut.png");
        write_png(&path, 64, 64);
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
        assert_eq!(prepare(&path).err().as_deref(), Some(UNREADABLE));
    }

    #[test]
    fn stores_the_real_format_of_a_renamed_file() {
        let source = tempfile::tempdir().unwrap();
        let path = source.path().join("really-a-jpeg.png");
        RgbImage::from_pixel(40, 20, Rgb([0, 200, 0]))
            .save_with_format(&path, ImageFormat::Jpeg)
            .unwrap();
        assert_eq!(prepare(&path).unwrap().extension, "jpg");
    }

    #[test]
    fn rejects_files_over_the_size_limit_before_reading_them() {
        let source = tempfile::tempdir().unwrap();
        let path = source.path().join("huge.png");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_SOURCE_BYTES + 1).unwrap();
        let error = prepare(&path).err().unwrap();
        assert!(error.contains("20MB"), "{error}");
    }

    #[test]
    fn rejects_too_many_pixels_from_the_header() {
        let source = tempfile::tempdir().unwrap();
        let path = source.path().join("bomb.png");
        // A real PNG of this size takes gigabytes to decode. Only the header is valid here,
        // so reaching the pixel check proves it runs before any allocation.
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        let header = [
            &20_000u32.to_be_bytes()[..],
            &20_000u32.to_be_bytes()[..],
            &[8, 2, 0, 0, 0],
        ]
        .concat();
        bytes.extend_from_slice(&header);
        let mut crc_input = b"IHDR".to_vec();
        crc_input.extend_from_slice(&header);
        bytes.extend_from_slice(&crc32(&crc_input).to_be_bytes());
        // An empty IDAT, because the decoder reads up to the first one before reporting the size.
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(b"IDAT");
        bytes.extend_from_slice(&crc32(b"IDAT").to_be_bytes());
        std::fs::write(&path, bytes).unwrap();
        let error = prepare(&path).err().unwrap();
        assert!(error.contains("画素数"), "{error}");
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for byte in data {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    #[test]
    fn keeps_images_within_the_limit_byte_for_byte() {
        let source = tempfile::tempdir().unwrap();
        let path = source.path().join("fits.png");
        write_png(&path, MAX_WIDTH, 10);
        let original = std::fs::read(&path).unwrap();
        assert_eq!(prepare(&path).unwrap().bytes, original);
    }

    #[test]
    fn shrinks_wide_images_for_every_format() {
        let source = tempfile::tempdir().unwrap();
        for (name, format, extension) in [
            ("wide.png", ImageFormat::Png, "png"),
            ("wide.jpg", ImageFormat::Jpeg, "jpg"),
            ("wide.webp", ImageFormat::WebP, "webp"),
        ] {
            let path = source.path().join(name);
            RgbImage::from_pixel(4800, 200, Rgb([10, 120, 200]))
                .save_with_format(&path, format)
                .unwrap();
            let prepared = prepare(&path).unwrap();
            assert_eq!(prepared.extension, extension);
            let decoded = image::load_from_memory(&prepared.bytes).unwrap();
            assert_eq!(decoded.dimensions(), (MAX_WIDTH, 100), "{name}");
        }
    }

    #[test]
    fn applies_exif_orientation_before_measuring_and_shrinking() {
        let source = tempfile::tempdir().unwrap();
        let path = source.path().join("camera.jpg");
        // Stored sideways. Orientation 6 turns it into 4800×100, which has to be shrunk.
        RgbImage::from_pixel(100, 4800, Rgb([120, 60, 30]))
            .save_with_format(&path, ImageFormat::Jpeg)
            .unwrap();
        let jpeg = std::fs::read(&path).unwrap();
        let mut exif = b"Exif\0\0MM\0*\0\0\0\x08\0\x01".to_vec();
        exif.extend_from_slice(b"\x01\x12\0\x03\0\0\0\x01\0\x06\0\0\0\0\0\0");
        let mut tagged = jpeg[..2].to_vec();
        tagged.extend_from_slice(&[0xFF, 0xE1]);
        tagged.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        tagged.extend_from_slice(&exif);
        tagged.extend_from_slice(&jpeg[2..]);
        std::fs::write(&path, tagged).unwrap();

        let decoded = image::load_from_memory(&prepare(&path).unwrap().bytes).unwrap();

        assert_eq!(decoded.dimensions(), (MAX_WIDTH, 50));
    }

    #[test]
    fn does_not_shrink_by_height() {
        let source = tempfile::tempdir().unwrap();
        let path = source.path().join("tall.png");
        write_png(&path, 2000, 3000);
        let decoded = image::load_from_memory(&prepare(&path).unwrap().bytes).unwrap();
        assert_eq!(decoded.dimensions(), (2000, 3000));
    }

    #[test]
    fn file_names_are_safe_and_distinct() {
        assert_eq!(file_stem("mf-8962830"), "mf-8962830");
        assert_eq!(file_stem("owned-1234"), "owned-1234");
        for reserved in ["con", "nul", "com1", "lpt9"] {
            assert!(file_stem(reserved).starts_with(&format!("{reserved}-")));
        }
        assert_eq!(file_stem("comma"), "comma");
        let dotted = file_stem("../evil");
        assert!(!dotted.contains('.') && !dotted.contains('/'), "{dotted}");
        assert_ne!(file_stem("a/b"), file_stem("a\\b"));
        assert_ne!(file_stem("Lime"), file_stem("lime"));
    }

    #[test]
    fn prune_keeps_referenced_files_only() {
        let data = tempfile::tempdir().unwrap();
        let heroes = data.path().join("hero_images");
        std::fs::create_dir_all(&heroes).unwrap();
        let source = tempfile::tempdir().unwrap();
        let picked = source.path().join("picked.png");
        write_png(&picked, 40, 20);
        let mut connection = db::open_memory().unwrap();
        avatar(&connection, "kept");
        store(&mut connection, &heroes, "kept", &prepare(&picked).unwrap()).unwrap();
        std::fs::write(heroes.join("gone.png"), b"x").unwrap();
        std::fs::write(heroes.join(".tmpABC"), b"x").unwrap();

        prune(&connection, &heroes).unwrap();

        assert_eq!(names(&heroes), ["kept.png"]);
    }

    #[test]
    fn prune_without_the_folder_is_not_an_error() {
        let data = tempfile::tempdir().unwrap();
        let connection = db::open_memory().unwrap();
        prune(&connection, &data.path().join("missing")).unwrap();
    }

    #[test]
    fn deleting_an_avatar_leaves_its_file_for_prune() {
        let source = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let heroes = data.path().join("hero_images");
        let picked = source.path().join("picked.png");
        write_png(&picked, 40, 20);
        let mut connection = db::open_memory().unwrap();
        avatar(&connection, "my-avatar");
        store(
            &mut connection,
            &heroes,
            "my-avatar",
            &prepare(&picked).unwrap(),
        )
        .unwrap();

        db::delete_avatar(&mut connection, "my-avatar").unwrap();
        prune(&connection, &heroes).unwrap();

        assert!(names(&heroes).is_empty());
    }

    #[test]
    fn saving_an_avatar_keeps_the_hero_image() {
        let source = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let heroes = data.path().join("hero_images");
        let picked = source.path().join("picked.png");
        write_png(&picked, 40, 20);
        let mut connection = db::open_memory().unwrap();
        avatar(&connection, "my-avatar");
        let stored = store(
            &mut connection,
            &heroes,
            "my-avatar",
            &prepare(&picked).unwrap(),
        )
        .unwrap();

        db::save_avatar(
            &mut connection,
            &crate::model::AvatarInput {
                id: Some("my-avatar".into()),
                display_name: "新しい名前".into(),
                base_body: None,
                aliases: Vec::new(),
                notes: String::new(),
            },
        )
        .unwrap();

        assert_eq!(
            hero_path(&connection, "my-avatar").as_deref(),
            stored.to_str()
        );
    }

    #[test]
    fn new_and_migrated_databases_have_the_column() {
        let has_column = |connection: &Connection| -> bool {
            connection
                .prepare("SELECT hero_image_path FROM avatars LIMIT 0")
                .is_ok()
        };
        let version = |connection: &Connection| -> i64 {
            connection
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap()
        };
        let data = tempfile::tempdir().unwrap();
        let path = data.path().join("library.sqlite");
        {
            let connection = db::open(&path).unwrap();
            assert_eq!(version(&connection), 10);
            assert!(has_column(&connection));
            avatar(&connection, "my-avatar");
            connection
                .execute_batch(
                    "ALTER TABLE avatars DROP COLUMN hero_image_path; PRAGMA user_version = 8;",
                )
                .unwrap();
            assert!(!has_column(&connection));
        }
        let connection = db::open(&path).unwrap();
        assert_eq!(version(&connection), 10);
        assert!(has_column(&connection));
        assert_eq!(hero_path(&connection, "my-avatar"), None);
    }

    #[test]
    fn display_falls_back_when_the_file_is_gone_and_versions_it_otherwise() {
        let data = tempfile::tempdir().unwrap();
        let path = data.path().join("a.png");
        std::fs::write(&path, b"x").unwrap();
        let mut connection = db::open_memory().unwrap();
        avatar(&connection, "my-avatar");
        db::set_avatar_hero_path(&connection, "my-avatar", path.to_str()).unwrap();
        let snapshot = db::load_snapshot(&mut connection).unwrap();

        let mut detail = crate::view::avatar_detail(&snapshot, "my-avatar").unwrap();
        fill_display(&mut detail);
        assert_eq!(detail.hero_image_path.as_deref(), path.to_str());
        assert!(detail.hero_image_version.is_some());

        std::fs::remove_file(&path).unwrap();
        let mut detail = crate::view::avatar_detail(&snapshot, "my-avatar").unwrap();
        fill_display(&mut detail);
        assert_eq!(detail.hero_image_path, None);
        assert_eq!(detail.hero_image_version, None);
    }
}
