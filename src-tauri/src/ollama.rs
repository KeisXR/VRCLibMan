use crate::model::{Category, Extraction, OllamaLoadedModel};
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::sync::OnceLock;
use std::time::Duration;
use url::Url;

pub(crate) const SYSTEM_PROMPT: &str = "\
あなたはVRChat向けBooth商品の抽出器です。タイトルと説明文に書かれている事実だけをJSONで返します。\
書かれていないアバター名を推測で足してはいけません。

genre は商品カテゴリの下位ジャンルです。指定された enum から最も近いものを返します。

category は次のどれか一つです。
- avatar: アバター本体。タイトルか説明に「オリジナル3Dモデル」とある商品。配布形式を並べたもの（.xavater / .vroid / .unitypackage のうち2つ以上、Editable model data、【VRChat / 3D model】）も本体。単なる「3Dモデル」や「VRChat向け3Dモデル」、衣装・アクセサリは含めない
- outfit: 衣装、服、靴、部屋着、ルームウェア、パーカー、ずぼん、ズボン、techwear、fullpack
- hair: 髪、ヘア、ウィッグ、ポニーテール
- accessory: メガネ、ピアス、イヤリング、ネックレス、指輪、リング、帽子、バッグ、ヘイロー、ディスク、カード
- texture: 瞳・目・メイク・肌のテクスチャ。衣装やギミックにしない
- gimmick: アバター用のギミック、表情、シェイプキー、フェイストラッキング、なでなで、キス、電話、キット、SPS、アバター用シェーダー、ツール。家具や布団は含めない
- world: ワールド本体、家具や小物（こたつ、毛布、布団、家具）、QvPenなどの差し替えモデル、見た目のスイッチ、ASMRなどのワールドギミック、Udon、UdonSharp、ワールド用シェーダー、ワールドプレハブ、VRChatのCPUや起動を扱う補助ツール。アバター用のギミックは gimmick
- other: どれにも当てはまらない。もちふぃった～のようなアバター編集ツールは other で、genre は tool_editor

compatible_avatars:
- 対応先は「対応アバター」「対応モデル」の欄から取る。タイトルの「25アバター対応」は人数であり、名前ではない
- 【半額セール中】【新作】【オリジナル3Dモデル】のような宣伝や種別の括弧は名前ではない
- 【プラム専用】のように専用・対応の括弧の中にある名前は入れる
- 欄の1行が1体。ショップ名、URL、括弧内のシェイプキー、作者名（様で終わるクレジット）は入れない
- ver や「版」が名前に付いていても、その名前を省略しない。「クルミ ver1.2」は「クルミ ver1.2」のまま1件として返す
- 商品自身がアバター本体のときは、そのアバター名を1件だけ入れる。説明に並ぶ兄弟の名前は入れない
- Modular Avatar、QvPen、lilToon、Poiyomi、PhysBone、シェーダー名はアバターではない
- 名前が一つも書かれていなければ空配列にする

is_generic は、汎用、どのアバターでも、素体を問わない、と書かれているときだけ true。ワールドは false。

