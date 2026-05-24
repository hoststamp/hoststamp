// SPDX-License-Identifier: FSL-1.1-ALv2

include!(concat!(env!("OUT_DIR"), "/dictionary_generated.rs"));

pub fn category_names() -> &'static [&'static str] {
    CATEGORY_NAMES
}

pub fn category_lengths(category: &str) -> Option<&'static [(usize, &'static [&'static str])]> {
    CATEGORIES
        .iter()
        .find_map(|(name, lengths)| (*name == category).then_some(*lengths))
}

pub fn words(category: &str, length: usize) -> Option<&'static [&'static str]> {
    category_lengths(category).and_then(|lengths| {
        lengths
            .iter()
            .find_map(|(bucket_length, words)| (*bucket_length == length).then_some(*words))
    })
}

pub fn words_in_category(category: &str) -> Option<Vec<&'static str>> {
    category_lengths(category).map(|lengths| {
        lengths
            .iter()
            .flat_map(|(_, words)| words.iter().copied())
            .collect()
    })
}

pub fn total_words(category: &str) -> Option<usize> {
    category_lengths(category).map(|lengths| lengths.iter().map(|(_, words)| words.len()).sum())
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

pub fn category_source_ids(category: &str) -> Option<&'static [&'static str]> {
    CATEGORY_SOURCE_IDS
        .iter()
        .find_map(|(name, source_ids)| (*name == category).then_some(*source_ids))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_generated_metadata() {
        assert_eq!(SCHEMA_VERSION, 1);
        assert!(!GENERATED_AT.is_empty());
        assert_eq!(artifact_sha256().len(), 64);
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
    fn exposes_category_source_ids() {
        let source_ids = category_source_ids("diceware").expect("diceware sources");

        assert!(source_ids.contains(&"eff-short-1"));
        assert!(source_ids.contains(&"eff-short-2"));
        assert!(source_ids.contains(&"eff-large"));
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
        assert!(category_source_ids("missing").is_none());
        assert!(source_by_id("missing").is_none());
    }
}
