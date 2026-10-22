use crate::model::{Avatar, Category};
use crate::normalize::{
    is_blocked_avatar_label, is_boundary_char, is_promo_name, label_variants, nfkc,
    normalize_label, preferred_display_name, quoted_segments, strip_tag_brackets,
    suggested_display_name,
};

pub struct ResolveInput<'a> {
    pub booth_id: &'a str,
    pub title: &'a str,
    pub description: &'a str,
    pub category: Option<Category>,
    pub raw_names: &'a [String],
    pub is_generic: bool,
    pub confidence: Option<f64>,
    pub user_confirmed: bool,
    pub threshold: f64,
    pub allow_create: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarDraft {
    pub id: String,
    pub display_name: String,
    pub aliases: Vec<String>,
}

pub struct ReviewFacts<'a> {
    pub category: Option<Category>,
    pub placeholder: bool,
    pub link_count: usize,
    pub unresolved: &'a [String],
    pub is_generic: bool,
    pub grounded: bool,
    pub user_confirmed: bool,
    pub threshold: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveOutput {
    pub avatar_ids: Vec<String>,
    pub unresolved: Vec<String>,
    pub created_avatars: Vec<AvatarDraft>,
    pub needs_review: bool,
}

pub fn resolve_name<'a>(raw: &str, avatars: &'a [Avatar]) -> Option<&'a Avatar> {
    if raw.trim().is_empty() || is_blocked_avatar_label(raw) {
        return None;
    }
    let mut hits = Vec::new();
    consider_key(&normalize_label(raw), avatars, &mut hits);
    if hits.len() == 1 {
        return Some(hits[0]);
    }
    if hits.len() > 1 {
        return None;
    }
    for variant in label_variants(raw) {
        if is_blocked_avatar_label(&variant) {
            continue;
        }
        consider_key(&normalize_label(&variant), avatars, &mut hits);
    }
    if hits.len() == 1 { Some(hits[0]) } else { None }
}

fn consider_key<'a>(key: &str, avatars: &'a [Avatar], hits: &mut Vec<&'a Avatar>) {
    if key.chars().count() < 2 {
        return;
    }
    for avatar in avatars {
        if !avatar_matchable(avatar) {
            continue;
        }
        if name_keys(avatar).iter().any(|candidate| candidate == key)
            && !hits.iter().any(|hit| hit.id == avatar.id)
        {
            hits.push(avatar);
        }
    }
}

pub fn match_title<'a>(title: &str, avatars: &'a [Avatar]) -> Option<&'a Avatar> {
    let mut quoted_hits: Vec<&Avatar> = Vec::new();
    for segment in quoted_segments(title) {
        if let Some(avatar) = resolve_name(&segment, avatars) {
            if !quoted_hits.iter().any(|hit| hit.id == avatar.id) {
                quoted_hits.push(avatar);
            }
        }
    }
    if quoted_hits.len() == 1 {
        return Some(quoted_hits[0]);
    }
    if quoted_hits.len() > 1 {
        return None;
    }

    let haystack = nfkc(&strip_tag_brackets(title)).to_lowercase();
    let mut hits: Vec<(usize, usize)> = Vec::new();
    for (index, avatar) in avatars.iter().enumerate() {
        if !avatar_matchable(avatar) {
            continue;
        }
        for form in name_forms(avatar) {
            let needle = crate::normalize::nfkc(&form).to_lowercase();
            if needle.chars().count() < 2 {
                continue;
            }
            if find_bounded(&haystack, &needle) {
                hits.push((index, needle.chars().count()));
            }
        }
    }
    if hits.is_empty() {
        return None;
    }
    hits.sort_by(|left, right| right.1.cmp(&left.1));
    let best_len = hits[0].1;
    let mut ids: Vec<&str> = hits
        .into_iter()
        .filter(|(_, len)| *len == best_len)
        .map(|(index, _)| avatars[index].id.as_str())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.len() == 1 {
        avatars.iter().find(|avatar| avatar.id == ids[0])
    } else {
        None
    }
}

