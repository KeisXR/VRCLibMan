use crate::model::{
    Avatar, BaseBody, Category, LibraryStats, Link, LocalFile, Product,
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
pub struct ItemView {
    pub booth_id: String,
    pub title: String,
    pub shop: String,
    pub url: String,
    pub description_snippet: String,
    pub thumbnail_url: Option<String>,
    pub thumbnail_path: Option<String>,
    pub category: Option<Category>,
    pub is_generic: bool,
    pub confidence: Option<f64>,
    pub via_base_body: bool,
    pub local_files: Vec<FileBrief>,
    pub local_file_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedBody {
    pub booth_id: String,
    pub title: String,
    pub shop: String,
    pub url: String,
    pub thumbnail_url: Option<String>,
    pub thumbnail_path: Option<String>,
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
    pub owned_products: Vec<OwnedBody>,
    pub sections: Vec<Section>,
    pub generic_items: Vec<ItemView>,
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
    pub is_generic: bool,
    pub confidence: Option<f64>,
    pub compatible_avatars: Vec<String>,
    pub unresolved_names: Vec<String>,
    pub linked_avatar_ids: Vec<String>,
    pub user_confirmed: bool,
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
}

struct Classified {
    thumbnail_path: Option<String>,
    thumbnail_url: Option<String>,
    owned: Vec<OwnedBody>,
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
    })
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
        owned_avatars: snapshot.avatars.iter().filter(|avatar| avatar.owned).count() as u32,
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
        let linked: HashSet<&str> = snapshot
            .links
            .iter()
            .filter(|link| link.product_id == product.booth_id)
            .map(|link| link.avatar_id.as_str())
            .collect();
        let in_family = linked.iter().any(|id| family.contains(*id));
        if product.category == Some(Category::Avatar) {
            if linked.contains(avatar.id.as_str()) {
                owned.push(OwnedBody {
                    booth_id: product.booth_id.clone(),
                    title: product.title.clone(),
                    shop: product.shop.clone(),
                    url: product.url.clone(),
                    thumbnail_url: product.thumbnail_url.clone(),
                    thumbnail_path: product.thumbnail_path.clone(),
                });
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
    for key in ["outfit", "hair", "accessory", "texture", "gimmick", "world", "other", "uncategorized"] {
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
    let local_file_count = files.len() as u32;
    files.truncate(8);
    ItemView {
        booth_id: product.booth_id.clone(),
        title: product.title.clone(),
        shop: product.shop.clone(),
        url: product.url.clone(),
        description_snippet: product
            .description_snippet
            .chars()
            .take(500)
            .collect(),
        thumbnail_url: product.thumbnail_url.clone(),
        thumbnail_path: product.thumbnail_path.clone(),
        category: product.category,
        is_generic: product.is_generic,
        confidence: product.confidence,
        via_base_body,
        local_files: files,
        local_file_count,
    }
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
                product.booth_id == link.product_id && product.category != Some(Category::Avatar)
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
        }
    }

    #[test]
    fn shared_base_shows_outfit_without_owning_unrelated_avatars() {
        let snapshot = sample();
        let cards = avatar_cards(&snapshot);
        let ids: HashSet<_> = cards.iter().map(|card| card.id.as_str()).collect();
        assert_eq!(ids, HashSet::from(["lime", "plum", "manuka", "shinano"]));
        assert!(cards.iter().find(|card| card.id == "lime").unwrap().owned);
        assert!(!cards.iter().find(|card| card.id == "shinano").unwrap().owned);

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
        assert!(!manuka
            .sections
            .iter()
            .flat_map(|section| &section.items)
            .any(|item| item.booth_id == "4" || item.booth_id == "6"));
        assert!(manuka.generic_items.iter().any(|item| item.booth_id == "5"));
    }
}
