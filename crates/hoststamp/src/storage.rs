// SPDX-License-Identifier: FSL-1.1-ALv2

use crate::profile::{ProfileConfig, ProfileSlug};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

pub const DATABASE_ENV: &str = "HOSTSTAMP_DATABASE_URL";
pub const DEFAULT_DATABASE_FILE: &str = "hoststamp.db";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageUrl {
    Sqlite(PathBuf),
    Postgres(String),
}

impl StorageUrl {
    pub fn parse(value: &str) -> Result<Self, String> {
        let value = value.trim();
        if value.is_empty() {
            return Err("database URL must not be empty".to_owned());
        }

        if let Some(path) = value.strip_prefix("sqlite://") {
            if path.is_empty() {
                return Err("sqlite database URL must include a path".to_owned());
            }
            return Ok(Self::Sqlite(PathBuf::from(path)));
        }
        if let Some(path) = value.strip_prefix("sqlite:") {
            if path.is_empty() {
                return Err("sqlite database URL must include a path".to_owned());
            }
            return Ok(Self::Sqlite(PathBuf::from(path)));
        }
        if value.starts_with("postgres://") || value.starts_with("postgresql://") {
            return Ok(Self::Postgres(value.to_owned()));
        }
        if value.contains("://") {
            return Err("database URL scheme must be sqlite, postgres, or postgresql".to_owned());
        }

        Ok(Self::Sqlite(PathBuf::from(value)))
    }
}

#[derive(Debug, Clone)]
pub struct StoredProfile {
    pub id: Uuid,
    pub slug: ProfileSlug,
    pub config: ProfileConfig,
    pub config_hash: [u8; 32],
    pub last_atomic_value: i64,
}

pub struct ProfileStore {
    connection: Connection,
}

impl ProfileStore {
    pub fn open(url: &StorageUrl) -> Result<Self> {
        match url {
            StorageUrl::Sqlite(path) => Self::open_sqlite(path),
            StorageUrl::Postgres(_) => {
                bail!("Postgres storage is planned but not implemented in this build")
            }
        }
    }

