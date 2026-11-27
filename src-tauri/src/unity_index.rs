//! 導入済みの判定（docs/unity-projects.md の段階 2）の、読み取りと判定の部分。
//!
//! - unitypackage は gzip した tar で、`<GUID>/asset`・`asset.meta`・`pathname` の組が並ぶ。
//!   ディスクへ取り出さずに順に読み、GUID ごとに入る場所・大きさと、外への参照（段階 3）を集める。
//! - プロジェクトは `Assets` の `.meta` の先頭から GUID を読むだけ。書き込まない。
//! - 判定は `verdict` の 1 か所。画面・MCP に複製しない。
//! - パスとファイル名はログに書かない。

use flate2::read::GzDecoder;
use std::collections::{HashMap, HashSet};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 1 つのパッケージで集める GUID の上限。超えたら「一部だけ読んだ」にする。
pub(crate) const MAX_GUIDS: usize = 200_000;
/// 展開後の大きさの上限。
pub(crate) const MAX_UNPACKED: u64 = 16 * 1024 * 1024 * 1024;
/// 1 つのパッケージを読む時間の上限。
pub(crate) const PACKAGE_TIMEOUT: Duration = Duration::from_secs(120);
/// 外への参照を探す asset の大きさの上限。これより大きいものとバイナリは読まない。
pub(crate) const MAX_REF_SCAN: u64 = 2 * 1024 * 1024;
/// 外への参照を集める上限。超えた分は集めない（段階 3 の材料で、導入済みの判定には使わない）。
pub(crate) const MAX_REFS: usize = 100_000;
/// zip の中で見る項目の上限（`file_browser` と同じ）。
const MAX_ZIP_ENTRIES: usize = 5000;
/// `pathname` から読む長さ。
const MAX_PATHNAME: u64 = 4096;

/// プロジェクトで歩く `.meta` の上限。
pub(crate) const MAX_PROJECT_FILES: usize = 500_000;
/// プロジェクトを歩く時間の上限。
pub(crate) const PROJECT_TIMEOUT: Duration = Duration::from_secs(60);

/// 一致率のしきい値（docs/unity-projects.md の計測で決めた）。
pub(crate) const INSTALLED_RATIO: f64 = 0.8;
pub(crate) const PARTIAL_RATIO: f64 = 0.05;
/// 1 つの VPM パッケージがこれ以上を持っていれば「同梱版」。
pub(crate) const BUNDLED_RATIO: f64 = 0.8;

/// 版の比較に使う、利用者が編集しにくい種類。
pub(crate) const STABLE_EXTENSIONS: &[&str] = &[
    "fbx", "png", "psd", "tga", "jpg", "jpeg", "dll", "cs", "shader",
];

pub(crate) type Guid = [u8; 16];

/// 32 桁の 16 進（小文字・大文字）を 16 バイトにする。
pub(crate) fn parse_guid(text: &[u8]) -> Option<Guid> {
    if text.len() != 32 {
        return None;
    }
    let mut guid = [0u8; 16];
    for (index, pair) in text.chunks(2).enumerate() {
        let high = (pair[0] as char).to_digit(16)?;
        let low = (pair[1] as char).to_digit(16)?;
        guid[index] = (high * 16 + low) as u8;
    }
    Some(guid)
}

