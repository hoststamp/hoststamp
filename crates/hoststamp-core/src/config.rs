// SPDX-License-Identifier: FSL-1.1-ALv2

use crate::{
    auth::{
        ADMIN_TOKEN_ENV, API_AUTH_REQUIRED_ENV, ApiAuthConfig, PROFILE_TOKEN_HASH_KEY_ENV,
        SecretString,
    },
    storage::{DATABASE_ENV, DEFAULT_DATABASE_FILE, StorageUrl},
};
use serde::Deserialize;
use std::{
    env, fmt, fs, io,
    net::{AddrParseError, SocketAddr},
    path::{Path, PathBuf},
};

pub const DEFAULT_ADDR: &str = "127.0.0.1:8080";

pub const CONFIG_ENV: &str = "HOSTSTAMP_CONFIG";
pub const ADDR_ENV: &str = "HOSTSTAMP_ADDR";

#[derive(Debug, Clone)]
pub struct Settings {
    pub addr: SocketAddr,
    pub config_path: Option<PathBuf>,
    pub database_url: StorageUrl,
    pub auth: ApiAuthConfig,
}

#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub config_path: Option<PathBuf>,
    pub addr: Option<SocketAddr>,
    pub database_url: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ConfigEnv {
    pub config_path: Option<PathBuf>,
    pub addr: Option<String>,
    pub database_url: Option<String>,
    pub api_auth_required: Option<String>,
    pub admin_token: Option<String>,
    pub token_hash_key: Option<String>,
    pub xdg_config_home: Option<PathBuf>,
    pub home: Option<PathBuf>,
}

impl ConfigEnv {
    pub fn process() -> Self {
        Self {
            config_path: env::var_os(CONFIG_ENV).map(PathBuf::from),
            addr: env::var(ADDR_ENV).ok(),
            database_url: env::var(DATABASE_ENV).ok(),
            api_auth_required: env::var(API_AUTH_REQUIRED_ENV).ok(),
            admin_token: env::var(ADMIN_TOKEN_ENV).ok(),
            token_hash_key: env::var(PROFILE_TOKEN_HASH_KEY_ENV).ok(),
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
    ParseDatabaseUrl {
        value: String,
        source_name: &'static str,
        reason: String,
    },
    ParseBool {
        value: String,
        source_name: &'static str,
    },
    InvalidSecret {
        source_name: String,
    },
    MissingSecret {
        source_name: String,
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
            Self::ParseDatabaseUrl {
                value,
                source_name,
                reason,
            } => write!(f, "invalid {source_name} database URL {value:?}: {reason}"),
            Self::ParseBool { value, source_name } => {
                write!(f, "invalid {source_name} boolean {value:?}")
            }
            Self::InvalidSecret { source_name } => write!(f, "{source_name} must not be empty"),
            Self::MissingSecret { source_name } => write!(f, "{source_name} is required"),
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MissingConfig { .. }
            | Self::ParseDatabaseUrl { .. }
            | Self::ParseBool { .. }
            | Self::InvalidSecret { .. }
            | Self::MissingSecret { .. } => None,
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
    storage: Option<StorageConfig>,
    api: Option<ApiConfig>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ServerConfig {
    addr: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct StorageConfig {
    url: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ApiConfig {
    auth: Option<ApiAuthFileConfig>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ApiAuthFileConfig {
    required: Option<bool>,
    admin_token_env: Option<String>,
    token_hash_key_env: Option<String>,
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
    let mut file = FileConfig::default();

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

    let database_url = resolve_database_url(
        overrides.database_url.as_deref(),
        env.database_url.as_deref(),
        file.storage
            .as_ref()
            .and_then(|storage| storage.url.as_deref()),
        config_path.as_deref(),
    )?;
    let auth = resolve_auth(&file, &env)?;

    Ok(Settings {
        addr,
        config_path: loaded_config_path,
        database_url,
        auth,
    })
}

fn resolve_auth(file: &FileConfig, env: &ConfigEnv) -> Result<ApiAuthConfig, LoadError> {
    let file_auth = file.api.as_ref().and_then(|api| api.auth.as_ref());
    let mut required = file_auth.and_then(|auth| auth.required).unwrap_or(false);
    if let Some(value) = env.api_auth_required.as_deref() {
        required = parse_bool(value, API_AUTH_REQUIRED_ENV)?;
    }

    let admin_env = file_auth
        .and_then(|auth| auth.admin_token_env.as_deref())
        .unwrap_or(ADMIN_TOKEN_ENV);
    let hash_key_env = file_auth
        .and_then(|auth| auth.token_hash_key_env.as_deref())
        .unwrap_or(PROFILE_TOKEN_HASH_KEY_ENV);

    let admin_token = env
        .admin_token
        .as_deref()
        .map(|value| parse_secret(value, admin_env))
        .transpose()?;
    let token_hash_key = env
        .token_hash_key
        .as_deref()
        .map(|value| parse_secret(value, hash_key_env))
        .transpose()?;

    if required && admin_token.is_none() {
        return Err(LoadError::MissingSecret {
            source_name: admin_env.to_owned(),
        });
    }
    if required && token_hash_key.is_none() {
        return Err(LoadError::MissingSecret {
            source_name: hash_key_env.to_owned(),
        });
    }

    Ok(ApiAuthConfig {
        required,
        admin_token,
        token_hash_key,
    })
}

fn parse_bool(value: &str, source_name: &'static str) -> Result<bool, LoadError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(LoadError::ParseBool {
            value: value.to_owned(),
            source_name,
        }),
    }
}

fn parse_secret(value: &str, source_name: &str) -> Result<SecretString, LoadError> {
    SecretString::new(value.to_owned()).map_err(|_| LoadError::InvalidSecret {
        source_name: source_name.to_owned(),
    })
}

fn resolve_database_url(
    cli: Option<&str>,
    env: Option<&str>,
    file: Option<&str>,
    config_path: Option<&Path>,
) -> Result<StorageUrl, LoadError> {
    if let Some(value) = cli {
        return parse_database_url(value, "cli --database-url");
    }
    if let Some(value) = env {
        return parse_database_url(value, DATABASE_ENV);
    }
    if let Some(value) = file {
        return parse_database_url(value, "storage.url");
    }

    Ok(StorageUrl::Sqlite(default_database_path(config_path)))
}

fn parse_database_url(value: &str, source_name: &'static str) -> Result<StorageUrl, LoadError> {
    StorageUrl::parse(value).map_err(|reason| LoadError::ParseDatabaseUrl {
        value: value.to_owned(),
        source_name,
        reason,
    })
}

fn default_database_path(config_path: Option<&Path>) -> PathBuf {
    config_path
        .and_then(Path::parent)
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.join(DEFAULT_DATABASE_FILE))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DATABASE_FILE))
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
    fn uses_default_addr_and_database_when_config_is_absent() {
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
        assert_eq!(
            settings.database_url,
            StorageUrl::Sqlite(home.join(".config").join("hoststamp").join("hoststamp.db"))
        );
    }

    #[test]
    fn reads_server_addr_and_storage_url_from_xdg_config_file() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let config_dir = tempdir.path().join("hoststamp");
        fs::create_dir_all(&config_dir).expect("config dir");
        fs::write(
            config_dir.join("config.toml"),
            r#"
                [server]
                addr = "127.0.0.1:9000"

                [storage]
                url = "sqlite:///tmp/custom-hoststamp.db"
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
        assert_eq!(
            settings.database_url,
            StorageUrl::Sqlite(PathBuf::from("/tmp/custom-hoststamp.db"))
        );
    }

    #[test]
    fn defaults_database_next_to_default_config_path() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let settings = load_with_env(
            Overrides::default(),
            ConfigEnv {
                xdg_config_home: Some(tempdir.path().to_path_buf()),
                ..ConfigEnv::default()
            },
        )
        .expect("settings");

        assert_eq!(
            settings.database_url,
            StorageUrl::Sqlite(tempdir.path().join("hoststamp").join("hoststamp.db"))
        );
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

                [storage]
                url = "sqlite:///tmp/file.db"
            "#,
        )
        .expect("config");

