//! 「Unityで開く」で、mp3・png・fbx のような単体のファイルを Unity に渡す。
//!
//! Unity の Import 画面は `.unitypackage` しか受けないので、選んだ 1 ファイルを
//! アプリのキャッシュ `unity-import/<商品ID>/` に `.unitypackage`（gzip した tar）として包む。
//! Unity へは既存の `-openfile` で渡し、Import 画面で本人が確かめてから取り込まれる。
//! プロジェクトには直接書き込まない。
//!
//! 入る場所は `Assets/VRCLibMan/<商品名>/<ファイル名>` に決め、渡す前に画面で見せる。
//! GUID は入る場所から決めるので、同じファイルをもう一度渡しても Unity は同じアセットとして扱い、
//! 複製を作らない。ライブラリのファイルは読むだけで、パスやファイル名はログに書かない。

use crate::unity_import::PreparedPackage;
use flate2::Compression;
use flate2::write::GzEncoder;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

/// 包んで渡せる拡張子。Unity が標準で取り込める音声・画像・3D だけ。
/// （flac・m4a・webp・clip は Unity が読めないので出さない。）
pub(crate) const LOOSE_EXTENSIONS: &[&str] = &[
    "mp3", "wav", "ogg", "aif", "aiff", "png", "jpg", "jpeg", "psd", "tga", "fbx", "blend",
];

/// 入る場所の親。
pub(crate) const ASSET_ROOT: &str = "Assets/VRCLibMan";

/// 商品名を使うときの長さの上限（文字数）。
const MAX_FOLDER_CHARS: usize = 60;
/// ファイル名の長さの上限（文字数）。
const MAX_FILE_CHARS: usize = 120;
/// tar の大きさの欄（8 進 11 桁）に入る上限。
const MAX_TAR_SIZE: u64 = 0o77_777_777_777;

pub(crate) fn is_loose_name(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    LOOSE_EXTENSIONS.iter().any(|ext| {
        lower
            .strip_suffix(&format!(".{ext}"))
            .is_some_and(|stem| !stem.trim().is_empty())
    })
}

/// Unity のアセット名に使えない文字を置き換え、前後の空白と末尾の `.` を外す。
fn clean_part(value: &str, max_chars: usize) -> String {
    let cleaned: String = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches(['.', ' ']);
    let cleaned = cleaned.trim_start_matches('.').trim();
    cleaned
        .chars()
        .take(max_chars)
        .collect::<String>()
        .trim_end_matches(['.', ' '])
        .to_string()
}

/// `Assets/VRCLibMan/` の下のフォルダ名。商品名があれば商品名、無ければ商品 ID、どちらも無ければ「その他」。
pub(crate) fn folder_name(title: Option<&str>, booth_id: Option<&str>) -> String {
    if let Some(title) = title {
        let cleaned = clean_part(title, MAX_FOLDER_CHARS);
        if !cleaned.is_empty() {
            return cleaned;
        }
    }
    match booth_id.map(str::trim) {
        Some(id) if crate::library_files::valid_booth_id(id) => id.to_string(),
        _ => "その他".to_string(),
    }
}

/// ファイル名の拡張子を残したまま縮める。
fn file_part(filename: &str) -> Option<String> {
    let cleaned = clean_part(filename, usize::MAX);
    if !is_loose_name(&cleaned) {
        return None;
    }
    if cleaned.chars().count() <= MAX_FILE_CHARS {
        return Some(cleaned);
    }
    let dot = cleaned.rfind('.')?;
    let ext = &cleaned[dot..];
    let stem: String = cleaned[..dot]
        .chars()
        .take(MAX_FILE_CHARS - ext.chars().count())
        .collect();
    Some(format!("{}{ext}", stem.trim_end_matches(['.', ' '])))
}

/// Unity の中で入る場所。`Assets/VRCLibMan/<フォルダ>/<ファイル名>`。
pub(crate) fn destination(folder: &str, filename: &str) -> Option<String> {
    let file = file_part(filename)?;
    let folder = clean_part(folder, MAX_FOLDER_CHARS);
    let folder = if folder.is_empty() {
        "その他".to_string()
    } else {
        folder
    };
    Some(format!("{ASSET_ROOT}/{folder}/{file}"))
}

/// 入る場所から決まる GUID（32 桁の 16 進）。FNV-1a 128 bit。
pub(crate) fn guid_for(path: &str) -> String {
    const OFFSET: u128 = 0x6c62272e07bb014262b821756295c58d;
    const PRIME: u128 = 0x0000000001000000000000000000013B;
    let mut hash = OFFSET;
    for byte in b"vrclibman-unity-wrap\0".iter().chain(path.as_bytes()) {
        hash ^= u128::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:032x}")
}

