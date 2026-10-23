use crate::model::{Avatar, Category};
use crate::normalize::{
    adjacent_reading, is_blocked_avatar_label, is_boundary_char, is_cjk, is_name_alias,
    is_promo_name, is_script_boundary, label_variants, nfkc, normalize_label, paren_pieces,
    preferred_display_name, quoted_segments, strip_tag_brackets, strip_title_noise,
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
    pub readings: Vec<(String, String)>,
    pub needs_review: bool,
}

pub fn resolve_name<'a>(raw: &str, avatars: &'a [Avatar]) -> Option<&'a Avatar> {
    let single_cjk = raw.chars().count() == 1 && raw.chars().all(is_cjk);
    // NSFW や素体は属性であって名前ではない。ただし同名のアバターが登録されていれば、そのアバターとして解決する。
    let attribute = crate::normalize::is_attribute_label(raw);
    if raw.trim().is_empty() || (is_blocked_avatar_label(raw) && !single_cjk && !attribute) {
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
        if is_blocked_avatar_label(&variant) && !crate::normalize::is_attribute_label(&variant) {
            continue;
        }
        consider_key(&normalize_label(&variant), avatars, &mut hits);
    }
    if hits.len() == 1 { Some(hits[0]) } else { None }
}

fn consider_key<'a>(key: &str, avatars: &'a [Avatar], hits: &mut Vec<&'a Avatar>) {
    if key.chars().count() < 2 && !key.chars().any(is_cjk) {
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
    let mut readings = Vec::new();
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
            consider_compatible_name(raw, avatars, &mut ids, &mut unresolved, &mut readings);
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
        readings,
        needs_review,
    }
}

fn consider_compatible_name(
    raw: &str,
    avatars: &[Avatar],
    ids: &mut Vec<String>,
    unresolved: &mut Vec<String>,
    readings: &mut Vec<(String, String)>,
) {
    let pieces = paren_pieces(raw);
    if is_reading_pair(&pieces) {
        let mut hits = Vec::new();
        for piece in &pieces {
            if let Some(avatar) = resolve_piece(piece, avatars) {
                if !hits.iter().any(|id: &String| id == &avatar.id) {
                    hits.push(avatar.id.clone());
                }
            }
        }
        if hits.len() == 1 {
            if !ids.contains(&hits[0]) {
                ids.push(hits[0].clone());
            }
            for piece in &pieces {
                if resolve_piece(piece, avatars).is_none() && is_name_alias(piece) {
                    readings.push((hits[0].clone(), piece.clone()));
                }
            }
            return;
        }
        if hits.len() > 1 {
            for id in hits {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            return;
        }
        if let Some(label) = pieces.iter().find(|piece| piece.chars().any(is_cjk)) {
            push_unresolved(unresolved, label);
        }
        return;
    }
    let single_cjk = raw.chars().count() == 1 && raw.chars().all(is_cjk);
    let attribute = crate::normalize::is_attribute_label(raw);
    if is_blocked_avatar_label(raw) && !single_cjk && !attribute {
        return;
    }
    if let Some(avatar) = resolve_piece(raw, avatars) {
        if !ids.contains(&avatar.id) {
            ids.push(avatar.id.clone());
        }
        return;
    }
    // 素体や NSFW は、同名のアバターが無いときは未解決の名前にしない。
    if !attribute && keep_unresolved(raw, avatars) {
        push_unresolved(unresolved, &unresolved_label(raw));
    }
}

fn is_reading_pair(pieces: &[String]) -> bool {
    pieces.len() == 2
        && pieces.iter().any(|piece| piece.chars().any(is_cjk))
        && pieces
            .iter()
            .any(|piece| piece.chars().any(|ch| ch.is_ascii_alphabetic()))
}

fn resolve_piece<'a>(piece: &str, avatars: &'a [Avatar]) -> Option<&'a Avatar> {
    let single_cjk = piece.chars().count() == 1 && piece.chars().all(is_cjk);
    let attribute = crate::normalize::is_attribute_label(piece);
    if is_blocked_avatar_label(piece) && !single_cjk && !attribute {
        return None;
    }
    resolve_name(piece, avatars).or_else(|| {
        if !single_cjk {
            return None;
        }
        avatars.iter().find(|avatar| {
            avatar_matchable(avatar) && name_keys(avatar).iter().any(|key| key == piece)
        })
    })
}