pub fn resolve_product(input: &ResolveInput<'_>, avatars: &[Avatar]) -> ResolveOutput {
    let _model_confidence = input.confidence;
    let mut created = Vec::new();
    let mut unresolved = Vec::new();
    let mut ids = Vec::new();
    let usable = usable_names(input.raw_names);

    if input.category == Some(Category::Avatar) {
        // An avatar product lists shared-body siblings in its description.
        // Those names describe the body family; they do not mean the user owns them.
        // A name from the model or a rule wins over a title token, so a sale bracket
        // cannot rename the avatar after the model already returned 真冬 or Nayu.
        let resolved = resolved_ids(&usable, avatars);
        let title_hit = match_title(input.title, avatars).map(|avatar| avatar.id.clone());
        let primary = if resolved.len() == 1 {
            Some(resolved[0].clone())
        } else if resolved.len() > 1 {
            title_hit.filter(|id| resolved.iter().any(|resolved_id| resolved_id == id))
        } else if let Some(label) = single_unresolved_label(&usable, avatars) {
            create_placeholder(input, label, &usable, &mut created)
        } else if let Some(id) = title_hit {
            Some(id)
        } else if let Some(label) = acceptable_suggested(&suggested_display_name(input.title)) {
            create_placeholder(input, label, &usable, &mut created)
        } else {
            None
        };
        if let Some(primary) = primary {
            ids.push(primary);
        }
    } else {
        for raw in &usable {
            if let Some(avatar) = resolve_name(raw, avatars) {
                if !ids.contains(&avatar.id) {
                    ids.push(avatar.id.clone());
                }
            } else if keep_unresolved(raw, avatars) {
                let label = unresolved_label(raw);
                if !unresolved
                    .iter()
                    .any(|existing: &String| existing == &label)
                {
                    unresolved.push(label);
                }
            }
        }
    }

    let placeholder = ids.iter().any(|id| id.starts_with("owned-"));
    let grounded = links_grounded(&ids, avatars, input.title, input.description);
    let needs_review = product_needs_review(&ReviewFacts {
        category: input.category,
        placeholder,
        link_count: ids.len(),
        unresolved: &unresolved,
        is_generic: input.is_generic,
        grounded,
        user_confirmed: input.user_confirmed,
        threshold: input.threshold,
    });

    ResolveOutput {
        avatar_ids: ids,
        unresolved,
        created_avatars: created,
        needs_review,
    }
}

pub fn product_needs_review(facts: &ReviewFacts<'_>) -> bool {
    if facts.user_confirmed {
        return false;
    }
    if facts.category.is_none() {
        return true;
    }
    certainty(facts) < facts.threshold
}

fn certainty(facts: &ReviewFacts<'_>) -> f64 {
    if facts.placeholder || !facts.unresolved.is_empty() {
        return 0.45;
    }
    if facts.category == Some(Category::Avatar) && facts.link_count == 0 {
        return 0.2;
    }
    let needs_link = matches!(
        facts.category,
        Some(Category::Outfit | Category::Hair | Category::Accessory | Category::Texture)
    );
    if needs_link && !facts.is_generic && facts.link_count == 0 {
        return 0.35;
    }
    if facts.link_count > 0 && facts.grounded {
        return 0.95;
    }
    if facts.link_count > 0 {
        // Resolved in the library, but the name is not in the title or description.
        return 0.5;
    }
    0.86
}

