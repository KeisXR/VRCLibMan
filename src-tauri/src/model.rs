use serde::{Deserialize, Serialize};

/// Fresh databases only. `INSERT OR IGNORE` leaves a model the user already saved.
pub const DEFAULT_OLLAMA_MODEL: &str = "gemma4:12b";

/// Booth page and item JSON were both missing. Not a failed extraction.
pub const UNAVAILABLE_DESCRIPTION: &str = "公開終了（取得できず）";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Avatar,
    Outfit,
    Hair,
    Accessory,
    Texture,
    Gimmick,
    World,
    Other,
}

impl Category {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "avatar" => Some(Self::Avatar),
            "outfit" => Some(Self::Outfit),
            "hair" => Some(Self::Hair),
            "accessory" => Some(Self::Accessory),
            "texture" => Some(Self::Texture),
            "gimmick" => Some(Self::Gimmick),
            "world" => Some(Self::World),
            "other" => Some(Self::Other),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Avatar => "avatar",
            Self::Outfit => "outfit",
            Self::Hair => "hair",
            Self::Accessory => "accessory",
            Self::Texture => "texture",
            Self::Gimmick => "gimmick",
            Self::World => "world",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Product {
    pub booth_id: String,
    pub title: String,
    pub shop: String,
    pub description_snippet: String,
    pub url: String,
    pub thumbnail_url: Option<String>,
    pub thumbnail_path: Option<String>,
    pub category: Option<Category>,
    pub category_locked: bool,
    pub compatible_avatars: Vec<String>,
    pub unresolved_names: Vec<String>,
    pub is_generic: bool,
    pub confidence: Option<f64>,
    pub links_locked: bool,
    pub user_confirmed: bool,
    pub needs_review: bool,
    pub imported_at: String,
    pub updated_at: String,
    pub extracted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalFile {
    pub path: String,
    pub filename: String,
    pub is_dir: bool,
    pub size: u64,
    pub linked_product_id: Option<String>,
    pub candidate_product_id: Option<String>,
    pub match_score: f64,
    pub link_source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Avatar {
    pub id: String,
    pub display_name: String,
    pub base_body: Option<String>,
    pub aliases: Vec<String>,
    pub is_builtin: bool,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseBody {
    pub id: String,
    pub display_name: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub product_id: String,
    pub avatar_id: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncomingProduct {
    pub booth_id: String,
    pub title: String,
    pub shop: String,
    pub description_snippet: String,
    pub url: String,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Extraction {
    pub category: Category,
    pub compatible_avatars: Vec<String>,
    pub is_generic: bool,
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub inserted: u32,
    pub updated: u32,
    pub unchanged: u32,
    pub skipped: u32,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub indexed: u32,
    pub linked: u32,
    pub truncated: bool,
    pub root: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractReport {
    pub processed: u32,
    pub failed: u32,
    pub cancelled: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionLink {
    pub token: String,
    pub port: u16,
    pub listening: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub ollama_base_url: String,
    pub ollama_model: String,
    pub confidence_threshold: f64,
    pub library_folder: String,
    pub builtin_browser: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsInput {
    pub ollama_base_url: String,
    pub ollama_model: String,
    pub confidence_threshold: f64,
    pub library_folder: String,
    #[serde(default)]
    pub builtin_browser: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaStatus {
    pub models: Vec<String>,
    pub model_ready: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductUpdate {
    pub booth_id: String,
    pub category: Option<String>,
    pub is_generic: Option<bool>,
    pub avatar_ids: Option<Vec<String>>,
    pub confirm: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarInput {
    pub id: Option<String>,
    pub display_name: String,
    pub base_body: Option<String>,
    pub aliases: Vec<String>,
    pub notes: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseBodyInput {
    pub id: Option<String>,
    pub display_name: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Knowledge {
    pub avatars: Vec<Avatar>,
    pub base_bodies: Vec<BaseBody>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    pub products: u32,
    pub owned_avatars: u32,
    pub unextracted: u32,
    pub needs_review: u32,
    pub queue_count: u32,
    pub unlinked_files: u32,
    pub ollama_model: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobProgress {
    pub job: String,
    pub current: u32,
    pub total: u32,
    pub message: String,
}

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}
