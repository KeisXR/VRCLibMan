use crate::model::{BoothCategoryInput, IncomingDownload, IncomingProduct, IncomingVariation};
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct ParsedExport {
    pub products: Vec<IncomingProduct>,
    pub warnings: Vec<String>,
    pub skipped: u32,
}

pub fn parse_export(text: &str) -> Result<ParsedExport, String> {
    let value: Value = serde_json::from_str(text)
        .map_err(|err| format!("JSONを読めません: {err}"))?;
    let items = match value {
        Value::Array(items) => items,
        Value::Object(map) => {
            if let Some(Value::Array(items)) = map.get("products").cloned() {
                items
            } else if map.contains_key("boothId")
                || map.contains_key("booth_id")
                || map.contains_key("url")
                || map.contains_key("title")
                || map.contains_key("name")
            {
                vec![Value::Object(map)]
            } else {
                return Err("products 配列が見つかりません".into());
            }
        }
        _ => return Err("JSONの形式が対応外です".into()),
    };

    let mut products = Vec::new();
    let mut index = HashMap::<String, usize>::new();
    let mut warnings = Vec::new();
    let mut skipped = 0;
    for item in items {
        match parse_one(&item) {
            Ok(product) => {
                if let Some(existing) = index.get(&product.booth_id).copied() {
                    warnings.push(format!(
                        "商品ID {} が重複していたので、後の情報を使いました",
                        product.booth_id
                    ));
                    products[existing] = product;
                } else {
                    index.insert(product.booth_id.clone(), products.len());
                    products.push(product);
                }
            }
            Err(warning) => {
                skipped += 1;
                warnings.push(warning);
            }
        }
    }
    if warnings.len() > 30 {
        let extra = warnings.len() - 30;
        warnings.truncate(30);
        warnings.push(format!("ほか {extra} 件の注意があります"));
    }
    Ok(ParsedExport {
        products,
        warnings,
        skipped,
    })
}

fn parse_one(value: &Value) -> Result<IncomingProduct, String> {
    let Some(map) = value.as_object() else {
        return Err("商品データがオブジェクトではありません".into());
    };
    let mut booth_id = first_text(map, &["boothId", "booth_id", "id"]);
    if !valid_booth_id(&booth_id) {
        let url = first_text(map, &["url"]);
        booth_id = item_id_from_url(&url).unwrap_or_default();
    }
    if !valid_booth_id(&booth_id) {
        return Err("商品IDを特定できないデータをスキップしました".into());
    }
    let title = cap(&first_text(map, &["title", "name"]), 300);
    if title.is_empty() {
        return Err(format!("商品 {booth_id} はタイトルがないのでスキップしました"));
    }
    let url_text = cap(&first_text(map, &["url"]), 500);
    let url = if url_text.is_empty() {
        format!("https://booth.pm/ja/items/{booth_id}")
    } else {
        url_text
    };
    let thumbnail = first_text(map, &["thumbnailUrl", "thumbnail_url", "imageUrl", "image"]);
    let is_adult = optional_bool(map, &["isAdult", "is_adult"]);
    let (description_snippet, booth_category, booth_tags) = if is_adult == Some(true) {
        (String::new(), None, None)
    } else {
        (
            cap(
                &first_text(map, &["descriptionSnippet", "description_snippet", "description"]),
                8000,
            ),
            parse_booth_category(map),
            parse_booth_tags(map),
        )
    };
    Ok(IncomingProduct {
        booth_id,
        title,
        shop: cap(&first_text(map, &["shop", "shopName", "shop_name"]), 120),
        description_snippet,
        url,
        thumbnail_url: if thumbnail.is_empty() {
            None
        } else {
            Some(cap(&thumbnail, 800))
        },
        variations: parse_variations(map.get("variations")),
        downloads: parse_downloads(map.get("downloads").or_else(|| map.get("files"))),
        is_free: optional_bool(map, &["isFree", "is_free"]),
        is_adult,
        unpublished: optional_bool(map, &["unpublished"]),
        booth_category,
        booth_tags,
    })
}

