// SPDX-License-Identifier: FSL-1.1-ALv2

use crate::profile::{ProfileAccess, ProfileConfig, ProfileSlug};
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
    pub access: ProfileAccess,
    pub config: ProfileConfig,
    pub config_hash: [u8; 32],
    pub last_atomic_value: i64,
}

#[derive(Debug, Clone)]
pub struct StoredProfileToken {
    pub token_id: String,
    pub profile_id: Uuid,
    pub name: String,
    pub created_at_ms: i64,
    pub expires_at_ms: Option<i64>,
    pub last_used_at_ms: Option<i64>,
    pub revoked_at_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ProfileTokenAuthRecord {
    pub profile_id: Uuid,
    pub token_hash: Vec<u8>,
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
                    id, slug, access, config_json, config_hash, last_atomic_value, created_at_ms, updated_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)",
                params![
                    Uuid::now_v7().as_bytes().as_slice(),
                    slug.as_str(),
                    ProfileAccess::Private.to_string(),
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
            "SELECT id, slug, access, config_json, config_hash, last_atomic_value
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
                id, slug, access, config_json, config_hash, last_atomic_value, created_at_ms, updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)",
            params![
                Uuid::now_v7().as_bytes().as_slice(),
                slug.as_str(),
                ProfileAccess::Private.to_string(),
                config_json,
                config_hash.as_slice(),
                now,
            ],
        )?;

        let profile = select_profile(&tx, slug)?;
        tx.commit()?;
        Ok(profile)
    }

    pub fn import_profile(
        &mut self,
        slug: &ProfileSlug,
        id: Uuid,
        access: ProfileAccess,
        config: &ProfileConfig,
        last_atomic_value: i64,
    ) -> Result<StoredProfile> {
        if last_atomic_value < 0 {
            bail!("last atomic value must be at least 0");
        }

        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = unix_epoch_millis()?;
        let config_json = serde_json::to_string(config)?;
        let config_hash = config_hash(config)?;
        if let Some((owner_slug, owner_active)) = profile_id_owner(&tx, id)?
            && (owner_slug != slug.as_str() || !owner_active)
        {
            bail!("profile id {id} already exists");
        }

        if active_profile_exists(&tx, slug)? {
            let current = select_profile(&tx, slug)?;
            if current.id == id {
                tx.execute(
                    "UPDATE hoststamp_profiles
                     SET access = ?1,
                         config_json = ?2,
                         config_hash = ?3,
                         last_atomic_value = ?4,
                         updated_at_ms = ?5
                     WHERE id = ?6 AND replaced_at_ms IS NULL",
                    params![
                        access.to_string(),
                        config_json,
                        config_hash.as_slice(),
                        last_atomic_value,
                        now,
                        id.as_bytes().as_slice(),
                    ],
                )?;
                let profile = select_profile(&tx, slug)?;
                tx.commit()?;
                return Ok(profile);
            }

            tx.execute(
                "UPDATE hoststamp_profiles
                 SET replaced_at_ms = ?1,
                     replaced_by_id = ?2,
                     updated_at_ms = ?1
                 WHERE id = ?3",
                params![
                    now,
                    id.as_bytes().as_slice(),
                    current.id.as_bytes().as_slice(),
                ],
            )?;
        }

        tx.execute(
            "INSERT INTO hoststamp_profiles (
                id, slug, access, config_json, config_hash, last_atomic_value, created_at_ms, updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![
                id.as_bytes().as_slice(),
                slug.as_str(),
                access.to_string(),
                config_json,
                config_hash.as_slice(),
                last_atomic_value,
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

    pub fn set_profile_access(
        &mut self,
        slug: &ProfileSlug,
        access: ProfileAccess,
    ) -> Result<StoredProfile> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = unix_epoch_millis()?;
        let changed = tx.execute(
            "UPDATE hoststamp_profiles
             SET access = ?1,
                 updated_at_ms = ?2
             WHERE slug = ?3 AND replaced_at_ms IS NULL",
            params![access.to_string(), now, slug.as_str()],
        )?;
        if changed == 0 {
            bail!("profile {:?} does not exist", slug.as_str());
        }
        let profile = select_profile(&tx, slug)?;
        tx.commit()?;
        Ok(profile)
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
                id, slug, access, config_json, config_hash, last_atomic_value, created_at_ms, updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)",
            params![
                new_id.as_bytes().as_slice(),
                slug.as_str(),
                ProfileAccess::Private.to_string(),
                config_json,
                config_hash.as_slice(),
                now,
            ],
        )?;

        let profile = select_profile(&tx, slug)?;
        tx.commit()?;
        Ok(profile)
    }

    pub fn create_profile_token(
        &mut self,
        profile_id: Uuid,
        token_id: &str,
        name: &str,
        token_hash: [u8; 32],
        expires_at_ms: Option<i64>,
    ) -> Result<StoredProfileToken> {
        let name = name.trim();
        validate_profile_token_name(name)?;

        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = unix_epoch_millis()?;
        if expires_at_ms.is_some_and(|expires_at_ms| expires_at_ms <= now) {
            bail!("profile token expiration must be in the future");
        }
        tx.execute(
            "UPDATE hoststamp_profile_tokens
             SET revoked_at_ms = ?3
             WHERE profile_id = ?1
               AND name = ?2
               AND revoked_at_ms IS NULL
               AND expires_at_ms IS NOT NULL
               AND expires_at_ms <= ?3",
            params![profile_id.as_bytes().as_slice(), name, now],
        )?;
        let active_name_count: i64 = tx.query_row(
            "SELECT COUNT(*)
             FROM hoststamp_profile_tokens
             WHERE profile_id = ?1
               AND name = ?2
               AND revoked_at_ms IS NULL",
            params![profile_id.as_bytes().as_slice(), name],
            |row| row.get(0),
        )?;
        if active_name_count > 0 {
            bail!("active profile token name {name:?} already exists");
        }
        tx.execute(
            "INSERT INTO hoststamp_profile_tokens (
                token_id, profile_id, name, token_hash, created_at_ms, expires_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                token_id,
                profile_id.as_bytes().as_slice(),
                name,
                token_hash.as_slice(),
                now,
                expires_at_ms,
            ],
        )?;
        let token = select_profile_token(&tx, profile_id, token_id)?;
        tx.commit()?;
        Ok(token)
    }

    pub fn list_profile_tokens(&self, profile_id: Uuid) -> Result<Vec<StoredProfileToken>> {
        let mut statement = self.connection.prepare(
            "SELECT token_id, profile_id, name, created_at_ms, expires_at_ms, last_used_at_ms, revoked_at_ms
             FROM hoststamp_profile_tokens
             WHERE profile_id = ?1
             ORDER BY created_at_ms ASC",
        )?;
        let tokens = statement
            .query_map(
                params![profile_id.as_bytes().as_slice()],
                profile_token_from_row,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(tokens)
    }

    pub fn revoke_profile_token(
        &mut self,
        profile_id: Uuid,
        token_id: &str,
    ) -> Result<StoredProfileToken> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = unix_epoch_millis()?;
        let changed = tx.execute(
            "UPDATE hoststamp_profile_tokens
             SET revoked_at_ms = ?1
             WHERE profile_id = ?2 AND token_id = ?3 AND revoked_at_ms IS NULL",
            params![now, profile_id.as_bytes().as_slice(), token_id],
        )?;
        if changed == 0 {
            bail!("active profile token {token_id:?} does not exist");
        }
        let token = select_profile_token(&tx, profile_id, token_id)?;
        tx.commit()?;
        Ok(token)
    }

    pub fn load_profile_token_auth(
        &self,
        token_id: &str,
    ) -> Result<Option<ProfileTokenAuthRecord>> {
        let now = unix_epoch_millis()?;
        self.connection
            .query_row(
                "SELECT profile_id, token_hash
                 FROM hoststamp_profile_tokens
                 WHERE token_id = ?1
                   AND revoked_at_ms IS NULL
                   AND (expires_at_ms IS NULL OR expires_at_ms > ?2)",
                params![token_id, now],
                |row| {
                    let profile_id_blob: Vec<u8> = row.get(0)?;
                    let profile_id = Uuid::from_slice(&profile_id_blob).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Blob,
                            error.into(),
                        )
                    })?;
                    Ok(ProfileTokenAuthRecord {
                        profile_id,
                        token_hash: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn mark_profile_token_used(&mut self, profile_id: Uuid, token_id: &str) -> Result<()> {
        let now = unix_epoch_millis()?;
        self.connection.execute(
            "UPDATE hoststamp_profile_tokens
             SET last_used_at_ms = ?1
             WHERE profile_id = ?2
               AND token_id = ?3
               AND revoked_at_ms IS NULL
               AND (expires_at_ms IS NULL OR expires_at_ms > ?1)",
            params![now, profile_id.as_bytes().as_slice(), token_id],
        )?;
        Ok(())
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
            access TEXT NOT NULL DEFAULT 'private' CHECK(access IN ('public', 'private')),
            config_json TEXT NOT NULL,
            config_hash BLOB NOT NULL CHECK(length(config_hash) = 32),
            last_atomic_value INTEGER NOT NULL DEFAULT 0,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            replaced_at_ms INTEGER,
            replaced_by_id BLOB CHECK(replaced_by_id IS NULL OR length(replaced_by_id) = 16)
        );

        CREATE TABLE IF NOT EXISTS hoststamp_profile_tokens (
            token_id TEXT PRIMARY KEY NOT NULL,
            profile_id BLOB NOT NULL CHECK(length(profile_id) = 16)
                REFERENCES hoststamp_profiles(id),
            name TEXT NOT NULL,
            token_hash BLOB NOT NULL CHECK(length(token_hash) = 32),
            created_at_ms INTEGER NOT NULL,
            expires_at_ms INTEGER,
            last_used_at_ms INTEGER,
            revoked_at_ms INTEGER
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

        CREATE INDEX IF NOT EXISTS idx_hoststamp_profile_tokens_profile_id
            ON hoststamp_profile_tokens(profile_id);

        CREATE UNIQUE INDEX IF NOT EXISTS idx_hoststamp_profile_tokens_active_name
            ON hoststamp_profile_tokens(profile_id, name)
            WHERE revoked_at_ms IS NULL;
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

fn validate_profile_token_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("profile token name must not be empty");
    }
    if name.len() > 64 {
        bail!("profile token name must be 64 characters or fewer");
    }
    if !name.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
    }) {
        bail!(
            "profile token name must use lowercase ASCII letters, digits, hyphen, underscore, or dot"
        );
    }
    if !name
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
        || !name
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
    {
        bail!("profile token name must start and end with a letter or digit");
    }
    Ok(())
}