例:
- 「ライム -Lime- / オリジナル3Dモデル」「ライムのオリジナル3Dモデルです」→ avatar, [\"ライム\"], false
- 「【プラム専用】Hairグラデーション」→ hair, [\"プラム\"], false
- 「Noctilume EyeTexture」→ texture, [], false
- 「ネオンヘイロー」「Sample Earring」「磁気ディスク」→ accessory, [], false
- 「表情アニメーション」「表情ギミック」「なでなで」「Sample Phone」→ gimmick, [], false
- 「Face Tracking」「シェイプキー」→ gimmick, genre は expression_animation, [], false
- 「どのアバターでも使えるペンライト」→ gimmick, [], true
- 「サンプルこたつ」「毛布」→ world, genre は furniture_props, [], false
- 「QvPen差し替えモデル」→ world, genre は texture_material, [], false
- 「もちふぃった～」→ other, genre は tool_editor, [], false
- 「ワールド用の小物」「Udonのスイッチ」「ワールド用シェーダー」「World Prefab」→ world, [], false
- 「【半額セール中】真冬 Mafuyu / オリジナル3Dモデル」→ avatar, [\"真冬\"], false
- 「クルミ ver1.2 対応のワンピース」→ outfit, [\"クルミ ver1.2\"], false

入力にある「BOOTHカテゴリ:」と「タグ:」はヒントです。説明文と食い違うときは説明文を優先します。「なし」のときはその行を使いません。

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledModel {
    pub name: String,
    pub size_bytes: u64,
}

pub fn model_ready(models: &[InstalledModel], wanted: &str) -> bool {
    selected_model(models, wanted).is_some()
}

pub fn selected_model<'a>(
    models: &'a [InstalledModel],
    wanted: &str,
) -> Option<&'a InstalledModel> {
    let matches = |name: &str| {
        name == wanted
            || name
                .strip_prefix(wanted)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(':'))
    };
    models
        .iter()
        .find(|model| model.name == wanted)
        .or_else(|| models.iter().find(|model| matches(&model.name)))
}

pub fn installed_size(models: &[InstalledModel], wanted: &str) -> Option<u64> {
    selected_model(models, wanted)
        .map(|model| model.size_bytes)
        .filter(|size| *size > 0)
}

pub fn parse_installed_models(text: &str) -> Result<Vec<InstalledModel>, String> {
    let parsed: TagsResponse =
        serde_json::from_str(text).map_err(|_| "Ollamaの応答を解釈できません".to_string())?;
    let mut models = Vec::new();
    for model in parsed.models.unwrap_or_default() {
        let Some(name) = model.name.filter(|name| !name.is_empty()) else {
            continue;
        };
        models.push(InstalledModel {
            name,
            size_bytes: model.size.unwrap_or(0),
        });
    }
    models.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(models)
}

pub fn parse_loaded_models(text: &str) -> Result<Vec<OllamaLoadedModel>, String> {
    let parsed: PsResponse =
        serde_json::from_str(text).map_err(|_| "Ollamaの応答を解釈できません".to_string())?;
    let mut models = Vec::new();
    for model in parsed.models.unwrap_or_default() {
        let name = model.name.trim();
        if name.is_empty() {
            continue;
        }
        models.push(OllamaLoadedModel {
            name: name.to_string(),
            size_bytes: model.size.unwrap_or(0),
            expires_at: model.expires_at.filter(|value| !value.is_empty()),
        });
    }
    models.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(models)
}

pub async fn loaded_models(
    client: &reqwest::Client,
    base: &str,
) -> Result<Vec<OllamaLoadedModel>, String> {
    let base = validate_base_url(base)?;
    let response = client
        .get(format!("{base}/api/ps"))
        .send()
        .await
        .map_err(connection_error)?;
    let status = response.status();
    let text = response.text().await.map_err(connection_error)?;
    if !status.is_success() {
        return Err(format!("Ollamaの読み込み状態を取得できません: {status}"));
    }
    parse_loaded_models(&text)
}

pub async fn unload_model(client: &reqwest::Client, base: &str, model: &str) -> Result<(), String> {
    let base = validate_base_url(base)?;
    let model = validate_model_name(model)?;
    let response = client
        .post(format!("{base}/api/generate"))
        .json(&serde_json::json!({
            "model": model,
            "keep_alive": 0
        }))
        .send()
        .await
        .map_err(connection_error)?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "Ollamaのモデルをアンロードできません: {status} {body}"
        ));
    }
    Ok(())
}

pub async fn list_models(
    client: &reqwest::Client,
    base: &str,
) -> Result<Vec<InstalledModel>, String> {
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
    parse_installed_models(&text)
}