    pub fn load_or_seed_profile(
        &mut self,
        slug: &ProfileSlug,
        seed_config: &ProfileConfig,
    ) -> Result<StoredProfile> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        let now = unix_epoch_millis()?;
        let config_json = serde_json::to_string(seed_config)?;
        let config_hash = config_hash(seed_config)?;
        if !active_profile_exists(&tx, slug)? {
            tx.execute(
                "INSERT INTO hoststamp_profiles (
                    id, slug, config_json, config_hash, last_atomic_value, created_at_ms, updated_at_ms
                ) VALUES (?1, ?2, ?3, ?4, 0, ?5, ?5)",
                params![
                    Uuid::now_v7().as_bytes().as_slice(),
                    slug.as_str(),
                    config_json,
                    config_hash.as_slice(),
                    now,
                ],
            )?;
        }

        let profile = select_profile(&tx, slug)?;
        tx.commit()?;
        Ok(profile)
    }

    pub fn load_profile(&self, slug: &ProfileSlug) -> Result<StoredProfile> {
        select_profile(&self.connection, slug)
    }

    pub fn list_profiles(&self) -> Result<Vec<StoredProfile>> {
        let mut statement = self.connection.prepare(
            "SELECT id, slug, config_json, config_hash, last_atomic_value
             FROM hoststamp_profiles
             WHERE replaced_at_ms IS NULL
             ORDER BY slug ASC",
        )?;
        let profiles = statement
            .query_map([], profile_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(profiles)
    }

    pub fn create_profile(
        &mut self,
        slug: &ProfileSlug,
        config: &ProfileConfig,
    ) -> Result<StoredProfile> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        if active_profile_exists(&tx, slug)? {
            bail!("profile {:?} already exists", slug.as_str());
        }

        let now = unix_epoch_millis()?;
        let config_json = serde_json::to_string(config)?;
        let config_hash = config_hash(config)?;
        tx.execute(
            "INSERT INTO hoststamp_profiles (
                id, slug, config_json, config_hash, last_atomic_value, created_at_ms, updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, 0, ?5, ?5)",
            params![
                Uuid::now_v7().as_bytes().as_slice(),
                slug.as_str(),
                config_json,
                config_hash.as_slice(),
                now,
            ],
        )?;

        let profile = select_profile(&tx, slug)?;
        tx.commit()?;
        Ok(profile)
    }

    pub fn delete_profile(&mut self, slug: &ProfileSlug) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = unix_epoch_millis()?;
        let changed = tx.execute(
            "UPDATE hoststamp_profiles
             SET replaced_at_ms = ?1,
                 updated_at_ms = ?1
             WHERE slug = ?2 AND replaced_at_ms IS NULL",
            params![now, slug.as_str()],
        )?;
        if changed == 0 {
            bail!("profile {:?} does not exist", slug.as_str());
        }
        tx.commit()?;
        Ok(())
    }

    pub fn reset_atomic_value(
        &mut self,
        slug: &ProfileSlug,
        atomic_value: i64,
    ) -> Result<StoredProfile> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = unix_epoch_millis()?;
        let changed = tx.execute(
            "UPDATE hoststamp_profiles
             SET last_atomic_value = ?1,
                 updated_at_ms = ?2
             WHERE slug = ?3 AND replaced_at_ms IS NULL",
            params![atomic_value, now, slug.as_str()],
        )?;
        if changed == 0 {
            bail!("profile {:?} does not exist", slug.as_str());
        }
        let profile = select_profile(&tx, slug)?;
        tx.commit()?;
        Ok(profile)
    }

    pub fn increment_atomic_value(&mut self, slug: &ProfileSlug) -> Result<i64> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        let now = unix_epoch_millis()?;
        let value = tx
            .query_row(
                "UPDATE hoststamp_profiles
                 SET last_atomic_value = last_atomic_value + 1,
                     updated_at_ms = ?1
                 WHERE slug = ?2
                   AND replaced_at_ms IS NULL
                   AND last_atomic_value < 9223372036854775807
                 RETURNING last_atomic_value",
                params![now, slug.as_str()],
                |row| row.get(0),
            )
            .optional()?;

        let Some(value) = value else {
            let current = tx
                .query_row(
                    "SELECT last_atomic_value
                     FROM hoststamp_profiles
                     WHERE slug = ?1 AND replaced_at_ms IS NULL",
                    params![slug.as_str()],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .with_context(|| format!("profile {:?} does not exist", slug.as_str()))?;
            if current == i64::MAX {
                bail!(
                    "atomic counter exhausted for profile {:?}: maximum atomic value is {}",
                    slug.as_str(),
                    i64::MAX
                );
            }
            bail!(
                "failed to increment atomic value for profile {:?}",
                slug.as_str()
            );
        };

        tx.commit()?;
        Ok(value)
    }

    pub fn replace_profile_config(
        &mut self,
        slug: &ProfileSlug,
        config: &ProfileConfig,
    ) -> Result<StoredProfile> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        let current = select_profile(&tx, slug)?;
        let now = unix_epoch_millis()?;
        let new_id = Uuid::now_v7();
        tx.execute(
            "UPDATE hoststamp_profiles
             SET replaced_at_ms = ?1,
                 replaced_by_id = ?2,
                 updated_at_ms = ?1
             WHERE id = ?3",
            params![
                now,
                new_id.as_bytes().as_slice(),
                current.id.as_bytes().as_slice(),
            ],
        )?;

        let config_json = serde_json::to_string(config)?;
        let config_hash = config_hash(config)?;
        tx.execute(
            "INSERT INTO hoststamp_profiles (
                id, slug, config_json, config_hash, last_atomic_value, created_at_ms, updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, 0, ?5, ?5)",
            params![
                new_id.as_bytes().as_slice(),
                slug.as_str(),
                config_json,
                config_hash.as_slice(),
                now,
            ],
        )?;

        let profile = select_profile(&tx, slug)?;
        tx.commit()?;
        Ok(profile)
    }

    fn open_sqlite(path: &Path) -> Result<Self> {
        if path != Path::new(":memory:") {
            ensure_database_parent(path)?;
        }

        let connection = Connection::open(path)
            .with_context(|| format!("failed to open sqlite database {}", path.display()))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .context("failed to configure sqlite busy timeout")?;
        connection.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            ",
        )?;
        migrate(&connection)?;
        Ok(Self { connection })
    }
}

