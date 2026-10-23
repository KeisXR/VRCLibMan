use std::path::Path;

const ALLOW: &[&str] = &[
    "裸足",
    "ランジェリー",
    "成人向け",
    "脱衣",
    "水着",
    "下着",
    "レオタード",
];

const TOOLS: &[&str] = &[
    "lilToon",
    "Poiyomi",
    "Poiyomi Toon",
    "PhysBone",
    "Modular Avatar",
    "QvPen",
    "VRCFury",
    "Gesture Manager",
];

pub fn tool_names() -> &'static [&'static str] {
    TOOLS
}

pub fn is_known_tool(name: &str) -> bool {
    let folded = fold_tool(name);
    const FOLDED: &[&str] = &[
        "liltoon",
        "poiyomi",
        "poiyomitoon",
        "physbone",
        "modularavatar",
        "qvpen",
        "vrcfury",
        "gesturemanager",
    ];
    FOLDED.iter().any(|tool| {
        folded == *tool
            || folded.strip_prefix(tool).is_some_and(|rest| {
                !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
            })
    })
}

fn fold_tool(name: &str) -> String {
    name.chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '_' && *ch != '-')
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

pub fn wordlist_dir_from_env_or_manifest() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("VRCLIBMAN_WORDLIST_DIR") {
        return std::path::PathBuf::from(dir);
    }
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/wordlists")
}

pub fn mask_text(text: &str, keep: &[String], dir: &Path) -> Result<String, String> {
    let ja = std::fs::read_to_string(dir.join("Sexual.txt"))
        .map_err(|_| "日本語の語リストを読めません".to_string())?;
    let en = std::fs::read_to_string(dir.join("en"))
        .map_err(|_| "英語の語リストを読めません".to_string())?;
    Ok(mask_with_lists(text, keep, &ja, &en))
}

pub fn mask_with_lists(text: &str, keep: &[String], ja: &str, en: &str) -> String {
    let mut protected = text.to_string();
    let mut phrases: Vec<String> = keep
        .iter()
        .cloned()
        .chain(ALLOW.iter().map(|item| (*item).to_string()))
        .chain(tool_names().iter().map(|item| (*item).to_string()))
        .filter(|item| !item.is_empty())
        .collect();
    phrases.sort_by_key(|item| std::cmp::Reverse(item.chars().count()));
    phrases.dedup();
    let mut slots = Vec::new();
    for (index, phrase) in phrases.iter().enumerate() {
        if !protected.contains(phrase.as_str()) {
            continue;
        }
        let token = format!("\u{E000}{index}\u{E001}");
        protected = protected.replace(phrase, &token);
        slots.push((token, phrase.clone()));
    }
    let mut words: Vec<&str> = ja
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    words.sort_by_key(|word| std::cmp::Reverse(word.chars().count()));
    words.dedup();
    for word in words {
        if ALLOW.iter().any(|kept| *kept == word) {
            continue;
        }
        if protected.contains(word) {
            protected = protected.replace(word, "[省略]");
        }
    }
    for line in en.lines() {
        let word = line.trim();
        if word.is_empty() {
            continue;
        }
        protected = replace_english(&protected, word);
    }
    for (token, phrase) in slots {
        protected = protected.replace(&token, &phrase);
    }
    protected
}