fn parse_booth_category(map: &Map<String, Value>) -> Option<BoothCategoryInput> {
    if let Some(value) = map.get("category").or_else(|| map.get("boothCategory")) {
        if let Some(object) = value.as_object() {
            let parent = match object.get("parent") {
                Some(Value::String(text)) => cap(text.trim(), 80),
                Some(Value::Object(parent)) => cap(&first_text(parent, &["name"]), 80),
                _ => String::new(),
            };
            return Some(BoothCategoryInput {
                id: object.get("id").and_then(value_i64),
                name: cap(&first_text(object, &["name"]), 80),
                parent,
            });
        }
        if !value.is_null() {
            return None;
        }
    }
    let id = map
        .get("boothCategoryId")
        .or_else(|| map.get("booth_category_id"))
        .and_then(value_i64);
    let name = cap(
        &first_text(map, &["boothCategoryName", "booth_category_name"]),
        80,
    );
    let parent = cap(
        &first_text(map, &["boothCategoryParent", "booth_category_parent"]),
        80,
    );
    if id.is_none() && name.is_empty() && parent.is_empty() {
        None
    } else {
        Some(BoothCategoryInput { id, name, parent })
    }
}

fn parse_booth_tags(map: &Map<String, Value>) -> Option<Vec<String>> {
    let value = map
        .get("tags")
        .or_else(|| map.get("boothTags"))
        .or_else(|| map.get("booth_tags"))?;
    if value.is_null() {
        return None;
    }
    let Value::Array(items) = value else {
        return None;
    };
    let mut tags = Vec::new();
    for item in items {
        let name = match item {
            Value::String(text) => cap(text.trim(), 40),
            Value::Object(object) => cap(&first_text(object, &["name"]), 40),
            _ => continue,
        };
        if name.is_empty() || tags.iter().any(|existing| existing == &name) {
            continue;
        }
        tags.push(name);
        if tags.len() == 40 {
            break;
        }
    }
    Some(tags)
}

fn value_i64(value: &Value) -> Option<i64> {
    let id = match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }?;
    (1..1_000_000).contains(&id).then_some(id)
}

fn parse_variations(value: Option<&Value>) -> Vec<IncomingVariation> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    let mut variations: Vec<IncomingVariation> = Vec::new();
    for item in items {
        let (name, owned) = if let Some(text) = item.as_str() {
            (text.to_string(), true)
        } else if let Some(map) = item.as_object() {
            (
                first_text(map, &["name", "variation"]),
                optional_bool(map, &["owned"]).unwrap_or(false),
            )
        } else {
            continue;
        };
        let Some(name) = crate::library_files::clean_variation(&name) else {
            continue;
        };
        if let Some(existing) = variations.iter_mut().find(|row| row.name == name) {
            if owned {
                existing.owned = true;
            }
        } else {
            variations.push(IncomingVariation { name, owned });
        }
    }
    variations
}

fn parse_downloads(value: Option<&Value>) -> Vec<IncomingDownload> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    let mut downloads: Vec<IncomingDownload> = Vec::new();
    for item in items {
        let Some(map) = item.as_object() else {
            continue;
        };
        let downloadable_id = clean_downloadable_id(&first_text(
            map,
            &["downloadableId", "downloadable_id"],
        ));
        let Some(filename) = crate::library_files::safe_filename(&first_text(
            map,
            &["filename", "name"],
        ))
        .or_else(|| crate::library_files::safe_filename(&downloadable_id)) else {
            continue;
        };
        let variation = crate::library_files::clean_variation(&first_text(
            map,
            &["variation", "variationName"],
        ))
        .unwrap_or_default();
        let size_bytes = map
            .get("sizeBytes")
            .or_else(|| map.get("size"))
            .and_then(|value| match value {
                Value::Number(number) => number.as_u64(),
                Value::String(text) => text.trim().parse().ok(),
                _ => None,
            });
        if downloads.iter().any(|row| {
            if !downloadable_id.is_empty() && row.downloadable_id == downloadable_id {
                return true;
            }
            downloadable_id.is_empty()
                && row.downloadable_id.is_empty()
                && row.filename == filename
                && row.variation == variation
        }) {
            continue;
        }
        downloads.push(IncomingDownload {
            filename,
            size_bytes,
            variation,
            downloadable_id,
        });
    }
    downloads
}

fn clean_downloadable_id(value: &str) -> String {
    let id = value.trim();
    if (1..=12).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_digit()) {
        id.to_string()
    } else {
        String::new()
    }
}

