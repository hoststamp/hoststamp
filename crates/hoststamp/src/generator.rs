// SPDX-License-Identifier: FSL-1.1-ALv2

use crate::wordlists::{EFF_LARGE, EFF_SHORT_1, EFF_SHORT_2, Wordlist};
use anyhow::{Result, anyhow, bail};
use serde::Deserialize;
use sha1::{Digest, Sha1};
use std::{collections::HashSet, fmt, str::FromStr, sync::OnceLock};
use uuid::Uuid;

pub const DEFAULT_WORDS: usize = 2;
pub const DEFAULT_WORD_LENGTH: usize = 5;
pub const DEFAULT_SUFFIX_LEN: usize = 5;
pub const MAX_SUFFIX_LEN: usize = 40;
pub const DEFAULT_COUNT: usize = 1;
pub const MAX_COUNT: usize = 50;
const BLOCKED_SERVER_WORDS: &str = include_str!("../data/blocked-server-words.txt");
static BLOCKED_WORDS: OnceLock<HashSet<&'static str>> = OnceLock::new();

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
pub enum Dictionary {
    #[serde(rename = "eff_short")]
    Short,
    #[serde(rename = "eff_short_2")]
    Short2,
    #[serde(rename = "eff_large")]
    Large,
}

impl Dictionary {
    pub fn wordlist(self) -> Wordlist {
        match self {
            Self::Short => EFF_SHORT_1,
            Self::Short2 => EFF_SHORT_2,
            Self::Large => EFF_LARGE,
        }
    }
}

impl fmt::Display for Dictionary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Short => f.write_str("eff_short"),
            Self::Short2 => f.write_str("eff_short_2"),
            Self::Large => f.write_str("eff_large"),
        }
    }
}