fn octal(field: &mut [u8], value: u64) {
    let digits = field.len() - 1;
    let text = format!("{value:0digits$o}");
    field[..digits].copy_from_slice(text.as_bytes());
    field[digits] = 0;
}

/// ustar のヘッダー 1 枚。名前は `<guid>/asset` のような短い ASCII だけ。
fn tar_header(name: &str, size: u64) -> [u8; 512] {
    let mut header = [0u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    octal(&mut header[100..108], 0o644);
    octal(&mut header[108..116], 0);
    octal(&mut header[116..124], 0);
    octal(&mut header[124..136], size);
    octal(&mut header[136..148], 0);
    header[148..156].copy_from_slice(b"        ");
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let sum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
    let text = format!("{sum:06o}\0 ");
    header[148..156].copy_from_slice(text.as_bytes());
    header
}

fn pad<W: Write>(out: &mut W, size: u64) -> std::io::Result<()> {
    let rest = (512 - (size % 512) as usize) % 512;
    out.write_all(&vec![0u8; rest])
}

fn small_entry<W: Write>(out: &mut W, name: &str, body: &[u8]) -> std::io::Result<()> {
    out.write_all(&tar_header(name, body.len() as u64))?;
    out.write_all(body)?;
    pad(out, body.len() as u64)
}

/// `source` を `asset_path` に入る `.unitypackage` として `package` に書く。
/// 読み込み中に大きさが変わったら失敗にする。
pub(crate) fn write_package(
    source: &Path,
    asset_path: &str,
    package: &Path,
) -> Result<u64, String> {
    let mut input = File::open(source).map_err(|_| "ファイルを開けません".to_string())?;
    let size = input
        .metadata()
        .map_err(|_| "ファイルを開けません".to_string())?
        .len();
    if size > MAX_TAR_SIZE || size > crate::unity_import::MAX_PACKAGE_BYTES {
        return Err("ファイルが大きすぎるため、Unity パッケージにしません".into());
    }
    let guid = guid_for(asset_path);
    let meta = format!("fileFormatVersion: 2\nguid: {guid}\n");
    let mut write = || -> std::io::Result<u64> {
        let file = File::create(package)?;
        let mut out = GzEncoder::new(BufWriter::new(file), Compression::default());
        out.write_all(&tar_header(&format!("{guid}/asset"), size))?;
        let copied = std::io::copy(&mut (&mut input).take(size), &mut out)?;
        if copied != size {
            return Err(std::io::Error::other("size changed"));
        }
        pad(&mut out, size)?;
        small_entry(&mut out, &format!("{guid}/asset.meta"), meta.as_bytes())?;
        small_entry(&mut out, &format!("{guid}/pathname"), asset_path.as_bytes())?;
        out.write_all(&[0u8; 1024])?;
        let mut inner = out.finish()?;
        inner.flush()?;
        inner
            .into_inner()
            .map_err(|err| err.into_error())?
            .sync_all()?;
        Ok(copied)
    };
    write().map_err(|_| "Unity パッケージを作れません".to_string())
}

/// 選んだ 1 ファイルを `dest` に包む。名前は `VRCLibMan-<GUID の先頭 12 桁>.unitypackage`。
/// 日本語の名前の `.unitypackage` は Unity 2022.3 が展開に失敗した（Temp/TarGZ への書き出しで
/// 「指定されたファイルが見つかりません」）ので、包んだものの名前は ASCII にする。
pub(crate) fn wrap_file(
    source: &Path,
    asset_path: &str,
    dest: &Path,
) -> Result<PreparedPackage, String> {
    let filename = source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    file_part(&filename).ok_or_else(|| "Unity に入れられないファイルです".to_string())?;
    std::fs::create_dir_all(dest).map_err(|_| "取り出し先のフォルダを作れません".to_string())?;
    let dest = std::fs::canonicalize(dest)
        .map(|path| crate::library_files::without_verbatim_prefix(&path))
        .map_err(|_| "取り出し先のフォルダを作れません".to_string())?;
    let name = format!("VRCLibMan-{}.unitypackage", &guid_for(asset_path)[..12]);
    let target: PathBuf = dest.join(&name);
    if target.parent() != Some(dest.as_path()) {
        return Err("取り出し先がフォルダの外になるため、作りません".into());
    }
    let partial = dest.join(format!("{name}.part"));
    let size = match write_package(source, asset_path, &partial) {
        Ok(size) => size,
        Err(err) => {
            let _ = std::fs::remove_file(&partial);
            return Err(err);
        }
    };
    let _ = std::fs::remove_file(&target);
    std::fs::rename(&partial, &target).map_err(|_| {
        let _ = std::fs::remove_file(&partial);
        "取り出し先に書き込めません".to_string()
    })?;
    Ok(PreparedPackage {
        name: filename,
        path: target.display().to_string(),
        size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;

    fn entries(package: &Path) -> Vec<(String, Vec<u8>)> {
        let mut bytes = Vec::new();
        GzDecoder::new(File::open(package).unwrap())
            .read_to_end(&mut bytes)
            .unwrap();
        let mut found = Vec::new();
        let mut at = 0;
        while at + 512 <= bytes.len() {
            let header = &bytes[at..at + 512];
            if header.iter().all(|byte| *byte == 0) {
                break;
            }
            let stored: u32 =
                u32::from_str_radix(std::str::from_utf8(&header[148..154]).unwrap(), 8).unwrap();
            let mut blank = header.to_vec();
            blank[148..156].copy_from_slice(b"        ");
            assert_eq!(stored, blank.iter().map(|b| u32::from(*b)).sum::<u32>());
            let name_end = header[..100].iter().position(|b| *b == 0).unwrap();
            let name = String::from_utf8(header[..name_end].to_vec()).unwrap();
            let size = u64::from_str_radix(std::str::from_utf8(&header[124..135]).unwrap(), 8)
                .unwrap() as usize;
            at += 512;
            found.push((name, bytes[at..at + size].to_vec()));
            at += size.div_ceil(512) * 512;
        }
        found
    }

    #[test]
    fn only_files_unity_can_read_are_wrapped() {
        assert!(is_loose_name("水滴_近く_15分.mp3"));
        assert!(is_loose_name("Body.FBX"));
        assert!(!is_loose_name(".mp3"));
        assert!(!is_loose_name("track.flac"));
        assert!(!is_loose_name("pack.unitypackage"));
        assert!(!is_loose_name("setup.exe"));
    }

    #[test]
    fn the_destination_uses_the_product_name_and_stays_one_folder_deep() {
        assert_eq!(
            folder_name(Some("自然の環境音｜Ambient"), Some("1234567")),
            "自然の環境音｜Ambient"
        );
        assert_eq!(folder_name(Some(" a/b:c. "), None), "a_b_c");
        assert_eq!(folder_name(Some("  "), Some("1234567")), "1234567");
        assert_eq!(folder_name(None, Some("../x")), "その他");
        assert_eq!(
            destination("自然の環境音", "水滴_近く_15分.mp3").as_deref(),
            Some("Assets/VRCLibMan/自然の環境音/水滴_近く_15分.mp3")
        );
        assert_eq!(
            destination("..", "a.mp3").as_deref(),
            Some("Assets/VRCLibMan/その他/a.mp3")
        );
        assert_eq!(destination("x", "note.txt"), None);
        let long = format!("{}.wav", "あ".repeat(300));
        let path = destination("x", &long).unwrap();
        assert!(path.ends_with(".wav"));
        assert!(path.chars().count() < 200);
    }

    #[test]
    fn the_same_place_gets_the_same_guid() {
        let a = guid_for("Assets/VRCLibMan/x/a.mp3");
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|ch| ch.is_ascii_hexdigit()));
        assert_eq!(a, guid_for("Assets/VRCLibMan/x/a.mp3"));
        assert_ne!(a, guid_for("Assets/VRCLibMan/x/b.mp3"));
    }

    #[test]
    fn a_file_becomes_a_package_unity_can_import() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("雨.mp3");
        let body: Vec<u8> = (0..1500u32).map(|n| (n % 251) as u8).collect();
        std::fs::write(&source, &body).unwrap();
        let place = "Assets/VRCLibMan/環境音/雨.mp3";
        let prepared = wrap_file(&source, place, &dir.path().join("cache")).unwrap();
        assert_eq!(prepared.name, "雨.mp3");
        assert_eq!(prepared.size, 1500);
        let guid = guid_for(place);
        assert!(
            prepared
                .path
                .ends_with(&format!("VRCLibMan-{}.unitypackage", &guid[..12]))
        );
        let found = entries(Path::new(&prepared.path));
        assert_eq!(found.len(), 3);
        assert_eq!(found[0], (format!("{guid}/asset"), body));
        assert_eq!(found[1].0, format!("{guid}/asset.meta"));
        assert!(String::from_utf8_lossy(&found[1].1).contains(&format!("guid: {guid}")));
        assert_eq!(
            found[2],
            (format!("{guid}/pathname"), place.as_bytes().to_vec())
        );
        // 途中のファイルは残さない。
        assert!(!Path::new(&format!("{}.part", prepared.path)).exists());
        // もう一度包むと置き換える。
        let again = wrap_file(&source, place, &dir.path().join("cache")).unwrap();
        assert_eq!(again.path, prepared.path);
    }
}
