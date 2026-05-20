// SPDX-License-Identifier: FSL-1.1-ALv2

use crate::generator::{self, GenerateOptions, SuffixHash, SuffixSource};
use serde::{Deserialize, Deserializer};
use std::{
    env, fmt, fs, io,
    net::{AddrParseError, SocketAddr},
    path::{Path, PathBuf},
};

pub const DEFAULT_ADDR: &str = "127.0.0.1:8080";

pub const CONFIG_ENV: &str = "HOSTSTAMP_CONFIG";
pub const ADDR_ENV: &str = "HOSTSTAMP_ADDR";
pub const COUNT_ENV: &str = "HOSTSTAMP_COUNT";
pub const WORD1_ENABLED_ENV: &str = "HOSTSTAMP_WORD1_ENABLED";
pub const WORD1_LENGTHS_ENV: &str = "HOSTSTAMP_WORD1_LENGTHS";
pub const WORD1_CATEGORIES_ENV: &str = "HOSTSTAMP_WORD1_CATEGORIES";
pub const WORD2_ENABLED_ENV: &str = "HOSTSTAMP_WORD2_ENABLED";
pub const WORD2_LENGTHS_ENV: &str = "HOSTSTAMP_WORD2_LENGTHS";
pub const WORD2_CATEGORIES_ENV: &str = "HOSTSTAMP_WORD2_CATEGORIES";
pub const SUFFIX_ENABLED_ENV: &str = "HOSTSTAMP_SUFFIX_ENABLED";
pub const SUFFIX_LENGTH_ENV: &str = "HOSTSTAMP_SUFFIX_LENGTH";
pub const SUFFIX_SOURCE_ENV: &str = "HOSTSTAMP_SUFFIX_SOURCE";
pub const SUFFIX_HASH_ENV: &str = "HOSTSTAMP_SUFFIX_HASH";

#[derive(Debug, Clone)]
pub struct Settings {
    pub addr: SocketAddr,
    pub config_path: Option<PathBuf>,
    pub generator: GenerateOptions,
}

#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub config_path: Option<PathBuf>,
    pub addr: Option<SocketAddr>,
}

#[derive(Debug, Clone, Default)]
pub struct ConfigEnv {
    pub config_path: Option<PathBuf>,
    pub addr: Option<String>,
    pub count: Option<String>,
    pub word1_enabled: Option<String>,
    pub word1_lengths: Option<String>,
    pub word1_categories: Option<String>,
    pub word2_enabled: Option<String>,
    pub word2_lengths: Option<String>,
    pub word2_categories: Option<String>,
    pub suffix_enabled: Option<String>,
    pub suffix_length: Option<String>,
    pub suffix_source: Option<String>,
    pub suffix_hash: Option<String>,
    pub xdg_config_home: Option<PathBuf>,
    pub home: Option<PathBuf>,
}

impl ConfigEnv {
    pub fn process() -> Self {
        Self {
            config_path: env::var_os(CONFIG_ENV).map(PathBuf::from),
            addr: env::var(ADDR_ENV).ok(),
            count: env::var(COUNT_ENV).ok(),
            word1_enabled: env::var(WORD1_ENABLED_ENV).ok(),
            word1_lengths: env::var(WORD1_LENGTHS_ENV).ok(),
            word1_categories: env::var(WORD1_CATEGORIES_ENV).ok(),
            word2_enabled: env::var(WORD2_ENABLED_ENV).ok(),
            word2_lengths: env::var(WORD2_LENGTHS_ENV).ok(),
            word2_categories: env::var(WORD2_CATEGORIES_ENV).ok(),
            suffix_enabled: env::var(SUFFIX_ENABLED_ENV).ok(),
            suffix_length: env::var(SUFFIX_LENGTH_ENV).ok(),
            suffix_source: env::var(SUFFIX_SOURCE_ENV).ok(),
            suffix_hash: env::var(SUFFIX_HASH_ENV).ok(),
            xdg_config_home: env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            home: env::var_os("HOME").map(PathBuf::from),
        }
    }
}