fn push_unresolved(unresolved: &mut Vec<String>, label: &str) {
    if label.is_empty() || unresolved.iter().any(|existing| existing == label) {
        return;
    }
    unresolved.push(label.to_string());
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
        let single_cjk = trimmed.chars().count() == 1 && trimmed.chars().all(is_cjk);
        let attribute = crate::normalize::is_attribute_label(trimmed);
        if trimmed.is_empty() || (is_blocked_avatar_label(trimmed) && !single_cjk && !attribute) {
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
    let display = japanese_display(&label, input.title, input.description);
    if is_blocked_avatar_label(&display) || display == "名称未設定" {
        return None;
    }
    let id = format!("owned-{}", input.booth_id);
    created.push(AvatarDraft {
        id: id.clone(),
        display_name: display.clone(),
        aliases: draft_aliases(&display, raw_names, input.title, input.description),
    });
    Some(id)
}

fn japanese_display(label: &str, title: &str, description: &str) -> String {
    let preferred = preferred_display_name(label).unwrap_or_else(|| label.to_string());
    if preferred.chars().any(is_cjk) && !preferred.chars().any(|ch| ch.is_ascii_alphabetic()) {
        return preferred;
    }
    adjacent_reading(&preferred, title)
        .filter(|name| name.chars().any(is_cjk))
        .or_else(|| {
            adjacent_reading(&preferred, description).filter(|name| name.chars().any(is_cjk))
        })
        .or_else(|| sole_title_cjk(title))
        .unwrap_or(preferred)
}

fn sole_title_cjk(title: &str) -> Option<String> {
    let folded = nfkc(&strip_tag_brackets(title));
    let chars: Vec<char> = folded.chars().collect();
    let mut runs = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if !is_cjk(chars[index]) {
            index += 1;
            continue;
        }
        let begin = index;
        while index < chars.len() && is_cjk(chars[index]) {
            index += 1;
        }
        let name: String = chars[begin..index].iter().collect();
        let count = name.chars().count();
        const SKIP: &str = "のをにはがでと版式";
        if count == 1 && SKIP.contains(chars[begin]) {
            continue;
        }
        if name.contains("対応") || name.contains("専用") || name.contains("向け") {
            continue;
        }
        if count > 1 && is_blocked_avatar_label(&name) {
            continue;
        }
        if (1..=6).contains(&count) && !runs.iter().any(|existing: &String| existing == &name) {
            runs.push(name);
        }
    }
    if runs.len() == 1 { runs.pop() } else { None }
}

fn is_cjk_name(text: &str) -> bool {
    let count = text.chars().count();
    (2..=16).contains(&count)
        && text.chars().any(is_cjk)
        && text.chars().all(|ch| is_cjk(ch) || ch.is_whitespace())
}

fn draft_aliases(
    display: &str,
    raw_names: &[String],
    title: &str,
    description: &str,
) -> Vec<String> {
    let mut aliases = Vec::new();
    let display_key = normalize_label(display);
    let mut push = |variant: &str| {
        if !is_name_alias(variant) || is_promo_name(variant) {
            return;
        }
        if variant == display || normalize_label(variant) == display_key {
            return;
        }
        if !aliases.iter().any(|existing: &String| existing == variant) {
            aliases.push(variant.to_string());
        }
    };
    for raw in raw_names {
        for variant in label_variants(raw) {
            push(&variant);
        }
    }
    for text in [title, description] {
        if let Some(reading) = adjacent_reading(display, text) {
            push(&reading);
        }
    }
    if let Some(partner) = script_partner(display, title) {
        push(&partner);
    }
    aliases.truncate(4);
    aliases
}