pub async fn extract_product(
    client: &reqwest::Client,
    base: &str,
    model: &str,
    title: &str,
    description: &str,
    category_line: &str,
    tags_line: &str,
) -> Result<Extraction, String> {
    let base = validate_base_url(base)?;
    let model = validate_model_name(model)?;
    let description = clip_for_model(description);
    let user = format!(
        "タイトル: {title}\nBOOTHカテゴリ: {category_line}\nタグ: {tags_line}\n説明文:\n{description}"
    );
    let mut payload = serde_json::json!({
        "model": model,
        "stream": false,
        "think": false,
        "format": extraction_schema(),
        "options": {"temperature": 0.1, "num_ctx": 8192},
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": user}
        ]
    });
    let first = post_chat(client, &base, &payload).await?;
    let text = if first.status == 400 {
        let object = payload.as_object_mut().expect("payload object");
        object.remove("think");
        if let Some(options) = object.get_mut("options").and_then(Value::as_object_mut) {
            options.remove("num_ctx");
        }
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
    let json = json_object(&without_think)
        .ok_or("分類結果を読み取れませんでした。もう一度分類してください")?;
    let value: Value = serde_json::from_str(json)
        .map_err(|err| format!("分類結果を読み取れませんでした: {err}"))?;
    let category_text = value
        .get("category")
        .and_then(Value::as_str)
        .map(|text| text.trim().to_ascii_lowercase());
    let category = category_text
        .as_deref()
        .and_then(Category::parse)
        .ok_or("分類結果の商品カテゴリを読み取れませんでした。もう一度分類してください")?;
    let is_generic = value
        .get("is_generic")
        .and_then(Value::as_bool)
        .ok_or("分類結果の汎用・専用の区別を読み取れませんでした。もう一度分類してください")?;
    let confidence = value
        .get("confidence")
        .and_then(Value::as_f64)
        .ok_or("分類結果の確信度を読み取れませんでした。もう一度分類してください")?;
    let mut names = Vec::new();
    let list = match value.get("compatible_avatars") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(list)) => list.clone(),
        Some(_) => {
            return Err(
                "分類結果の対応アバターを読み取れませんでした。もう一度分類してください".into(),
            );
        }
    };
    for item in &list {
        let Some(name) = item.as_str() else {
            continue;
        };
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed.chars().count() > 80 {
            continue;
        }
        if crate::mask::is_known_tool(trimmed) {
            continue;
        }
        if !names.iter().any(|existing: &String| existing == trimmed) {
            names.push(trimmed.to_string());
        }
        if names.len() == 32 {
            break;
        }
    }
    let genre = value
        .get("genre")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(Extraction {
        category,
        genre,
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

pub async fn pull_model(
    client: &reqwest::Client,
    base: &str,
    model: &str,
    mut on_progress: impl FnMut(u64, u64, &str),
) -> Result<(), String> {
    let base = validate_base_url(base)?;
    let model = validate_model_name(model)?;
    let mut response = client
        .post(format!("{base}/api/pull"))
        .json(&serde_json::json!({ "model": model, "stream": true }))
        .send()
        .await
        .map_err(connection_error)?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(ollama_status_error(status, &body));
    }
    let mut pending = String::new();
    while let Some(chunk) = response.chunk().await.map_err(connection_error)? {
        pending.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(newline) = pending.find('\n') {
            let line: String = pending.drain(..=newline).collect();
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if let Some(error) = value.get("error").and_then(Value::as_str) {
                return Err(format!("モデルを取得できません: {error}"));
            }
            let status = value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("取得中");
            let completed = value.get("completed").and_then(Value::as_u64).unwrap_or(0);
            let total = value.get("total").and_then(Value::as_u64).unwrap_or(0);
            on_progress(completed, total, status);
        }
    }
    Ok(())
}

pub fn model_download_status(status: &str) -> &'static str {
    if status.starts_with("pulling manifest") {
        "モデルの情報を確認しています"
    } else if status.starts_with("pulling ") {
        "モデルをダウンロードしています"
    } else if status.starts_with("verifying ") {
        "ダウンロードしたモデルを確認しています"
    } else if status.starts_with("writing ") || status.starts_with("removing ") {
        "モデルを保存しています"
    } else if status == "success" {
        "モデルを取得しました"
    } else {
        "モデルを取得しています"
    }
}

pub fn pull_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|err| format!("HTTPクライアントを初期化できません: {err}"))
}

pub fn catalog_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(60))
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

async fn post_chat(
    client: &reqwest::Client,
    base: &str,
    payload: &Value,
) -> Result<ChatBody, String> {
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
    let value: Value =
        serde_json::from_str(body).map_err(|_| "Ollamaの応答を解釈できません".to_string())?;
    let content = value
        .pointer("/message/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if !content.is_empty() {
        return Ok(content.to_string());
    }
    // Some models still put the JSON in the thinking channel when content is empty.
    if let Some(thinking) = value
        .pointer("/message/thinking")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| json_object(text).is_some())
    {
        return Ok(thinking.to_string());
    }
    Err("Ollamaが空の応答を返しました".into())
}

