// SPDX-License-Identifier: FSL-1.1-ALv2

use crate::generator::{GenerateOptions, SuffixHash, SuffixSource};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

pub const DEFAULT_PROFILE_SLUG: &str = "_";
pub const MAX_PROFILE_SLUG_LEN: usize = 63;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProfileSlug(String);

impl ProfileSlug {
    pub fn default_profile() -> Self {
        Self(DEFAULT_PROFILE_SLUG.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProfileSlug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ProfileSlug {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        parse_profile_slug(value)
    }
}

pub fn parse_profile_slug(value: &str) -> Result<ProfileSlug, String> {
    let value = value.trim();
    if value == DEFAULT_PROFILE_SLUG {
        return Ok(ProfileSlug::default_profile());
    }

    if value.is_empty() {
        return Err("profile slug must not be empty".to_owned());
    }
    if value.len() > MAX_PROFILE_SLUG_LEN {
        return Err(format!(
            "profile slug must be at most {MAX_PROFILE_SLUG_LEN} characters"
        ));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(
            "profile slug must use lowercase ASCII letters, digits, and hyphens".to_owned(),
        );
    }
    if !value
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        || !value
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return Err("profile slug must start and end with a letter or digit".to_owned());
    }

    Ok(ProfileSlug(value.to_owned()))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfileConfig {
    pub word1: WordProfileConfig,
    pub word2: WordProfileConfig,
    pub suffix: SuffixProfileConfig,
}

impl Default for ProfileConfig {
    fn default() -> Self {
        Self::from(&GenerateOptions::default())
    }
}

impl From<&GenerateOptions> for ProfileConfig {
    fn from(options: &GenerateOptions) -> Self {
        Self {
            word1: WordProfileConfig {
                enabled: options.word1_enabled,
                lengths: options.word1_lengths.clone(),
                categories: options.word1_categories.clone(),
            },
            word2: WordProfileConfig {
                enabled: options.word2_enabled,
                lengths: options.word2_lengths.clone(),
                categories: options.word2_categories.clone(),
            },
            suffix: SuffixProfileConfig {
                enabled: options.suffix_enabled,
                length: options.suffix_length,
                source: options.suffix_source,
                hash: options.suffix_hash,
            },
        }
    }
}

impl ProfileConfig {
    pub fn to_generate_options(&self, count: usize) -> GenerateOptions {
        GenerateOptions {
            word1_enabled: self.word1.enabled,
            word1_lengths: self.word1.lengths.clone(),
            word1_categories: self.word1.categories.clone(),
            word2_enabled: self.word2.enabled,
            word2_lengths: self.word2.lengths.clone(),
            word2_categories: self.word2.categories.clone(),
            suffix_enabled: self.suffix.enabled,
            suffix_length: self.suffix.length,
            suffix_source: self.suffix.source,
            suffix_hash: self.suffix.hash,
            count,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WordProfileConfig {
    pub enabled: bool,
    pub lengths: Option<Vec<usize>>,
    pub categories: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SuffixProfileConfig {
    pub enabled: bool,
    pub length: usize,
    pub source: SuffixSource,
    pub hash: SuffixHash,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator;

    #[test]
    fn parses_default_and_user_profile_slugs() {
        assert_eq!(
            parse_profile_slug("_").expect("default").as_str(),
            DEFAULT_PROFILE_SLUG
        );
        assert_eq!(
            parse_profile_slug("team-a-1").expect("slug").as_str(),
            "team-a-1"
        );
    }

    #[test]
    fn rejects_invalid_profile_slugs() {
        for value in ["", "Team", "team_a", "-team", "team-", "team.a"] {
            assert!(parse_profile_slug(value).is_err(), "{value}");
        }
    }

    #[test]
    fn profile_config_excludes_count() {
        let profile = ProfileConfig::from(&GenerateOptions {
            count: 37,
            ..GenerateOptions::default()
        });
        let options = profile.to_generate_options(3);

        assert_eq!(options.count, 3);
        assert_eq!(
            options.word1_lengths,
            Some(vec![generator::DEFAULT_WORD_LENGTH])
        );
    }
}
