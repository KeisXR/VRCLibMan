use crate::model::{Category, Extraction};
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::sync::OnceLock;
use std::time::Duration;
use url::Url;

const SYSTEM_PROMPT: &str = "\
あなたはVRChat向けBooth商品の抽出器です。タイトルと説明文に書かれている事実だけをJSONで返します。\
書かれていないアバター名を推測で足してはいけません。

category は次のどれか一つです。
- avatar: アバター本体、または素体そのもの
- outfit: 衣装、服、靴、アクセサリ
- hair: 髪、ヘア、ウィッグ
- gimmick: ギミック、シェーダー、ツール、小物機能
- world: ワールド
- other: どれにも当てはまらない

compatible_avatars:
- 対応、専用、向け、共通素体として文中に挙がったアバター名を、文中の表記のまま入れる
- 商品自身がアバター本体のときは、そのアバター名を1件入れる
- ショップ名や作者名は入れない
- 重複は入れない
- 名前が一つも書かれていなければ空配列にする

is_generic は、汎用、どのアバターでも、素体を問わない、と書かれているときだけ true。\
特定アバター対応と併記されていても、汎用と明記されていれば true。

confidence は 0 から 1。対応アバターかカテゴリが文中ではっきり分かるとき 0.8 以上。\
推測が混ざるとき 0.5 以下。情報がほとんど無いとき 0.2 以下。";

pub fn validate_base_url(value: &str) -> Result<String, String> {
    let trimmed = value.trim().trim_end_matches('/');
    let url = Url::parse(trimmed).map_err(|_| "OllamaのURLが正しくありません".to_string())?;
    if url.scheme() != "http" {
        return Err("OllamaのURLは http://127.0.0.1:11434 の形式にしてください".into());
    }
    let host = url.host_str().unwrap_or("");
    if host != "127.0.0.1" && host != "localhost" && host != "[::1]" && host != "::1" {
        return Err("Ollamaは手元のPC（localhost）だけに接続します".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("OllamaのURLに認証情報は入れられません".into());
    }
    Ok(trimmed.to_string())
}

pub fn validate_model_name(value: &str) -> Result<String, String> {
    let name = value.trim();
    if name.is_empty() || name.len() > 80 {
        return Err("モデル名が空か、長すぎます".into());
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "._:-/".contains(ch))
    {
        return Err("モデル名に使えない文字があります".into());
    }
    Ok(name.to_string())
}

pub fn model_ready(models: &[String], wanted: &str) -> bool {
    models.iter().any(|name| {
        name == wanted
            || name
                .strip_prefix(wanted)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(':'))
    })
}

pub async fn list_models(client: &reqwest::Client, base: &str) -> Result<Vec<String>, String> {
    let base = validate_base_url(base)?;
    let response = client
        .get(format!("{base}/api/tags"))
        .send()
        .await
        .map_err(connection_error)?;
    let status = response.status();
    let text = response.text().await.map_err(connection_error)?;
    if !status.is_success() {
        return Err(format!("Ollamaのモデル一覧を取得できません: {status}"));
    }
    let parsed: TagsResponse = serde_json::from_str(&text)
        .map_err(|_| "Ollamaの応答を解釈できません".to_string())?;
    let mut names = Vec::new();
    for model in parsed.models.unwrap_or_default() {
        if let Some(name) = model.name {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

pub async fn extract_product(
    client: &reqwest::Client,
    base: &str,
    model: &str,
    title: &str,
    description: &str,
) -> Result<Extraction, String> {
    let base = validate_base_url(base)?;
    let model = validate_model_name(model)?;
    let description: String = if description.trim().is_empty() {
        "（説明文なし）".into()
    } else {
        description.chars().take(6000).collect()
    };
    let user = format!("タイトル: {title}\n説明文:\n{description}");
    let mut payload = serde_json::json!({
        "model": model,
        "stream": false,
        "think": false,
        "format": extraction_schema(),
        "options": {"temperature": 0.1},
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": user}
        ]
    });
    let first = post_chat(client, &base, &payload).await?;
    let text = if first.status == 400 {
        payload
            .as_object_mut()
            .expect("payload object")
            .remove("think");
        let second = post_chat(client, &base, &payload).await?;
        if !second.status.is_success() {
            return Err(ollama_status_error(second.status, &second.body));
        }
        second.body
    } else if !first.status.is_success() {
        return Err(ollama_status_error(first.status, &first.body));
    } else {
        first.body
    };
    let content = chat_content(&text)?;
    parse_extraction(&content)
}

pub fn parse_extraction(raw: &str) -> Result<Extraction, String> {
    let without_think = strip_think(raw);
    let json = json_object(&without_think).ok_or("抽出結果のJSONが見つかりません")?;
    let value: Value = serde_json::from_str(json)
        .map_err(|err| format!("抽出結果のJSONが壊れています: {err}"))?;
    let category = value
        .get("category")
        .and_then(Value::as_str)
        .and_then(Category::parse)
        .ok_or("category が不正です")?;
    let is_generic = value
        .get("is_generic")
        .and_then(Value::as_bool)
        .ok_or("is_generic が不正です")?;
    let confidence = value
        .get("confidence")
        .and_then(Value::as_f64)
        .ok_or("confidence が不正です")?;
    let mut names = Vec::new();
    let Some(list) = value.get("compatible_avatars").and_then(Value::as_array) else {
        return Err("compatible_avatars が不正です".into());
    };
    for item in list {
        let Some(name) = item.as_str() else {
            continue;
        };
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed.chars().count() > 80 {
            continue;
        }
        if !names.iter().any(|existing: &String| existing == trimmed) {
            names.push(trimmed.to_string());
        }
        if names.len() == 32 {
            break;
        }
    }
    Ok(Extraction {
        category,
        compatible_avatars: names,
        is_generic,
        confidence: confidence.clamp(0.0, 1.0),
    })
}

pub fn extraction_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|err| format!("HTTPクライアントを初期化できません: {err}"))
}