fn optional_bool(map: &Map<String, Value>, keys: &[&str]) -> Option<bool> {
    for key in keys {
        let Some(value) = map.get(*key) else {
            continue;
        };
        return match value {
            Value::Bool(bit) => Some(*bit),
            Value::Null => None,
            Value::Number(number) => number.as_u64().map(|number| number != 0),
            Value::String(text) => match text.trim() {
                "true" | "1" => Some(true),
                "false" | "0" => Some(false),
                _ => None,
            },
            _ => None,
        };
    }
    None
}

fn first_text(map: &Map<String, Value>, keys: &[&str]) -> String {
    for key in keys {
        if let Some(value) = map.get(*key) {
            let text = match value {
                Value::String(text) => text.trim().to_string(),
                Value::Number(number) => number
                    .as_u64()
                    .map(|number| number.to_string())
                    .unwrap_or_default(),
                _ => String::new(),
            };
            if !text.is_empty() {
                return text;
            }
        }
    }
    String::new()
}

fn valid_booth_id(value: &str) -> bool {
    (4..=12).contains(&value.len()) && value.chars().all(|ch| ch.is_ascii_digit())
}

fn item_id_from_url(url: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"/items/(\d+)").unwrap());
    re.captures(url).map(|captures| captures[1].to_string())
}