fn select_profile(connection: &Connection, slug: &ProfileSlug) -> Result<StoredProfile> {
    connection
        .query_row(
            "SELECT id, slug, access, config_json, config_hash, last_atomic_value
             FROM hoststamp_profiles
             WHERE slug = ?1 AND replaced_at_ms IS NULL",
            params![slug.as_str()],
            profile_from_row,
        )
        .optional()?
        .with_context(|| format!("profile {:?} does not exist", slug.as_str()))
}

fn profile_id_owner(connection: &Connection, id: Uuid) -> Result<Option<(String, bool)>> {
    Ok(connection
        .query_row(
            "SELECT slug, replaced_at_ms IS NULL
             FROM hoststamp_profiles
             WHERE id = ?1",
            params![id.as_bytes().as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?)
}

fn profile_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredProfile> {
    let id_blob: Vec<u8> = row.get(0)?;
    let slug_value: String = row.get(1)?;
    let access_value: String = row.get(2)?;
    let config_json: String = row.get(3)?;
    let config_hash_blob: Vec<u8> = row.get(4)?;
    let last_atomic_value: i64 = row.get(5)?;
    stored_profile_from_parts(
        id_blob,
        slug_value,
        access_value,
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
    access_value: String,
    config_json: String,
    config_hash_blob: Vec<u8>,
    last_atomic_value: i64,
) -> Result<StoredProfile> {
    let id = Uuid::from_slice(&id_blob).context("stored profile id is not a UUID")?;
    let slug = slug_value
        .parse::<ProfileSlug>()
        .map_err(anyhow::Error::msg)
        .context("stored profile slug is invalid")?;
    let access = access_value
        .parse::<ProfileAccess>()
        .map_err(anyhow::Error::msg)
        .context("stored profile access is invalid")?;
    let config = serde_json::from_str::<ProfileConfig>(&config_json)
        .context("stored profile config is invalid")?;
    let config_hash = fixed_hash(config_hash_blob)?;
    Ok(StoredProfile {
        id,
        slug,
        access,
        config,
        config_hash,
        last_atomic_value,
    })
}

fn select_profile_token(
    connection: &Connection,
    profile_id: Uuid,
    token_id: &str,
) -> Result<StoredProfileToken> {
    connection
        .query_row(
            "SELECT token_id, profile_id, name, created_at_ms, expires_at_ms, last_used_at_ms, revoked_at_ms
             FROM hoststamp_profile_tokens
             WHERE profile_id = ?1 AND token_id = ?2",
            params![profile_id.as_bytes().as_slice(), token_id],
            profile_token_from_row,
        )
        .optional()?
        .with_context(|| format!("profile token {token_id:?} does not exist"))
}

fn profile_token_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredProfileToken> {
    let token_id: String = row.get(0)?;
    let profile_id_blob: Vec<u8> = row.get(1)?;
    let profile_id = Uuid::from_slice(&profile_id_blob).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Blob, error.into())
    })?;

    Ok(StoredProfileToken {
        token_id,
        profile_id,
        name: row.get(2)?,
        created_at_ms: row.get(3)?,
        expires_at_ms: row.get(4)?,
        last_used_at_ms: row.get(5)?,
        revoked_at_ms: row.get(6)?,
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
            StorageUrl::parse("sqlite:/tmp/hoststamp.db").expect("sqlite"),
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
        assert!(matches!(
            StorageUrl::parse("postgresql://localhost/hoststamp").expect("postgres"),
            StorageUrl::Postgres(_)
        ));
        assert!(StorageUrl::parse("").is_err());
        assert!(StorageUrl::parse("sqlite://").is_err());
        assert!(StorageUrl::parse("sqlite:").is_err());
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
        assert_eq!(loaded.access, ProfileAccess::Private);
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
    fn concurrent_atomic_increments_are_gapless() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let url = StorageUrl::Sqlite(tempdir.path().join(DEFAULT_DATABASE_FILE));
        let mut store = ProfileStore::open(&url).expect("store");
        let slug = ProfileSlug::default_profile();
        store
            .load_or_seed_profile(&slug, &ProfileConfig::default())
            .expect("profile");
        drop(store);

        let mut handles = Vec::new();
        for _ in 0..16 {
            let url = url.clone();
            let slug = slug.clone();
            handles.push(std::thread::spawn(move || {
                let mut store = ProfileStore::open(&url).expect("store");
                store.increment_atomic_value(&slug).expect("value")
            }));
        }

        let mut values = handles
            .into_iter()
            .map(|handle| handle.join().expect("thread"))
            .collect::<Vec<_>>();
        values.sort_unstable();

        assert_eq!(values, (1..=16).collect::<Vec<_>>());
    }

    #[test]
    fn concurrent_profile_token_names_are_reserved_once() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let url = StorageUrl::Sqlite(tempdir.path().join(DEFAULT_DATABASE_FILE));
        let mut store = ProfileStore::open(&url).expect("store");
        let slug = "team-a".parse::<ProfileSlug>().expect("slug");
        let profile = store
            .create_profile(&slug, &ProfileConfig::default())
            .expect("profile");
        drop(store);

        let mut handles = Vec::new();
        for index in 0..8 {
            let url = url.clone();
            let profile_id = profile.id;
            handles.push(std::thread::spawn(move || {
                let mut store = ProfileStore::open(&url).expect("store");
                store
                    .create_profile_token(
                        profile_id,
                        &format!("token-id-{index}"),
                        "deploy",
                        [7_u8; 32],
                        None,
                    )
                    .map(|token| token.token_id)
                    .map_err(|error| error.to_string())
            }));
        }

        let results = handles
            .into_iter()
            .map(|handle| handle.join().expect("thread"))
            .collect::<Vec<_>>();

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| result
                    .as_ref()
                    .is_err_and(|error| error.contains("already exists")))
                .count(),
            7
        );

        let store = ProfileStore::open(&url).expect("store");
        let tokens = store.list_profile_tokens(profile.id).expect("tokens");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].name, "deploy");
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
        let duplicate = store
            .create_profile(&slug, &seed)
            .expect_err("duplicate active profile should fail");
        assert!(duplicate.to_string().contains("already exists"));

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

        let missing = "missing".parse::<ProfileSlug>().expect("slug");
        assert!(store.delete_profile(&missing).is_err());
        assert!(
            store
                .set_profile_access(&missing, ProfileAccess::Public)
                .is_err()
        );
        assert!(store.reset_atomic_value(&missing, 1).is_err());
    }

    #[test]
    fn import_profile_replaces_active_slug_when_id_changes() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let url = StorageUrl::Sqlite(tempdir.path().join(DEFAULT_DATABASE_FILE));
        let mut store = ProfileStore::open(&url).expect("store");
        let slug = "team-a".parse::<ProfileSlug>().expect("slug");
        let seed = ProfileConfig::default();
        let original = store.create_profile(&slug, &seed).expect("created");
        let imported_id = Uuid::now_v7();

        let imported = store
            .import_profile(&slug, imported_id, ProfileAccess::Public, &seed, 7)
            .expect("imported");

        assert_eq!(imported.id, imported_id);
        assert_eq!(imported.slug, slug);
        assert_eq!(imported.access, ProfileAccess::Public);
        assert_eq!(imported.last_atomic_value, 7);
        assert_eq!(store.load_profile(&slug).expect("active").id, imported_id);

        let old_replaced: Option<i64> = store
            .connection
            .query_row(
                "SELECT replaced_at_ms FROM hoststamp_profiles WHERE id = ?1",
                params![original.id.as_bytes().as_slice()],
                |row| row.get(0),
            )
            .expect("old profile");
        assert!(old_replaced.is_some());

        let negative = store
            .import_profile(&slug, Uuid::now_v7(), ProfileAccess::Private, &seed, -1)
            .expect_err("negative atomic value should fail");
        assert!(negative.to_string().contains("last atomic value"));
    }

    #[test]
    fn import_profile_rejects_id_owned_by_another_profile() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let url = StorageUrl::Sqlite(tempdir.path().join(DEFAULT_DATABASE_FILE));
        let mut store = ProfileStore::open(&url).expect("store");
        let original_slug = "team-a".parse::<ProfileSlug>().expect("slug");
        let imported_slug = "team-b".parse::<ProfileSlug>().expect("slug");
        let seed = ProfileConfig::default();
        let original = store
            .create_profile(&original_slug, &seed)
            .expect("created");

        let error = store
            .import_profile(
                &imported_slug,
                original.id,
                ProfileAccess::Private,
                &seed,
                0,
            )
            .expect_err("duplicate id should be rejected");

        assert!(error.to_string().contains("already exists"));
    }

    #[test]
    fn profile_access_and_tokens_round_trip() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let url = StorageUrl::Sqlite(tempdir.path().join(DEFAULT_DATABASE_FILE));
        let mut store = ProfileStore::open(&url).expect("store");
        let slug = "team-a".parse::<ProfileSlug>().expect("slug");
        let profile = store
            .create_profile(&slug, &ProfileConfig::default())
            .expect("profile");

        let public = store
            .set_profile_access(&slug, ProfileAccess::Public)
            .expect("access");
        assert_eq!(public.access, ProfileAccess::Public);

        let hash = [7_u8; 32];
        let token = store
            .create_profile_token(profile.id, "token-id", "deploy", hash, None)
            .expect("token");
        assert_eq!(token.name, "deploy");
        assert!(token.expires_at_ms.is_none());
        assert_eq!(
            store.list_profile_tokens(profile.id).expect("tokens").len(),
            1
        );

        let auth_record = store
            .load_profile_token_auth("token-id")
            .expect("auth")
            .expect("record");
        assert_eq!(auth_record.profile_id, profile.id);
        assert_eq!(auth_record.token_hash, hash);

        store
            .mark_profile_token_used(profile.id, "token-id")
            .expect("mark used");
        let used = store
            .list_profile_tokens(profile.id)
            .expect("tokens")
            .into_iter()
            .find(|token| token.token_id == "token-id")
            .expect("token");
        assert!(used.last_used_at_ms.is_some());

        let duplicate = store
            .create_profile_token(profile.id, "token-id-2", "deploy", hash, None)
            .expect_err("duplicate active name should be rejected");
        assert!(duplicate.to_string().contains("already exists"));

        let invalid_name = store
            .create_profile_token(profile.id, "token-id-3", "Deploy", hash, None)
            .expect_err("invalid name should be rejected");
        assert!(invalid_name.to_string().contains("lowercase ASCII"));

        let revoked = store
            .revoke_profile_token(profile.id, "token-id")
            .expect("revoked");
        assert!(revoked.revoked_at_ms.is_some());
        assert!(
            store
                .load_profile_token_auth("token-id")
                .expect("auth")
                .is_none()
        );
    }

    #[test]
    fn expired_profile_tokens_do_not_authorize() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let url = StorageUrl::Sqlite(tempdir.path().join(DEFAULT_DATABASE_FILE));
        let mut store = ProfileStore::open(&url).expect("store");
        let slug = "team-a".parse::<ProfileSlug>().expect("slug");
        let profile = store
            .create_profile(&slug, &ProfileConfig::default())
            .expect("profile");
        let hash = [7_u8; 32];
        let expires_at_ms = unix_epoch_millis().expect("now") + 60_000;
        let token = store
            .create_profile_token(profile.id, "token-id", "deploy", hash, Some(expires_at_ms))
            .expect("token");
        assert_eq!(token.expires_at_ms, Some(expires_at_ms));

        store
            .connection
            .execute(
                "UPDATE hoststamp_profile_tokens SET expires_at_ms = 1 WHERE token_id = ?1",
                params!["token-id"],
            )
            .expect("expire token");

        assert!(
            store
                .load_profile_token_auth("token-id")
                .expect("auth")
                .is_none()
        );

        let replacement = store
            .create_profile_token(profile.id, "token-id-2", "deploy", hash, None)
            .expect("replacement token");
        assert_eq!(replacement.name, "deploy");
        let expired =
            select_profile_token(&store.connection, profile.id, "token-id").expect("expired token");
        assert!(expired.revoked_at_ms.is_some());
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