#[cfg(test)]
pub(crate) fn guid_text(guid: &Guid) -> String {
    guid.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Unity に標準で入っているもの（`0000000000000000f000000000000000` など）。外への参照に数えない。
pub(crate) fn is_builtin_guid(guid: &Guid) -> bool {
    guid[..8].iter().all(|byte| *byte == 0) && guid[9..].iter().all(|byte| *byte == 0)
}

/// パッケージの 1 つの GUID。`size` が `None` ならフォルダ（asset が無い）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PackageAsset {
    pub guid: Guid,
    pub size: Option<u64>,
    /// 拡張子（小文字、8 文字まで）。無ければ空。
    pub ext: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PackageIndex {
    pub assets: Vec<PackageAsset>,
    /// asset の本文が指していた GUID のうち、このパッケージに無いもの（標準のものは除く）。
    pub refs: Vec<Guid>,
    /// 上限で打ち切っていない。
    pub complete: bool,
}

impl PackageIndex {
    pub(crate) fn file_count(&self) -> usize {
        self.assets
            .iter()
            .filter(|asset| asset.size.is_some())
            .count()
    }
}

#[derive(Default)]
struct Entry {
    /// `pathname` を読んだ。値は、`Assets/` の下か、と拡張子。パスそのものは持たない
    /// （GUID の上限まで長いパスを並べたパッケージで、メモリを使い切らないため）。
    place: Option<(bool, String)>,
    size: Option<u64>,
}

fn octal(field: &[u8]) -> Option<u64> {
    // GNU の大きい値（先頭ビットが立った 2 進）。
    if field.first().is_some_and(|byte| byte & 0x80 != 0) {
        let mut value: u64 = u64::from(field[0] & 0x7f);
        for byte in &field[1..] {
            value = value.checked_mul(256)?.checked_add(u64::from(*byte))?;
        }
        return Some(value);
    }
    let text = std::str::from_utf8(field).ok()?;
    let text = text.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    if text.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(text, 8).ok()
}

fn header_name(header: &[u8; 512]) -> String {
    let field = |range: std::ops::Range<usize>| {
        let bytes = &header[range];
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).to_string()
    };
    let name = field(0..100);
    let ustar = &header[257..262] == b"ustar";
    let prefix = if ustar {
        field(345..500)
    } else {
        String::new()
    };
    if prefix.is_empty() {
        name
    } else {
        format!("{prefix}/{name}")
    }
}

/// `./<guid>/asset` のような名前を GUID と葉に分ける。形が違えば `None`。
fn split_name(name: &str) -> Option<(Guid, &str)> {
    let name = name.trim_start_matches("./");
    let (guid, leaf) = name.split_once('/')?;
    if leaf.contains('/') {
        return None;
    }
    Some((parse_guid(guid.as_bytes())?, leaf))
}

/// 読み飛ばす。大きな項目でも期限を過ぎたらやめ、`false` を返す。
fn skip<R: Read>(reader: &mut R, size: u64, deadline: Instant) -> io::Result<bool> {
    const CHUNK: u64 = 8 * 1024 * 1024;
    let mut left = size;
    while left > 0 {
        if Instant::now() >= deadline {
            return Ok(false);
        }
        let step = left.min(CHUNK);
        let copied = io::copy(&mut reader.take(step), &mut io::sink())?;
        if copied < step {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "tar が途中で終わっています",
            ));
        }
        left -= step;
    }
    Ok(true)
}

/// 先頭の `limit` バイトまでを読み、残りは読み飛ばす。期限を過ぎたら `None`。
fn read_up_to<R: Read>(
    reader: &mut R,
    size: u64,
    limit: u64,
    deadline: Instant,
) -> io::Result<Option<Vec<u8>>> {
    let take = size.min(limit);
    let mut buffer = Vec::with_capacity(take as usize);
    reader.take(take).read_to_end(&mut buffer)?;
    if (buffer.len() as u64) < take {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "tar が途中で終わっています",
        ));
    }
    Ok(skip(reader, size - take, deadline)?.then_some(buffer))
}

/// asset の本文から `guid: <32桁>` を拾う。YAML のテキストだけを見る。
fn referenced_guids(body: &[u8], into: &mut HashSet<Guid>) {
    if !body.starts_with(b"%YAML") {
        return;
    }
    const KEY: &[u8] = b"guid: ";
    let mut at = 0;
    while let Some(found) = body[at..]
        .windows(KEY.len())
        .position(|window| window == KEY)
    {
        let start = at + found + KEY.len();
        if let Some(guid) = body.get(start..start + 32).and_then(parse_guid)
            && !is_builtin_guid(&guid)
        {
            if into.len() >= MAX_REFS {
                return;
            }
            into.insert(guid);
        }
        at = start;
    }
}