fn cap(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wrapper_array_and_url_id() {
        let wrapped = r#"{
            "format": "vrclibman.booth-export",
            "products": [
                {"boothId": "4876459", "title": "ライム", "shop": "あまとうさぎ", "description": "説明", "url": "https://booth.pm/ja/items/4876459"}
            ]
        }"#;
        let parsed = parse_export(wrapped).unwrap();
        assert_eq!(parsed.products.len(), 1);
        assert_eq!(parsed.products[0].shop, "あまとうさぎ");

        let from_url = r#"[{"name": "服", "url": "https://booth.pm/ja/items/123456"}]"#;
        let parsed = parse_export(from_url).unwrap();
        assert_eq!(parsed.products[0].booth_id, "123456");
        assert_eq!(parsed.products[0].title, "服");
    }

    #[test]
    fn last_duplicate_wins_and_bad_rows_are_skipped() {
        let json = r#"[
            {"boothId": "10001", "title": "先"},
            {"boothId": "10001", "title": "後"},
            {"title": "IDなし"},
            {"boothId": "10002", "title": ""}
        ]"#;
        let parsed = parse_export(json).unwrap();
        assert_eq!(parsed.products.len(), 1);
        assert_eq!(parsed.products[0].title, "後");
        assert_eq!(parsed.skipped, 2);
    }

    #[test]
    fn full_library_export_shape_imports() {
        let json = r#"{
            "format": "vrclibman.booth-export",
            "version": 1,
            "exportedAt": "2026-10-21T00:00:00.000Z",
            "source": "library",
            "products": [
                {"boothId":"100001","title":"服A","shop":"工房","descriptionSnippet":"説明A","url":"https://booth.pm/ja/items/100001","thumbnailUrl":"https://booth.pximg.net/c/150x150/a.jpg"},
                {"boothId":"100002","title":"服B","shop":"工房","descriptionSnippet":"説明B","url":"https://booth.pm/ja/items/100002","thumbnailUrl":null}
            ]
        }"#;
        let parsed = parse_export(json).unwrap();
        assert_eq!(parsed.products.len(), 2);
        assert_eq!(parsed.skipped, 0);
        assert_eq!(parsed.products[0].description_snippet, "説明A");
        assert_eq!(parsed.products[1].title, "服B");
        assert!(parsed.products[1].thumbnail_url.is_none());

        let unpublished = r#"{
            "format": "vrclibman.booth-export",
            "version": 1,
            "source": "library",
            "products": [
                {"boothId":"6874323","title":"消えた商品","shop":"mokemoke-dy","descriptionSnippet":"公開終了（取得できず）","url":"https://booth.pm/ja/items/6874323","unavailable":"not-found"}
            ]
        }"#;
        let parsed = parse_export(unpublished).unwrap();
        assert_eq!(parsed.products.len(), 1);
        assert_eq!(parsed.products[0].description_snippet, "公開終了（取得できず）");
        assert_eq!(parsed.products[0].shop, "mokemoke-dy");
        assert_eq!(parsed.products[0].unpublished, None);
    }

    #[test]
    fn unpublished_flag_is_optional_and_separate_from_not_found() {
        let hidden = parse_export(
            r#"{"boothId":"424242","title":"ダミー衣装","unpublished":true,"isFree":true}"#,
        )
        .unwrap();
        assert_eq!(hidden.products[0].unpublished, Some(true));
        assert_eq!(hidden.products[0].is_free, Some(true));
        let cleared = parse_export(
            r#"{"boothId":"424242","title":"ダミー衣装","unpublished":false}"#,
        )
        .unwrap();
        assert_eq!(cleared.products[0].unpublished, Some(false));
        let omitted = parse_export(r#"{"boothId":"424242","title":"ダミー衣装"}"#).unwrap();
        assert_eq!(omitted.products[0].unpublished, None);
    }

    #[test]
    fn variations_downloads_and_flags_are_optional() {
        let json = r#"{
            "boothId": "424242",
            "title": "架空の服",
            "isFree": true,
            "isAdult": false,
            "variations": [
                {"name": "通常", "owned": true},
                {"name": "特典", "owned": false},
                {"name": "通常", "owned": false}
            ],
            "downloads": [
                {"filename": "body.unitypackage", "variation": "通常", "sizeBytes": 12},
                {"filename": "../secret.unitypackage", "variation": "通常"}
            ]
        }"#;
        let parsed = parse_export(json).unwrap();
        let product = &parsed.products[0];
        assert_eq!(product.is_free, Some(true));
        assert_eq!(product.is_adult, Some(false));
        assert_eq!(product.variations.len(), 2);
        assert!(product.variations.iter().any(|row| row.name == "通常" && row.owned));
        assert!(product.variations.iter().any(|row| row.name == "特典" && !row.owned));
        assert_eq!(product.downloads.len(), 1);
        assert_eq!(product.downloads[0].filename, "body.unitypackage");
        assert_eq!(product.downloads[0].size_bytes, Some(12));

        let omitted = parse_export(r#"[{"boothId":"424243","title":"説明だけ"}]"#).unwrap();
        assert!(omitted.products[0].variations.is_empty());
        assert!(omitted.products[0].downloads.is_empty());
        assert_eq!(omitted.products[0].is_free, None);
        assert_eq!(omitted.products[0].is_adult, None);
    }

    #[test]
    fn caps_long_descriptions() {
        let description = "あ".repeat(9000);
        let json = format!(r#"[{{"boothId":"10001","title":"服","description":"{description}"}}]"#);
        let parsed = parse_export(&json).unwrap();
        assert_eq!(parsed.products[0].description_snippet.chars().count(), 8000);
    }

    #[test]
    fn category_and_tags_are_kept_and_adult_payloads_drop_them() {
        let parsed = parse_export(
            r#"{"boothId":"424242","title":"ダミー衣装","description":"説明です","category":{"id":209,"name":"3D衣装","parent":{"name":"3Dモデル"}},"tags":[{"name":"VRChat"},"ダミー"]}"#,
        )
        .unwrap();
        let product = &parsed.products[0];
        let category = product.booth_category.as_ref().unwrap();
        assert_eq!(category.id, Some(209));
        assert_eq!(category.name, "3D衣装");
        assert_eq!(category.parent, "3Dモデル");
        assert_eq!(
            product.booth_tags.as_deref(),
            Some(["VRChat".to_string(), "ダミー".to_string()].as_slice())
        );
        let omitted = parse_export(r#"{"boothId":"424243","title":"説明だけ"}"#).unwrap();
        assert!(omitted.products[0].booth_category.is_none());
        assert!(omitted.products[0].booth_tags.is_none());
        let adult = parse_export(
            r#"{"boothId":"424244","title":"ダミー","is_adult":true,"description":"残してはいけない説明","category":{"id":209,"name":"3D衣装"},"tags":["ダミー"]}"#,
        )
        .unwrap();
        assert_eq!(adult.products[0].is_adult, Some(true));
        assert!(adult.products[0].description_snippet.is_empty());
        assert!(adult.products[0].booth_category.is_none());
        assert!(adult.products[0].booth_tags.is_none());
    }
}
