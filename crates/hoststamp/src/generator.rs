// SPDX-License-Identifier: FSL-1.1-ALv2

use crate::dictionary;
use anyhow::{Context, Result, anyhow, bail};
use sha2::{Digest, Sha256};
use sqids::Sqids;
use std::collections::HashSet;
use uuid::Uuid;

pub const DEFAULT_WORD_LENGTH: usize = 5;
pub const DEFAULT_SUFFIX_MIN_LENGTH: usize = 5;
pub const MIN_SUFFIX_MIN_LENGTH: usize = 4;
pub const MAX_SUFFIX_MIN_LENGTH: usize = 13;
pub const SUFFIX_ALPHABET_SIZE: u32 = 36;
pub const DEFAULT_COUNT: usize = 1;
pub const MAX_COUNT: usize = 50;
pub const SUFFIX_ALPHABET: &str = "0123456789abcdefghijklmnopqrstuvwxyz";
pub const ATOMIC_MIN_VALUE: i64 = 1;
pub const ATOMIC_STORAGE_MAX_VALUE: i64 = i64::MAX;
pub const DEFAULT_WORD1_CATEGORIES: &[&str] = &["adjective", "adverb"];
pub const DEFAULT_WORD2_CATEGORIES: &[&str] = &[
    "animal",
    "deity",
    "element",
    "gemstone",
    "metal",
    "monster",
    "name",
    "noun",
    "ocean",
    "phonetic",
    "planet",
    "river",
    "scientist",
    "star",
    "stone",
    "tolkien",
    "wind",
];

#[derive(Debug, Clone)]
pub struct GenerateOptions {
    pub word1_enabled: bool,
    pub word1_lengths: Option<Vec<usize>>,
    pub word1_categories: Vec<String>,
    pub word2_enabled: bool,
    pub word2_lengths: Option<Vec<usize>>,
    pub word2_categories: Vec<String>,
    pub suffix_enabled: bool,
    pub suffix_min_length: usize,
    pub count: usize,
}

impl Default for GenerateOptions {
    fn default() -> Self {
        Self {
            word1_enabled: true,
            word1_lengths: Some(vec![DEFAULT_WORD_LENGTH]),
            word1_categories: DEFAULT_WORD1_CATEGORIES
                .iter()
                .map(|category| (*category).to_owned())
                .collect(),
            word2_enabled: true,
            word2_lengths: Some(vec![DEFAULT_WORD_LENGTH]),
            word2_categories: DEFAULT_WORD2_CATEGORIES
                .iter()
                .map(|category| (*category).to_owned())
                .collect(),
            suffix_enabled: true,
            suffix_min_length: DEFAULT_SUFFIX_MIN_LENGTH,
            count: DEFAULT_COUNT,
        }
    }
}