fn replace_english(text: &str, word: &str) -> String {
    if word.is_empty() || !text.to_ascii_lowercase().contains(&word.to_ascii_lowercase()) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let needle: Vec<char> = word.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if starts_english(&chars, index, &needle) {
            out.push_str("[省略]");
            index += needle.len();
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

fn starts_english(chars: &[char], index: usize, needle: &[char]) -> bool {
    if index + needle.len() > chars.len() {
        return false;
    }
    if !chars[index..index + needle.len()]
        .iter()
        .zip(needle)
        .all(|(left, right)| left.eq_ignore_ascii_case(right))
    {
        return false;
    }
    let before = index.checked_sub(1).map(|at| chars[at]);
    let after = chars.get(index + needle.len()).copied();
    !before.is_some_and(|ch| ch.is_ascii_alphanumeric())
        && !after.is_some_and(|ch| ch.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_a_known_digest() {
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn vendored_wordlists_are_the_pinned_bytes() {
        let dir = wordlist_dir_from_env_or_manifest();
        let ja = std::fs::read(dir.join("Sexual.txt")).unwrap();
        let en = std::fs::read(dir.join("en")).unwrap();
        assert_eq!(sha256(&ja), "3cb18f94ebe23194bcf3a2f998690e0c723cb17ac9e5f458ec2e8960478a6043");
        assert_eq!(sha256(&en), "af851ecef1d5f212caba17339b12ac39cc2fef7d78c74876f67237644fcee8bd");
    }

    #[test]
    fn japanese_masks_longest_first_and_keeps_the_allowlist() {
        let ja = "成人\n成人向け\n裸足\n";
        let en = "secret\n";
        let masked = mask_with_lists("成人向けの説明と裸足と secret と SECRET.", &[], ja, en);
        assert!(masked.contains("成人向け"));
        assert!(masked.contains("裸足"));
        assert!(masked.contains("[省略]"), "{masked}");
        assert!(!masked.to_lowercase().contains("secret"));
    }

    #[test]
    fn english_respects_word_boundaries_and_known_names_stay() {
        let ja = "";
        let en = "ass\n";
        let masked = mask_with_lists(
            "class asset と クルミ ver1.2 と lilToon",
            &["クルミ ver1.2".into()],
            ja,
            en,
        );
        assert!(masked.contains("class"));
        assert!(masked.contains("asset"));
        assert!(masked.contains("クルミ ver1.2"));
        assert!(masked.contains("lilToon"));
    }

    #[test]
    fn tool_names_are_not_avatar_names_and_versions_stay() {
        assert!(is_known_tool("lilToon"));
        assert!(is_known_tool("Modular Avatar"));
        assert!(is_known_tool("Poiyomi 1.2"));
        assert!(!is_known_tool("クルミ ver1.2"));
    }

    fn sha256(bytes: &[u8]) -> String {
        let digest = sha256_bytes(bytes);
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn sha256_bytes(message: &[u8]) -> [u8; 32] {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];
        let mut state = [
            0x6a09e667u32, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
            0x5be0cd19,
        ];
        let bit_len = (message.len() as u64).wrapping_mul(8);
        let mut data = message.to_vec();
        data.push(0x80);
        while (data.len() % 64) != 56 {
            data.push(0);
        }
        data.extend(bit_len.to_be_bytes());
        for chunk in data.chunks(64) {
            let mut words = [0u32; 64];
            for index in 0..16 {
                words[index] = u32::from_be_bytes(chunk[index * 4..index * 4 + 4].try_into().unwrap());
            }
            for index in 16..64 {
                let s0 = words[index - 15].rotate_right(7)
                    ^ words[index - 15].rotate_right(18)
                    ^ (words[index - 15] >> 3);
                let s1 = words[index - 2].rotate_right(17)
                    ^ words[index - 2].rotate_right(19)
                    ^ (words[index - 2] >> 10);
                words[index] = words[index - 16]
                    .wrapping_add(s0)
                    .wrapping_add(words[index - 7])
                    .wrapping_add(s1);
            }
            let mut a = state[0];
            let mut b = state[1];
            let mut c = state[2];
            let mut d = state[3];
            let mut e = state[4];
            let mut f = state[5];
            let mut g = state[6];
            let mut h = state[7];
            for index in 0..64 {
                let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
                let ch = (e & f) ^ ((!e) & g);
                let temp1 = h
                    .wrapping_add(s1)
                    .wrapping_add(ch)
                    .wrapping_add(K[index])
                    .wrapping_add(words[index]);
                let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
                let maj = (a & b) ^ (a & c) ^ (b & c);
                let temp2 = s0.wrapping_add(maj);
                h = g;
                g = f;
                f = e;
                e = d.wrapping_add(temp1);
                d = c;
                c = b;
                b = a;
                a = temp1.wrapping_add(temp2);
            }
            state[0] = state[0].wrapping_add(a);
            state[1] = state[1].wrapping_add(b);
            state[2] = state[2].wrapping_add(c);
            state[3] = state[3].wrapping_add(d);
            state[4] = state[4].wrapping_add(e);
            state[5] = state[5].wrapping_add(f);
            state[6] = state[6].wrapping_add(g);
            state[7] = state[7].wrapping_add(h);
        }
        let mut out = [0u8; 32];
        for (index, word) in state.iter().enumerate() {
            out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}
