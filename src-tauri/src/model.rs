use serde::{Deserialize, Serialize};

/// Fresh databases only. `INSERT OR IGNORE` leaves a model the user already saved.
pub const DEFAULT_OLLAMA_MODEL: &str = "qwen3.5:9b";

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
    pub genre: Option<String>,
    pub genre_locked: bool,
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
pub struct ManagedProduct {
    pub booth_id: String,
    pub title: String,
    pub shop: String,
    pub category: Option<Category>,
    pub excluded: bool,
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
    /// Library card badge「非公開」. Missing must not clear a stored flag.
    /// This is not「公開終了」 and must not force category other.
    pub unpublished: Option<bool>,
    /// Missing means this payload does not know. It must not clear a stored category.
    pub booth_category: Option<BoothCategoryInput>,
    /// Missing means this payload does not know. An empty list is a known empty tag list.
    pub booth_tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoothCategoryInput {
    pub id: Option<i64>,
    pub name: String,
    pub parent: String,
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
    pub unpublished: Option<bool>,
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
    pub genre: Option<String>,
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
pub struct OllamaLoadedModel {
    pub name: String,
    pub size_bytes: u64,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractReport {
    pub processed: u32,
    pub failed: u32,
    pub cancelled: bool,
    pub errors: Vec<String>,
    /// Decided from the BOOTH category id or name, and tags when those were enough.
    pub booth_settled: u32,
    /// Sent to Ollama or Apple because the category and tags did not decide it.
    pub model_used: u32,
    /// Age-restricted products left unclassified because the cloud classifier was
    /// selected and no local model was ready.
    pub cloud_withheld: u32,
}

/// Result of the cloud classifier connection test. Never carries the key.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudTestResult {
    pub ok: bool,
    pub kind: String,
    pub message: String,
    pub model_listed: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationStatus {
    pub classifier_version: i32,
    pub stale_count: u32,
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
    /// `external` (system browser) or `builtin` (in-app browser tab).
    pub booth_link_target: String,
    pub remote_library_folder: String,
    pub remote_auto_sync: bool,
    pub route_downloads: bool,
    pub llm_backend: String,
    /// OpenAI-compatible base URL for the opt-in cloud classifier. The key is never here.
    pub cloud_base_url: String,
    pub cloud_model: String,
    pub background_mode: bool,
    pub launch_at_login: bool,
    pub classification_dismissed_version: i32,
    pub ui_rail_width: u32,
    pub ui_genre_width: u32,
    pub preview_pane: bool,
    pub preview_pane_width: u32,
    pub check_update_on_startup: bool,
    pub ui_update_dismissed_version: String,
    pub unload_ollama_after_classification: bool,
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
    #[serde(default = "default_booth_link_target")]
    pub booth_link_target: String,
    #[serde(default)]
    pub remote_library_folder: String,
    #[serde(default)]
    pub remote_auto_sync: bool,
    #[serde(default)]
    pub route_downloads: bool,
    #[serde(default = "default_llm_backend")]
    pub llm_backend: String,
    #[serde(default = "default_cloud_base_url")]
    pub cloud_base_url: String,
    #[serde(default)]
    pub cloud_model: String,
    #[serde(default)]
    pub background_mode: bool,
    #[serde(default)]
    pub launch_at_login: bool,
    #[serde(default)]
    pub classification_dismissed_version: i32,
    #[serde(default = "default_ui_rail_width")]
    pub ui_rail_width: u32,
    #[serde(default = "default_ui_genre_width")]
    pub ui_genre_width: u32,
    #[serde(default = "default_preview_pane")]
    pub preview_pane: bool,
    #[serde(default = "default_preview_pane_width")]
    pub preview_pane_width: u32,
    #[serde(default)]
    pub check_update_on_startup: bool,
    #[serde(default)]
    pub ui_update_dismissed_version: String,
    #[serde(default = "default_unload_ollama_after_classification")]
    pub unload_ollama_after_classification: bool,
}

fn default_unload_ollama_after_classification() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatch {
    pub ollama_base_url: Option<String>,
    pub ollama_model: Option<String>,
    pub confidence_threshold: Option<f64>,
    pub library_folder: Option<String>,
    pub builtin_browser: Option<bool>,
    pub booth_link_target: Option<String>,
    pub remote_library_folder: Option<String>,
    pub remote_auto_sync: Option<bool>,
    pub route_downloads: Option<bool>,
    pub llm_backend: Option<String>,
    pub cloud_base_url: Option<String>,
    pub cloud_model: Option<String>,
    pub background_mode: Option<bool>,
    pub launch_at_login: Option<bool>,
    pub classification_dismissed_version: Option<i32>,
    pub ui_rail_width: Option<u32>,
    pub ui_genre_width: Option<u32>,
    #[serde(default)]
    pub preview_pane: Option<bool>,
    pub preview_pane_width: Option<u32>,
    pub check_update_on_startup: Option<bool>,
    pub ui_update_dismissed_version: Option<String>,
    pub unload_ollama_after_classification: Option<bool>,
}

impl SettingsPatch {
    pub fn apply(&self, current: Settings) -> SettingsInput {
        SettingsInput {
            ollama_base_url: self
                .ollama_base_url
                .clone()
                .unwrap_or(current.ollama_base_url),
            ollama_model: self.ollama_model.clone().unwrap_or(current.ollama_model),
            confidence_threshold: self
                .confidence_threshold
                .unwrap_or(current.confidence_threshold),
            library_folder: self
                .library_folder
                .clone()
                .unwrap_or(current.library_folder),
            builtin_browser: self.builtin_browser.unwrap_or(current.builtin_browser),
            booth_link_target: self
                .booth_link_target
                .clone()
                .unwrap_or(current.booth_link_target),
            remote_library_folder: self
                .remote_library_folder
                .clone()
                .unwrap_or(current.remote_library_folder),
            remote_auto_sync: self.remote_auto_sync.unwrap_or(current.remote_auto_sync),
            route_downloads: self.route_downloads.unwrap_or(current.route_downloads),
            llm_backend: self.llm_backend.clone().unwrap_or(current.llm_backend),
            cloud_base_url: self
                .cloud_base_url
                .clone()
                .unwrap_or(current.cloud_base_url),
            cloud_model: self.cloud_model.clone().unwrap_or(current.cloud_model),
            background_mode: self.background_mode.unwrap_or(current.background_mode),
            launch_at_login: self.launch_at_login.unwrap_or(current.launch_at_login),
            classification_dismissed_version: self
                .classification_dismissed_version
                .unwrap_or(current.classification_dismissed_version),
            ui_rail_width: self.ui_rail_width.unwrap_or(current.ui_rail_width),
            ui_genre_width: self.ui_genre_width.unwrap_or(current.ui_genre_width),
            preview_pane: self.preview_pane.unwrap_or(current.preview_pane),
            preview_pane_width: clamp_preview_pane_width(
                self.preview_pane_width.unwrap_or(current.preview_pane_width),
            ),
            check_update_on_startup: self
                .check_update_on_startup
                .unwrap_or(current.check_update_on_startup),
            ui_update_dismissed_version: self
                .ui_update_dismissed_version
                .clone()
                .unwrap_or(current.ui_update_dismissed_version),
            unload_ollama_after_classification: self
                .unload_ollama_after_classification
                .unwrap_or(current.unload_ollama_after_classification),
        }
    }
}

fn default_llm_backend() -> String {
    "ollama".into()
}

fn default_cloud_base_url() -> String {
    crate::cloud::DEFAULT_BASE_URL.into()
}

fn default_booth_link_target() -> String {
    "external".into()
}

fn default_ui_rail_width() -> u32 {
    232
}

fn default_ui_genre_width() -> u32 {
    200
}

fn default_preview_pane() -> bool {
    false
}

fn default_preview_pane_width() -> u32 {
    300
}

pub(crate) fn clamp_ui_rail_width(value: u32) -> u32 {
    value.clamp(180, 360)
}

pub(crate) fn clamp_ui_genre_width(value: u32) -> u32 {
    value.clamp(160, 320)
}

pub(crate) fn clamp_preview_pane_width(value: u32) -> u32 {
    value.clamp(240, 1600)
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

/// One avatar link captured so a manual placement can be undone.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotLink {
    pub avatar_id: String,
    pub source: String,
}

/// Owned-flag change that belongs with a classification snapshot.
/// `owned` is the value to restore. A placeholder created for this placement
/// is removed when `remove_if_unused` is set and nothing else still links to it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OwnedAvatarRestore {
    pub id: String,
    pub display_name: String,
    pub aliases: Vec<String>,
    pub owned: bool,
    pub remove_if_unused: bool,
}

/// Classification fields captured before a manual genre move, so the move can be undone.
/// A product is pinned when both `category_locked` and `genre_locked` are set.
/// Link and ownership fields are set only when the placement changed them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationSnapshot {
    pub booth_id: String,
    pub category: Option<String>,
    pub genre: Option<String>,
    pub category_locked: bool,
    pub genre_locked: bool,
    pub user_confirmed: bool,
    pub needs_review: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links_locked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<SnapshotLink>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owned_avatars: Vec<OwnedAvatarRestore>,
    /// Extraction names from before this placement. Absent on a genre move,
    /// so restoring that move leaves the name lists alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatible_avatars: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unresolved_names: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_generic: Option<bool>,
}

/// Result of registering a misclassified product as an owned avatar body.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RegisterOwnedBodyResult {
    Registered {
        snapshot: ClassificationSnapshot,
        avatar_id: String,
        display_name: String,
    },
    NeedsName {
        suggested_name: String,
        candidates: Vec<String>,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductUpdate {
    pub booth_id: String,
    pub category: Option<String>,
    pub genre: Option<String>,
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
