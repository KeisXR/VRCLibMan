use crate::model::{
    Avatar, BaseBody, Category, DownloadFact, FilePlace, LibraryStats, Link, LocalFile,
    PlacedDownload, Product, ProductFlag, VariationFact,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub products: Vec<Product>,
    pub files: Vec<LocalFile>,
    pub avatars: Vec<Avatar>,
    pub base_bodies: Vec<BaseBody>,
    pub links: Vec<Link>,
    pub variations: Vec<VariationFact>,
    pub download_rows: Vec<DownloadFact>,
    pub placed_downloads: Vec<PlacedDownload>,
    pub product_flags: Vec<ProductFlag>,
    pub remote_configured: bool,
    pub remote_checked: bool,
    pub remote_reachable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileBrief {
    pub path: String,
    pub filename: String,
    pub is_dir: bool,
    pub size: u64,
    pub match_score: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariationView {
    pub name: String,
    pub owned: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadView {
    pub variation: String,
    pub filename: String,
    pub size_bytes: Option<u64>,
    pub state: FilePlace,
    pub local_path: Option<String>,
    pub remote_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    pub booth_id: String,
    pub title: String,
    pub shop: String,
    pub url: String,
    pub description_snippet: String,
    pub thumbnail_url: Option<String>,
    pub thumbnail_path: Option<String>,
    pub category: Option<Category>,
    pub genre: Option<String>,
    /// True when both category and genre were locked by a manual placement.
    pub classification_pinned: bool,
    pub is_generic: bool,
    pub confidence: Option<f64>,
    pub via_base_body: bool,
    pub local_files: Vec<FileBrief>,
    pub local_file_count: u32,
    pub variations: Vec<VariationView>,
    pub downloads: Vec<DownloadView>,
    pub is_free: bool,
    pub is_adult: bool,
    /// Library badge「非公開」. False when the flag was never set. Not「公開終了」.
    pub unpublished: bool,
    pub remote_configured: bool,
    pub remote_checked: bool,
    pub remote_reachable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenreProducts {
    pub items: Vec<ItemView>,
    pub counts: HashMap<String, u32>,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProductProbe {
    pub booth_id: String,
    pub remote_checked: bool,
    pub remote_reachable: bool,
    pub downloads: Vec<DownloadView>,
}

pub fn remote_product_probes(
    snapshot: &Snapshot,
    probes: &[crate::library_files::ProductRemoteProbe],
) -> Vec<RemoteProductProbe> {
    probes
        .iter()
        .map(|probe| RemoteProductProbe {
            booth_id: probe.booth_id.clone(),
            remote_checked: probe.remote_checked,
            remote_reachable: probe.remote_reachable,
            downloads: download_views(snapshot, &probe.booth_id),
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    pub category: String,
    pub items: Vec<ItemView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sibling {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarCard {
    pub id: String,
    pub display_name: String,
    pub base_body: Option<String>,
    pub base_body_name: Option<String>,
    pub aliases: Vec<String>,
    pub is_builtin: bool,
    pub is_placeholder: bool,
    pub owned: bool,
    pub thumbnail_path: Option<String>,
    pub thumbnail_url: Option<String>,
    pub outfit_count: u32,
    pub hair_count: u32,
    pub gimmick_count: u32,
    pub generic_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarDetail {
    pub id: String,
    pub display_name: String,
    pub base_body: Option<String>,
    pub base_body_name: Option<String>,
    pub base_body_note: Option<String>,
    pub aliases: Vec<String>,
    pub is_builtin: bool,
    pub is_placeholder: bool,
    pub owned: bool,
    pub siblings: Vec<Sibling>,
    pub owned_products: Vec<ItemView>,
    pub sections: Vec<Section>,
    pub generic_items: Vec<ItemView>,
    pub unsupported_outfits: Vec<ItemView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueProduct {
    pub booth_id: String,
    pub title: String,
    pub shop: String,
    pub url: String,
    pub description_snippet: String,
    pub thumbnail_url: Option<String>,
    pub thumbnail_path: Option<String>,
    pub category: Option<Category>,
    pub genre: Option<String>,
    pub classification_pinned: bool,
    pub is_generic: bool,
    pub confidence: Option<f64>,
    pub compatible_avatars: Vec<String>,
    pub unresolved_names: Vec<String>,
    pub linked_avatar_ids: Vec<String>,
    pub user_confirmed: bool,
    /// local, partial, missing, or unknown. Remote is filled on the avatar page after a probe.
    pub file_presence: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueFile {
    pub path: String,
    pub filename: String,
    pub is_dir: bool,
    pub size: u64,
    pub match_score: f64,
    pub candidate_product_id: Option<String>,
    pub candidate_title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuePage {
    pub unextracted: u32,
    pub needs_review: u32,
    pub unlinked_files: u32,
    pub products: Vec<QueueProduct>,
    pub files: Vec<QueueFile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductOption {
    pub booth_id: String,
    pub title: String,
    pub shop: String,
    pub category: Option<Category>,
}

struct Classified {
    thumbnail_path: Option<String>,
    thumbnail_url: Option<String>,
    owned: Vec<ItemView>,
    sections: Vec<Section>,
    generic: Vec<ItemView>,
}

pub fn avatar_cards(snapshot: &Snapshot) -> Vec<AvatarCard> {
    let mut cards = Vec::new();
    for avatar in &snapshot.avatars {
        if !avatar.owned && !referenced_only(snapshot, &avatar.id) {
            continue;
        }
        let classified = classify(snapshot, avatar);
        cards.push(AvatarCard {
            id: avatar.id.clone(),
            display_name: avatar.display_name.clone(),
            base_body: avatar.base_body.clone(),
            base_body_name: base_name(snapshot, avatar.base_body.as_deref()),
            aliases: avatar.aliases.clone(),
            is_builtin: avatar.is_builtin,
            is_placeholder: avatar.id.starts_with("owned-"),
            owned: avatar.owned,
            thumbnail_path: classified.thumbnail_path,
            thumbnail_url: classified.thumbnail_url,
            outfit_count: section_len(&classified.sections, "outfit"),
            hair_count: section_len(&classified.sections, "hair"),
            gimmick_count: section_len(&classified.sections, "gimmick"),
            generic_count: classified.generic.len() as u32,
        });
    }
    cards
}

pub fn avatar_detail(snapshot: &Snapshot, id: &str) -> Option<AvatarDetail> {
    let avatar = snapshot.avatars.iter().find(|avatar| avatar.id == id)?;
    let classified = classify(snapshot, avatar);
    let mut siblings: Vec<Sibling> = snapshot
        .avatars
        .iter()
        .filter(|other| other.id != avatar.id && other.owned && shared_base(avatar, other))
        .map(|other| Sibling {
            id: other.id.clone(),
            display_name: other.display_name.clone(),
        })
        .collect();
    siblings.sort_by(|left, right| left.display_name.cmp(&right.display_name));
    let base = avatar
        .base_body
        .as_deref()
        .and_then(|base_id| snapshot.base_bodies.iter().find(|body| body.id == base_id));
    Some(AvatarDetail {
        id: avatar.id.clone(),
        display_name: avatar.display_name.clone(),
        base_body: avatar.base_body.clone(),
        base_body_name: base.map(|body| body.display_name.clone()),
        base_body_note: base.map(|body| body.note.clone()),
        aliases: avatar.aliases.clone(),
        is_builtin: avatar.is_builtin,
        is_placeholder: avatar.id.starts_with("owned-"),
        owned: avatar.owned,
        siblings,
        owned_products: classified.owned,
        sections: classified.sections,
        generic_items: classified.generic,
        unsupported_outfits: outfits_for_adjustment(snapshot, avatar),
    })
}

pub fn products_by_ids(snapshot: &Snapshot, ids: &[String]) -> Vec<ItemView> {
    ids.iter()
        .filter_map(|id| {
            snapshot
                .products
                .iter()
                .find(|product| product.booth_id == *id)
        })
        .map(|product| item_view(snapshot, product, false))
        .collect()
}

/// `all_ids` is every product of the kind, in list order.
/// `selected` is the browse key (`None` lists every product of the kind).
/// A genre missing or outside `genres` is counted under `genre::UNSET`.
/// Undownloaded free products that are not classification-pinned leave that
/// genre and are counted only under `genre::FREE`.
pub fn genre_products(
    snapshot: &Snapshot,
    genres: &[(&str, &str)],
    all_ids: &[String],
    selected: Option<&str>,
) -> GenreProducts {
    let wanted: HashSet<&str> = all_ids.iter().map(String::as_str).collect();
    let mut counts: HashMap<String, u32> = HashMap::new();
    let mut buckets: HashMap<&str, String> = HashMap::new();
    let mut total = 0;
    for product in snapshot
        .products
        .iter()
        .filter(|product| wanted.contains(product.booth_id.as_str()))
    {
        total += 1;
        let key = browse_bucket(snapshot, product, genres);
        *counts.entry(key.clone()).or_insert(0) += 1;
        buckets.insert(product.booth_id.as_str(), key);
    }
    let ids: Vec<String> = all_ids
        .iter()
        .filter(|id| match selected {
            None => buckets.contains_key(id.as_str()),
            Some(genre) => buckets.get(id.as_str()).map(String::as_str) == Some(genre),
        })
        .cloned()
        .collect();
    GenreProducts {
        items: products_by_ids(snapshot, &ids),
        counts,
        total,
    }
}

fn browse_bucket(snapshot: &Snapshot, product: &Product, genres: &[(&str, &str)]) -> String {
    if free_shelf(snapshot, product) {
        return crate::genre::FREE.to_string();
    }
    product
        .genre
        .as_deref()
        .filter(|genre| genres.iter().any(|(known, _)| known == genre))
        .unwrap_or(crate::genre::UNSET)
        .to_string()
}

/// Free products stay out of the ordinary genres until a local copy exists.
/// A manual placement (both locks) keeps the genre the user chose.
fn free_shelf(snapshot: &Snapshot, product: &Product) -> bool {
    if product.category_locked && product.genre_locked {
        return false;
    }
    let free = snapshot
        .product_flags
        .iter()
        .find(|flag| flag.booth_id == product.booth_id)
        .and_then(|flag| flag.is_free)
        .unwrap_or(false);
    free && !product_has_local_copy(snapshot, &product.booth_id)
}

/// Same local signal the item card uses: a download placed locally, a linked
/// file that is not one of those downloads, or the stored downloaded flag
/// when placement has not been filled in yet.
fn product_has_local_copy(snapshot: &Snapshot, booth_id: &str) -> bool {
    let downloads = download_views(snapshot, booth_id);
    if downloads.iter().any(|row| row.state == FilePlace::Local) {
        return true;
    }
    let names: HashSet<&str> = downloads.iter().map(|row| row.filename.as_str()).collect();
    if snapshot.files.iter().any(|file| {
        file.linked_product_id.as_deref() == Some(booth_id)
            && !names.contains(file.filename.as_str())
    }) {
        return true;
    }
    let placed = snapshot
        .placed_downloads
        .iter()
        .any(|row| row.booth_id == booth_id);
    if placed {
        return false;
    }
    snapshot
        .download_rows
        .iter()
        .any(|row| row.booth_id == booth_id && (row.downloaded || row.local_path.is_some()))
}

fn file_presence(snapshot: &Snapshot, booth_id: &str) -> String {
    let rows: Vec<&DownloadFact> = snapshot
        .download_rows
        .iter()
        .filter(|row| row.booth_id == booth_id)
        .collect();
    let names: HashSet<&str> = rows.iter().map(|row| row.filename.as_str()).collect();
    let extra_local = snapshot
        .files
        .iter()
        .filter(|file| {
            !file.is_dir
                && file.linked_product_id.as_deref() == Some(booth_id)
                && !names.contains(file.filename.as_str())
        })
        .count();
    let local_rows = rows
        .iter()
        .filter(|row| row.downloaded || row.local_path.is_some())
        .count();
    let missing = rows.len().saturating_sub(local_rows);
    let has_local = local_rows > 0 || extra_local > 0;
    if rows.is_empty() && extra_local == 0 {
        return "unknown".into();
    }
    if has_local && missing > 0 {
        return "partial".into();
    }
    if has_local {
        return "local".into();
    }
    "missing".into()
}

pub fn queue_page(snapshot: &Snapshot, kind: &str, offset: u32, limit: u32) -> QueuePage {
    let unextracted = snapshot
        .products
        .iter()
        .filter(|product| product.category.is_none())
        .count() as u32;
    let needs_review = snapshot
        .products
        .iter()
        .filter(|product| product.needs_review && product.category.is_some())
        .count() as u32;
    let unlinked_files = snapshot
        .files
        .iter()
        .filter(|file| file.linked_product_id.is_none())
        .count() as u32;
    let mut products = Vec::new();
    let mut files = Vec::new();
    if kind == "files" {
        let mut rows: Vec<&LocalFile> = snapshot
            .files
            .iter()
            .filter(|file| file.linked_product_id.is_none())
            .collect();
        rows.sort_by(|left, right| left.filename.cmp(&right.filename));
        let titles: HashMap<&str, &str> = snapshot
            .products
            .iter()
            .map(|product| (product.booth_id.as_str(), product.title.as_str()))
            .collect();
        files = rows
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .map(|file| QueueFile {
                path: file.path.clone(),
                filename: file.filename.clone(),
                is_dir: file.is_dir,
                size: file.size,
                match_score: file.match_score,
                candidate_product_id: file.candidate_product_id.clone(),
                candidate_title: file
                    .candidate_product_id
                    .as_deref()
                    .and_then(|id| titles.get(id).copied())
                    .map(str::to_string),
            })
            .collect();
    } else {
        let mut rows: Vec<&Product> = snapshot
            .products
            .iter()
            .filter(|product| match kind {
                "unextracted" => product.category.is_none(),
                _ => product.needs_review && product.category.is_some(),
            })
            .collect();
        rows.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        products = rows
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .map(|product| QueueProduct {
                booth_id: product.booth_id.clone(),
                title: product.title.clone(),
                shop: product.shop.clone(),
                url: product.url.clone(),
                description_snippet: product.description_snippet.clone(),
                thumbnail_url: product.thumbnail_url.clone(),
                thumbnail_path: product.thumbnail_path.clone(),
                category: product.category,
                genre: product.genre.clone(),
                classification_pinned: product.category_locked && product.genre_locked,
                is_generic: product.is_generic,
                confidence: product.confidence,
                compatible_avatars: product.compatible_avatars.clone(),
                unresolved_names: product.unresolved_names.clone(),
                linked_avatar_ids: snapshot
                    .links
                    .iter()
                    .filter(|link| link.product_id == product.booth_id)
                    .map(|link| link.avatar_id.clone())
                    .collect(),
                user_confirmed: product.user_confirmed,
                file_presence: file_presence(snapshot, &product.booth_id),
            })
            .collect();
    }
    QueuePage {
        unextracted,
        needs_review,
        unlinked_files,
        products,
        files,
    }
}

pub fn library_stats(snapshot: &Snapshot, model: &str) -> LibraryStats {
    let unextracted = snapshot
        .products
        .iter()
        .filter(|product| product.category.is_none())
        .count() as u32;
    let needs_review = snapshot
        .products
        .iter()
        .filter(|product| product.needs_review && product.category.is_some())
        .count() as u32;
    LibraryStats {
        products: snapshot.products.len() as u32,
        owned_avatars: snapshot
            .avatars
            .iter()
            .filter(|avatar| avatar.owned)
            .count() as u32,
        unextracted,
        needs_review,
        queue_count: unextracted + needs_review,
        unlinked_files: snapshot
            .files
            .iter()
            .filter(|file| file.linked_product_id.is_none())
            .count() as u32,
        ollama_model: model.to_string(),
    }
}

pub fn product_options(snapshot: &Snapshot) -> Vec<ProductOption> {
    let mut options: Vec<ProductOption> = snapshot
        .products
        .iter()
        .map(|product| ProductOption {
            booth_id: product.booth_id.clone(),
            title: product.title.clone(),
            shop: product.shop.clone(),
            category: product.category,
        })
        .collect();
    options.sort_by(|left, right| left.title.cmp(&right.title));
    options
}

fn classify(snapshot: &Snapshot, avatar: &Avatar) -> Classified {
    let family = family_ids(snapshot, avatar);
    let mut specific: Vec<(String, ItemView)> = Vec::new();
    let mut generic = Vec::new();
    let mut owned = Vec::new();
    for product in &snapshot.products {
        if product.category == Some(Category::World) {
            continue;
        }
        let linked: HashSet<&str> = snapshot
            .links
            .iter()
            .filter(|link| link.product_id == product.booth_id)
            .map(|link| link.avatar_id.as_str())
            .collect();
        let in_family = linked.iter().any(|id| family.contains(*id));
        if product.category == Some(Category::Avatar) {
            if linked.contains(avatar.id.as_str()) {
                owned.push(item_view(snapshot, product, false));
            }
            continue;
        }
        if in_family {
            let key = product
                .category
                .map(Category::as_str)
                .unwrap_or("uncategorized")
                .to_string();
            specific.push((
                key,
                item_view(snapshot, product, !linked.contains(avatar.id.as_str())),
            ));
        } else if product.is_generic && linked.is_empty() {
            generic.push(item_view(snapshot, product, false));
        }
    }
    specific.sort_by(|left, right| {
        left.1
            .title
            .cmp(&right.1.title)
            .then_with(|| left.1.booth_id.cmp(&right.1.booth_id))
    });
    generic.sort_by(|left, right| left.title.cmp(&right.title));
    let thumbnail_path = owned.iter().find_map(|body| body.thumbnail_path.clone());
    let thumbnail_url = owned.iter().find_map(|body| body.thumbnail_url.clone());
    let mut sections = Vec::new();
    for key in [
        "outfit",
        "hair",
        "accessory",
        "texture",
        "gimmick",
        "other",
        "uncategorized",
    ] {
        let items: Vec<ItemView> = specific
            .iter()
            .filter(|(category, _)| category == key)
            .map(|(_, item)| item.clone())
            .collect();
        if !items.is_empty() {
            sections.push(Section {
                category: key.to_string(),
                items,
            });
        }
    }
    Classified {
        thumbnail_path,
        thumbnail_url,
        owned,
        sections,
        generic,
    }
}

fn outfits_for_adjustment(snapshot: &Snapshot, avatar: &Avatar) -> Vec<ItemView> {
    let family = family_ids(snapshot, avatar);
    let linked: HashSet<&str> = snapshot
        .links
        .iter()
        .map(|link| link.product_id.as_str())
        .collect();
    let supported: HashSet<&str> = snapshot
        .links
        .iter()
        .filter(|link| family.contains(&link.avatar_id))
        .map(|link| link.product_id.as_str())
        .collect();
    let mut outfits: Vec<_> = snapshot
        .products
        .iter()
        .filter(|product| {
            product.category == Some(Category::Outfit)
                && !supported.contains(product.booth_id.as_str())
                && !(product.is_generic && !linked.contains(product.booth_id.as_str()))
        })
        .map(|product| item_view(snapshot, product, false))
        .collect();
    outfits.sort_by(|left, right| {
        left.title
            .cmp(&right.title)
            .then_with(|| left.booth_id.cmp(&right.booth_id))
    });
    outfits
}

fn item_view(snapshot: &Snapshot, product: &Product, via_base_body: bool) -> ItemView {
    let mut files: Vec<FileBrief> = snapshot
        .files
        .iter()
        .filter(|file| file.linked_product_id.as_deref() == Some(product.booth_id.as_str()))
        .map(|file| FileBrief {
            path: file.path.clone(),
            filename: file.filename.clone(),
            is_dir: file.is_dir,
            size: file.size,
            match_score: file.match_score,
        })
        .collect();
    let downloads = download_views(snapshot, &product.booth_id);
    let download_names: HashSet<&str> = downloads
        .iter()
        .map(|file| file.filename.as_str())
        .collect();
    files.retain(|file| !download_names.contains(file.filename.as_str()));
    let local_file_count = files.len() as u32;
    files.truncate(8);
    let flag = snapshot
        .product_flags
        .iter()
        .find(|flag| flag.booth_id == product.booth_id);
    ItemView {
        booth_id: product.booth_id.clone(),
        title: product.title.clone(),
        shop: product.shop.clone(),
        url: product.url.clone(),
        description_snippet: product.description_snippet.chars().take(500).collect(),
        thumbnail_url: product.thumbnail_url.clone(),
        thumbnail_path: product.thumbnail_path.clone(),
        category: product.category,
        genre: product.genre.clone(),
        classification_pinned: product.category_locked && product.genre_locked,
        is_generic: product.is_generic,
        confidence: product.confidence,
        via_base_body,
        local_files: files,
        local_file_count,
        variations: variation_views(snapshot, &product.booth_id),
        downloads,
        is_free: flag.and_then(|flag| flag.is_free).unwrap_or(false),
        is_adult: flag.and_then(|flag| flag.is_adult).unwrap_or(false),
        unpublished: flag.and_then(|flag| flag.unpublished).unwrap_or(false),
        remote_configured: snapshot.remote_configured,
        remote_checked: snapshot.remote_checked,
        remote_reachable: snapshot.remote_reachable,
    }
}

fn variation_views(snapshot: &Snapshot, booth_id: &str) -> Vec<VariationView> {
    let mut rows: Vec<VariationView> = snapshot
        .variations
        .iter()
        .filter(|row| row.booth_id == booth_id)
        .map(|row| VariationView {
            name: row.name.clone(),
            owned: row.owned,
        })
        .collect();
    rows.sort_by(|left, right| {
        right
            .owned
            .cmp(&left.owned)
            .then_with(|| left.name.cmp(&right.name))
    });
    rows
}

fn download_views(snapshot: &Snapshot, booth_id: &str) -> Vec<DownloadView> {
    let mut rows: Vec<DownloadView> = snapshot
        .placed_downloads
        .iter()
        .filter(|row| row.booth_id == booth_id)
        .map(|row| DownloadView {
            variation: row.variation.clone(),
            filename: row.filename.clone(),
            size_bytes: row.size_bytes,
            state: row.state,
            local_path: row.local_path.clone(),
            remote_path: row.remote_path.clone(),
        })
        .collect();
    if rows.is_empty() {
        rows = snapshot
            .download_rows
            .iter()
            .filter(|row| row.booth_id == booth_id)
            .map(|row| DownloadView {
                variation: row.variation.clone(),
                filename: row.filename.clone(),
                size_bytes: row.size_bytes,
                state: FilePlace::Missing,
                local_path: row.local_path.clone(),
                remote_path: None,
            })
            .collect();
    }
    rows.sort_by(|left, right| left.filename.cmp(&right.filename));
    rows
}

fn family_ids(snapshot: &Snapshot, avatar: &Avatar) -> HashSet<String> {
    let mut ids = HashSet::new();
    ids.insert(avatar.id.clone());
    if let Some(base) = avatar.base_body.as_ref().filter(|base| !base.is_empty()) {
        for other in &snapshot.avatars {
            if other.base_body.as_ref() == Some(base) {
                ids.insert(other.id.clone());
            }
        }
    }
    ids
}

fn referenced_only(snapshot: &Snapshot, avatar_id: &str) -> bool {
    snapshot.links.iter().any(|link| {
        link.avatar_id == avatar_id
            && snapshot.products.iter().any(|product| {
                product.booth_id == link.product_id
                    && !matches!(product.category, Some(Category::Avatar | Category::World))
            })
    })
}

fn shared_base(left: &Avatar, right: &Avatar) -> bool {
    match (&left.base_body, &right.base_body) {
        (Some(left_base), Some(right_base)) => !left_base.is_empty() && left_base == right_base,
        _ => false,
    }
}

fn base_name(snapshot: &Snapshot, id: Option<&str>) -> Option<String> {
    let id = id?;
    snapshot
        .base_bodies
        .iter()
        .find(|body| body.id == id)
        .map(|body| body.display_name.clone())
}

fn section_len(sections: &[Section], key: &str) -> u32 {
    sections
        .iter()
        .find(|section| section.category == key)
        .map(|section| section.items.len() as u32)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn product(id: &str, title: &str, category: Option<Category>, generic: bool) -> Product {
        Product {
            booth_id: id.into(),
            title: title.into(),
            shop: "shop".into(),
            description_snippet: String::new(),
            url: format!("https://booth.pm/ja/items/{id}"),
            thumbnail_url: None,
            thumbnail_path: None,
            category,
            category_locked: false,
            genre: Some("other".into()),
            genre_locked: false,
            compatible_avatars: Vec::new(),
            unresolved_names: Vec::new(),
            is_generic: generic,
            confidence: Some(0.9),
            links_locked: false,
            user_confirmed: false,
            needs_review: false,
            imported_at: String::new(),
            updated_at: id.into(),
            extracted_at: Some("t".into()),
        }
    }

    fn avatar(id: &str, name: &str, base: Option<&str>) -> Avatar {
        Avatar {
            id: id.into(),
            display_name: name.into(),
            base_body: base.map(str::to_string),
            aliases: Vec::new(),
            is_builtin: true,
            owned: matches!(id, "lime" | "plum" | "manuka"),
            notes: String::new(),
        }
    }

    fn link(product: &str, avatar: &str) -> Link {
        Link {
            product_id: product.into(),
            avatar_id: avatar.into(),
            source: "extracted".into(),
        }
    }

    fn sample() -> Snapshot {
        Snapshot {
            products: vec![
                product("1", "ライム", Some(Category::Avatar), false),
                product("2", "プラム", Some(Category::Avatar), false),
                product("3", "マヌカ", Some(Category::Avatar), false),
                product("4", "ライム対応ワンピース", Some(Category::Outfit), false),
                product("5", "汎用ギミック", Some(Category::Gimmick), true),
                product("6", "しなの専用ヘア", Some(Category::Hair), false),
                product("7", "しなの専用ギミック", Some(Category::Gimmick), true),
            ],
            files: Vec::new(),
            avatars: vec![
                avatar("lime", "ライム", Some("komado")),
                avatar("plum", "プラム", Some("komado")),
                avatar("manuka", "マヌカ", None),
                avatar("shinano", "しなの", None),
            ],
            base_bodies: vec![BaseBody {
                id: "komado".into(),
                display_name: "あまとうさぎ共通素体".into(),
                note: "note".into(),
            }],
            links: vec![
                link("1", "lime"),
                link("2", "plum"),
                link("3", "manuka"),
                link("4", "lime"),
                link("6", "shinano"),
                link("7", "shinano"),
            ],
            variations: Vec::new(),
            download_rows: Vec::new(),
            placed_downloads: Vec::new(),
            product_flags: Vec::new(),
            remote_configured: false,
            remote_checked: true,
            remote_reachable: false,
        }
    }

    #[test]
    fn shared_base_shows_outfit_without_owning_unrelated_avatars() {
        let snapshot = sample();
        let cards = avatar_cards(&snapshot);
        let ids: HashSet<_> = cards.iter().map(|card| card.id.as_str()).collect();
        assert_eq!(ids, HashSet::from(["lime", "plum", "manuka", "shinano"]));
        assert!(cards.iter().find(|card| card.id == "lime").unwrap().owned);
        assert!(
            !cards
                .iter()
                .find(|card| card.id == "shinano")
                .unwrap()
                .owned
        );

        let lime = avatar_detail(&snapshot, "lime").unwrap();
        assert!(lime.sections.iter().any(|section| {
            section.category == "outfit"
                && section
                    .items
                    .iter()
                    .any(|item| item.booth_id == "4" && !item.via_base_body)
        }));
        assert!(lime.generic_items.iter().any(|item| item.booth_id == "5"));
        assert!(!lime.generic_items.iter().any(|item| item.booth_id == "7"));
        assert!(lime.siblings.iter().any(|sibling| sibling.id == "plum"));

        let plum = avatar_detail(&snapshot, "plum").unwrap();
        let outfit = plum
            .sections
            .iter()
            .flat_map(|section| &section.items)
            .find(|item| item.booth_id == "4")
            .unwrap();
        assert!(outfit.via_base_body);

        let manuka = avatar_detail(&snapshot, "manuka").unwrap();
        assert!(
            !manuka
                .sections
                .iter()
                .flat_map(|section| &section.items)
                .any(|item| item.booth_id == "4" || item.booth_id == "6")
        );
        assert!(manuka.generic_items.iter().any(|item| item.booth_id == "5"));
    }

    #[test]
    fn owned_body_includes_download_rows() {
        let mut snapshot = sample();
        snapshot.remote_configured = true;
        snapshot.placed_downloads.push(PlacedDownload {
            booth_id: "1".into(),
            variation: String::new(),
            filename: "body.unitypackage".into(),
            size_bytes: Some(12),
            state: FilePlace::Missing,
            local_path: None,
            remote_path: None,
        });
        let lime = avatar_detail(&snapshot, "lime").unwrap();
        let body = lime
            .owned_products
            .iter()
            .find(|item| item.booth_id == "1")
            .unwrap();
        assert_eq!(body.category, Some(Category::Avatar));
        assert!(!body.via_base_body);
        assert!(body.remote_configured);
        assert_eq!(body.downloads.len(), 1);
        assert_eq!(body.downloads[0].filename, "body.unitypackage");
        assert_eq!(body.downloads[0].state, FilePlace::Missing);
        assert!(lime
            .sections
            .iter()
            .flat_map(|section| &section.items)
            .all(|item| item.booth_id != "1"));
    }

    #[test]
    fn worlds_stay_in_the_world_library_and_never_create_avatar_entries() {
        let mut snapshot = sample();
        snapshot
            .avatars
            .push(avatar("world-only", "ワールド専用の名前", None));
        for (id, title, generic) in [
            ("8", "汎用ワールド", true),
            ("10", "対応付きワールド", false),
            ("11", "ワールドだけ", true),
        ] {
            snapshot
                .products
                .push(product(id, title, Some(Category::World), generic));
        }
        snapshot
            .links
            .extend([link("10", "lime"), link("11", "world-only")]);
        snapshot
            .products
            .push(product("9", "表情ギミック", Some(Category::Gimmick), false));
        snapshot.links.push(link("9", "lime"));
        for id in ["lime", "plum", "manuka", "world-only"] {
            let detail = avatar_detail(&snapshot, id).unwrap();
            assert!(
                detail
                    .sections
                    .iter()
                    .all(|section| section.category != "world")
            );
            assert!(
                detail
                    .generic_items
                    .iter()
                    .all(|item| item.category != Some(Category::World))
            );
            assert!(
                detail
                    .unsupported_outfits
                    .iter()
                    .all(|item| item.category != Some(Category::World))
            );
        }
        let lime = avatar_detail(&snapshot, "lime").unwrap();
        assert!(
            lime.sections
                .iter()
                .any(|section| section.category == "gimmick"
                    && section.items.iter().any(|item| item.booth_id == "9"))
        );
        assert!(lime.generic_items.iter().any(|item| item.booth_id == "5"));
        assert!(
            avatar_cards(&snapshot)
                .iter()
                .all(|card| card.id != "world-only")
        );
        let worlds = products_by_ids(&snapshot, &["8".into(), "10".into(), "11".into()]);
        assert_eq!(worlds.len(), 3);
        assert!(
            worlds
                .iter()
                .all(|item| item.category == Some(Category::World))
        );
    }

    #[test]
    fn other_outfits_are_separate_from_direct_shared_body_and_generic_matches() {
        let mut snapshot = sample();
        for (id, title, category, generic) in [
            ("12", "マヌカ専用衣装", Some(Category::Outfit), false),
            ("13", "複数アバター対応衣装", Some(Category::Outfit), false),
            ("14", "対応先未登録の衣装", Some(Category::Outfit), false),
            ("15", "汎用衣装", Some(Category::Outfit), true),
            ("16", "しなの用の汎用衣装", Some(Category::Outfit), true),
            ("17", "未分類の商品", None, false),
            ("18", "その他の商品", Some(Category::Other), false),
        ] {
            snapshot
                .products
                .push(product(id, title, category, generic));
        }
        snapshot.links.extend([
            link("12", "manuka"),
            link("13", "lime"),
            link("13", "manuka"),
            link("16", "shinano"),
        ]);
        for id in ["lime", "plum"] {
            let detail = avatar_detail(&snapshot, id).unwrap();
            let ids: HashSet<_> = detail
                .unsupported_outfits
                .iter()
                .map(|item| item.booth_id.as_str())
                .collect();
            assert_eq!(ids, HashSet::from(["12", "14", "16"]));
            assert!(
                detail
                    .unsupported_outfits
                    .iter()
                    .all(|item| !item.via_base_body)
            );
            let supported: HashSet<_> = detail
                .sections
                .iter()
                .flat_map(|section| &section.items)
                .map(|item| item.booth_id.as_str())
                .collect();
            assert!(supported.contains("4") && supported.contains("13"));
            assert!(
                detail
                    .generic_items
                    .iter()
                    .any(|item| item.booth_id == "15")
            );
            let card = avatar_cards(&snapshot)
                .into_iter()
                .find(|card| card.id == id)
                .unwrap();
            assert_eq!(card.outfit_count, 2);
        }
        let manuka = avatar_detail(&snapshot, "manuka").unwrap();
        let ids: HashSet<_> = manuka
            .unsupported_outfits
            .iter()
            .map(|item| item.booth_id.as_str())
            .collect();
        assert_eq!(ids, HashSet::from(["4", "14", "16"]));
        assert_eq!(
            snapshot
                .links
                .iter()
                .filter(|link| link.product_id == "12")
                .map(|link| link.avatar_id.as_str())
                .collect::<Vec<_>>(),
            vec!["manuka"]
        );
    }

    #[test]
    fn unsupported_outfits_have_stable_title_and_id_order() {
        let mut snapshot = sample();
        snapshot
            .products
            .retain(|item| item.category != Some(Category::Outfit));
        for (id, title) in [("c", "B outfit"), ("b", "A outfit"), ("a", "A outfit")] {
            snapshot
                .products
                .push(product(id, title, Some(Category::Outfit), false));
        }
        let detail = avatar_detail(&snapshot, "lime").unwrap();
        assert_eq!(
            detail
                .unsupported_outfits
                .iter()
                .map(|item| item.booth_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn genre_products_count_every_genre_of_the_kind_and_list_only_the_selected_one() {
        let mut snapshot = sample();
        snapshot.products[3].genre = Some("outfit".into());
        snapshot.products[4].genre = Some("gimmick".into());
        snapshot.products[5].genre = Some("hair".into());
        snapshot.products[6].genre = None;
        let all: Vec<String> = ["4", "5", "6", "7"]
            .iter()
            .map(|id| id.to_string())
            .collect();
        let result = genre_products(&snapshot, crate::genre::ASSET_GENRES, &all, Some("hair"));
        assert_eq!(result.total, 4);
        assert_eq!(result.counts.get("outfit"), Some(&1));
        assert_eq!(result.counts.get("gimmick"), Some(&1));
        assert_eq!(result.counts.get("hair"), Some(&1));
        assert_eq!(result.counts.get(crate::genre::UNSET), Some(&1));
        assert_eq!(result.counts.values().sum::<u32>(), 4);

        snapshot.products[3].genre = Some("skybox".into());
        let world = genre_products(
            &snapshot,
            crate::genre::WORLD_GENRES,
            &all,
            Some("lighting_postprocess"),
        );
        assert_eq!(world.counts.get("skybox"), Some(&1));
        assert_eq!(world.counts.get(crate::genre::UNSET), Some(&3));
        assert!(world.items.is_empty());
        assert_eq!(
            result
                .items
                .iter()
                .map(|item| item.booth_id.as_str())
                .collect::<Vec<_>>(),
            vec!["6"]
        );
    }

    fn flag(id: &str, free: bool) -> ProductFlag {
        ProductFlag {
            booth_id: id.into(),
            is_free: Some(free),
            is_adult: None,
            unpublished: None,
        }
    }

    fn local_file(id: &str, filename: &str, is_dir: bool) -> LocalFile {
        LocalFile {
            path: format!("C:\\Assets\\{filename}"),
            filename: filename.into(),
            is_dir,
            size: 8,
            linked_product_id: Some(id.into()),
            candidate_product_id: Some(id.into()),
            match_score: 1.0,
            link_source: Some("auto".into()),
        }
    }

    fn placed(id: &str, filename: &str, state: FilePlace) -> PlacedDownload {
        PlacedDownload {
            booth_id: id.into(),
            variation: String::new(),
            filename: filename.into(),
            size_bytes: Some(8),
            state,
            local_path: if state == FilePlace::Local {
                Some(format!("C:\\Assets\\{filename}"))
            } else {
                None
            },
            remote_path: if state == FilePlace::Remote {
                Some(format!("\\\\nas\\{id}\\{filename}"))
            } else {
                None
            },
        }
    }

    #[test]
    fn undownloaded_free_products_leave_their_genre_until_a_local_copy_or_a_manual_move() {
        let mut snapshot = sample();
        for (id, title, category, genre) in [
            (
                "30",
                "未入手の無料小物",
                Category::Accessory,
                Some("accessory"),
            ),
            (
                "31",
                "手元の無料小物",
                Category::Accessory,
                Some("accessory"),
            ),
            ("32", "手動の無料小物", Category::Outfit, Some("outfit")),
            (
                "33",
                "未入手の無料ワールド",
                Category::World,
                Some("furniture_props"),
            ),
            ("34", "ジャンルなしの無料", Category::Gimmick, None),
            ("35", "リモートだけの無料", Category::Hair, Some("hair")),
            ("36", "記録だけの無料", Category::Gimmick, Some("gimmick")),
            (
                "37",
                "フォルダだけの無料",
                Category::Accessory,
                Some("accessory"),
            ),
        ] {
            let mut row = product(id, title, Some(category), false);
            row.genre = genre.map(str::to_string);
            snapshot.products.push(row);
        }
        snapshot
            .products
            .iter_mut()
            .find(|row| row.booth_id == "32")
            .unwrap()
            .category_locked = true;
        snapshot
            .products
            .iter_mut()
            .find(|row| row.booth_id == "32")
            .unwrap()
            .genre_locked = true;
        snapshot.product_flags = ["30", "31", "32", "33", "34", "35", "36", "37"]
            .into_iter()
            .map(|id| flag(id, true))
            .collect();
        snapshot.placed_downloads = vec![
            placed("31", "local.zip", FilePlace::Local),
            placed("35", "remote.zip", FilePlace::Remote),
        ];
        snapshot.download_rows.push(DownloadFact {
            booth_id: "36".into(),
            variation: String::new(),
            filename: "saved.unitypackage".into(),
            size_bytes: Some(4),
            downloaded: true,
            local_path: Some("C:\\Assets\\saved.unitypackage".into()),
        });
        snapshot.files.push(local_file("37", "folder", true));

        let asset_ids: Vec<String> = ["30", "31", "32", "34", "35", "36", "37"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let free = genre_products(
            &snapshot,
            crate::genre::ASSET_GENRES,
            &asset_ids,
            Some(crate::genre::FREE),
        );
        assert_eq!(free.counts.get(crate::genre::FREE), Some(&3));
        assert_eq!(free.counts.get("accessory"), Some(&2));
        assert_eq!(free.counts.get("outfit"), Some(&1));
        assert_eq!(free.counts.get("hair"), None);
        assert_eq!(free.counts.get("gimmick"), Some(&1));
        assert_eq!(free.counts.get(crate::genre::UNSET), None);
        assert_eq!(free.counts.values().sum::<u32>(), free.total);
        assert_eq!(
            free.items
                .iter()
                .map(|item| item.booth_id.as_str())
                .collect::<Vec<_>>(),
            vec!["30", "34", "35"]
        );
        assert!(free.items.iter().all(|item| item.is_free));

        let accessory = genre_products(
            &snapshot,
            crate::genre::ASSET_GENRES,
            &asset_ids,
            Some("accessory"),
        );
        assert_eq!(
            accessory
                .items
                .iter()
                .map(|item| item.booth_id.as_str())
                .collect::<Vec<_>>(),
            vec!["31", "37"]
        );
        let outfit = genre_products(
            &snapshot,
            crate::genre::ASSET_GENRES,
            &asset_ids,
            Some("outfit"),
        );
        assert_eq!(outfit.items[0].booth_id, "32");
        assert!(outfit.items[0].classification_pinned);
        let gimmick = genre_products(
            &snapshot,
            crate::genre::ASSET_GENRES,
            &asset_ids,
            Some("gimmick"),
        );
        assert_eq!(gimmick.items[0].booth_id, "36");
        let hair = genre_products(
            &snapshot,
            crate::genre::ASSET_GENRES,
            &asset_ids,
            Some("hair"),
        );
        assert!(hair.items.is_empty());

        let everyone = genre_products(&snapshot, crate::genre::ASSET_GENRES, &asset_ids, None);
        assert_eq!(everyone.total, 7);
        assert_eq!(everyone.items.len(), 7);
        assert!(everyone.items.iter().any(|item| item.booth_id == "30"));

        let worlds = genre_products(
            &snapshot,
            crate::genre::WORLD_GENRES,
            &["33".to_string()],
            Some(crate::genre::FREE),
        );
        assert_eq!(worlds.counts.get(crate::genre::FREE), Some(&1));
        assert_eq!(worlds.counts.get("furniture_props"), None);
        assert_eq!(worlds.items[0].booth_id, "33");
        assert_eq!(worlds.items[0].category, Some(Category::World));
    }
}
