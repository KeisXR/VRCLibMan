use crate::model::{Avatar, Category};
use crate::normalize::{is_boundary_char, normalize_label, quoted_segments, suggested_display_name};

pub struct ResolveInput<'a> {
    pub booth_id: &'a str,
    pub title: &'a str,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveOutput {
    pub avatar_ids: Vec<String>,
    pub unresolved: Vec<String>,
    pub created_avatars: Vec<AvatarDraft>,
    pub needs_review: bool,
}

pub fn resolve_name<'a>(raw: &str, avatars: &'a [Avatar]) -> Option<&'a Avatar> {
    let key = normalize_label(raw);
    if key.chars().count() < 2 {
        return None;
    }
    let mut found: Option<&Avatar> = None;
    for avatar in avatars {
        if name_keys(avatar).iter().any(|candidate| candidate == &key) {
            if let Some(previous) = found {
                if previous.id != avatar.id {
                    return None;
                }
            }
            found = Some(avatar);
        }
    }
    found
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

    let haystack = crate::normalize::nfkc(title).to_lowercase();
    let mut hits: Vec<(usize, usize)> = Vec::new();
    for (index, avatar) in avatars.iter().enumerate() {
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
    let mut created = Vec::new();
    let mut unresolved = Vec::new();
    let mut ids = Vec::new();

    if input.category == Some(Category::Avatar) {
        // An avatar product lists shared-body siblings in its description.
        // Those names describe the body family; they do not mean the user owns them.
        let primary = if let Some(avatar) = match_title(input.title, avatars) {
            Some(avatar.id.clone())
        } else if let Some(avatar) = single_resolved(input.raw_names, avatars) {
            Some(avatar.id.clone())
        } else if input.allow_create {
            let id = format!("owned-{}", input.booth_id);
            let display_name = suggested_display_name(input.title);
            created.push(AvatarDraft {
                id: id.clone(),
                display_name,
            });
            Some(id)
        } else {
            None
        };
        if let Some(primary) = primary {
            ids.push(primary);
        }
    } else {
        for raw in input.raw_names {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            match resolve_name(trimmed, avatars) {
                Some(avatar) => {
                    if !ids.contains(&avatar.id) {
                        ids.push(avatar.id.clone());
                    }
                }
                None => unresolved.push(trimmed.to_string()),
            }
        }
    }

    let placeholder = created.iter().any(|draft| ids.contains(&draft.id));
    let low_confidence = input.confidence.unwrap_or(0.0) < input.threshold;
    let needs_review = if input.user_confirmed {
        false
    } else if input.category.is_none()
        || placeholder
        || low_confidence
        || (input.category == Some(Category::Avatar) && ids.is_empty())
    {
        true
    } else if input.category != Some(Category::Avatar) && !unresolved.is_empty() {
        true
    } else if input.category != Some(Category::Avatar) && ids.is_empty() && !input.is_generic {
        true
    } else {
        false
    };

    ResolveOutput {
        avatar_ids: ids,
        unresolved,
        created_avatars: created,
        needs_review,
    }
}

fn single_resolved<'a>(raw_names: &[String], avatars: &'a [Avatar]) -> Option<&'a Avatar> {
    let mut found: Option<&Avatar> = None;
    for raw in raw_names {
        if raw.trim().is_empty() {
            continue;
        }
        let Some(avatar) = resolve_name(raw, avatars) else {
            return None;
        };
        if let Some(previous) = found {
            if previous.id != avatar.id {
                return None;
            }
        } else {
            found = Some(avatar);
        }
    }
    found
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
    if head.starts_with("対応") || head.starts_with("専用") || head.starts_with("向け") {
        return true;
    }
    if head.starts_with('用') {
        return chars.len() == 1 || is_boundary_char(chars[1]);
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
        assert_eq!(match_title("ライム -Lime- / オリジナル3Dモデル", &avatars).unwrap().id, "lime");
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
}