#[derive(Debug)]
pub enum LoadError {
    MissingConfig {
        path: PathBuf,
    },
    Read {
        path: PathBuf,
        source: io::Error,
    },
    ParseToml {
        path: PathBuf,
        source: toml::de::Error,
    },
    ParseAddr {
        source: AddrParseError,
        value: String,
        source_name: &'static str,
    },
    ParseGenerator {
        source_name: &'static str,
        value: String,
        reason: String,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingConfig { path } => {
                write!(f, "config file does not exist: {}", path.display())
            }
            Self::Read { path, source } => {
                write!(f, "failed to read config file {}: {source}", path.display())
            }
            Self::ParseToml { path, source } => {
                write!(
                    f,
                    "failed to parse config file {}: {source}",
                    path.display()
                )
            }
            Self::ParseAddr {
                source,
                value,
                source_name,
            } => write!(f, "invalid {source_name} address {value:?}: {source}"),
            Self::ParseGenerator {
                source_name,
                value,
                reason,
            } => write!(f, "invalid {source_name} value {value:?}: {reason}"),
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MissingConfig { .. } | Self::ParseGenerator { .. } => None,
            Self::Read { source, .. } => Some(source),
            Self::ParseToml { source, .. } => Some(source),
            Self::ParseAddr { source, .. } => Some(source),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    server: Option<ServerConfig>,
    generate: Option<GenerateFileConfig>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ServerConfig {
    addr: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct GenerateFileConfig {
    count: Option<usize>,
    word1: Option<WordFileConfig>,
    word2: Option<WordFileConfig>,
    suffix: Option<SuffixFileConfig>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct WordFileConfig {
    enabled: Option<bool>,
    lengths: Option<LengthsSpec>,
    categories: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct SuffixFileConfig {
    enabled: Option<bool>,
    length: Option<usize>,
    source: Option<SuffixSource>,
    hash: Option<SuffixHash>,
}

#[derive(Debug, Clone)]
enum LengthsSpec {
    Any,
    Exact(Vec<usize>),
}

impl<'de> Deserialize<'de> for LengthsSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            String(String),
            Array(Vec<usize>),
        }
        match Helper::deserialize(deserializer)? {
            Helper::String(s) if s.eq_ignore_ascii_case("any") => Ok(Self::Any),
            Helper::String(s) => Err(serde::de::Error::custom(format!(
                "unexpected lengths value {s:?}; expected \"any\" or an array of integers"
            ))),
            Helper::Array(v) => Ok(Self::Exact(v)),
        }
    }
}

impl LengthsSpec {
    fn into_option(self) -> Option<Vec<usize>> {
        match self {
            Self::Any => None,
            Self::Exact(v) => Some(v),
        }
    }
}

pub fn load(overrides: Overrides) -> Result<Settings, LoadError> {
    load_with_env(overrides, ConfigEnv::process())
}

pub fn load_with_env(overrides: Overrides, env: ConfigEnv) -> Result<Settings, LoadError> {
    let config_arg = overrides.config_path.clone();
    let config_env = env.config_path.clone();
    let config_path = config_arg
        .clone()
        .or(config_env.clone())
        .or_else(|| default_config_path(&env));
    let explicit_config = config_arg.is_some() || config_env.is_some();

    let mut addr = parse_addr(DEFAULT_ADDR, "default")?;
    let mut loaded_config_path = None;
    let mut file: FileConfig = FileConfig::default();

    if let Some(path) = config_path.as_deref() {
        if path.exists() {
            file = read_file_config(path)?;
            if let Some(config_addr) = file.server.as_ref().and_then(|server| server.addr.as_ref())
            {
                addr = parse_addr(config_addr, "server.addr")?;
            }
            loaded_config_path = Some(path.to_path_buf());
        } else if explicit_config {
            return Err(LoadError::MissingConfig {
                path: path.to_path_buf(),
            });
        }
    }

    if let Some(env_addr) = env.addr.as_deref() {
        addr = parse_addr(env_addr, ADDR_ENV)?;
    }

    if let Some(cli_addr) = overrides.addr {
        addr = cli_addr;
    }

    let generator = merge_generator(file.generate.take(), &env)?;

    Ok(Settings {
        addr,
        config_path: loaded_config_path,
        generator,
    })
}

fn merge_generator(
    file: Option<GenerateFileConfig>,
    env: &ConfigEnv,
) -> Result<GenerateOptions, LoadError> {
    let mut options = GenerateOptions::default();

    if let Some(file) = file {
        if let Some(count) = file.count {
            options.count = count;
        }
        if let Some(word1) = file.word1 {
            apply_word_file(
                &mut options.word1_enabled,
                &mut options.word1_lengths,
                &mut options.word1_categories,
                word1,
            );
        }
        if let Some(word2) = file.word2 {
            apply_word_file(
                &mut options.word2_enabled,
                &mut options.word2_lengths,
                &mut options.word2_categories,
                word2,
            );
        }
        if let Some(suffix) = file.suffix {
            if let Some(v) = suffix.enabled {
                options.suffix_enabled = v;
            }
            if let Some(v) = suffix.length {
                options.suffix_length = v;
            }
            if let Some(v) = suffix.source {
                options.suffix_source = v;
            }
            if let Some(v) = suffix.hash {
                options.suffix_hash = v;
            }
        }
    }

    if let Some(value) = env.count.as_deref() {
        options.count = parse_env(COUNT_ENV, value, generator::parse_count)?;
    }
    apply_word_env(
        &mut options.word1_enabled,
        &mut options.word1_lengths,
        &mut options.word1_categories,
        env.word1_enabled.as_deref(),
        env.word1_lengths.as_deref(),
        env.word1_categories.as_deref(),
        WORD1_ENABLED_ENV,
        WORD1_LENGTHS_ENV,
        WORD1_CATEGORIES_ENV,
    )?;
    apply_word_env(
        &mut options.word2_enabled,
        &mut options.word2_lengths,
        &mut options.word2_categories,
        env.word2_enabled.as_deref(),
        env.word2_lengths.as_deref(),
        env.word2_categories.as_deref(),
        WORD2_ENABLED_ENV,
        WORD2_LENGTHS_ENV,
        WORD2_CATEGORIES_ENV,
    )?;
    if let Some(value) = env.suffix_enabled.as_deref() {
        options.suffix_enabled = parse_bool_env(SUFFIX_ENABLED_ENV, value)?;
    }
    if let Some(value) = env.suffix_length.as_deref() {
        options.suffix_length =
            parse_env(SUFFIX_LENGTH_ENV, value, generator::parse_suffix_length)?;
    }
    if let Some(value) = env.suffix_source.as_deref() {
        options.suffix_source =
            parse_env(SUFFIX_SOURCE_ENV, value, generator::parse_suffix_source)?;
    }
    if let Some(value) = env.suffix_hash.as_deref() {
        options.suffix_hash = parse_env(SUFFIX_HASH_ENV, value, generator::parse_suffix_hash)?;
    }

    Ok(options)
}

fn apply_word_file(
    enabled: &mut bool,
    lengths: &mut Option<Vec<usize>>,
    categories: &mut Vec<String>,
    file: WordFileConfig,
) {
    if let Some(v) = file.enabled {
        *enabled = v;
    }
    if let Some(spec) = file.lengths {
        *lengths = spec.into_option();
    }
    if let Some(v) = file.categories {
        *categories = v;
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_word_env(
    enabled: &mut bool,
    lengths: &mut Option<Vec<usize>>,
    categories: &mut Vec<String>,
    enabled_value: Option<&str>,
    lengths_value: Option<&str>,
    categories_value: Option<&str>,
    enabled_name: &'static str,
    lengths_name: &'static str,
    categories_name: &'static str,
) -> Result<(), LoadError> {
    if let Some(value) = enabled_value {
        *enabled = parse_bool_env(enabled_name, value)?;
    }
    if let Some(value) = lengths_value {
        *lengths = parse_env(lengths_name, value, generator::parse_lengths)?;
    }
    if let Some(value) = categories_value {
        *categories = parse_env(categories_name, value, generator::parse_categories)?;
    }
    Ok(())
}

fn parse_env<T, F>(name: &'static str, value: &str, parser: F) -> Result<T, LoadError>
where
    F: FnOnce(&str) -> Result<T, String>,
{
    parser(value.trim()).map_err(|reason| LoadError::ParseGenerator {
        source_name: name,
        value: value.to_owned(),
        reason,
    })
}

fn parse_bool_env(name: &'static str, value: &str) -> Result<bool, LoadError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(LoadError::ParseGenerator {
            source_name: name,
            value: value.to_owned(),
            reason: "expected true or false".to_owned(),
        }),
    }
}

fn default_config_path(env: &ConfigEnv) -> Option<PathBuf> {
    if let Some(path) = env
        .xdg_config_home
        .as_ref()
        .filter(|path| !path.as_os_str().is_empty())
    {
        return Some(path.join("hoststamp").join("config.toml"));
    }

    env.home
        .as_ref()
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| path.join(".config").join("hoststamp").join("config.toml"))
}

fn read_file_config(path: &Path) -> Result<FileConfig, LoadError> {
    let contents = fs::read_to_string(path).map_err(|source| LoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    toml::from_str(&contents).map_err(|source| LoadError::ParseToml {
        path: path.to_path_buf(),
        source,
    })
}

fn parse_addr(value: &str, source_name: &'static str) -> Result<SocketAddr, LoadError> {
    value.parse().map_err(|source| LoadError::ParseAddr {
        source,
        value: value.to_owned(),
        source_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_default_addr_when_config_is_absent() {
        let home = PathBuf::from("/tmp/hoststamp-test-home");
        let settings = load_with_env(
            Overrides::default(),
            ConfigEnv {
                home: Some(home.clone()),
                ..ConfigEnv::default()
            },
        )
        .expect("settings");

        assert_eq!(
            settings.addr,
            parse_addr(DEFAULT_ADDR, "test").expect("addr")
        );
        assert_eq!(settings.config_path, None);
        let defaults = GenerateOptions::default();
        assert_eq!(
            settings.generator.word1_categories,
            defaults.word1_categories
        );
        assert_eq!(settings.generator.suffix_length, defaults.suffix_length);
    }

    #[test]
    fn reads_server_addr_from_xdg_config_file() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let config_dir = tempdir.path().join("hoststamp");
        fs::create_dir_all(&config_dir).expect("config dir");
        fs::write(
            config_dir.join("config.toml"),
            r#"
                [server]
                addr = "127.0.0.1:9000"
            "#,
        )
        .expect("config");

        let settings = load_with_env(
            Overrides::default(),
            ConfigEnv {
                xdg_config_home: Some(tempdir.path().to_path_buf()),
                ..ConfigEnv::default()
            },
        )
        .expect("settings");

        assert_eq!(
            settings.addr,
            parse_addr("127.0.0.1:9000", "test").expect("addr")
        );
        assert_eq!(settings.config_path, Some(config_dir.join("config.toml")));
    }

    #[test]
    fn applies_precedence_cli_env_file_default() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let config_path = tempdir.path().join("config.toml");
        fs::write(
            &config_path,
            r#"
                [server]
                addr = "127.0.0.1:9000"
            "#,
        )
        .expect("config");

        let settings = load_with_env(
            Overrides {
                config_path: Some(config_path),
                addr: Some(parse_addr("127.0.0.1:9002", "test").expect("addr")),
            },
            ConfigEnv {
                addr: Some("127.0.0.1:9001".to_owned()),
                ..ConfigEnv::default()
            },
        )
        .expect("settings");

        assert_eq!(
            settings.addr,
            parse_addr("127.0.0.1:9002", "test").expect("addr")
        );
    }

    #[test]
    fn explicit_missing_config_is_an_error() {
        let path = PathBuf::from("/tmp/hoststamp-missing-config.toml");
        let error = load_with_env(
            Overrides {
                config_path: Some(path.clone()),
                ..Overrides::default()
            },
            ConfigEnv::default(),
        )
        .expect_err("missing config");

        assert!(matches!(error, LoadError::MissingConfig { path: p } if p == path));
    }

    #[test]
    fn invalid_config_toml_is_an_error() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let config_path = tempdir.path().join("config.toml");
        fs::write(&config_path, "[server").expect("config");

        let error = load_with_env(
            Overrides {
                config_path: Some(config_path),
                ..Overrides::default()
            },
            ConfigEnv::default(),
        )
        .expect_err("invalid toml");

        assert!(matches!(error, LoadError::ParseToml { .. }));
    }

    #[test]
    fn unreadable_config_path_is_an_error() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let error = load_with_env(
            Overrides {
                config_path: Some(tempdir.path().to_path_buf()),
                ..Overrides::default()
            },
            ConfigEnv::default(),
        )
        .expect_err("read error");

        assert!(matches!(error, LoadError::Read { .. }));
        assert!(error.to_string().contains("failed to read config file"));
        assert!(std::error::Error::source(&error).is_some());
    }