fn acceptable_suggested(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "名称未設定" || is_blocked_avatar_label(trimmed) {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn usable_names(raw_names: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    for name in raw_names {
        let trimmed = name.trim();
        if trimmed.is_empty() || is_blocked_avatar_label(trimmed) {
            continue;
        }
        if !names.iter().any(|existing: &String| existing == trimmed) {
            names.push(trimmed.to_string());
        }
    }
    names
}

fn resolved_ids(names: &[String], avatars: &[Avatar]) -> Vec<String> {
    let mut ids = Vec::new();
    for name in names {
        if let Some(avatar) = resolve_name(name, avatars) {
            if !ids.contains(&avatar.id) {
                ids.push(avatar.id.clone());
            }
        }
    }
    ids
}

fn single_unresolved_label(names: &[String], avatars: &[Avatar]) -> Option<String> {
    let mut labels = Vec::new();
    for name in names {
        if resolve_name(name, avatars).is_some() || !keep_unresolved(name, avatars) {
            continue;
        }
        if let Some(label) = preferred_display_name(name) {
            if !labels.iter().any(|existing: &String| existing == &label) {
                labels.push(label);
            }
        }
    }
    if labels.len() == 1 {
        labels.pop()
    } else {
        None
    }
}

fn create_placeholder(
    input: &ResolveInput<'_>,
    label: String,
    raw_names: &[String],
    created: &mut Vec<AvatarDraft>,
) -> Option<String> {
    if !input.allow_create {
        return None;
    }
    let display = preferred_display_name(&label).unwrap_or(label);
    if is_blocked_avatar_label(&display) || display == "名称未設定" {
        return None;
    }
    let id = format!("owned-{}", input.booth_id);
    created.push(AvatarDraft {
        id: id.clone(),
        display_name: display.clone(),
        aliases: draft_aliases(&display, raw_names, input.title),
    });
    Some(id)
}

fn draft_aliases(display: &str, raw_names: &[String], title: &str) -> Vec<String> {
    let mut aliases = Vec::new();
    let display_key = normalize_label(display);
    for source in raw_names
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(title))
    {
        for variant in label_variants(&strip_tag_brackets(source)) {
            if variant == display || is_blocked_avatar_label(&variant) || is_promo_name(&variant) {
                continue;
            }
            if normalize_label(&variant) == display_key || variant.chars().count() > 32 {
                continue;
            }
            if !aliases.iter().any(|existing: &String| existing == &variant) {
                aliases.push(variant);
            }
        }
    }
    aliases.truncate(8);
    aliases
}

fn keep_unresolved(raw: &str, avatars: &[Avatar]) -> bool {
    !raw.trim().ends_with('様') && !mashed_known_names(raw, avatars)
}

fn unresolved_label(raw: &str) -> String {
    let mut text = raw.trim().to_string();
    for suffix in ["対応", "専用", "向け"] {
        if let Some(stripped) = text.strip_suffix(suffix) {
            if stripped.chars().count() >= 2 {
                text = stripped.trim().to_string();
            }
        }
    }
    preferred_display_name(&text).unwrap_or(text)
}

fn mashed_known_names(raw: &str, avatars: &[Avatar]) -> bool {
    let key = normalize_label(raw);
    if key.chars().count() < 4 {
        return false;
    }
    let mut ids = Vec::new();
    for avatar in avatars {
        if !avatar_matchable(avatar) {
            continue;
        }
        for form in name_keys(avatar) {
            if form.chars().count() >= 2 && form != key && key.contains(form.as_str()) {
                if !ids.contains(&avatar.id) {
                    ids.push(avatar.id.clone());
                }
                break;
            }
        }
    }
    ids.len() >= 2
}

fn avatar_matchable(avatar: &Avatar) -> bool {
    !is_promo_name(&avatar.display_name) && !is_blocked_avatar_label(&avatar.display_name)
}

pub fn links_grounded(ids: &[String], avatars: &[Avatar], title: &str, description: &str) -> bool {
    if ids.is_empty() {
        return false;
    }
    let haystack = format!("{}\n{}", nfkc(title), nfkc(description)).to_lowercase();
    ids.iter().all(|id| {
        let Some(avatar) = avatars.iter().find(|avatar| &avatar.id == id) else {
            return false;
        };
        name_forms(avatar).iter().any(|form| {
            let needle = nfkc(form).to_lowercase();
            needle.chars().count() >= 2 && find_bounded(&haystack, &needle)
        })
    })
}

fn name_forms(avatar: &Avatar) -> Vec<String> {
    let mut forms = vec![avatar.display_name.clone()];
    forms.extend(avatar.aliases.iter().cloned());
    if avatar.id.chars().any(|ch| ch.is_ascii_alphabetic()) {
        forms.push(avatar.id.clone());
    }
    forms
}

fn name_keys(avatar: &Avatar) -> Vec<String> {
    name_forms(avatar)
        .into_iter()
        .map(|form| normalize_label(&form))
        .filter(|key| key.chars().count() >= 2)
        .collect()
}

pub fn known_name_in(text: &str, avatars: &[Avatar]) -> Option<String> {
    let haystack = crate::normalize::nfkc(text).to_lowercase();
    let mut best: Option<(usize, String)> = None;
    for avatar in avatars {
        if !avatar_matchable(avatar) {
            continue;
        }
        for form in name_forms(avatar) {
            let needle = crate::normalize::nfkc(&form).to_lowercase();
            let len = needle.chars().count();
            if len < 2 || !find_bounded(&haystack, &needle) {
                continue;
            }
            if best.as_ref().is_none_or(|(current, _)| len > *current) {
                best = Some((len, avatar.display_name.clone()));
            }
        }
    }
    best.map(|(_, name)| name)
}

pub fn known_names_in(text: &str, avatars: &[Avatar]) -> Vec<String> {
    let haystack = nfkc(text).to_lowercase();
    let mut found = Vec::new();
    for avatar in avatars {
        if !avatar_matchable(avatar) {
            continue;
        }
        for form in name_forms(avatar) {
            let needle = nfkc(&form).to_lowercase();
            if needle.chars().count() >= 2 && find_bounded(&haystack, &needle) {
                if !found
                    .iter()
                    .any(|name: &String| name == &avatar.display_name)
                {
                    found.push(avatar.display_name.clone());
                }
                break;
            }
        }
    }
    found
}

fn find_bounded(haystack: &str, needle: &str) -> bool {
    let haystack: Vec<char> = haystack.chars().collect();
    let needle: Vec<char> = needle.chars().collect();
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    for index in 0..=haystack.len() - needle.len() {
        if haystack[index..index + needle.len()] != needle[..] {
            continue;
        }
        let left_ok = index == 0 || is_boundary_char(haystack[index - 1]);
        let right = index + needle.len();
        let right_ok = right == haystack.len()
            || is_boundary_char(haystack[right])
            || allowed_trailer(&haystack[right..]);
        if left_ok && right_ok {
            return true;
        }
    }
    false
}

fn allowed_trailer(chars: &[char]) -> bool {
    let head: String = chars.iter().take(8).collect();
    if head.starts_with("対応")
        || head.starts_with("専用")
        || head.starts_with("向け")
        || head.starts_with("ちゃん")
        || head.starts_with("くん")
        || head.starts_with("さん")
        || head.starts_with("様")
    {
        return true;
    }
    if head.starts_with('用') {
        return true;
    }
    let lower = head.to_lowercase();
    if lower.starts_with("ver") {
        return true;
    }
    lower.starts_with('v') && chars.get(1).is_some_and(|ch| ch.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn avatar(id: &str, display: &str, aliases: &[&str], base: Option<&str>) -> Avatar {
        Avatar {
            id: id.into(),
            display_name: display.into(),
            base_body: base.map(str::to_string),
            aliases: aliases.iter().map(|alias| (*alias).to_string()).collect(),
            is_builtin: true,
            notes: String::new(),
        }
    }

    fn sample() -> Vec<Avatar> {
        vec![
            avatar("lime", "ライム", &["Lime"], Some("komado")),
            avatar("plum", "プラム", &["Plum"], Some("komado")),
            avatar("manuka", "マヌカ", &["Manuka"], None),
            avatar("shinano", "しなの", &["Shinano"], None),
        ]
    }

    #[test]
    fn resolves_alias_and_rejects_longer_neighbor() {
        let avatars = sample();
        assert_eq!(resolve_name("Lime対応", &avatars).unwrap().id, "lime");
        assert!(resolve_name("ショコラミント", &avatars).is_none());
        assert!(match_title("ショコラミントの衣装", &avatars).is_none());
        assert_eq!(
            match_title("ライム -Lime- / オリジナル3Dモデル", &avatars)
                .unwrap()
                .id,
            "lime"
        );
    }

    #[test]
    fn avatar_product_does_not_own_listed_siblings() {
        let avatars = sample();
        let raw = vec![
            "ライム".into(),
            "プラム".into(),
            "シフォン".into(),
            "ショコラ".into(),
        ];
        let output = resolve_product(
            &ResolveInput {
                booth_id: "4876459",
                title: "ライム -Lime- / オリジナル3Dモデル",
                description: "",
                category: Some(Category::Avatar),
                raw_names: &raw,
                is_generic: false,
                confidence: Some(0.92),
                user_confirmed: false,
                threshold: 0.6,
                allow_create: true,
            },
            &avatars,
        );
        assert_eq!(output.avatar_ids, vec!["lime".to_string()]);
        assert!(!output.needs_review);
        assert!(output.created_avatars.is_empty());
    }

    #[test]
    fn unknown_outfit_name_goes_to_review() {
        let avatars = sample();
        let raw = vec!["ライム".into(), "架空ちゃん".into()];
        let output = resolve_product(
            &ResolveInput {
                booth_id: "200",
                title: "しなの・ライム・架空ちゃん対応ワンピース",
                description: "",
                category: Some(Category::Outfit),
                raw_names: &raw,
                is_generic: false,
                confidence: Some(0.9),
                user_confirmed: false,
                threshold: 0.6,
                allow_create: true,
            },
            &avatars,
        );
        assert_eq!(output.avatar_ids, vec!["lime".to_string()]);
        assert_eq!(output.unresolved, vec!["架空ちゃん".to_string()]);
        assert!(output.needs_review);
    }

    #[test]
    fn unknown_avatar_product_becomes_placeholder() {
        let output = resolve_product(
            &ResolveInput {
                booth_id: "300",
                title: "オリジナル3Dモデル「架空ちゃん」",
                description: "",
                category: Some(Category::Avatar),
                raw_names: &[],
                is_generic: false,
                confidence: Some(0.95),
                user_confirmed: false,
                threshold: 0.6,
                allow_create: true,
            },
            &sample(),
        );
        assert_eq!(output.created_avatars[0].id, "owned-300");
        assert_eq!(output.created_avatars[0].display_name, "架空ちゃん");
        assert!(output.needs_review);
    }

    fn catalog() -> Vec<Avatar> {
        vec![
            avatar("lime", "ライム", &["Lime"], Some("komado")),
            avatar("plum", "プラム", &["Plum"], Some("komado")),
            avatar("chiffon", "シフォン", &["Chiffon"], Some("komado")),
            avatar("chocolat", "ショコラ", &["Chocolat"], Some("komado")),
            avatar("manuka", "マヌカ", &["Manuka"], None),
            avatar("celestia", "セレスティア", &["Celestia", "Selestia"], None),
            avatar("rurune", "ルルネ", &["rurune", "Rurune"], None),
            avatar("eku", "エク", &["Eku"], None),
            avatar("mafuyu", "真冬", &["Mafuyu"], None),
            avatar("mizuki", "瑞希", &[], None),
        ]
    }

    #[test]
    fn alias_forms_resolve_without_merging_sale_brackets() {
        let avatars = catalog();
        let cases = [
            ("ショコラ -Chocolat-", "chocolat"),
            ("ライム -Lime-", "lime"),
            ("rurune -ルルネ-", "rurune"),
            ("manuka-マヌカ", "manuka"),
            ("Selestia", "celestia"),
            ("プラムちゃん", "plum"),
            ("シフォン(Chiffon)", "chiffon"),
            ("エク - Eku", "eku"),
            ("真冬 Mafuyu", "mafuyu"),
        ];
        for (raw, id) in cases {
            assert_eq!(resolve_name(raw, &avatars).unwrap().id, id, "{raw}");
        }

        let mut library = avatars.clone();
        library.push(avatar("sale", "半額セール中", &[], None));
        let mafuyu = resolve_product(
            &ResolveInput {
                booth_id: "5007531",
                title: "【半額セール中】真冬 Mafuyu / オリジナル3Dモデル",
                description: "",
                category: Some(Category::Avatar),
                raw_names: &["真冬 Mafuyu".to_string()],
                is_generic: false,
                confidence: Some(0.2),
                user_confirmed: false,
                threshold: 0.6,
                allow_create: true,
            },
            &library,
        );
        assert_eq!(mafuyu.avatar_ids, vec!["mafuyu".to_string()]);
        assert!(mafuyu.created_avatars.is_empty());

        let nayu = resolve_product(
            &ResolveInput {
                booth_id: "4023598",
                title: "【半額セール中】【オリジナル3Dモデル】Nayu - ナユ",
                description: "",
                category: Some(Category::Avatar),
                raw_names: &["Nayu".to_string()],
                is_generic: false,
                confidence: Some(0.9),
                user_confirmed: false,
                threshold: 0.6,
                allow_create: true,
            },
            &library,
        );
        assert_eq!(nayu.created_avatars.len(), 1);
        assert_eq!(nayu.created_avatars[0].display_name, "Nayu");
        assert!(
            nayu.created_avatars[0]
                .aliases
                .iter()
                .any(|alias| alias == "ナユ")
        );
        assert_ne!(nayu.avatar_ids[0], "sale");
        assert_ne!(nayu.avatar_ids[0], "mafuyu");
    }

    #[test]
    fn count_titles_do_not_become_avatars_and_review_ignores_raw_confidence() {
        let output = resolve_product(
            &ResolveInput {
                booth_id: "19",
                title: "19アバター対応(仮)",
                description: "",
                category: Some(Category::Avatar),
                raw_names: &[],
                is_generic: false,
                confidence: Some(0.95),
                user_confirmed: false,
                threshold: 0.6,
                allow_create: true,
            },
            &sample(),
        );
        assert!(output.created_avatars.is_empty());
        assert!(output.avatar_ids.is_empty());
        assert!(output.needs_review);

        let lime = resolve_product(
            &ResolveInput {
                booth_id: "4876459",
                title: "ライム -Lime- / オリジナル3Dモデル",
                description: "ライムのオリジナル3Dモデルです",
                category: Some(Category::Avatar),
                raw_names: &[],
                is_generic: false,
                confidence: Some(0.2),
                user_confirmed: false,
                threshold: 0.6,
                allow_create: true,
            },
            &sample(),
        );
        assert_eq!(lime.avatar_ids, vec!["lime".to_string()]);
        assert!(!lime.needs_review, "a name in the title is enough");

        let world = resolve_product(
            &ResolveInput {
                booth_id: "1",
                title: "ネオンの部屋",
                description: "",
                category: Some(Category::World),
                raw_names: &[],
                is_generic: false,
                confidence: Some(0.2),
                user_confirmed: false,
                threshold: 0.6,
                allow_create: true,
            },
            &sample(),
        );
        assert!(!world.needs_review);

        let gimmick = resolve_product(
            &ResolveInput {
                booth_id: "2",
                title: "ライム用ペン",
                description: "QvPen と Modular Avatar は道具です",
                category: Some(Category::Gimmick),
                raw_names: &["ライム".into(), "QvPen".into(), "Modular Avatar".into()],
                is_generic: false,
                confidence: Some(0.99),
                user_confirmed: false,
                threshold: 0.6,
                allow_create: true,
            },
            &sample(),
        );
        assert_eq!(gimmick.avatar_ids, vec!["lime".to_string()]);
        assert!(gimmick.unresolved.is_empty());
        assert!(!gimmick.needs_review);

        let lapwing = resolve_product(
            &ResolveInput {
                booth_id: "3",
                title: "Lapwing対応",
                description: "",
                category: Some(Category::Gimmick),
                raw_names: &["ライム".into(), "Lapwing".into(), "もち山金魚様".into()],
                is_generic: false,
                confidence: Some(0.99),
                user_confirmed: false,
                threshold: 0.6,
                allow_create: true,
            },
            &catalog(),
        );
        assert_eq!(lapwing.unresolved, vec!["Lapwing".to_string()]);
        assert!(lapwing.needs_review);

        let mashed = resolve_product(
            &ResolveInput {
                booth_id: "4",
                title: "ボディ",
                description: "",
                category: Some(Category::Gimmick),
                raw_names: &["瑞希ルルネ".into()],
                is_generic: false,
                confidence: Some(0.9),
                user_confirmed: false,
                threshold: 0.6,
                allow_create: true,
            },
            &catalog(),
        );
        assert!(mashed.avatar_ids.is_empty());
        assert!(mashed.unresolved.is_empty());
        assert!(!mashed.needs_review);
    }
}