impl GenerateOptions {
    pub fn with_overrides(&self, overrides: GenerateOverrides) -> Self {
        Self {
            word1_enabled: overrides.word1_enabled.unwrap_or(self.word1_enabled),
            word1_lengths: overrides
                .word1_lengths
                .unwrap_or_else(|| self.word1_lengths.clone()),
            word1_categories: overrides
                .word1_categories
                .unwrap_or_else(|| self.word1_categories.clone()),
            word2_enabled: overrides.word2_enabled.unwrap_or(self.word2_enabled),
            word2_lengths: overrides
                .word2_lengths
                .unwrap_or_else(|| self.word2_lengths.clone()),
            word2_categories: overrides
                .word2_categories
                .unwrap_or_else(|| self.word2_categories.clone()),
            suffix_enabled: overrides.suffix_enabled.unwrap_or(self.suffix_enabled),
            suffix_min_length: overrides
                .suffix_min_length
                .unwrap_or(self.suffix_min_length),
            count: overrides.count.unwrap_or(self.count),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct GenerateOverrides {
    pub word1_enabled: Option<bool>,
    pub word1_lengths: Option<Option<Vec<usize>>>,
    pub word1_categories: Option<Vec<String>>,
    pub word2_enabled: Option<bool>,
    pub word2_lengths: Option<Option<Vec<usize>>>,
    pub word2_categories: Option<Vec<String>>,
    pub suffix_enabled: Option<bool>,
    pub suffix_min_length: Option<usize>,
    pub count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapacityReport {
    pub word1_count: Option<usize>,
    pub word2_count: Option<usize>,
    pub overlapping_words: usize,
    pub unique_word_combinations: u128,
    pub suffix_enabled: bool,
    pub suffix_min_length: Option<usize>,
    pub suffix_variants: Option<String>,
    pub suffix_bits: Option<usize>,
    pub random_fallback_max_value: Option<u64>,
    pub atomic_storage_max_value: Option<i64>,
    pub total_variants: String,
}

struct SelectionPlan {
    cells: Vec<SelectionCell>,
    total: usize,
}

struct SelectionCell {
    upper_bound: usize,
    words: Vec<&'static str>,
}

pub fn generate_hostname(options: GenerateOptions) -> Result<String> {
    let plans = build_word_plans(&options)?;
    generate_hostname_with_plans(&options, &plans)
}

pub fn generate_many(options: GenerateOptions) -> Result<Vec<String>> {
    validate_count(options.count)?;
    let plans = build_word_plans(&options)?;
    (0..options.count)
        .map(|_| generate_hostname_with_plans(&options, &plans))
        .collect::<Result<Vec<_>>>()
}

pub fn generate_many_with_suffix<F>(
    options: GenerateOptions,
    mut next_suffix: F,
) -> Result<Vec<String>>
where
    F: FnMut() -> Result<String>,
{
    validate_count(options.count)?;
    let plans = build_word_plans(&options)?;
    (0..options.count)
        .map(|_| generate_hostname_with_plans_and_suffix(&options, &plans, &mut next_suffix))
        .collect::<Result<Vec<_>>>()
}

pub fn capacity_report(options: &GenerateOptions) -> Result<CapacityReport> {
    let plans = build_word_plans_with_distinct_check(options, false)?;
    let word1_count = options.word1_enabled.then(|| plans[0].total);
    let word2_count = if options.word1_enabled && options.word2_enabled {
        Some(plans[1].total)
    } else if options.word2_enabled {
        Some(plans[0].total)
    } else {
        None
    };

    let overlapping_words = if options.word1_enabled && options.word2_enabled {
        let word1_words = plans[0].all_words().collect::<HashSet<_>>();
        plans[1]
            .all_words()
            .filter(|word| word1_words.contains(word))
            .count()
    } else {
        0
    };
    let unique_word_combinations = match (word1_count, word2_count) {
        (Some(word1), Some(word2)) => {
            let total = (word1 as u128) * (word2 as u128);
            total - (overlapping_words as u128)
        }
        (Some(word1), None) => word1 as u128,
        (None, Some(word2)) => word2 as u128,
        (None, None) => 1,
    };

    let (suffix_min_length, suffix_variants, suffix_bits, random_fallback_max_value) =
        if options.suffix_enabled {
            let variants = decimal_power(SUFFIX_ALPHABET_SIZE, options.suffix_min_length);
            (
                Some(options.suffix_min_length),
                Some(variants),
                Some(suffix_entropy_bits(options.suffix_min_length)),
                Some(random_fallback_max_value(options.suffix_min_length)?),
            )
        } else {
            (None, None, None, None)
        };
    let total_variants = if let Some(suffix_variants) = suffix_variants.as_deref() {
        decimal_multiply(suffix_variants, unique_word_combinations)
    } else {
        unique_word_combinations.to_string()
    };

    Ok(CapacityReport {
        word1_count,
        word2_count,
        overlapping_words,
        unique_word_combinations,
        suffix_enabled: options.suffix_enabled,
        suffix_min_length,
        suffix_variants,
        suffix_bits,
        random_fallback_max_value,
        atomic_storage_max_value: options.suffix_enabled.then_some(ATOMIC_STORAGE_MAX_VALUE),
        total_variants,
    })
}

pub fn validate_generate_options(options: &GenerateOptions) -> Result<()> {
    build_word_plans(options).map(|_| ())
}

fn generate_hostname_with_plans(
    options: &GenerateOptions,
    plans: &[SelectionPlan],
) -> Result<String> {
    generate_hostname_with_plans_and_suffix(options, plans, &mut || {
        random_sqids_suffix(options.suffix_min_length)
    })
}

fn generate_hostname_with_plans_and_suffix<F>(
    options: &GenerateOptions,
    plans: &[SelectionPlan],
    next_suffix: &mut F,
) -> Result<String>
where
    F: FnMut() -> Result<String>,
{
    let mut selected = HashSet::with_capacity(plans.len());
    let mut parts = Vec::with_capacity(plans.len() + usize::from(options.suffix_enabled));

    for plan in plans {
        loop {
            let word = plan.random_word();
            if selected.insert(word) {
                parts.push(word.to_owned());
                break;
            }
            if selected.len() == plan.total {
                bail!("selected word positions exhaust their pool before producing distinct words");
            }
        }
    }

    if options.suffix_enabled {
        parts.push(next_suffix()?);
    }

    if parts.is_empty() {
        bail!("nothing to generate: all positions are disabled");
    }

    Ok(parts.join("-"))
}

fn build_word_plans(options: &GenerateOptions) -> Result<Vec<SelectionPlan>> {
    build_word_plans_with_distinct_check(options, true)
}

fn build_word_plans_with_distinct_check(
    options: &GenerateOptions,
    require_enough_distinct_words: bool,
) -> Result<Vec<SelectionPlan>> {
    validate_options(options)?;

    let mut plans = Vec::new();
    if options.word1_enabled {
        plans.push(SelectionPlan::build(
            &options.word1_categories,
            options.word1_lengths.as_deref(),
            "word1",
        )?);
    }
    if options.word2_enabled {
        plans.push(SelectionPlan::build(
            &options.word2_categories,
            options.word2_lengths.as_deref(),
            "word2",
        )?);
    }

    if require_enough_distinct_words && plans.len() > 1 {
        let distinct = plans
            .iter()
            .flat_map(SelectionPlan::all_words)
            .collect::<HashSet<_>>()
            .len();
        if distinct < plans.len() {
            bail!(
                "selected categories contain {distinct} unique words across enabled word positions, but {required} were required",
                required = plans.len()
            );
        }
    }

    Ok(plans)
}

impl SelectionPlan {
    fn build(categories: &[String], lengths: Option<&[usize]>, position: &str) -> Result<Self> {
        if categories.is_empty() {
            bail!("{position} categories must not be empty");
        }
        if let Some(lengths) = lengths
            && lengths.is_empty()
        {
            bail!("{position} lengths must not be empty (omit to allow any length)");
        }

        let mut seen = HashSet::new();
        let mut total = 0usize;
        let mut cells = Vec::new();

        for category in categories {
            let buckets = dictionary::category_lengths(category)
                .ok_or_else(|| anyhow!("unknown category {category:?}"))?;
            for (length, words) in buckets {
                if lengths.is_some_and(|allowed| !allowed.contains(length)) {
                    continue;
                }

                let cell_words = words
                    .iter()
                    .copied()
                    .filter(|word| seen.insert(*word))
                    .collect::<Vec<_>>();
                if cell_words.is_empty() {
                    continue;
                }

                total += cell_words.len();
                cells.push(SelectionCell {
                    upper_bound: total,
                    words: cell_words,
                });
            }
        }

        if total == 0 {
            bail!("{position} categories do not contain words matching the requested filters");
        }

        Ok(Self { cells, total })
    }

    fn random_word(&self) -> &'static str {
        let index = random_index(self.total);
        let cell = self
            .cells
            .iter()
            .find(|cell| index < cell.upper_bound)
            .expect("index is within total");
        let lower_bound = cell.upper_bound - cell.words.len();
        cell.words[index - lower_bound]
    }

    fn all_words(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.cells
            .iter()
            .flat_map(|cell| cell.words.iter().copied())
    }
}

fn validate_options(options: &GenerateOptions) -> Result<()> {
    if !options.word1_enabled && !options.word2_enabled && !options.suffix_enabled {
        bail!("at least one position must be enabled");
    }

    if options.suffix_enabled
        && !(MIN_SUFFIX_MIN_LENGTH..=MAX_SUFFIX_MIN_LENGTH).contains(&options.suffix_min_length)
    {
        bail!(
            "suffix minimum length must be between {MIN_SUFFIX_MIN_LENGTH} and {MAX_SUFFIX_MIN_LENGTH}"
        );
    }

    validate_count(options.count)?;

    Ok(())
}

pub fn compute_profile_suffix(
    profile_id: Uuid,
    atomic_value: i64,
    suffix_min_length: usize,
) -> Result<String> {
    if atomic_value < 1 {
        bail!("atomic value must be at least 1");
    }
    let value = u64::try_from(atomic_value).expect("positive i64 fits in u64");
    sqids_for_profile(Some(profile_id), suffix_min_length)?
        .encode(&[value])
        .map_err(Into::into)
}

pub fn random_sqids_suffix(suffix_min_length: usize) -> Result<String> {
    let max = random_fallback_max_value(suffix_min_length)?;
    let value = random_u64(1, max);
    sqids_for_profile(None, suffix_min_length)?
        .encode(&[value])
        .map_err(Into::into)
}

fn random_index(len: usize) -> usize {
    assert!(len > 0, "random_index requires a non-empty range");
    // Hoststamp's word buckets are small enough that modulo bias across UUIDv4's
    // random space is operationally negligible for hostname generation.
    let len = u128::try_from(len).expect("usize fits in u128");
    usize::try_from(Uuid::new_v4().as_u128() % len).expect("bounded by original usize")
}

fn checked_base36_capacity(suffix_min_length: usize) -> Option<u128> {
    let mut value = 1u128;
    for _ in 0..suffix_min_length {
        value = value.checked_mul(u128::from(SUFFIX_ALPHABET_SIZE))?;
    }
    Some(value)
}

pub fn fixed_suffix_variants(suffix_min_length: usize) -> Result<u128> {
    if !(MIN_SUFFIX_MIN_LENGTH..=MAX_SUFFIX_MIN_LENGTH).contains(&suffix_min_length) {
        bail!(
            "suffix minimum length must be between {MIN_SUFFIX_MIN_LENGTH} and {MAX_SUFFIX_MIN_LENGTH}"
        );
    }
    checked_base36_capacity(suffix_min_length).context("suffix capacity exceeds u128")
}

pub fn random_fallback_max_value(suffix_min_length: usize) -> Result<u64> {
    let variants = fixed_suffix_variants(suffix_min_length)?;
    let variants = variants.min(ATOMIC_STORAGE_MAX_VALUE as u128);
    u64::try_from(variants / 2).context("suffix random fallback capacity exceeds u64")
}

fn random_u64(min: u64, max: u64) -> u64 {
    assert!(min <= max, "random_u64 requires a valid range");
    let span = u128::from(max - min) + 1;
    let value = u64::try_from(Uuid::new_v4().as_u128() % span).expect("bounded by u64 span");
    min + value
}

fn sqids_for_profile(profile_id: Option<Uuid>, suffix_min_length: usize) -> Result<Sqids> {
    let min_length = u8::try_from(suffix_min_length).context("suffix minimum length exceeds u8")?;
    Sqids::builder()
        .alphabet(profile_alphabet(profile_id).chars().collect())
        .min_length(min_length)
        .blocklist(sqids::default_blocklist())
        .build()
        .map_err(Into::into)
}

fn profile_alphabet(profile_id: Option<Uuid>) -> String {
    let Some(profile_id) = profile_id else {
        return SUFFIX_ALPHABET.to_owned();
    };

    let mut chars = SUFFIX_ALPHABET.chars().collect::<Vec<_>>();
    for index in (1..chars.len()).rev() {
        let mut hasher = Sha256::new();
        hasher.update(profile_id.as_bytes());
        hasher.update(index.to_be_bytes());
        let digest = hasher.finalize();
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&digest[..8]);
        let swap_with = usize::try_from(u64::from_be_bytes(bytes) % ((index + 1) as u64))
            .expect("swap index fits usize");
        chars.swap(index, swap_with);
    }

    chars.into_iter().collect()
}

fn suffix_entropy_bits(suffix_min_length: usize) -> usize {
    ((suffix_min_length as f64) * f64::from(SUFFIX_ALPHABET_SIZE).log2()).floor() as usize
}

fn decimal_power(base: u32, exponent: usize) -> String {
    let mut value = "1".to_owned();
    for _ in 0..exponent {
        value = decimal_multiply(&value, u128::from(base));
    }
    value
}

fn decimal_multiply(value: &str, multiplier: u128) -> String {
    if multiplier == 0 || value == "0" {
        return "0".to_owned();
    }

    let mut carry = 0u128;
    let mut digits = Vec::with_capacity(value.len() + 40);
    for digit in value.bytes().rev() {
        let product = u128::from(digit - b'0') * multiplier + carry;
        digits.push(char::from(
            b'0' + u8::try_from(product % 10).expect("digit"),
        ));
        carry = product / 10;
    }
    while carry > 0 {
        digits.push(char::from(b'0' + u8::try_from(carry % 10).expect("digit")));
        carry /= 10;
    }

    digits.iter().rev().collect()
}

pub fn parse_categories(value: &str) -> std::result::Result<Vec<String>, String> {
    let categories = value
        .split(',')
        .map(str::trim)
        .filter(|category| !category.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();

    if categories.is_empty() {
        Err("category list must not be empty".to_owned())
    } else {
        Ok(categories)
    }
}

pub fn parse_lengths(value: &str) -> std::result::Result<Option<Vec<usize>>, String> {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("any") {
        return Ok(None);
    }

    let mut lengths = Vec::new();
    for part in trimmed.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let length = part
            .parse::<usize>()
            .map_err(|source| format!("invalid length {part:?}: {source}"))?;
        if length == 0 {
            return Err("length must be at least 1".to_owned());
        }
        lengths.push(length);
    }

    if lengths.is_empty() {
        return Err("length list must not be empty (use \"any\" for no length filter)".to_owned());
    }

    Ok(Some(lengths))
}

pub fn parse_suffix_min_length(value: &str) -> std::result::Result<usize, String> {
    let length = value
        .parse::<usize>()
        .map_err(|source| format!("invalid suffix minimum length {value:?}: {source}"))?;
    if !(MIN_SUFFIX_MIN_LENGTH..=MAX_SUFFIX_MIN_LENGTH).contains(&length) {
        return Err(format!(
            "suffix minimum length must be between {MIN_SUFFIX_MIN_LENGTH} and {MAX_SUFFIX_MIN_LENGTH}"
        ));
    }
    Ok(length)
}

pub fn parse_count(value: &str) -> std::result::Result<usize, String> {
    let count = value
        .parse::<usize>()
        .map_err(|source| format!("invalid count {value:?}: {source}"))?;
    validate_count(count).map_err(|error| error.to_string())?;
    Ok(count)
}

pub fn validate_count(count: usize) -> Result<()> {
    if !(1..=MAX_COUNT).contains(&count) {
        bail!("count must be between 1 and {MAX_COUNT}");
    }
    Ok(())
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

pub fn is_base36_suffix(value: &str) -> bool {
    value
        .chars()
        .all(|character| character.is_ascii_digit() || character.is_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_word_word_hash_by_default() {
        let hostname = generate_hostname(GenerateOptions::default()).expect("hostname");
        let parts = parse_hostname_parts(&hostname);

        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].chars().count(), DEFAULT_WORD_LENGTH);
        assert_eq!(parts[1].chars().count(), DEFAULT_WORD_LENGTH);
        assert!(parts[2].len() >= DEFAULT_SUFFIX_MIN_LENGTH);
        assert!(is_base36_suffix(parts[2]));
        ensure_no_repeated_words(&hostname, 2).expect("unique words");
    }

    #[test]
    fn suffix_can_be_disabled() {
        let hostname = generate_hostname(GenerateOptions {
            suffix_enabled: false,
            ..GenerateOptions::default()
        })
        .expect("hostname");

        assert_eq!(parse_hostname_parts(&hostname).len(), 2);
    }

    #[test]
    fn word2_can_be_disabled() {
        let hostname = generate_hostname(GenerateOptions {
            word2_enabled: false,
            suffix_enabled: false,
            ..GenerateOptions::default()
        })
        .expect("hostname");

        let parts = parse_hostname_parts(&hostname);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].chars().count(), DEFAULT_WORD_LENGTH);
    }