fn extraction_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "category": {
                "type": "string",
                "enum": ["avatar", "outfit", "hair", "accessory", "texture", "gimmick", "world", "other"]
            },
            "genre": {"type": "string", "enum": ["outfit", "shoes_socks", "hair", "accessory", "head_ears_tail", "makeup_skin", "eye_texture", "expression_animation", "pose", "gimmick", "particle_effect", "shader", "tool_editor", "other", "world", "furniture_props", "world_gimmick", "texture_material", "skybox", "lighting_postprocess", "world_shader", "building_terrain", "audio_bgm", "tool_other"]},
            "compatible_avatars": {
                "type": "array",
                "items": {"type": "string"}
            },
            "is_generic": {"type": "boolean"},
            "confidence": {"type": "number"}
        },
        "required": ["category", "genre", "compatible_avatars", "is_generic", "confidence"]
    })
}

fn clip_for_model(description: &str) -> String {
    crate::compat::clip_description(description, 4000)
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
        "Ollamaに接続できません。Ollamaを起動し、モデルを取得してください（例: ollama pull qwen3.5:9b）。"
            .into()
    } else if err.is_timeout() {
        "Ollamaの応答がタイムアウトしました".into()
    } else {
        format!("Ollamaへの通信に失敗しました: {err}")
    }
}

fn ollama_status_error(status: reqwest::StatusCode, body: &str) -> String {
    let detail = ollama_error_detail(body);
    if status.as_u16() == 404 {
        format!("Ollamaにモデルがありません。設定のモデル名を確認してください。{detail}")
    } else if detail.is_empty() {
        format!("Ollamaがエラーを返しました ({status})")
    } else {
        format!("Ollamaがエラーを返しました ({status}): {detail}")
    }
}

/// Keep the short `error` field. The raw body can echo the prompt, so it is not logged.
fn ollama_error_detail(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(Value::as_str)
                .map(|text| text.chars().take(120).collect())
        })
        .unwrap_or_default()
}

#[derive(Deserialize)]
struct TagsResponse {
    models: Option<Vec<TagModel>>,
}

#[derive(Deserialize)]
struct TagModel {
    name: Option<String>,
    size: Option<u64>,
}

#[derive(Deserialize)]
struct PsResponse {
    models: Option<Vec<PsModel>>,
}