fn extension_of(path: &str) -> String {
    let leaf = path.rsplit('/').next().unwrap_or(path);
    match leaf.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.len() <= 8 => ext.to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// gzip した tar（unitypackage）を順に読む。
pub(crate) fn read_package<R: Read>(reader: R, deadline: Instant) -> Result<PackageIndex, String> {
    read_tar(GzDecoder::new(reader), deadline)
}

fn read_tar<R: Read>(mut reader: R, deadline: Instant) -> Result<PackageIndex, String> {
    let broken =
        |_: io::Error| "Unity パッケージを読めません（壊れているか、形が違います）".to_string();
    let mut entries: HashMap<Guid, Entry> = HashMap::new();
    let mut refs: HashSet<Guid> = HashSet::new();
    let mut unpacked: u64 = 0;
    let mut complete = true;
    let mut long_name: Option<String> = None;
    loop {
        if Instant::now() >= deadline || entries.len() > MAX_GUIDS {
            complete = false;
            break;
        }
        let mut header = [0u8; 512];
        // 終わりの 0 のブロックを書かない tar もあるので、ヘッダーの区切りで正しく終われば終わりとする。
        // gzip が途中で切れているときは読み取りがエラーを返すので、壊れているとする。
        let mut filled = 0;
        while filled < header.len() {
            match reader.read(&mut header[filled..]) {
                Ok(0) => break,
                Ok(read) => filled += read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(broken(error)),
            }
        }
        if filled == 0 {
            break;
        }
        if filled < header.len() {
            return Err(broken(io::Error::from(io::ErrorKind::UnexpectedEof)));
        }
        if header.iter().all(|byte| *byte == 0) {
            break;
        }
        let size = octal(&header[124..136]).ok_or("Unity パッケージの大きさを読めません")?;
        unpacked = unpacked.saturating_add(size);
        if unpacked > MAX_UNPACKED {
            complete = false;
            break;
        }
        let padded = size.div_ceil(512) * 512;
        let kind = header[156];
        let name = match long_name.take() {
            Some(name) => name,
            None => header_name(&header),
        };
        // GNU の長い名前。次の項目の名前になる。
        if kind == b'L' {
            let Some(body) =
                read_up_to(&mut reader, padded, 4096.min(padded), deadline).map_err(broken)?
            else {
                complete = false;
                break;
            };
            let end = body
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(body.len());
            long_name = Some(String::from_utf8_lossy(&body[..end]).to_string());
            continue;
        }
        let file = kind == b'0' || kind == 0;
        let Some((guid, leaf)) = split_name(&name).filter(|_| file) else {
            if !skip(&mut reader, padded, deadline).map_err(broken)? {
                complete = false;
                break;
            }
            continue;
        };
        let finished = match leaf {
            "pathname" => {
                match read_up_to(&mut reader, padded, MAX_PATHNAME.min(size), deadline)
                    .map_err(broken)?
                {
                    Some(body) => {
                        let text = String::from_utf8_lossy(&body);
                        let line = text.lines().next().unwrap_or("").trim();
                        // Packages/ に入るものや、形の違う場所は数えない。
                        entries.entry(guid).or_default().place =
                            Some((line.starts_with("Assets/"), extension_of(line)));
                        true
                    }
                    None => false,
                }
            }
            "asset" => {
                let finished = if size <= MAX_REF_SCAN {
                    match read_up_to(&mut reader, padded, size, deadline).map_err(broken)? {
                        Some(body) => {
                            referenced_guids(&body, &mut refs);
                            true
                        }
                        None => false,
                    }
                } else {
                    skip(&mut reader, padded, deadline).map_err(broken)?
                };
                entries.entry(guid).or_default().size = Some(size);
                finished
            }
            _ => skip(&mut reader, padded, deadline).map_err(broken)?,
        };
        if !finished {
            complete = false;
            break;
        }
    }
    let mut assets: Vec<PackageAsset> = entries
        .into_iter()
        .filter_map(|(guid, entry)| {
            let (in_assets, ext) = entry.place?;
            in_assets.then_some(PackageAsset {
                guid,
                size: entry.size,
                ext,
            })
        })
        .collect();
    assets.sort_by(|a, b| a.guid.cmp(&b.guid));
    let own: HashSet<Guid> = assets.iter().map(|asset| asset.guid).collect();
    let mut refs: Vec<Guid> = refs
        .into_iter()
        .filter(|guid| !own.contains(guid))
        .collect();
    refs.sort();
    Ok(PackageIndex {
        assets,
        refs,
        complete,
    })
}

/// ライブラリのファイル 1 つから読んだもの。`entry` は zip の中のパス（単体は空）。
pub(crate) struct ReadPackage {
    pub entry: String,
    pub index: Result<PackageIndex, String>,
}

/// `.unitypackage` か、zip の中の `.unitypackage` をすべて読む。それ以外の拡張子は空。
pub(crate) fn read_source(path: &Path) -> Result<Vec<ReadPackage>, String> {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    if lower.ends_with(".unitypackage") {
        let file = std::fs::File::open(path).map_err(|_| "ファイルを開けません".to_string())?;
        let index = read_package(io::BufReader::new(file), Instant::now() + PACKAGE_TIMEOUT);
        return Ok(vec![ReadPackage {
            entry: String::new(),
            index,
        }]);
    }
    if !lower.ends_with(".zip") {
        return Ok(Vec::new());
    }
    let file = std::fs::File::open(path).map_err(|_| "ファイルを開けません".to_string())?;
    let mut archive = zip::ZipArchive::new(io::BufReader::new(file))
        .map_err(|_| "zip を読めません".to_string())?;
    let mut found = Vec::new();
    for index in 0..archive.len().min(MAX_ZIP_ENTRIES) {
        let Ok(item) = archive.by_index(index) else {
            continue;
        };
        let name = item.name().to_string();
        if item.is_dir() || !name.to_ascii_lowercase().ends_with(".unitypackage") {
            continue;
        }
        let read = read_package(item, Instant::now() + PACKAGE_TIMEOUT);
        found.push(ReadPackage {
            entry: name,
            index: read,
        });
    }
    Ok(found)
}

/// `.meta` の先頭から GUID を読む。
pub(crate) fn meta_guid(head: &[u8]) -> Option<Guid> {
    head.split(|byte| *byte == b'\n').find_map(|line| {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let rest = line.strip_prefix(b"guid:")?;
        let rest = rest.trim_ascii();
        parse_guid(rest)
    })
}

/// フォルダの下の `.meta` を歩き、GUID と、その asset のパスを集める。リンクはたどらない。
pub(crate) struct MetaScan {
    pub found: HashMap<Guid, PathBuf>,
    pub files: usize,
    pub complete: bool,
}

pub(crate) fn scan_metas(root: &Path, deadline: Instant, limit: usize) -> MetaScan {
    let mut found = HashMap::new();
    let mut files = 0;
    let mut complete = true;
    for entry in walkdir::WalkDir::new(root).follow_links(false).into_iter() {
        // たどれないフォルダ（Unity が書き換えている途中など）があれば、最後まで調べたとは言わない。
        let Ok(entry) = entry else {
            complete = false;
            continue;
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if !name.ends_with(".meta") {
            continue;
        }
        files += 1;
        if files > limit || Instant::now() >= deadline {
            complete = false;
            break;
        }
        let mut head = [0u8; 256];
        // 読めない .meta があれば、その GUID を見落とすので、最後まで調べたとは言わない。
        let read = match std::fs::File::open(entry.path()).and_then(|mut file| file.read(&mut head))
        {
            Ok(read) => read,
            Err(_) => {
                complete = false;
                continue;
            }
        };
        if let Some(guid) = meta_guid(&head[..read]) {
            let asset = entry.path().with_extension("");
            found.insert(guid, asset);
        }
    }
    MetaScan {
        found,
        files,
        complete,
    }
}

/// パッケージ 1 つの判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Verdict {
    /// 固有のファイルの 80% 以上がプロジェクトにある。
    Installed,
    /// 5% 以上 80% 未満。
    Partial,
    /// 5% 未満。
    Missing,
    /// 固有のファイルが無い（同梱のシェーダーだけ、など）。
    Unknown,
    /// VPM パッケージの同梱版。商品の判定に数えない。
    Bundled,
}

/// 判定の材料。数は asset のある GUID（ファイル）だけ。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    /// ほかの商品のパッケージに現れないファイルの数と、そのうちプロジェクトにあった数。
    pub unique: usize,
    pub unique_matched: usize,
    /// いちばん多くを持っている VPM パッケージが持つ数と、ファイルの総数。
    pub vpm_owned: usize,
    pub files: usize,
}