    #[test]
    fn word1_can_be_disabled() {
        let hostname = generate_hostname(GenerateOptions {
            word1_enabled: false,
            suffix_enabled: false,
            ..GenerateOptions::default()
        })
        .expect("hostname");

        let parts = parse_hostname_parts(&hostname);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].chars().count(), DEFAULT_WORD_LENGTH);
    }

    #[test]
    fn suffix_only_when_both_words_disabled() {
        let hostname = generate_hostname(GenerateOptions {
            word1_enabled: false,
            word2_enabled: false,
            ..GenerateOptions::default()
        })
        .expect("hostname");

        assert!(hostname.len() >= DEFAULT_SUFFIX_MIN_LENGTH);
        assert!(is_base36_suffix(&hostname));
    }

    #[test]
    fn all_positions_disabled_is_an_error() {
        let error = generate_hostname(GenerateOptions {
            word1_enabled: false,
            word2_enabled: false,
            suffix_enabled: false,
            ..GenerateOptions::default()
        })
        .expect_err("error");

        assert!(error.to_string().contains("at least one position"));
    }

    #[test]
    fn filters_words_by_single_length() {
        let hostname = generate_hostname(GenerateOptions {
            word1_lengths: Some(vec![4]),
            word2_lengths: Some(vec![4]),
            suffix_enabled: false,
            ..GenerateOptions::default()
        })
        .expect("hostname");

        let parts = parse_hostname_parts(&hostname);
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|part| part.chars().count() == 4));
    }

    #[test]
    fn filters_words_by_length_set() {
        let hostname = generate_hostname(GenerateOptions {
            word1_lengths: Some(vec![4, 5, 6]),
            word2_lengths: Some(vec![4, 5, 6]),
            suffix_enabled: false,
            ..GenerateOptions::default()
        })
        .expect("hostname");

        let parts = parse_hostname_parts(&hostname);
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|part| {
            let n = part.chars().count();
            (4..=6).contains(&n)
        }));
    }

    #[test]
    fn any_length_is_allowed_when_lengths_is_none() {
        let hostname = generate_hostname(GenerateOptions {
            word1_lengths: None,
            word2_lengths: None,
            suffix_enabled: false,
            ..GenerateOptions::default()
        })
        .expect("hostname");

        assert_eq!(parse_hostname_parts(&hostname).len(), 2);
    }

    #[test]
    fn empty_lengths_is_an_error() {
        let error = generate_hostname(GenerateOptions {
            word1_lengths: Some(Vec::new()),
            ..GenerateOptions::default()
        })
        .expect_err("error");

        assert!(error.to_string().contains("lengths must not be empty"));
    }

    #[test]
    fn errors_when_word_filter_has_no_matches() {
        let error = generate_hostname(GenerateOptions {
            word1_lengths: Some(vec![100]),
            ..GenerateOptions::default()
        })
        .expect_err("error");

        assert!(error.to_string().contains("do not contain"));
    }

    #[test]
    fn errors_when_unknown_category_is_selected() {
        let error = generate_hostname(GenerateOptions {
            word1_categories: vec!["missing".to_owned()],
            ..GenerateOptions::default()
        })
        .expect_err("error");

        assert!(error.to_string().contains("unknown category"));
    }

    #[test]
    fn errors_when_category_selection_is_empty() {
        let error = generate_hostname(GenerateOptions {
            word1_categories: Vec::new(),
            ..GenerateOptions::default()
        })
        .expect_err("error");

        assert!(error.to_string().contains("categories must not be empty"));
    }

    #[test]
    fn validates_distinct_words_across_word_positions() {
        let error = generate_hostname(GenerateOptions {
            word1_categories: vec!["planet".to_owned()],
            word2_categories: vec!["planet".to_owned()],
            word1_lengths: Some(vec![8]),
            word2_lengths: Some(vec![8]),
            ..GenerateOptions::default()
        })
        .expect_err("error");

        // planet has exactly one word of length 8 ('makemake'); can't fill both positions.
        assert!(error.to_string().contains("unique words"));
    }

    #[test]
    fn rejects_suffix_min_length_below_floor() {
        let error = generate_hostname(GenerateOptions {
            suffix_min_length: MIN_SUFFIX_MIN_LENGTH - 1,
            ..GenerateOptions::default()
        })
        .expect_err("error");

        assert!(
            error
                .to_string()
                .contains("suffix minimum length must be between")
        );
    }

    #[test]
    fn rejects_suffix_min_length_above_ceiling() {
        let error = generate_hostname(GenerateOptions {
            suffix_min_length: MAX_SUFFIX_MIN_LENGTH + 1,
            ..GenerateOptions::default()
        })
        .expect_err("error");

        assert!(
            error
                .to_string()
                .contains("suffix minimum length must be between")
        );
    }

    #[test]
    fn parses_category_lists() {
        assert_eq!(
            parse_categories("adjective, animal").expect("categories"),
            vec!["adjective", "animal"]
        );
        assert!(parse_categories(" , ").is_err());
    }

    #[test]
    fn parses_length_lists() {
        assert_eq!(parse_lengths("5").expect("lengths"), Some(vec![5]));
        assert_eq!(
            parse_lengths("4, 5, 6").expect("lengths"),
            Some(vec![4, 5, 6])
        );
        assert_eq!(parse_lengths("any").expect("lengths"), None);
        assert_eq!(parse_lengths("ANY").expect("lengths"), None);
        assert!(parse_lengths("0").is_err());
        assert!(parse_lengths("nope").is_err());
        assert!(parse_lengths(" , ").is_err());
    }

    #[test]
    fn parse_helpers_reject_invalid_values() {
        assert!(parse_count("nope").is_err());
        assert!(parse_count("0").is_err());
        assert!(parse_suffix_min_length("nope").is_err());
        assert!(parse_suffix_min_length("3").is_err());
        assert!(parse_suffix_min_length("14").is_err());
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
    fn suffix_min_length_floor_and_ceiling_at_parser() {
        assert!(parse_suffix_min_length("4").is_ok());
        assert!(parse_suffix_min_length("13").is_ok());
        assert!(parse_suffix_min_length("3").is_err());
        assert!(parse_suffix_min_length("14").is_err());
        assert_eq!(random_fallback_max_value(5).expect("max"), 30_233_088);
    }

    #[test]
    fn generate_many_returns_count_hostnames() {
        let hostnames = generate_many(GenerateOptions {
            count: 3,
            suffix_enabled: false,
            ..GenerateOptions::default()
        })
        .expect("hostnames");

        assert_eq!(hostnames.len(), 3);
        assert!(hostnames.iter().all(|hostname| {
            let parts = parse_hostname_parts(hostname);
            parts.len() == 2 && parts[0] != parts[1]
        }));
    }

    #[test]
    fn generate_many_with_suffix_calls_provider_per_hostname() {
        let mut value = 0;
        let hostnames = generate_many_with_suffix(
            GenerateOptions {
                count: 3,
                suffix_min_length: 4,
                ..GenerateOptions::default()
            },
            || {
                value += 1;
                Ok(format!("{value:04x}"))
            },
        )
        .expect("hostnames");

        assert_eq!(hostnames.len(), 3);
        assert_eq!(value, 3);
        assert!(hostnames[0].ends_with("-0001"));
        assert!(hostnames[1].ends_with("-0002"));
        assert!(hostnames[2].ends_with("-0003"));
    }

    #[test]
    fn profile_suffix_is_stable_per_profile_and_value() {
        let profile_id = Uuid::parse_str("018f3f7a-4f34-7c6a-a1f0-6ec4b6ec7c1a").expect("uuid");

        let first = compute_profile_suffix(profile_id, 1, 8).expect("suffix");
        let first_again = compute_profile_suffix(profile_id, 1, 8).expect("suffix");
        let second = compute_profile_suffix(profile_id, 2, 8).expect("suffix");

        assert_eq!(first, first_again);
        assert_ne!(first, second);
        assert!(first.len() >= 8);
        assert!(is_base36_suffix(&first));
        assert!(compute_profile_suffix(profile_id, 0, 8).is_err());
    }

    #[test]
    fn profile_suffix_is_unique_for_initial_counter_values() {
        let profile_id = Uuid::parse_str("018f3f7a-4f34-7c6a-a1f0-6ec4b6ec7c1a").expect("uuid");

        let mut seen = HashSet::new();
        for atomic_value in 1..=100 {
            let suffix = compute_profile_suffix(profile_id, atomic_value, 5).expect("suffix");
            assert!(suffix.len() >= 5);
            assert!(is_base36_suffix(&suffix));
            assert!(seen.insert(suffix));
        }
    }

    #[test]
    fn overrides_apply_per_field() {
        let options = GenerateOptions::default().with_overrides(GenerateOverrides {
            word1_categories: Some(vec!["star".to_owned()]),
            word1_lengths: Some(Some(vec![6])),
            suffix_enabled: Some(false),
            ..GenerateOverrides::default()
        });

        assert_eq!(options.word1_categories, vec!["star"]);
        assert_eq!(options.word1_lengths.as_deref(), Some(&[6][..]));
        assert!(!options.suffix_enabled);
        assert_eq!(
            options.word2_categories,
            DEFAULT_WORD2_CATEGORIES
                .iter()
                .map(|category| (*category).to_owned())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn default_categories_match_profile_policy() {
        let options = GenerateOptions::default();

        assert_eq!(options.word1_categories, vec!["adjective", "adverb"]);
        assert!(options.word2_categories.contains(&"animal".to_owned()));
        assert!(!options.word2_categories.contains(&"adjective".to_owned()));
        assert!(!options.word2_categories.contains(&"adverb".to_owned()));
        assert!(!options.word2_categories.contains(&"diceware".to_owned()));
    }

    #[test]
    fn capacity_report_counts_unique_pairs_and_suffix_space() {
        let diceware_count = dictionary::words("diceware", 5).expect("diceware").len();
        let report = capacity_report(&GenerateOptions {
            word1_categories: vec!["diceware".to_owned()],
            word2_categories: vec!["diceware".to_owned()],
            word1_lengths: Some(vec![5]),
            word2_lengths: Some(vec![5]),
            suffix_min_length: 5,
            ..GenerateOptions::default()
        })
        .expect("report");

        assert_eq!(report.word1_count, Some(diceware_count));
        assert_eq!(report.word2_count, Some(diceware_count));
        assert_eq!(report.overlapping_words, diceware_count);
        assert_eq!(
            report.unique_word_combinations,
            (diceware_count as u128) * (diceware_count as u128) - diceware_count as u128
        );
        assert_eq!(report.suffix_variants.as_deref(), Some("60466176"));
        assert_eq!(report.suffix_bits, Some(25));
        assert_eq!(report.random_fallback_max_value, Some(30_233_088));
        assert_eq!(report.atomic_storage_max_value, Some(i64::MAX));
    }

    #[test]
    fn capacity_report_includes_suffix_number_bounds() {
        let report = capacity_report(&GenerateOptions {
            suffix_min_length: 5,
            ..GenerateOptions::default()
        })
        .expect("report");

        assert_eq!(report.suffix_variants.as_deref(), Some("60466176"));
        assert_eq!(report.suffix_bits, Some(25));
        assert_eq!(report.random_fallback_max_value, Some(30_233_088));
        assert_eq!(report.atomic_storage_max_value, Some(i64::MAX));
    }

    #[test]
    fn capacity_report_handles_suffix_only_large_hash_space() {
        let report = capacity_report(&GenerateOptions {
            word1_enabled: false,
            word2_enabled: false,
            suffix_min_length: MAX_SUFFIX_MIN_LENGTH,
            ..GenerateOptions::default()
        })
        .expect("report");

        assert_eq!(report.unique_word_combinations, 1);
        assert_eq!(report.suffix_bits, Some(67));
        assert_eq!(
            report.suffix_variants.as_deref(),
            Some("170581728179578208256")
        );
        assert_eq!(report.total_variants, "170581728179578208256");
        assert_eq!(
            report.random_fallback_max_value,
            Some(4_611_686_018_427_387_903)
        );
    }

    #[test]
    fn capacity_report_can_return_zero_unique_word_pairs() {
        let report = capacity_report(&GenerateOptions {
            word1_categories: vec!["planet".to_owned()],
            word2_categories: vec!["planet".to_owned()],
            word1_lengths: Some(vec![8]),
            word2_lengths: Some(vec![8]),
            suffix_enabled: false,
            ..GenerateOptions::default()
        })
        .expect("report");

        assert_eq!(report.word1_count, Some(1));
        assert_eq!(report.word2_count, Some(1));
        assert_eq!(report.overlapping_words, 1);
        assert_eq!(report.unique_word_combinations, 0);
        assert_eq!(report.total_variants, "0");
    }
}
