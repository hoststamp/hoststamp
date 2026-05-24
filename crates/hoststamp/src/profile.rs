// SPDX-License-Identifier: FSL-1.1-ALv2

use crate::{dictionary, generator::GenerateOptions};
use anyhow::{Result, anyhow};
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

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProfileAccess {
    Public,
    #[default]
    Private,
}

impl fmt::Display for ProfileAccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Public => f.write_str("public"),
            Self::Private => f.write_str("private"),
        }
    }
}

impl FromStr for ProfileAccess {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "public" => Ok(Self::Public),
            "private" => Ok(Self::Private),
            _ => Err("profile access must be public or private".to_owned()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfileConfig {
    pub dictionary_version: u32,
    pub dictionary_version_hash: String,
    pub blocklist_version: u32,
    pub blocklist_version_hash: String,
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
        Self::try_from_options(options).expect("profile config must resolve to valid word pools")
    }
}

impl ProfileConfig {
    pub fn try_from_options(options: &GenerateOptions) -> Result<Self> {
        let dictionary_version_hash =
            dictionary::dictionary_version_hash(options.dictionary_version)
                .ok_or_else(|| {
                    anyhow!("unknown dictionary version {}", options.dictionary_version)
                })?
                .to_owned();
        let blocklist_version_hash = dictionary::blocklist_version_hash(options.blocklist_version)
            .ok_or_else(|| anyhow!("unknown blocklist version {}", options.blocklist_version))?
            .to_owned();
        Ok(Self {
            dictionary_version: options.dictionary_version,
            dictionary_version_hash,
            blocklist_version: options.blocklist_version,
            blocklist_version_hash,
            word1: WordProfileConfig {
                enabled: options.word1_enabled,
                lengths: options.word1_lengths.clone(),
                categories: options.word1_categories.clone(),
                pool_hash: pool_hash(
                    options.dictionary_version,
                    options.blocklist_version,
                    options.word1_enabled,
                    &options.word1_categories,
                    options.word1_lengths.as_deref(),
                )?,
            },
            word2: WordProfileConfig {
                enabled: options.word2_enabled,
                lengths: options.word2_lengths.clone(),
                categories: options.word2_categories.clone(),
                pool_hash: pool_hash(
                    options.dictionary_version,
                    options.blocklist_version,
                    options.word2_enabled,
                    &options.word2_categories,
                    options.word2_lengths.as_deref(),
                )?,
            },
            suffix: SuffixProfileConfig {
                enabled: options.suffix_enabled,
                min_length: options.suffix_min_length,
            },
        })
    }

    pub fn uses_current_dictionary(&self) -> bool {
        self.dictionary_version_hash
            == dictionary::dictionary_version_hash(self.dictionary_version).unwrap_or_default()
            && self.blocklist_version_hash
                == dictionary::blocklist_version_hash(self.blocklist_version).unwrap_or_default()
            && self.word1.pool_hash
                == pool_hash(
                    self.dictionary_version,
                    self.blocklist_version,
                    self.word1.enabled,
                    &self.word1.categories,
                    self.word1.lengths.as_deref(),
                )
                .ok()
                .flatten()
            && self.word2.pool_hash
                == pool_hash(
                    self.dictionary_version,
                    self.blocklist_version,
                    self.word2.enabled,
                    &self.word2.categories,
                    self.word2.lengths.as_deref(),
                )
                .ok()
                .flatten()
    }

    pub fn to_generate_options(&self, count: usize) -> GenerateOptions {
        GenerateOptions {
            dictionary_version: self.dictionary_version,
            blocklist_version: self.blocklist_version,
            word1_enabled: self.word1.enabled,
            word1_lengths: self.word1.lengths.clone(),
            word1_categories: self.word1.categories.clone(),
            word2_enabled: self.word2.enabled,
            word2_lengths: self.word2.lengths.clone(),
            word2_categories: self.word2.categories.clone(),
            suffix_enabled: self.suffix.enabled,
            suffix_min_length: self.suffix.min_length,
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
    pub pool_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SuffixProfileConfig {
    pub enabled: bool,
    pub min_length: usize,
}

fn pool_hash(
    dictionary_version: u32,
    blocklist_version: u32,
    enabled: bool,
    categories: &[String],
    lengths: Option<&[usize]>,
) -> Result<Option<String>> {
    if !enabled {
        return Ok(None);
    }
    let words =
        dictionary::resolve_words(dictionary_version, blocklist_version, categories, lengths)
            .map_err(anyhow::Error::msg)?;
    Ok(Some(dictionary::resolved_pool_hash(&words)))
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
        assert!(profile.uses_current_dictionary());
        assert_eq!(
            profile.dictionary_version,
            dictionary::default_dictionary_version()
        );
        assert_eq!(
            profile.blocklist_version,
            dictionary::default_blocklist_version()
        );
        assert!(profile.word1.pool_hash.is_some());
    }

    #[test]
    fn parses_profile_access() {
        assert_eq!(
            "public".parse::<ProfileAccess>().expect("public"),
            ProfileAccess::Public
        );
        assert_eq!(
            "private".parse::<ProfileAccess>().expect("private"),
            ProfileAccess::Private
        );
        assert!("missing".parse::<ProfileAccess>().is_err());
    }

    #[test]
    fn detects_stale_dictionary_version_hash() {
        let profile = ProfileConfig {
            dictionary_version_hash: "old".to_owned(),
            ..ProfileConfig::default()
        };

        assert!(!profile.uses_current_dictionary());
    }

    #[test]
    fn detects_stale_word_pool_hash() {
        let profile = ProfileConfig {
            word1: WordProfileConfig {
                pool_hash: Some("old".to_owned()),
                ..ProfileConfig::default().word1
            },
            ..ProfileConfig::default()
        };

        assert!(!profile.uses_current_dictionary());
    }
}