pub(crate) fn verdict(counts: Counts) -> Verdict {
    if counts.files > 0 && counts.vpm_owned as f64 >= counts.files as f64 * BUNDLED_RATIO {
        return Verdict::Bundled;
    }
    if counts.unique == 0 {
        return Verdict::Unknown;
    }
    let ratio = counts.unique_matched as f64 / counts.unique as f64;
    if ratio >= INSTALLED_RATIO {
        Verdict::Installed
    } else if ratio >= PARTIAL_RATIO {
        Verdict::Partial
    } else {
        Verdict::Missing
    }
}

/// 商品の判定。同梱版を除いたパッケージのうち、いちばん強いもの。
pub(crate) fn product_verdict(verdicts: &[Verdict]) -> Verdict {
    let rank = |verdict: &Verdict| match verdict {
        Verdict::Installed => 4,
        Verdict::Partial => 3,
        Verdict::Missing => 2,
        Verdict::Unknown => 1,
        Verdict::Bundled => 0,
    };
    verdicts
        .iter()
        .filter(|verdict| **verdict != Verdict::Bundled)
        .max_by_key(|verdict| rank(verdict))
        .copied()
        .unwrap_or(if verdicts.is_empty() {
            Verdict::Unknown
        } else {
            Verdict::Bundled
        })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    pub(crate) fn guid(n: u8) -> String {
        format!("{n:02x}{}", "ab".repeat(15))
    }

    fn tar_header(name: &str, size: u64) -> [u8; 512] {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        let size_text = format!("{size:011o}\0");
        header[124..136].copy_from_slice(size_text.as_bytes());
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header
    }

    /// （GUID、入る場所、asset の中身。None はフォルダ）から unitypackage を作る。
    pub(crate) fn package(items: &[(String, &str, Option<&[u8]>)]) -> Vec<u8> {
        let mut tar = Vec::new();
        let mut put = |name: String, body: &[u8]| {
            tar.extend_from_slice(&tar_header(&name, body.len() as u64));
            tar.extend_from_slice(body);
            tar.extend(std::iter::repeat_n(0u8, (512 - body.len() % 512) % 512));
        };
        for (guid, path, body) in items {
            put(format!("{guid}/pathname"), format!("{path}\n00").as_bytes());
            put(
                format!("{guid}/asset.meta"),
                format!("fileFormatVersion: 2\nguid: {guid}\n").as_bytes(),
            );
            if let Some(body) = body {
                put(format!("./{guid}/asset"), body);
            }
        }
        tar.extend([0u8; 1024]);
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar).unwrap();
        gz.finish().unwrap()
    }

    fn far() -> Instant {
        Instant::now() + Duration::from_secs(60)
    }

    #[test]
    fn guids_parse_and_print() {
        let text = guid(7);
        let parsed = parse_guid(text.as_bytes()).unwrap();
        assert_eq!(guid_text(&parsed), text);
        assert_eq!(parse_guid(text.to_uppercase().as_bytes()), Some(parsed));
        assert!(parse_guid(b"xyz").is_none());
        assert!(parse_guid(&[b'g'; 32]).is_none());
        assert!(is_builtin_guid(
            &parse_guid(b"0000000000000000f000000000000000").unwrap()
        ));
        assert!(!is_builtin_guid(&parsed));
    }

    #[test]
    fn package_reading_collects_files_folders_and_outside_refs() {
        let material = format!(
            "%YAML 1.1\n--- !u!21 &2100000\nMaterial:\n  m_Shader: {{fileID: 4800000, guid: {}, type: 3}}\n  m_Tex: {{fileID: 2800000, guid: {}, type: 3}}\n  m_Std: {{fileID: 46, guid: 0000000000000000f000000000000000, type: 0}}\n",
            guid(90),
            guid(2)
        );
        let bytes = package(&[
            (guid(1), "Assets/Shop", None),
            (guid(2), "Assets/Shop/Tex.PNG", Some(b"\x89PNG....")),
            (guid(3), "Assets/Shop/Mat.mat", Some(material.as_bytes())),
            (guid(4), "Packages/com.x/Runtime.cs", Some(b"class X {}")),
        ]);
        let index = read_package(bytes.as_slice(), far()).unwrap();
        assert!(index.complete);
        assert_eq!(index.assets.len(), 3);
        assert_eq!(index.file_count(), 2);
        let tex = index
            .assets
            .iter()
            .find(|a| guid_text(&a.guid) == guid(2))
            .unwrap();
        assert_eq!(tex.ext, "png");
        assert_eq!(tex.size, Some(8));
        assert!(
            index
                .assets
                .iter()
                .any(|a| guid_text(&a.guid) == guid(1) && a.size.is_none())
        );
        // 自分の GUID（Tex）と標準のものは外への参照に入らない。
        assert_eq!(
            index.refs.iter().map(guid_text).collect::<Vec<_>>(),
            [guid(90)]
        );
    }

    #[test]
    fn broken_and_odd_packages() {
        assert!(read_package(&b"not gzip"[..], far()).is_err());
        let bytes = package(&[(guid(1), "Assets/A.png", Some(b"x"))]);
        assert!(read_package(&bytes[..bytes.len() / 2], far()).is_err());
        // 期限を過ぎていたら途中で止め、一部だけとする。
        let late = read_package(bytes.as_slice(), Instant::now()).unwrap();
        assert!(!late.complete);
        assert_eq!(extension_of("Assets/a.b/c"), "");
        assert_eq!(extension_of("Assets/.hidden"), "");
        assert_eq!(extension_of("Assets/x.FBX"), "fbx");
    }

    #[test]
    fn zip_sources_read_every_inner_package() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Shop.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("Shop/Body.unitypackage", options).unwrap();
        zip.write_all(&package(&[(guid(1), "Assets/A.fbx", Some(b"fbx"))]))
            .unwrap();
        zip.start_file("Shop/readme.txt", options).unwrap();
        zip.write_all(b"hello").unwrap();
        zip.start_file("Shop/lilToon.UnityPackage", options)
            .unwrap();
        zip.write_all(&package(&[(
            guid(2),
            "Assets/lilToon/x.shader",
            Some(b"s"),
        )]))
        .unwrap();
        zip.finish().unwrap();
        let found = read_source(&path).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].entry, "Shop/Body.unitypackage");
        assert_eq!(found[1].index.as_ref().unwrap().assets[0].ext, "shader");
        let single = dir.path().join("x.unitypackage");
        std::fs::write(&single, package(&[(guid(3), "Assets/B.png", Some(b"b"))])).unwrap();
        assert_eq!(read_source(&single).unwrap()[0].entry, "");
        std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
        assert!(read_source(&dir.path().join("a.txt")).unwrap().is_empty());
    }

    #[test]
    fn meta_scan_reads_guids_without_following_links() {
        let dir = tempfile::tempdir().unwrap();
        let assets = dir.path().join("Assets").join("Shop");
        std::fs::create_dir_all(&assets).unwrap();
        std::fs::write(assets.join("A.png"), b"x").unwrap();
        std::fs::write(
            assets.join("A.png.meta"),
            format!("fileFormatVersion: 2\r\nguid: {}\r\n", guid(5)),
        )
        .unwrap();
        std::fs::write(assets.join("broken.meta"), b"nothing").unwrap();
        let scan = scan_metas(&dir.path().join("Assets"), far(), MAX_PROJECT_FILES);
        assert!(scan.complete);
        assert_eq!(scan.files, 2);
        let found = scan
            .found
            .get(&parse_guid(guid(5).as_bytes()).unwrap())
            .unwrap();
        assert!(found.ends_with("A.png"));
        let limited = scan_metas(&dir.path().join("Assets"), far(), 1);
        assert!(!limited.complete);
        // たどれないフォルダ（ここでは消えたフォルダ）があれば、最後まで調べたとは言わない。
        let missing = scan_metas(&dir.path().join("Gone"), far(), MAX_PROJECT_FILES);
        assert!(!missing.complete);
    }

    #[test]
    fn verdicts_follow_the_measured_thresholds() {
        let counts = |unique, unique_matched, vpm_owned, files| Counts {
            unique,
            unique_matched,
            vpm_owned,
            files,
        };
        assert_eq!(verdict(counts(100, 100, 0, 100)), Verdict::Installed);
        assert_eq!(verdict(counts(79, 43, 0, 79)), Verdict::Partial);
        assert_eq!(verdict(counts(35, 1, 0, 35)), Verdict::Missing);
        assert_eq!(verdict(counts(0, 0, 0, 10)), Verdict::Unknown);
        assert_eq!(verdict(counts(0, 0, 361, 367)), Verdict::Bundled);
        assert_eq!(verdict(counts(10, 0, 0, 0)), Verdict::Missing);
        assert_eq!(
            product_verdict(&[Verdict::Bundled, Verdict::Missing, Verdict::Installed]),
            Verdict::Installed
        );
        assert_eq!(product_verdict(&[Verdict::Bundled]), Verdict::Bundled);
        assert_eq!(product_verdict(&[]), Verdict::Unknown);
    }
}
