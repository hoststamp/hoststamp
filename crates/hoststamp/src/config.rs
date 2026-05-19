// SPDX-License-Identifier: FSL-1.1-ALv2

use serde::Deserialize;
use std::{
    env, fmt, fs, io,
    net::{AddrParseError, SocketAddr},
    path::{Path, PathBuf},
};

pub const DEFAULT_ADDR: &str = "127.0.0.1:8080";
pub const CONFIG_ENV: &str = "HOSTSTAMP_CONFIG";
pub const ADDR_ENV: &str = "HOSTSTAMP_ADDR";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub addr: SocketAddr,
    pub config_path: Option<PathBuf>,
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
    pub xdg_config_home: Option<PathBuf>,
    pub home: Option<PathBuf>,
}

impl ConfigEnv {
    pub fn process() -> Self {
        Self {
            config_path: env::var_os(CONFIG_ENV).map(PathBuf::from),
            addr: env::var(ADDR_ENV).ok(),
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
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MissingConfig { .. } => None,
            Self::Read { source, .. } => Some(source),
            Self::ParseToml { source, .. } => Some(source),
            Self::ParseAddr { source, .. } => Some(source),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    server: Option<ServerConfig>,
}

#[derive(Debug, Deserialize, Default)]
struct ServerConfig {
    addr: Option<String>,
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

    if let Some(path) = config_path.as_deref() {
        if path.exists() {
            if let Some(config_addr) = read_file_config(path)?
                .server
                .and_then(|server| server.addr)
            {
                addr = parse_addr(&config_addr, "server.addr")?;
            }
            loaded_config_path = Some(path.to_path_buf());
        } else if explicit_config {
            return Err(LoadError::MissingConfig {
                path: path.to_path_buf(),
            });
        }
    }

    if let Some(env_addr) = env.addr {
        addr = parse_addr(&env_addr, ADDR_ENV)?;
    }

    if let Some(cli_addr) = overrides.addr {
        addr = cli_addr;
    }

    Ok(Settings {
        addr,
        config_path: loaded_config_path,
    })
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
}