fn script_partner(display: &str, title: &str) -> Option<String> {
    let display_cjk = is_cjk_name(display);
    let needle = normalize_label(display);
    if needle.is_empty() {
        return None;
    }
    let mut chunks = vec![strip_title_noise(&strip_tag_brackets(title))];
    for separator in [" / ", "・", "、", "/", "／"] {
        let mut next = Vec::new();
        for chunk in chunks {
            if chunk.contains(separator) {
                next.extend(chunk.split(separator).map(str::to_string));
            } else {
                next.push(chunk);
            }
        }
        chunks = next;
    }
    for chunk in chunks {
        let variants = label_variants(&chunk);
        if !variants
            .iter()
            .any(|variant| normalize_label(variant) == needle)
        {
            continue;
        }
        let partners: Vec<String> = variants
            .into_iter()
            .filter(|variant| {
                is_name_alias(variant)
                    && normalize_label(variant) != needle
                    && is_cjk_name(variant) != display_cjk
            })
            .collect();
        if partners.len() == 1 {
            return Some(partners[0].clone());
        }
    }
    None
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
    let name = &avatar.display_name;
    let single_cjk = name.chars().count() == 1 && name.chars().all(is_cjk);
    if single_cjk || crate::normalize::is_attribute_label(name) {
        return true;
    }
    !is_promo_name(name) && !is_blocked_avatar_label(name)
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
            let count = needle.chars().count();
            (count >= 2 || (count == 1 && needle.chars().all(crate::normalize::is_cjk)))
                && find_bounded(&haystack, &needle)
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
        .filter(|key| {
            let count = key.chars().count();
            count >= 2 || (count == 1 && key.chars().all(crate::normalize::is_cjk))
        })
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
        let left_ok = index == 0
            || is_boundary_char(haystack[index - 1])
            || is_script_boundary(haystack[index - 1], needle[0]);
        let right = index + needle.len();
        let right_ok = right == haystack.len()
            || is_boundary_char(haystack[right])
            || allowed_trailer(&haystack[right..])
            || is_script_boundary(needle[needle.len() - 1], haystack[right]);
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
            owned: false,
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
    fn hiragana_or_kanji_into_katakana_bounds_a_name() {
        let mame = vec![avatar("mame", "まめひなた", &["Mamehinata"], None)];
        assert_eq!(
            known_name_in("まめひなたフェイスアニメーションセット", &mame).as_deref(),
            Some("まめひなた")
        );
        assert!(known_name_in("まめひなたんの衣装", &mame).is_none());
        assert_eq!(
            known_name_in("真冬フェイス", &catalog()).as_deref(),
            Some("真冬")
        );
        assert!(known_name_in("ショコラミントの衣装", &catalog()).is_none());
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
    fn a_known_attribute_name_links_and_an_unknown_one_stays_unnamed() {
        let avatars = vec![avatar("body", "素体", &[], None)];
        let linked = resolve_product(
            &ResolveInput {
                booth_id: "1",
                title: "【素体対応】服",
                description: "",
                category: Some(Category::Outfit),
                raw_names: &["素体".into()],
                is_generic: false,
                confidence: Some(0.9),
                user_confirmed: false,
                threshold: 0.6,
                allow_create: true,
            },
            &avatars,
        );
        assert_eq!(linked.avatar_ids, vec!["body".to_string()]);
        assert!(linked.unresolved.is_empty());
        assert!(!linked.needs_review);
        let dropped = resolve_product(
            &ResolveInput {
                booth_id: "2",
                title: "【NSFW対応】【脱衣対応】服",
                description: "",
                category: Some(Category::Outfit),
                raw_names: &["NSFW".into(), "脱衣".into()],
                is_generic: false,
                confidence: Some(0.9),
                user_confirmed: false,
                threshold: 0.6,
                allow_create: true,
            },
            &avatars,
        );
        assert!(dropped.avatar_ids.is_empty());
        assert!(dropped.unresolved.is_empty());
        assert!(dropped.created_avatars.is_empty());
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
        assert_eq!(nayu.created_avatars[0].display_name, "ナユ");
        assert_eq!(nayu.created_avatars[0].aliases, vec!["Nayu".to_string()]);
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

    #[test]
    fn unclosed_parentheses_split_and_aliases_stay_name_like() {
        let output = resolve_product(
            &ResolveInput {
                booth_id: "9",
                title: "Dreamy Step",
                description: "",
                category: Some(Category::Outfit),
                raw_names: &["エルキュナ２(Eruqyuna2".into(), "萌(Moe".into()],
                is_generic: false,
                confidence: Some(0.9),
                user_confirmed: false,
                threshold: 0.6,
                allow_create: true,
            },
            &sample(),
        );
        assert_eq!(
            output.unresolved,
            vec!["エルキュナ2".to_string(), "萌".to_string()]
        );

        let eku = resolve_product(
            &ResolveInput {
                booth_id: "7524041",
                title: "エク - Eku PC版 ver.2.5",
                description: "",
                category: Some(Category::Avatar),
                raw_names: &["エク".into()],
                is_generic: false,
                confidence: Some(0.9),
                user_confirmed: false,
                threshold: 0.6,
                allow_create: true,
            },
            &sample(),
        );
        assert_eq!(eku.created_avatars[0].display_name, "エク");
        assert_eq!(eku.created_avatars[0].aliases, vec!["Eku".to_string()]);

        let sio = resolve_product(
            &ResolveInput {
                booth_id: "8",
                title: "しお - Sio ver.2.5",
                description: "",
                category: Some(Category::Avatar),
                raw_names: &["Sio".into()],
                is_generic: false,
                confidence: Some(0.9),
                user_confirmed: false,
                threshold: 0.6,
                allow_create: true,
            },
            &sample(),
        );
        assert_eq!(sio.created_avatars[0].display_name, "しお");
        assert_eq!(sio.created_avatars[0].aliases, vec!["Sio".to_string()]);

        let described = resolve_product(
            &ResolveInput {
                booth_id: "7224250",
                title: "Sio",
                description: "しお（Sio）のオリジナル3Dモデルです。",
                category: Some(Category::Avatar),
                raw_names: &["Sio".into()],
                is_generic: false,
                confidence: Some(0.9),
                user_confirmed: false,
                threshold: 0.6,
                allow_create: true,
            },
            &sample(),
        );
        assert_eq!(described.created_avatars[0].display_name, "しお");
        assert!(
            described.created_avatars[0]
                .aliases
                .iter()
                .any(|alias| alias == "Sio")
        );

        let known = vec![avatar("anomea", "あのめあ", &[], None)];
        let paired = resolve_product(
            &ResolveInput {
                booth_id: "30",
                title: "Dreamy Step",
                description: "",
                category: Some(Category::Outfit),
                raw_names: &["あのめあ(Anomeoa)".into(), "ヌーヴェル(Nouvelle)".into()],
                is_generic: false,
                confidence: Some(0.9),
                user_confirmed: false,
                threshold: 0.6,
                allow_create: true,
            },
            &known,
        );
        assert_eq!(paired.avatar_ids, vec!["anomea".to_string()]);
        assert_eq!(paired.unresolved, vec!["ヌーヴェル".to_string()]);
        assert!(
            paired
                .readings
                .iter()
                .any(|(id, alias)| id == "anomea" && alias == "Anomeoa")
        );
    }
}
