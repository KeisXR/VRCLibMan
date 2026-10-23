use crate::normalize::{is_cjk, nfkc};
use serde_json::Value;

pub const SOURCE_URL: &str = "https://github.com/eringiriri/mochifitter_list";
pub const DATA_URL: &str =
    "https://raw.githubusercontent.com/eringiriri/mochifitter_list/main/data/profiles.json";
pub const TERMS_URL: &str =
    "https://github.com/eringiriri/mochifitter_list/blob/main/terms.html";
pub const AUTHOR: &str = "えりんぎ";
pub const CREDIT: &str = "もちふぃった～プロファイル一覧";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub booth_id: String,
    pub display_name: String,
    pub aliases: Vec<String>,
    pub author: String,
    pub shop: String,
    pub body_base: String,
}

#[derive(Debug, Clone)]
pub struct ParsedCatalog {
    pub fetched_on: String,
    pub upstream_updated: String,
    pub entries: Vec<CatalogEntry>,
}

pub fn bundled() -> Result<ParsedCatalog, String> {
    parse_catalog_json(include_str!("../knowledge/mochifitter-profiles.json"))
}

pub fn parse_catalog_json(text: &str) -> Result<ParsedCatalog, String> {
    let value: Value = serde_json::from_str(text)
        .map_err(|err| format!("プロファイル一覧のJSONを読めません: {err}"))?;
    let fetched_on = value
        .get("fetchedOn")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let upstream_updated = value
        .get("upstreamUpdated")
        .or_else(|| value.get("lastUpdated"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let profiles = value
        .get("profiles")
        .and_then(Value::as_array)
        .ok_or("プロファイル一覧に profiles がありません")?;
    let mut order: Vec<String> = Vec::new();
    let mut grouped: Vec<RawGroup> = Vec::new();
    for item in profiles {
        let Some(booth_id) = booth_id_of(item.get("avatarNameUrl").and_then(Value::as_str).unwrap_or(""))
        else {
            continue;
        };
        let name = item
            .get("avatarName")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if name.is_empty() {
            continue;
        }
        let author = clip(item.get("avatarAuthor").and_then(Value::as_str).unwrap_or(""));
        let shop = clip(item.get("avatarshopname").and_then(Value::as_str).unwrap_or(""));
        let body = clip(item.get("bodyBase").and_then(Value::as_str).unwrap_or(""));
        if let Some(index) = order.iter().position(|id| id == &booth_id) {
            let group = &mut grouped[index];
            if !group.names.iter().any(|existing| existing == name) {
                group.names.push(name.to_string());
            }
            if group.author.is_empty() {
                group.author = author;
            }
            if group.shop.is_empty() {
                group.shop = shop;
            }
            if group.body_base.is_empty() {
                group.body_base = body;
            }
        } else {
            order.push(booth_id);
            grouped.push(RawGroup {
                names: vec![name.to_string()],
                author,
                shop,
                body_base: body,
            });
        }
    }
    let mut entries = Vec::new();
    for (booth_id, group) in order.into_iter().zip(grouped) {
        let Some(entry) = entry_from_names(&booth_id, &group) else {
            continue;
        };
        entries.push(entry);
    }
    if entries.is_empty() {
        return Err("プロファイル一覧からアバターを取り出せませんでした".into());
    }
    Ok(ParsedCatalog {
        fetched_on,
        upstream_updated,
        entries,
    })
}

struct RawGroup {
    names: Vec<String>,
    author: String,
    shop: String,
    body_base: String,
}

fn entry_from_names(booth_id: &str, group: &RawGroup) -> Option<CatalogEntry> {
    let mut display = String::new();
    let mut aliases = Vec::new();
    for name in &group.names {
        let (next, extra) = split_bilingual_name(name);
        if next.chars().count() > 80 {
            continue;
        }
        if display.is_empty() {
            display = next;
        } else if next != display && !aliases.iter().any(|alias: &String| alias == &next) {
            aliases.push(next);
        }
        for alias in extra {
            if alias != display && !aliases.iter().any(|existing| existing == &alias) {
                aliases.push(alias);
            }
        }
    }
    if display.is_empty() {
        return None;
    }
    aliases.truncate(32);
    Some(CatalogEntry {
        booth_id: booth_id.to_string(),
        display_name: display,
        aliases,
        author: group.author.clone(),
        shop: group.shop.clone(),
        body_base: group.body_base.clone(),
    })
}

pub fn catalog_note(entry: &CatalogEntry) -> String {
    let mut parts = vec![format!("出典: {CREDIT}（{AUTHOR}）")];
    if !entry.author.is_empty() {
        parts.push(format!("作者: {}", entry.author));
    }
    if !entry.shop.is_empty() {
        parts.push(format!("ショップ: {}", entry.shop));
    }
    if !entry.body_base.is_empty() {
        parts.push(format!("素体メモ: {}", entry.body_base));
    }
    let note = parts.join("。");
    note.chars().take(400).collect()
}

/// 「ルキフェル -Lucifer-」や「Nayu - ナユ」を、日本語の表示名と別名に分ける。
pub fn split_bilingual_name(raw: &str) -> (String, Vec<String>) {
    let folded = nfkc(raw).trim().trim_matches(['/', '／']).trim().to_string();
    if folded.is_empty() {
        return (String::new(), Vec::new());
    }
    let (base, paren_aliases) = take_parens(&folded);
    let mut pieces = Vec::new();
    for part in split_separators(&base) {
        for bit in split_glued(&part) {
            let cleaned = strip_version(&trim_edges(&bit));
            push_piece(&mut pieces, &cleaned);
        }
    }
    if pieces.len() == 1 {
        let tokens: Vec<&str> = pieces[0].split_whitespace().collect();
        let mixed = tokens.len() >= 2
            && tokens.iter().any(|token| script_of(token) == Script::Japanese)
            && tokens.iter().any(|token| script_of(token) == Script::Latin);
        if mixed {
            pieces = tokens.into_iter().map(str::to_string).collect();
        } else {
            let runs = split_script_run(&pieces[0]);
            if runs.len() > 1 {
                pieces = runs;
            }
        }
    }
    for alias in paren_aliases {
        push_piece(&mut pieces, &strip_version(&trim_edges(&alias)));
    }
    if pieces.is_empty() {
        return (trim_edges(&folded), Vec::new());
    }
    let display_at = preferred_display(&pieces);
    let display = pieces[display_at].clone();
    let aliases = pieces
        .into_iter()
        .enumerate()
        .filter(|(index, _)| *index != display_at)
        .map(|(_, piece)| piece)
        .filter(|piece| piece.chars().count() >= 2 || piece.chars().any(is_cjk))
        .take(32)
        .collect();
    (display, aliases)
}

fn push_piece(pieces: &mut Vec<String>, piece: &str) {
    if piece.is_empty() || is_noise(piece) {
        return;
    }
    if !pieces.iter().any(|existing| existing == piece) {
        pieces.push(piece.to_string());
    }
}

fn preferred_display(pieces: &[String]) -> usize {
    let mut best: Option<(usize, i32)> = None;
    for (index, piece) in pieces.iter().enumerate() {
        let kanji = piece.chars().filter(|ch| is_kanji(*ch)).count() as i32;
        let japanese = piece.chars().any(is_cjk);
        if !japanese {
            continue;
        }
        let score = kanji * 10 + if piece.chars().any(|ch| ch.is_ascii_alphabetic()) { 0 } else { 1 };
        if best.is_none_or(|(_, current)| score > current) {
            best = Some((index, score));
        }
    }
    best.map(|(index, _)| index).unwrap_or(0)
}

fn take_parens(text: &str) -> (String, Vec<String>) {
    let chars: Vec<char> = text.chars().collect();
    let mut base = String::new();
    let mut aliases = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let open = chars[index];
        if open == '（' || open == '(' {
            let close = if open == '（' { '）' } else { ')' };
            if let Some(end) = chars[index + 1..].iter().position(|ch| *ch == close) {
                let inner: String = chars[index + 1..index + 1 + end].iter().collect();
                if !is_noise(inner.trim()) {
                    aliases.push(inner);
                }
                index += end + 2;
                continue;
            }
            let inner: String = chars[index + 1..].iter().collect();
            if !is_noise(inner.trim()) {
                aliases.push(inner);
            }
            break;
        }
        base.push(open);
        index += 1;
    }
    (base, aliases)
}

fn split_separators(text: &str) -> Vec<String> {
    let mut parts = vec![text.to_string()];
    for separator in [" / ", "/", "／", " - ", " – ", " — ", " － "] {
        let mut next = Vec::new();
        for part in parts {
            if part.contains(separator) {
                next.extend(
                    part.split(separator)
                        .map(str::trim)
                        .filter(|bit| !bit.is_empty())
                        .map(str::to_string),
                );
            } else {
                next.push(part);
            }
        }
        parts = next;
    }
    parts
}

fn split_glued(text: &str) -> Vec<String> {
    if !text.contains('-') {
        return vec![text.to_string()];
    }
    let bits: Vec<&str> = text
        .split('-')
        .map(str::trim)
        .filter(|bit| !bit.is_empty())
        .collect();
    if bits.len() < 2 {
        return vec![text.to_string()];
    }
    let japanese = bits.iter().any(|bit| script_of(bit) == Script::Japanese);
    let latin = bits.iter().any(|bit| script_of(bit) == Script::Latin);
    if japanese && latin {
        return bits.into_iter().map(str::to_string).collect();
    }
    vec![text.to_string()]
}

fn split_script_run(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 2 {
        return vec![text.to_string()];
    }
    let mut cuts = vec![0usize];
    for index in 1..chars.len() {
        let previous = script_char(chars[index - 1]);
        let current = script_char(chars[index]);
        let crosses = (previous == Script::Japanese && current == Script::Latin)
            || (previous == Script::Latin && current == Script::Japanese);
        if crosses {
            cuts.push(index);
        }
    }
    if cuts.len() == 1 {
        return vec![text.to_string()];
    }
    cuts.push(chars.len());
    let mut parts = Vec::new();
    for pair in cuts.windows(2) {
        let part: String = chars[pair[0]..pair[1]].iter().collect();
        let part = part.trim().to_string();
        if part.chars().count() >= 2 || part.chars().any(is_cjk) {
            parts.push(part);
        }
    }
    if parts.len() >= 2 { parts } else { vec![text.to_string()] }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Script {
    Japanese,
    Latin,
    Other,
}

fn script_of(text: &str) -> Script {
    let mut japanese = false;
    let mut latin = false;
    for ch in text.chars() {
        match script_char(ch) {
            Script::Japanese => japanese = true,
            Script::Latin => latin = true,
            Script::Other => {}
        }
    }
    if japanese && !latin {
        Script::Japanese
    } else if latin && !japanese {
        Script::Latin
    } else {
        Script::Other
    }
}

fn script_char(ch: char) -> Script {
    if is_cjk(ch) || is_kanji(ch) {
        Script::Japanese
    } else if ch.is_ascii_alphabetic() {
        Script::Latin
    } else {
        Script::Other
    }
}

fn is_kanji(ch: char) -> bool {
    ('\u{3400}'..='\u{9fff}').contains(&ch)
}

fn trim_edges(text: &str) -> String {
    text.trim()
        .trim_matches(|ch: char| {
            ch.is_whitespace() || matches!(ch, '-' | '–' | '—' | '－' | '/' | '／' | '.' | '・' | ':')
        })
        .trim()
        .to_string()
}

fn strip_version(text: &str) -> String {
    let mut end = text.len();
    let lower = text.to_lowercase();
    if let Some(index) = lower.rfind("gen") {
        let tail = &lower[index + 3..];
        if tail.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
            end = index;
        }
    }
    text[..end].trim().trim_end_matches(['-', ' ']).trim().to_string()
}

fn is_noise(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 40 {
        return true;
    }
    let lower = trimmed.to_lowercase();
    if matches!(lower.as_str(), "vrc" | "vrchat" | "xavatar") {
        return true;
    }
    trimmed.contains("素体")
        || trimmed.contains("向け")
        || trimmed.contains("形式")
        || lower.contains("xavatar")
}

fn booth_id_of(url: &str) -> Option<String> {
    let marker = "/items/";
    let start = url.find(marker)? + marker.len();
    let digits: String = url[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    if (4..=12).contains(&digits.len()) {
        Some(digits)
    } else {
        None
    }
}

fn clip(text: &str) -> String {
    text.trim().chars().take(80).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bilingual_titles_split_into_a_japanese_display_name() {
        let (display, aliases) = split_bilingual_name("ルキフェル -Lucifer-");
        assert_eq!(display, "ルキフェル");
        assert_eq!(aliases, vec!["Lucifer".to_string()]);
        let (display, aliases) = split_bilingual_name("Nayu - ナユ");
        assert_eq!(display, "ナユ");
        assert_eq!(aliases, vec!["Nayu".to_string()]);
        let (display, aliases) = split_bilingual_name("MIKOMIA - ミコミア -");
        assert_eq!(display, "ミコミア");
        assert_eq!(aliases, vec!["MIKOMIA".to_string()]);
        let (display, aliases) = split_bilingual_name("焔花（ほのか）");
        assert_eq!(display, "焔花");
        assert_eq!(aliases, vec!["ほのか".to_string()]);
        let (display, aliases) = split_bilingual_name("Techie/テッキー");
        assert_eq!(display, "テッキー");
        assert_eq!(aliases, vec!["Techie".to_string()]);
        let (display, aliases) = split_bilingual_name("彼方-Kanata-");
        assert_eq!(display, "彼方");
        assert_eq!(aliases, vec!["Kanata".to_string()]);
        let (display, aliases) = split_bilingual_name("ナナセ・ノワール -Nanase Noir-Gen2.2");
        assert_eq!(display, "ナナセ・ノワール");
        assert_eq!(aliases, vec!["Nanase Noir".to_string()]);
        let (display, aliases) = split_bilingual_name("DuCBody / VRChat向け男性素体");
        assert_eq!(display, "DuCBody");
        assert!(aliases.is_empty());
        let (display, aliases) = split_bilingual_name("真冬 Mafuyu");
        assert_eq!(display, "真冬");
        assert_eq!(aliases, vec!["Mafuyu".to_string()]);
        let (display, aliases) = split_bilingual_name("キプフェル Kipfel");
        assert_eq!(display, "キプフェル");
        assert_eq!(aliases, vec!["Kipfel".to_string()]);
    }

    #[test]
    fn bundled_snapshot_keeps_booth_ids_and_credits() {
        let catalog = bundled().unwrap();
        assert!(catalog.entries.len() > 800, "{}", catalog.entries.len());
        assert_eq!(catalog.fetched_on, "2026-10-23");
        let sukiha = catalog
            .entries
            .iter()
            .find(|entry| entry.booth_id == "3016885")
            .unwrap();
        assert_eq!(sukiha.display_name, "透羽");
        let lucifer = catalog.entries.iter().find(|entry| {
            entry.display_name == "ルキフェル" || entry.aliases.iter().any(|alias| alias == "Lucifer")
        });
        assert!(lucifer.is_some());
        let raw = r#"{
            "lastUpdated": "2026-02-12 18:36:02 JST",
            "profiles": [
                {
                    "avatarName": "ルキフェル -Lucifer-",
                    "avatarNameUrl": "https://example.booth.pm/items/4242421",
                    "avatarAuthor": "架空",
                    "avatarshopname": "架空工房",
                    "bodyBase": "",
                    "imageUrl": "https://example.invalid/secret.jpg",
                    "notes": "ここは取り込まない"
                }
            ]
        }"#;
        let parsed = parse_catalog_json(raw).unwrap();
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].display_name, "ルキフェル");
        assert_eq!(parsed.entries[0].aliases, vec!["Lucifer".to_string()]);
        assert_eq!(parsed.entries[0].booth_id, "4242421");
        assert!(!catalog_note(&parsed.entries[0]).contains("secret"));
        assert!(catalog_note(&parsed.entries[0]).contains(CREDIT));
    }
}
