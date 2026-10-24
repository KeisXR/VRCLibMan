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
    pub owned: bool,
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
pub struct IncomingVariation {
    pub name: String,
    pub owned: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncomingDownload {
    pub filename: String,
    pub size_bytes: Option<u64>,
    pub variation: String,
    pub downloadable_id: String,
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
    pub variations: Vec<IncomingVariation>,
    pub downloads: Vec<IncomingDownload>,
    /// Missing means this payload does not know. It must not clear a stored flag.
    pub is_free: Option<bool>,
    pub is_adult: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariationFact {
    pub booth_id: String,
    pub name: String,
    pub owned: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadFact {
    pub booth_id: String,
    pub variation: String,
    pub filename: String,
    pub size_bytes: Option<u64>,
    pub downloaded: bool,
    pub local_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductFlag {
    pub booth_id: String,
    pub is_free: Option<bool>,
    pub is_adult: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FilePlace {
    Local,
    Remote,
    Missing,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacedDownload {
    pub booth_id: String,
    pub variation: String,
    pub filename: String,
    pub size_bytes: Option<u64>,
    pub state: FilePlace,
    pub local_path: Option<String>,
    pub remote_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadFinished {
    pub booth_id: String,
    pub path: String,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub is_free: Option<bool>,
    #[serde(default)]
    pub is_adult: Option<bool>,
    #[serde(default)]
    pub variation: String,
    #[serde(default)]
    pub title: String,
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
    pub variations: u32,
    pub downloads: u32,
    pub unnamed_downloads: u32,
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
    pub remote_library_folder: String,
    pub route_downloads: bool,
    pub llm_backend: String,
    pub background_mode: bool,
    pub launch_at_login: bool,
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
    #[serde(default)]
    pub remote_library_folder: String,
    #[serde(default)]
    pub route_downloads: bool,
    #[serde(default = "default_llm_backend")]
    pub llm_backend: String,
    #[serde(default)]
    pub background_mode: bool,
    #[serde(default)]
    pub launch_at_login: bool,
}

fn default_llm_backend() -> String {
    "ollama".into()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedLeftover {
    pub path: String,
    pub booth_id: String,
    pub filename: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaStatus {
    pub models: Vec<String>,
    pub model_ready: bool,
    pub model_size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogInfo {
    pub count: u32,
    pub fetched_on: String,
    pub upstream_updated: String,
    pub source_url: String,
    pub terms_url: String,
    pub author: String,
    pub credit: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRefresh {
    pub added: u32,
    pub total: u32,
    pub upstream_updated: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatus {
    pub complete: bool,
    pub model: String,
    pub default_model: String,
    pub backend: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarCandidate {
    pub name: String,
    pub product_count: u32,
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
