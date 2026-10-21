use crate::model::IncomingProduct;
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
    Ok(IncomingProduct {
        booth_id,
        title,
        shop: cap(&first_text(map, &["shop", "shopName", "shop_name"]), 120),
        description_snippet: cap(
            &first_text(map, &["descriptionSnippet", "description_snippet", "description"]),
            8000,
        ),
        url,
        thumbnail_url: if thumbnail.is_empty() {
            None
        } else {
            Some(cap(&thumbnail, 800))
        },
    })
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
    fn caps_long_descriptions() {
        let description = "あ".repeat(9000);
        let json = format!(r#"[{{"boothId":"10001","title":"服","description":"{description}"}}]"#);
        let parsed = parse_export(&json).unwrap();
        assert_eq!(parsed.products[0].description_snippet.chars().count(), 8000);
    }
}