pub fn probe_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|err| format!("HTTPクライアントを初期化できません: {err}"))
}

struct ChatBody {
    status: reqwest::StatusCode,
    body: String,
}

async fn post_chat(client: &reqwest::Client, base: &str, payload: &Value) -> Result<ChatBody, String> {
    let response = client
        .post(format!("{base}/api/chat"))
        .json(payload)
        .send()
        .await
        .map_err(connection_error)?;
    let status = response.status();
    let body = response.text().await.map_err(connection_error)?;
    Ok(ChatBody { status, body })
}

fn chat_content(body: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|_| "Ollamaの応答を解釈できません".to_string())?;
    value
        .pointer("/message/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "Ollamaが空の応答を返しました".into())
}

fn extraction_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "category": {
                "type": "string",
                "enum": ["avatar", "outfit", "hair", "gimmick", "world", "other"]
            },
            "compatible_avatars": {
                "type": "array",
                "items": {"type": "string"}
            },
            "is_generic": {"type": "boolean"},
            "confidence": {"type": "number"}
        },
        "required": ["category", "compatible_avatars", "is_generic", "confidence"]
    })
}

fn strip_think(input: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?s)<think>.*?</think>").unwrap());
    re.replace_all(input, "").to_string()
}

fn json_object(input: &str) -> Option<&str> {
    let start = input.find('{')?;
    let end = input.rfind('}')?;
    if end < start {
        return None;
    }
    Some(&input[start..=end])
}

fn connection_error(err: reqwest::Error) -> String {
    if err.is_connect() {
        "Ollamaに接続できません。Ollamaを起動し、モデルを取得してください（例: ollama pull qwen3:4b）。"
            .into()
    } else if err.is_timeout() {
        "Ollamaの応答がタイムアウトしました".into()
    } else {
        format!("Ollamaへの通信に失敗しました: {err}")
    }
}

fn ollama_status_error(status: reqwest::StatusCode, body: &str) -> String {
    let detail: String = body.chars().take(240).collect();
    if status.as_u16() == 404 {
        format!("Ollamaにモデルがありません。設定のモデル名を確認してください。{detail}")
    } else {
        format!("Ollamaがエラーを返しました ({status}): {detail}")
    }
}

#[derive(Deserialize)]
struct TagsResponse {
    models: Option<Vec<TagModel>>,
}

#[derive(Deserialize)]
struct TagModel {
    name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_and_thinking_output() {
        let raw = "<think>候補を見る</think>\n```json\n{\"category\":\"outfit\",\"compatible_avatars\":[\"ライム\",\"ライム\"],\"is_generic\":false,\"confidence\":1.4}\n```";
        let parsed = parse_extraction(raw).unwrap();
        assert_eq!(parsed.category, Category::Outfit);
        assert_eq!(parsed.compatible_avatars, vec!["ライム".to_string()]);
        assert!((parsed.confidence - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn rejects_unknown_category() {
        let raw = r#"{"category":"shoes","compatible_avatars":[],"is_generic":false,"confidence":0.4}"#;
        assert!(parse_extraction(raw).is_err());
    }

    #[test]
    fn allows_only_loopback() {
        assert!(validate_base_url("http://127.0.0.1:11434").is_ok());
        assert!(validate_base_url("http://localhost:11434/").is_ok());
        assert!(validate_base_url("https://127.0.0.1:11434").is_err());
        assert!(validate_base_url("http://example.com:11434").is_err());
    }
}