impl FromStr for Dictionary {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "eff_short" => Ok(Self::Short),
            "eff_short_2" => Ok(Self::Short2),
            "eff_large" => Ok(Self::Large),
            _ => Err(format!(
                "invalid dictionary {value:?}; expected eff_short, eff_short_2, or eff_large"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GenerateOptions {
    pub words: usize,
    pub word_length: Option<usize>,
    pub dictionary: Dictionary,
    pub suffix_hash: bool,
    pub suffix_len: usize,
}

impl Default for GenerateOptions {
    fn default() -> Self {
        Self {
            words: DEFAULT_WORDS,
            word_length: Some(DEFAULT_WORD_LENGTH),
            dictionary: Dictionary::Short,
            suffix_hash: true,
            suffix_len: DEFAULT_SUFFIX_LEN,
        }
    }
}

impl GenerateOptions {
    pub fn with_overrides(self, overrides: GenerateOverrides) -> Self {
        Self {
            words: overrides.words.unwrap_or(self.words),
            word_length: overrides.word_length.or(self.word_length),
            dictionary: overrides.dictionary.unwrap_or(self.dictionary),
            suffix_hash: overrides.suffix_hash.unwrap_or(self.suffix_hash),
            suffix_len: overrides.suffix_len.unwrap_or(self.suffix_len),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GenerateOverrides {
    pub words: Option<usize>,
    pub word_length: Option<usize>,
    pub dictionary: Option<Dictionary>,
    pub suffix_hash: Option<bool>,
    pub suffix_len: Option<usize>,
}

pub fn generate_hostname(options: GenerateOptions) -> Result<String> {
    validate_options(options)?;

    let wordlist = options.dictionary.wordlist();
    let words = filtered_words(wordlist, options.word_length);

    if words.len() < options.words {
        bail!(
            "dictionary {} does not contain {} unique words matching the requested filters",
            options.dictionary,
            options.words
        );
    }

    let mut selected = HashSet::with_capacity(options.words);
    let mut parts = Vec::with_capacity(options.words + usize::from(options.suffix_hash));

    while parts.len() < options.words {
        let index = random_index(words.len());
        if selected.insert(index) {
            parts.push(words[index].to_owned());
        }
    }

    if options.suffix_hash {
        parts.push(random_suffix(options.suffix_len));
    }

    Ok(parts.join("-"))
}

fn filtered_words(wordlist: Wordlist, word_length: Option<usize>) -> Vec<&'static str> {
    wordlist
        .words()
        .into_iter()
        .filter(|word| word_length.is_none_or(|length| word.chars().count() == length))
        .filter(|word| is_allowed_server_word(word))
        .collect()
}

fn is_allowed_server_word(word: &str) -> bool {
    !blocked_words().contains(word)
}

fn blocked_words() -> &'static HashSet<&'static str> {
    BLOCKED_WORDS.get_or_init(|| {
        BLOCKED_SERVER_WORDS
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect()
    })
}

pub fn validate_count(count: usize) -> Result<()> {
    if !(1..=MAX_COUNT).contains(&count) {
        bail!("count must be between 1 and {MAX_COUNT}");
    }

    Ok(())
}

fn validate_options(options: GenerateOptions) -> Result<()> {
    if options.words == 0 {
        bail!("words must be at least 1");
    }

    let word_count = options.dictionary.wordlist().entry_count();
    if options.words > word_count {
        bail!(
            "words must be no greater than {word_count} for {}",
            options.dictionary
        );
    }

    if options.suffix_hash && !(1..=MAX_SUFFIX_LEN).contains(&options.suffix_len) {
        bail!("suffix length must be between 1 and {MAX_SUFFIX_LEN}");
    }

    Ok(())
}

fn random_index(len: usize) -> usize {
    let len = u128::try_from(len).expect("usize fits in u128");
    usize::try_from(Uuid::new_v4().as_u128() % len).expect("bounded by original usize")
}

fn random_suffix(len: usize) -> String {
    let uuid = Uuid::new_v4();
    let digest = Sha1::digest(uuid.as_bytes());
    hex_prefix(&digest, len).expect("sha1 hex prefix")
}

fn hex_prefix(bytes: &[u8], len: usize) -> Option<String> {
    if len > bytes.len() * 2 {
        return None;
    }

    let mut hex = String::with_capacity(len);
    for byte in bytes {
        if hex.len() == len {
            break;
        }

        let remaining = len - hex.len();
        if remaining >= 2 {
            hex.push_str(&format!("{byte:02x}"));
        } else {
            hex.push(nibble_to_hex(byte >> 4)?);
        }
    }

    Some(hex)
}

fn nibble_to_hex(nibble: u8) -> Option<char> {
    match nibble {
        0..=9 => Some(char::from(b'0' + nibble)),
        10..=15 => Some(char::from(b'a' + (nibble - 10))),
        _ => None,
    }
}

pub fn parse_dictionary(value: &str) -> std::result::Result<Dictionary, String> {
    value.parse()
}

pub fn parse_count(value: &str) -> std::result::Result<usize, String> {
    let count = value
        .parse::<usize>()
        .map_err(|source| format!("invalid count {value:?}: {source}"))?;
    validate_count(count).map_err(|error| error.to_string())?;
    Ok(count)
}

pub fn parse_words(value: &str) -> std::result::Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|source| format!("invalid words {value:?}: {source}"))
        .and_then(|words| {
            if words == 0 {
                Err("words must be at least 1".to_owned())
            } else {
                Ok(words)
            }
        })
}

pub fn parse_word_length(value: &str) -> std::result::Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|source| format!("invalid word length {value:?}: {source}"))
        .and_then(|length| {
            if length == 0 {
                Err("word length must be at least 1".to_owned())
            } else {
                Ok(length)
            }
        })
}

pub fn parse_suffix_len(value: &str) -> std::result::Result<usize, String> {
    let len = value
        .parse::<usize>()
        .map_err(|source| format!("invalid suffix length {value:?}: {source}"))?;

    if !(1..=MAX_SUFFIX_LEN).contains(&len) {
        return Err(format!(
            "suffix length must be between 1 and {MAX_SUFFIX_LEN}"
        ));
    }

    Ok(len)
}

pub fn generate_many(options: GenerateOptions, count: usize) -> Result<Vec<String>> {
    validate_count(count)?;
    (0..count)
        .map(|_| generate_hostname(options))
        .collect::<Result<Vec<_>>>()
}

pub fn parse_hostname_parts(hostname: &str) -> Vec<&str> {
    hostname.split('-').collect()
}

pub fn ensure_no_repeated_words(hostname: &str, word_count: usize) -> Result<()> {
    let parts = parse_hostname_parts(hostname);
    if parts.len() < word_count {
        return Err(anyhow!("hostname has fewer parts than expected"));
    }

    let words = &parts[..word_count];
    let unique = words.iter().collect::<HashSet<_>>();
    if unique.len() != words.len() {
        bail!("hostname repeats a word");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_name_name_hash_by_default() {
        let hostname = generate_hostname(GenerateOptions::default()).expect("hostname");
        let parts = parse_hostname_parts(&hostname);

        assert_eq!(parts.len(), 3);
        assert_eq!(parts[2].len(), DEFAULT_SUFFIX_LEN);
        assert!(parts[2].chars().all(|c| c.is_ascii_hexdigit()));
        ensure_no_repeated_words(&hostname, DEFAULT_WORDS).expect("unique words");
    }

    #[test]
    fn can_generate_without_suffix_hash() {
        let hostname = generate_hostname(GenerateOptions {
            suffix_hash: false,
            ..GenerateOptions::default()
        })
        .expect("hostname");

        assert_eq!(parse_hostname_parts(&hostname).len(), DEFAULT_WORDS);
    }

    #[test]
    fn filters_words_by_exact_length() {
        let hostname = generate_hostname(GenerateOptions {
            word_length: Some(4),
            suffix_hash: false,
            ..GenerateOptions::default()
        })
        .expect("hostname");

        let parts = parse_hostname_parts(&hostname);
        assert_eq!(parts.len(), DEFAULT_WORDS);
        assert!(parts.iter().all(|part| part.chars().count() == 4));
    }

    #[test]
    fn errors_when_word_filter_has_no_matches() {
        let error = generate_hostname(GenerateOptions {
            word_length: Some(100),
            ..GenerateOptions::default()
        })
        .expect_err("error");

        assert!(error.to_string().contains("does not contain"));
    }

    #[test]
    fn validates_word_count_against_dictionary_size() {
        let error = generate_hostname(GenerateOptions {
            words: EFF_SHORT_1.entry_count() + 1,
            ..GenerateOptions::default()
        })
        .expect_err("error");

        assert!(error.to_string().contains("words must be no greater than"));
    }

    #[test]
    fn suffix_len_is_ignored_when_suffix_hash_is_disabled() {
        let hostname = generate_hostname(GenerateOptions {
            suffix_hash: false,
            suffix_len: MAX_SUFFIX_LEN + 1,
            ..GenerateOptions::default()
        })
        .expect("hostname");

        assert_eq!(parse_hostname_parts(&hostname).len(), DEFAULT_WORDS);
    }

    #[test]
    fn filters_blocked_server_words() {
        let words = filtered_words(EFF_SHORT_1, Some(5));

        assert!(!words.contains(&"chump"));
        assert!(!words.contains(&"moist"));
        assert!(!words.contains(&"stank"));
        assert!(!words.contains(&"theft"));
        assert!(!words.contains(&"trump"));
        assert!(!words.contains(&"xerox"));
    }

    #[test]
    fn parses_and_displays_all_dictionaries() {
        assert_eq!(parse_dictionary("eff_short"), Ok(Dictionary::Short));
        assert_eq!(parse_dictionary("eff_short_2"), Ok(Dictionary::Short2));
        assert_eq!(parse_dictionary("eff_large"), Ok(Dictionary::Large));
        assert_eq!(Dictionary::Short.to_string(), "eff_short");
        assert_eq!(Dictionary::Short2.to_string(), "eff_short_2");
        assert_eq!(Dictionary::Large.to_string(), "eff_large");
        assert!(parse_dictionary("short").is_err());
        assert_eq!(Dictionary::Large.wordlist().entry_count(), 7776);
    }

    #[test]
    fn parse_helpers_reject_invalid_values() {
        assert!(parse_count("nope").is_err());
        assert!(parse_words("nope").is_err());
        assert!(parse_words("0").is_err());
        assert!(parse_word_length("nope").is_err());
        assert!(parse_word_length("0").is_err());
        assert!(parse_suffix_len("nope").is_err());
    }

    #[test]
    fn detects_repeated_or_missing_hostname_words() {
        assert!(ensure_no_repeated_words("alpha-alpha-12345", 2).is_err());
        assert!(ensure_no_repeated_words("alpha", 2).is_err());
    }

    #[test]
    fn count_is_capped_at_fifty() {
        assert!(validate_count(MAX_COUNT).is_ok());
        assert!(validate_count(MAX_COUNT + 1).is_err());
    }

    #[test]
    fn suffix_len_is_capped_at_sha1_hex_length() {
        assert!(parse_suffix_len("40").is_ok());
        assert!(parse_suffix_len("41").is_err());
    }
}
