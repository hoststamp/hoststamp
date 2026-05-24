// SPDX-License-Identifier: FSL-1.1-ALv2

use sha2::{Digest, Sha256};
use std::collections::HashSet;

include!(concat!(env!("OUT_DIR"), "/dictionary_generated.rs"));

pub fn default_dictionary_version() -> u32 {
    DEFAULT_DICTIONARY_VERSION
}

pub fn default_blocklist_version() -> u32 {
    DEFAULT_BLOCKLIST_VERSION
}

pub fn category_names() -> &'static [&'static str] {
    DEFAULT_CATEGORY_NAMES
}

pub fn dictionary_version(version: u32) -> Option<&'static DictionaryVersionMeta> {
    DICTIONARY_VERSIONS
        .iter()
        .find(|candidate| candidate.version == version)
}

pub fn blocklist_version(version: u32) -> Option<&'static BlocklistVersionMeta> {
    BLOCKLIST_VERSIONS
        .iter()
        .find(|candidate| candidate.version == version)
}

pub fn dictionary_version_hash(version: u32) -> Option<&'static str> {
    dictionary_version(version).map(|version| version.hash)
}

pub fn blocklist_version_hash(version: u32) -> Option<&'static str> {
    blocklist_version(version).map(|version| version.hash)
}

pub fn category_lengths(category: &str) -> Option<Vec<(usize, Vec<&'static str>)>> {
    category_lengths_for_versions(
        DEFAULT_DICTIONARY_VERSION,
        DEFAULT_BLOCKLIST_VERSION,
        category,
    )
}

pub fn category_lengths_for_versions(
    dictionary_version: u32,
    blocklist_version: u32,
    category: &str,
) -> Option<Vec<(usize, Vec<&'static str>)>> {
    let dictionary = self::dictionary_version(dictionary_version)?;
    let category = dictionary
        .categories
        .iter()
        .find(|candidate| candidate.category == category)?;
    let blocked = alpha_blocklist(blocklist_version)?;
    let mut buckets = Vec::<(usize, Vec<&'static str>)>::new();
    for word_id in category.word_ids {
        let word = ALLOWED_WORDS[usize::from(*word_id)];
        if blocked.contains(word) {
            continue;
        }
        let length = word.chars().count();
        match buckets.iter_mut().find(|(bucket, _)| *bucket == length) {
            Some((_, words)) => words.push(word),
            None => buckets.push((length, vec![word])),
        }
    }
    buckets.sort_by_key(|(length, _)| *length);
    Some(buckets)
}

pub fn words(category: &str, length: usize) -> Option<Vec<&'static str>> {
    category_lengths(category).and_then(|lengths| {
        lengths
            .into_iter()
            .find_map(|(bucket_length, words)| (bucket_length == length).then_some(words))
    })
}

pub fn words_in_category(category: &str) -> Option<Vec<&'static str>> {
    category_lengths(category)
        .map(|lengths| lengths.into_iter().flat_map(|(_, words)| words).collect())
}

pub fn total_words(category: &str) -> Option<usize> {
    category_lengths(category).map(|lengths| lengths.iter().map(|(_, words)| words.len()).sum())
}

pub fn resolve_words(
    dictionary_version: u32,
    blocklist_version: u32,
    categories: &[String],
    lengths: Option<&[usize]>,
) -> Result<Vec<&'static str>, String> {
    let dictionary = self::dictionary_version(dictionary_version)
        .ok_or_else(|| format!("unknown dictionary version {dictionary_version}"))?;
    let blocked = alpha_blocklist(blocklist_version)
        .ok_or_else(|| format!("unknown blocklist version {blocklist_version}"))?;
    let mut seen = HashSet::new();
    let mut words = Vec::new();

    for category_name in categories {
        let category = dictionary
            .categories
            .iter()
            .find(|candidate| candidate.category == category_name)
            .ok_or_else(|| format!("unknown category {category_name:?}"))?;
        for word_id in category.word_ids {
            let word = ALLOWED_WORDS[usize::from(*word_id)];
            if blocked.contains(word)
                || lengths.is_some_and(|allowed| !allowed.contains(&word.len()))
            {
                continue;
            }
            if seen.insert(word) {
                words.push(word);
            }
        }
    }

    Ok(words)
}