#[derive(Deserialize)]
struct PsModel {
    name: String,
    size: Option<u64>,
    expires_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qwen_candidates_use_structured_output_without_thinking() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let address = server.server_addr().to_ip().unwrap();
        let worker = std::thread::spawn(move || {
            for model in ["qwen3.5:4b", "qwen3.5:9b"] {
                let mut request = server
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap()
                    .unwrap();
                assert_eq!(request.url(), "/api/chat");
                let mut body = String::new();
                request.as_reader().read_to_string(&mut body).unwrap();
                let payload: Value = serde_json::from_str(&body).unwrap();
                assert_eq!(payload["model"], model);
                let user = payload["messages"][1]["content"].as_str().unwrap();
                assert!(user.contains("BOOTHカテゴリ: 3D衣装（親: 3Dモデル）"));
                assert!(user.contains("タグ: ダミー"));
                assert!(user.contains("説明文:\nテスト用の汎用衣装"));
                assert!(!user.starts_with("説明文:"));
                assert_eq!(payload["think"], false);
                assert_eq!(payload["format"]["type"], "object");
                let answer = serde_json::json!({"message": {"content": "{\"category\":\"outfit\",\"compatible_avatars\":[],\"is_generic\":true,\"confidence\":0.9}"}});
                request
                    .respond(tiny_http::Response::from_string(answer.to_string()))
                    .unwrap();
            }
        });
        tauri::async_runtime::block_on(async {
            let client = extraction_client().unwrap();
            for model in ["qwen3.5:4b", "qwen3.5:9b"] {
                let result = extract_product(
                    &client,
                    &format!("http://{address}"),
                    model,
                    "ダミー衣装",
                    "テスト用の汎用衣装",
                    "3D衣装（親: 3Dモデル）",
                    "ダミー",
                )
                .await
                .unwrap();
                assert_eq!(result.category, Category::Outfit);
                assert!(result.is_generic);
            }
        });
        worker.join().unwrap();
    }

    #[test]
    fn installed_size_comes_from_the_selected_model() {
        let parsed = parse_installed_models(
            r#"{"models":[
                {"name":"qwen3:4b","size":2497295778},
                {"name":"gemma4:12b","size":8147483648},
                {"name":"other:latest","size":0}
            ]}"#,
        )
        .unwrap();
        assert_eq!(installed_size(&parsed, "qwen3:4b"), Some(2497295778));
        assert_eq!(installed_size(&parsed, "gemma4:12b"), Some(8147483648));
        assert_eq!(installed_size(&parsed, "gemma4"), Some(8147483648));
        assert_eq!(installed_size(&parsed, "missing:7b"), None);
        assert_eq!(installed_size(&parsed, "other:latest"), None);
        assert!(model_ready(&parsed, "qwen3:4b"));
        assert!(!model_ready(&parsed, "missing:7b"));
    }

    #[test]
    fn parses_fenced_and_thinking_output() {
        let raw = "<think>候補を見る</think>\n```json\n{\"category\":\"outfit\",\"compatible_avatars\":[\"ライム\",\"ライム\"],\"is_generic\":false,\"confidence\":1.4}\n```";
        let parsed = parse_extraction(raw).unwrap();
        assert_eq!(parsed.category, Category::Outfit);
        assert_eq!(parsed.compatible_avatars, vec!["ライム".to_string()]);
        let kept = parse_extraction(
            r#"{"category":"outfit","compatible_avatars":["クルミ ver1.2","lilToon","Modular Avatar"],"is_generic":false,"confidence":0.8}"#,
        )
        .unwrap();
        assert_eq!(kept.compatible_avatars, vec!["クルミ ver1.2".to_string()]);
        assert!((parsed.confidence - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn schema_offers_world_without_changing_the_default_model() {
        let schema = extraction_schema();
        let categories = schema["properties"]["category"]["enum"]
            .as_array()
            .expect("category enum");
        assert!(categories.iter().any(|value| value == "world"));
        assert!(SYSTEM_PROMPT.contains("ワールド"));
        assert!(SYSTEM_PROMPT.contains("UdonSharp"));
        assert!(SYSTEM_PROMPT.contains("furniture_props"));
        assert!(!SYSTEM_PROMPT.contains("ワールド用の小物は gimmick"));
        assert!(!SYSTEM_PROMPT.contains("こたつ」「表情"));
        assert_eq!(crate::model::DEFAULT_OLLAMA_MODEL, "qwen3.5:9b");
    }

    #[test]
    fn empty_content_uses_thinking_json_and_a_loose_category_still_parses() {
        let body = r#"{"message":{"content":"","thinking":"考え中\n{\"category\":\"Outfit\",\"compatible_avatars\":null,\"is_generic\":true,\"confidence\":0.8}"}}"#;
        let parsed = parse_extraction(&chat_content(body).unwrap()).unwrap();
        assert_eq!(parsed.category, Category::Outfit);
        assert!(parsed.compatible_avatars.is_empty());
        assert!(chat_content(r#"{"message":{"content":"  "}}"#).unwrap_err().contains("空の応答"));
        let detail = ollama_error_detail(
            r#"{"error":"model failed","prompt":"タイトル: 架空の題\n説明文:\n残してはいけない"}"#,
        );
        assert_eq!(detail, "model failed");
        assert!(!detail.contains("架空"));
        assert!(!detail.contains("説明文"));
    }

    #[test]
    fn rejects_unknown_category() {
        let raw =
            r#"{"category":"shoes","compatible_avatars":[],"is_generic":false,"confidence":0.4}"#;
        assert!(parse_extraction(raw).is_err());
    }

    #[test]
    fn parses_loaded_models_from_ps() {
        let parsed = parse_loaded_models(
            r#"{"models":[{"name":"qwen3.5:9b","size":7000000000,"expires_at":"2026-10-27T12:00:00Z"}]}"#,
        )
        .unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "qwen3.5:9b");
        assert_eq!(parsed[0].size_bytes, 7_000_000_000);
        assert_eq!(
            parsed[0].expires_at.as_deref(),
            Some("2026-10-27T12:00:00Z")
        );
    }

    #[test]
    fn allows_only_loopback() {
        assert!(validate_base_url("http://127.0.0.1:11434").is_ok());
        assert!(validate_base_url("http://localhost:11434/").is_ok());
        assert!(validate_base_url("https://127.0.0.1:11434").is_err());
        assert!(validate_base_url("http://example.com:11434").is_err());
    }
}