fn ensure_database_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        if parent.exists() {
            if !parent.is_dir() {
                bail!(
                    "sqlite database parent is not a directory: {}",
                    parent.display()
                );
            }
            return Ok(());
        }
        if path.is_relative() {
            bail!(
                "sqlite database parent directory does not exist: {}",
                parent.display()
            );
        }

        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create database directory {}", parent.display()))?;
    }
    Ok(())
}

fn migrate(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS hoststamp_profiles (
            id BLOB PRIMARY KEY NOT NULL CHECK(length(id) = 16),
            slug TEXT NOT NULL,
            config_json TEXT NOT NULL,
            config_hash BLOB NOT NULL CHECK(length(config_hash) = 32),
            last_atomic_value INTEGER NOT NULL DEFAULT 0,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            replaced_at_ms INTEGER,
            replaced_by_id BLOB CHECK(replaced_by_id IS NULL OR length(replaced_by_id) = 16)
        );
        ",
    )?;
    connection.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_hoststamp_profiles_created_at_ms
            ON hoststamp_profiles(created_at_ms);

        CREATE INDEX IF NOT EXISTS idx_hoststamp_profiles_updated_at_ms
            ON hoststamp_profiles(updated_at_ms);

        CREATE UNIQUE INDEX IF NOT EXISTS idx_hoststamp_profiles_active_slug
            ON hoststamp_profiles(slug)
            WHERE replaced_at_ms IS NULL;
        ",
    )?;
    Ok(())
}

fn active_profile_exists(connection: &Connection, slug: &ProfileSlug) -> Result<bool> {
    let count: i64 = connection.query_row(
        "SELECT COUNT(*)
         FROM hoststamp_profiles
         WHERE slug = ?1 AND replaced_at_ms IS NULL",
        params![slug.as_str()],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn select_profile(connection: &Connection, slug: &ProfileSlug) -> Result<StoredProfile> {
    connection
        .query_row(
            "SELECT id, slug, config_json, config_hash, last_atomic_value
             FROM hoststamp_profiles
             WHERE slug = ?1 AND replaced_at_ms IS NULL",
            params![slug.as_str()],
            profile_from_row,
        )
        .optional()?
        .with_context(|| format!("profile {:?} does not exist", slug.as_str()))
}

fn profile_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredProfile> {
    let id_blob: Vec<u8> = row.get(0)?;
    let slug_value: String = row.get(1)?;
    let config_json: String = row.get(2)?;
    let config_hash_blob: Vec<u8> = row.get(3)?;
    let last_atomic_value: i64 = row.get(4)?;
    stored_profile_from_parts(
        id_blob,
        slug_value,
        config_json,
        config_hash_blob,
        last_atomic_value,
    )
    .map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, error.into())
    })
}

fn stored_profile_from_parts(
    id_blob: Vec<u8>,
    slug_value: String,
    config_json: String,
    config_hash_blob: Vec<u8>,
    last_atomic_value: i64,
) -> Result<StoredProfile> {
    let id = Uuid::from_slice(&id_blob).context("stored profile id is not a UUID")?;
    let slug = slug_value
        .parse::<ProfileSlug>()
        .map_err(anyhow::Error::msg)
        .context("stored profile slug is invalid")?;
    let config = serde_json::from_str::<ProfileConfig>(&config_json)
        .context("stored profile config is invalid")?;
    let config_hash = fixed_hash(config_hash_blob)?;
    Ok(StoredProfile {
        id,
        slug,
        config,
        config_hash,
        last_atomic_value,
    })
}

fn fixed_hash(value: Vec<u8>) -> Result<[u8; 32]> {
    value
        .try_into()
        .map_err(|_| anyhow::anyhow!("stored profile config hash is not 32 bytes"))
}

pub fn config_hash(config: &ProfileConfig) -> Result<[u8; 32]> {
    let bytes = serde_json::to_vec(config)?;
    Ok(Sha256::digest(bytes).into())
}