pub fn resolved_pool_hash(words: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"hoststamp-resolved-word-pool-v1");
    for word in words {
        hasher.update([0]);
        hasher.update(word.as_bytes());
    }
    hex_digest(hasher.finalize().as_slice())
}

pub fn blocklist_words(version: u32) -> Option<Vec<&'static str>> {
    let version = blocklist_version(version)?;
    let mut words = Vec::new();
    for source in version.sources {
        for word_id in source.word_ids {
            words.push(BLOCKED_WORDS[usize::from(*word_id)]);
        }
    }
    words.sort_unstable();
    words.dedup();
    Some(words)
}

pub fn sources() -> &'static [SourceMeta] {
    SOURCES
}

pub fn artifact_sha256() -> &'static str {
    ARTIFACT_SHA256
}

pub fn source_by_id(id: &str) -> Option<&'static SourceMeta> {
    SOURCES.iter().find(|source| source.id == id)
}

fn alpha_blocklist(version: u32) -> Option<HashSet<&'static str>> {
    Some(
        blocklist_words(version)?
            .into_iter()
            .filter(|word| word.bytes().all(|byte| byte.is_ascii_lowercase()))
            .collect(),
    )
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push_str(&format!("{byte:02x}"));
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_generated_metadata() {
        assert_eq!(SCHEMA_VERSION, 1);
        assert!(!GENERATED_AT.is_empty());
        assert_eq!(artifact_sha256().len(), 64);
        assert_eq!(default_dictionary_version(), 1);
        assert_eq!(default_blocklist_version(), 1);
        assert!(category_names().contains(&"adjective"));
        assert!(category_names().contains(&"animal"));
        assert!(!sources().is_empty());
    }

    #[test]
    fn looks_up_words_by_category_and_length() {
        let adjective_words = words("adjective", 5).expect("adjective words");

        assert!(adjective_words.contains(&"quick"));
        assert_eq!(
            adjective_words
                .iter()
                .filter(|word| **word == "quick")
                .count(),
            1
        );
        assert!(adjective_words.iter().all(|word| word.chars().count() == 5));
        assert!(words("missing", 5).is_none());
        assert!(words("adjective", 99).is_none());
    }

    #[test]
    fn exposes_version_hashes_and_sources() {
        let dictionary = dictionary_version(default_dictionary_version()).expect("dictionary");
        let blocklist = blocklist_version(default_blocklist_version()).expect("blocklist");

        assert_eq!(dictionary.hash.len(), 64);
        assert_eq!(blocklist.hash.len(), 64);
        assert!(dictionary.sources.contains(&"eff-large"));
        assert!(source_by_id("eff-large").is_some());
    }

    #[test]
    fn returns_all_words_in_category() {
        let all_words = words_in_category("planet").expect("planet words");
        let total = total_words("planet").expect("planet total");

        assert_eq!(all_words.len(), total);
        assert!(all_words.contains(&"earth"));
        assert!(words_in_category("missing").is_none());
        assert!(total_words("missing").is_none());
        assert!(source_by_id("missing").is_none());
    }

    #[test]
    fn resolves_words_with_blocklist_and_hashes_pool() {
        let categories = vec!["adjective".to_owned(), "adverb".to_owned()];
        let words = resolve_words(
            default_dictionary_version(),
            default_blocklist_version(),
            &categories,
            Some(&[5]),
        )
        .expect("words");
        let hash = resolved_pool_hash(&words);

        assert!(!words.is_empty());
        assert_eq!(hash.len(), 64);
        assert!(
            !blocklist_words(default_blocklist_version())
                .expect("blocklist")
                .iter()
                .any(|blocked| words.contains(blocked))
        );
    }
}