        let settings = load_with_env(
            Overrides {
                config_path: Some(config_path),
                addr: Some(parse_addr("127.0.0.1:9002", "test").expect("addr")),
                database_url: Some("sqlite:///tmp/cli.db".to_owned()),
            },
            ConfigEnv {
                addr: Some("127.0.0.1:9001".to_owned()),
                database_url: Some("sqlite:///tmp/env.db".to_owned()),
                ..ConfigEnv::default()
            },
        )
        .expect("settings");

        assert_eq!(
            settings.addr,
            parse_addr("127.0.0.1:9002", "test").expect("addr")
        );
        assert_eq!(
            settings.database_url,
            StorageUrl::Sqlite(PathBuf::from("/tmp/cli.db"))
        );
    }

    #[test]
    fn env_overrides_file_database_url() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let config_path = tempdir.path().join("config.toml");
        fs::write(
            &config_path,
            r#"
                [storage]
                url = "sqlite:///tmp/file.db"
            "#,
        )
        .expect("config");

        let settings = load_with_env(
            Overrides {
                config_path: Some(config_path),
                ..Overrides::default()
            },
            ConfigEnv {
                database_url: Some("sqlite:///tmp/env.db".to_owned()),
                ..ConfigEnv::default()
            },
        )
        .expect("settings");

        assert_eq!(
            settings.database_url,
            StorageUrl::Sqlite(PathBuf::from("/tmp/env.db"))
        );
    }

    #[test]
    fn reads_api_auth_from_config_and_env() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let config_path = tempdir.path().join("config.toml");
        fs::write(
            &config_path,
            r#"
                [api.auth]
                required = true
            "#,
        )
        .expect("config");

        let settings = load_with_env(
            Overrides {
                config_path: Some(config_path),
                ..Overrides::default()
            },
            ConfigEnv {
                admin_token: Some("admin".to_owned()),
                token_hash_key: Some("hash-key".to_owned()),
                ..ConfigEnv::default()
            },
        )
        .expect("settings");

        assert!(settings.auth.required);
        assert!(settings.auth.admin_token.is_some());
        assert!(settings.auth.token_hash_key.is_some());
    }

    #[test]
    fn required_api_auth_requires_secrets() {
        let error = load_with_env(
            Overrides::default(),
            ConfigEnv {
                api_auth_required: Some("true".to_owned()),
                ..ConfigEnv::default()
            },
        )
        .expect_err("missing auth secrets");

        assert!(matches!(error, LoadError::MissingSecret { .. }));
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
    fn invalid_database_url_is_an_error() {
        let error = load_with_env(
            Overrides::default(),
            ConfigEnv {
                database_url: Some("mysql://localhost/hoststamp".to_owned()),
                ..ConfigEnv::default()
            },
        )
        .expect_err("invalid database");

        assert!(
            matches!(error, LoadError::ParseDatabaseUrl { source_name, .. } if source_name == DATABASE_ENV)
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
}