pub fn unix_epoch_millis() -> Result<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?;
    i64::try_from(duration.as_millis()).context("current timestamp does not fit in i64")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{generator::GenerateOptions, profile::DEFAULT_PROFILE_SLUG};

    #[test]
    fn parses_storage_urls() {
        assert_eq!(
            StorageUrl::parse("sqlite:///tmp/hoststamp.db").expect("sqlite"),
            StorageUrl::Sqlite(PathBuf::from("/tmp/hoststamp.db"))
        );
        assert_eq!(
            StorageUrl::parse("/tmp/hoststamp.db").expect("path"),
            StorageUrl::Sqlite(PathBuf::from("/tmp/hoststamp.db"))
        );
        assert!(matches!(
            StorageUrl::parse("postgres://localhost/hoststamp").expect("postgres"),
            StorageUrl::Postgres(_)
        ));
        assert!(StorageUrl::parse("mysql://localhost/hoststamp").is_err());
    }

    #[test]
    fn seeds_and_loads_profile() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let url = StorageUrl::Sqlite(tempdir.path().join(DEFAULT_DATABASE_FILE));
        let mut store = ProfileStore::open(&url).expect("store");
        let seed = ProfileConfig::default();

        let profile = store
            .load_or_seed_profile(&ProfileSlug::default_profile(), &seed)
            .expect("profile");
        let loaded = store
            .load_or_seed_profile(
                &ProfileSlug::default_profile(),
                &ProfileConfig {
                    suffix: crate::profile::SuffixProfileConfig {
                        min_length: 9,
                        ..seed.suffix.clone()
                    },
                    ..seed.clone()
                },
            )
            .expect("loaded profile");

        assert_eq!(profile.id, loaded.id);
        assert_eq!(loaded.slug.as_str(), DEFAULT_PROFILE_SLUG);
        assert_eq!(loaded.config, seed);
        assert_eq!(loaded.config_hash, config_hash(&seed).expect("hash"));
        assert_eq!(loaded.last_atomic_value, 0);
    }

    #[test]
    fn profile_config_round_trips_through_sqlite() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let url = StorageUrl::Sqlite(tempdir.path().join(DEFAULT_DATABASE_FILE));
        let mut store = ProfileStore::open(&url).expect("store");
        let slug = "team-a".parse::<ProfileSlug>().expect("slug");
        let seed = ProfileConfig::from(&GenerateOptions {
            word1_lengths: Some(vec![4, 5]),
            suffix_min_length: 8,
            ..GenerateOptions::default()
        });

        let profile = store.load_or_seed_profile(&slug, &seed).expect("profile");
        let options = profile.config.to_generate_options(11);

        assert_eq!(profile.slug, slug);
        assert_eq!(options.count, 11);
        assert_eq!(options.word1_lengths, Some(vec![4, 5]));
        assert_eq!(options.suffix_min_length, 8);
    }

    #[test]
    fn increments_atomic_value_one_step_at_a_time() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let url = StorageUrl::Sqlite(tempdir.path().join(DEFAULT_DATABASE_FILE));
        let mut store = ProfileStore::open(&url).expect("store");
        let slug = ProfileSlug::default_profile();
        store
            .load_or_seed_profile(&slug, &ProfileConfig::default())
            .expect("profile");

        assert_eq!(store.increment_atomic_value(&slug).expect("value"), 1);
        assert_eq!(store.increment_atomic_value(&slug).expect("value"), 2);
        assert_eq!(
            store
                .load_or_seed_profile(&slug, &ProfileConfig::default())
                .expect("profile")
                .last_atomic_value,
            2
        );
    }

    #[test]
    fn increment_atomic_value_stops_at_i64_max() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let url = StorageUrl::Sqlite(tempdir.path().join(DEFAULT_DATABASE_FILE));
        let mut store = ProfileStore::open(&url).expect("store");
        let slug = ProfileSlug::default_profile();
        store
            .load_or_seed_profile(&slug, &ProfileConfig::default())
            .expect("profile");
        store
            .connection
            .execute(
                "UPDATE hoststamp_profiles SET last_atomic_value = ?1 WHERE slug = ?2",
                params![i64::MAX, slug.as_str()],
            )
            .expect("set counter");

        let error = store
            .increment_atomic_value(&slug)
            .expect_err("counter should be exhausted");

        assert!(error.to_string().contains("atomic counter exhausted"));
        assert_eq!(
            store
                .load_or_seed_profile(&slug, &ProfileConfig::default())
                .expect("profile")
                .last_atomic_value,
            i64::MAX
        );
    }

    #[test]
    fn replacing_profile_config_creates_new_active_profile() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let url = StorageUrl::Sqlite(tempdir.path().join(DEFAULT_DATABASE_FILE));
        let mut store = ProfileStore::open(&url).expect("store");
        let slug = "team-a".parse::<ProfileSlug>().expect("slug");
        let seed = ProfileConfig::default();
        let original = store.load_or_seed_profile(&slug, &seed).expect("profile");
        assert_eq!(store.increment_atomic_value(&slug).expect("value"), 1);

        let replacement_config = ProfileConfig::from(&GenerateOptions {
            word1_lengths: Some(vec![4]),
            ..GenerateOptions::default()
        });
        let replacement = store
            .replace_profile_config(&slug, &replacement_config)
            .expect("replacement");

        assert_ne!(replacement.id, original.id);
        assert_eq!(replacement.slug, slug);
        assert_eq!(replacement.config, replacement_config);
        assert_eq!(replacement.last_atomic_value, 0);
        assert_eq!(store.increment_atomic_value(&slug).expect("value"), 1);
        assert_eq!(
            store
                .load_or_seed_profile(&slug, &seed)
                .expect("profile")
                .id,
            replacement.id
        );

        let retired_slug: String = store
            .connection
            .query_row(
                "SELECT slug FROM hoststamp_profiles WHERE id = ?1",
                params![original.id.as_bytes().as_slice()],
                |row| row.get(0),
            )
            .expect("retired slug");
        assert_eq!(retired_slug, slug.as_str());
        let retired_atomic_value: i64 = store
            .connection
            .query_row(
                "SELECT last_atomic_value FROM hoststamp_profiles WHERE id = ?1",
                params![original.id.as_bytes().as_slice()],
                |row| row.get(0),
            )
            .expect("retired value");
        assert_eq!(retired_atomic_value, 1);
    }

    #[test]
    fn profile_admin_methods_manage_active_profiles() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let url = StorageUrl::Sqlite(tempdir.path().join(DEFAULT_DATABASE_FILE));
        let mut store = ProfileStore::open(&url).expect("store");
        let slug = "team-a".parse::<ProfileSlug>().expect("slug");
        let seed = ProfileConfig::default();

        let created = store.create_profile(&slug, &seed).expect("created");
        assert_eq!(created.slug, slug);
        assert_eq!(store.list_profiles().expect("profiles").len(), 1);

        let reset = store
            .reset_atomic_value(&slug, 10)
            .expect("reset atomic value");
        assert_eq!(reset.last_atomic_value, 10);
        assert_eq!(store.increment_atomic_value(&slug).expect("value"), 11);

        store.delete_profile(&slug).expect("delete");
        assert!(store.load_profile(&slug).is_err());
        assert!(store.list_profiles().expect("profiles").is_empty());

        let recreated = store.create_profile(&slug, &seed).expect("recreated");
        assert_ne!(recreated.id, created.id);
        assert_eq!(recreated.last_atomic_value, 0);
    }

    #[test]
    fn postgres_backend_returns_clear_error() {
        let error = match ProfileStore::open(&StorageUrl::Postgres(
            "postgres://localhost/hoststamp".to_owned(),
        )) {
            Ok(_) => panic!("postgres should not be implemented yet"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("Postgres storage is planned"));
    }

    #[test]
    fn relative_sqlite_path_does_not_create_missing_parent_dirs() {
        let url = StorageUrl::Sqlite(PathBuf::from(format!(
            "missing-parent-{}/hoststamp.db",
            Uuid::new_v4()
        )));
        let error = match ProfileStore::open(&url) {
            Ok(_) => panic!("relative missing parent should fail"),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("sqlite database parent directory does not exist")
        );
    }
}