    #[test]
    fn invalid_config_addr_is_an_error() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let config_path = tempdir.path().join("config.toml");
        fs::write(
            &config_path,
            r#"
                [server]
                addr = "not-an-addr"
            "#,
        )
        .expect("config");

        let error = load_with_env(
            Overrides {
                config_path: Some(config_path),
                ..Overrides::default()
            },
            ConfigEnv::default(),
        )
        .expect_err("invalid addr");

        assert!(
            matches!(error, LoadError::ParseAddr { source_name, .. } if source_name == "server.addr")
        );
    }

    #[test]
    fn invalid_env_addr_is_an_error() {
        let error = load_with_env(
            Overrides::default(),
            ConfigEnv {
                addr: Some("not-an-addr".to_owned()),
                ..ConfigEnv::default()
            },
        )
        .expect_err("invalid addr");

        assert!(
            matches!(error, LoadError::ParseAddr { source_name, .. } if source_name == ADDR_ENV)
        );
    }

    #[test]
    fn load_error_display_and_source_are_informative() {
        let missing = LoadError::MissingConfig {
            path: PathBuf::from("/tmp/missing.toml"),
        };
        assert!(missing.to_string().contains("config file does not exist"));
        assert!(std::error::Error::source(&missing).is_none());

        let parse_error = load_with_env(
            Overrides::default(),
            ConfigEnv {
                addr: Some("not-an-addr".to_owned()),
                ..ConfigEnv::default()
            },
        )
        .expect_err("invalid addr");
        assert!(
            parse_error
                .to_string()
                .contains("invalid HOSTSTAMP_ADDR address")
        );
        assert!(std::error::Error::source(&parse_error).is_some());
    }

    #[test]
    fn reads_generator_fields_from_config_file() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let config_path = tempdir.path().join("config.toml");
        fs::write(
            &config_path,
            r#"
                [generate]
                count = 7

                [generate.word1]
                enabled = true
                lengths = [4, 5, 6]
                categories = ["star", "planet"]

                [generate.word2]
                enabled = false

                [generate.suffix]
                enabled = true
                length = 8
                source = "random"
                hash = "sha1"
            "#,
        )
        .expect("config");

        let settings = load_with_env(
            Overrides {
                config_path: Some(config_path),
                ..Overrides::default()
            },
            ConfigEnv::default(),
        )
        .expect("settings");

        assert_eq!(settings.generator.count, 7);
        assert_eq!(
            settings.generator.word1_lengths.as_deref(),
            Some(&[4, 5, 6][..])
        );
        assert_eq!(
            settings.generator.word1_categories,
            vec!["star".to_owned(), "planet".to_owned()]
        );
        assert!(!settings.generator.word2_enabled);
        assert_eq!(settings.generator.suffix_length, 8);
    }

    #[test]
    fn config_file_lengths_any_string_becomes_none() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let config_path = tempdir.path().join("config.toml");
        fs::write(
            &config_path,
            r#"
                [generate.word1]
                lengths = "any"
            "#,
        )
        .expect("config");

        let settings = load_with_env(
            Overrides {
                config_path: Some(config_path),
                ..Overrides::default()
            },
            ConfigEnv::default(),
        )
        .expect("settings");

        assert!(settings.generator.word1_lengths.is_none());
    }

    #[test]
    fn config_file_rejects_unknown_lengths_string() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let config_path = tempdir.path().join("config.toml");
        fs::write(
            &config_path,
            r#"
                [generate.word1]
                lengths = "all"
            "#,
        )
        .expect("config");

        let error = load_with_env(
            Overrides {
                config_path: Some(config_path),
                ..Overrides::default()
            },
            ConfigEnv::default(),
        )
        .expect_err("unknown lengths value");

        assert!(matches!(error, LoadError::ParseToml { .. }));
    }

    #[test]
    fn env_overrides_file_generator_values() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let config_path = tempdir.path().join("config.toml");
        fs::write(
            &config_path,
            r#"
                [generate]
                count = 7

                [generate.word1]
                lengths = [5]
                categories = ["adjective"]
            "#,
        )
        .expect("config");

        let settings = load_with_env(
            Overrides {
                config_path: Some(config_path),
                ..Overrides::default()
            },
            ConfigEnv {
                count: Some("3".to_owned()),
                word1_lengths: Some("4,6".to_owned()),
                word1_categories: Some("star".to_owned()),
                ..ConfigEnv::default()
            },
        )
        .expect("settings");

        assert_eq!(settings.generator.count, 3);
        assert_eq!(
            settings.generator.word1_lengths.as_deref(),
            Some(&[4, 6][..])
        );
        assert_eq!(settings.generator.word1_categories, vec!["star".to_owned()]);
    }

    #[test]
    fn env_lengths_any_becomes_none() {
        let settings = load_with_env(
            Overrides::default(),
            ConfigEnv {
                word1_lengths: Some("any".to_owned()),
                ..ConfigEnv::default()
            },
        )
        .expect("settings");

        assert!(settings.generator.word1_lengths.is_none());
    }

    #[test]
    fn invalid_env_boolean_is_an_error() {
        let error = load_with_env(
            Overrides::default(),
            ConfigEnv {
                word1_enabled: Some("maybe".to_owned()),
                ..ConfigEnv::default()
            },
        )
        .expect_err("invalid bool");

        assert!(
            matches!(error, LoadError::ParseGenerator { source_name, .. } if source_name == WORD1_ENABLED_ENV)
        );
    }

    #[test]
    fn invalid_env_suffix_length_is_an_error() {
        let error = load_with_env(
            Overrides::default(),
            ConfigEnv {
                suffix_length: Some("3".to_owned()),
                ..ConfigEnv::default()
            },
        )
        .expect_err("suffix length out of range");

        assert!(matches!(error, LoadError::ParseGenerator { .. }));
        assert!(error.to_string().contains("suffix length must be between"));
    }

    #[test]
    fn env_suffix_source_round_trips() {
        let settings = load_with_env(
            Overrides::default(),
            ConfigEnv {
                suffix_source: Some("atomic".to_owned()),
                suffix_hash: Some("sha1".to_owned()),
                ..ConfigEnv::default()
            },
        )
        .expect("settings");

        assert_eq!(settings.generator.suffix_source, SuffixSource::Atomic);
        assert_eq!(settings.generator.suffix_hash, SuffixHash::Sha1);
    }
}
