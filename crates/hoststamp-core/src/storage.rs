// SPDX-License-Identifier: FSL-1.1-ALv2

use crate::profile::{ProfileAccess, ProfileConfig, ProfileSlug};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

pub const DATABASE_ENV: &str = "HOSTSTAMP_DATABASE_URL";
pub const DEFAULT_DATABASE_FILE: &str = "hoststamp.db";
pub const EVENT_RETENTION_LIMIT: i64 = 10_000;

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
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub replaced_at_ms: Option<i64>,
    pub replaced_by_id: Option<Uuid>,
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

#[derive(Debug, Clone, PartialEq)]
pub struct StoredEvent {
    pub id: Uuid,
    pub created_at_ms: i64,
    pub source: String,
    pub action: String,
    pub profile_slug: Option<ProfileSlug>,
    pub profile_id: Option<Uuid>,
    pub token_id: Option<String>,
    pub token_name: Option<String>,
    pub atomic_start: Option<i64>,
    pub atomic_end: Option<i64>,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct BackupSnapshot {
    pub profiles: Vec<StoredProfile>,
    pub profile_tokens: Vec<StoredProfileToken>,
    pub events: Vec<StoredEvent>,
}

#[derive(Debug, Clone)]
pub struct NewEvent {
    pub source: &'static str,
    pub action: &'static str,
    pub profile_slug: Option<ProfileSlug>,
    pub profile_id: Option<Uuid>,
    pub token_id: Option<String>,
    pub token_name: Option<String>,
    pub atomic_start: Option<i64>,
    pub atomic_end: Option<i64>,
    pub metadata: Value,
}

impl NewEvent {
    pub fn new(source: &'static str, action: &'static str) -> Self {
        Self {
            source,
            action,
            profile_slug: None,
            profile_id: None,
            token_id: None,
            token_name: None,
            atomic_start: None,
            atomic_end: None,
            metadata: Value::Object(serde_json::Map::new()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EventFilter {
    pub profile_slug: Option<ProfileSlug>,
    pub action: Option<String>,
    pub source: Option<String>,
    pub token_name: Option<String>,
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
    pub limit: usize,
}

impl Default for EventFilter {
    fn default() -> Self {
        Self {
            profile_slug: None,
            action: None,
            source: None,
            token_name: None,
            since_ms: None,
            until_ms: None,
            limit: 50,
        }
    }
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
            "SELECT id, slug, access, config_json, config_hash, last_atomic_value,
                    created_at_ms, updated_at_ms, replaced_at_ms, replaced_by_id
             FROM hoststamp_profiles
             WHERE replaced_at_ms IS NULL
             ORDER BY slug ASC",
        )?;
        let profiles = statement
            .query_map([], profile_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(profiles)
    }

    pub fn list_profile_history(&self, slug: &ProfileSlug) -> Result<Vec<StoredProfile>> {
        let mut statement = self.connection.prepare(
            "SELECT id, slug, access, config_json, config_hash, last_atomic_value,
                    created_at_ms, updated_at_ms, replaced_at_ms, replaced_by_id
             FROM hoststamp_profiles
             WHERE slug = ?1
             ORDER BY created_at_ms ASC, updated_at_ms ASC, id ASC",
        )?;
        let profiles = statement
            .query_map(params![slug.as_str()], profile_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(profiles)
    }

    pub fn load_profile_by_id(&self, id: Uuid) -> Result<StoredProfile> {
        select_profile_by_id(&self.connection, id)
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

    pub fn record_event(&mut self, event: NewEvent) -> Result<StoredEvent> {
        validate_event_text("event source", event.source)?;
        validate_event_text("event action", event.action)?;
        validate_atomic_range(event.atomic_start, event.atomic_end)?;

        let id = Uuid::now_v7();
        let now = unix_epoch_millis()?;
        let metadata_json = serde_json::to_string(&event.metadata)?;
        self.connection.execute(
            "INSERT INTO hoststamp_events (
                id, created_at_ms, source, action, profile_slug, profile_id,
                token_id, token_name, atomic_start, atomic_end, metadata_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id.as_bytes().as_slice(),
                now,
                event.source,
                event.action,
                event.profile_slug.as_ref().map(ProfileSlug::as_str),
                event
                    .profile_id
                    .map(|profile_id| profile_id.as_bytes().to_vec()),
                event.token_id,
                event.token_name,
                event.atomic_start,
                event.atomic_end,
                metadata_json,
            ],
        )?;
        prune_events(&self.connection, EVENT_RETENTION_LIMIT)?;
        select_event(&self.connection, id)
    }

    pub fn list_events(&self, filter: &EventFilter) -> Result<Vec<StoredEvent>> {
        if filter.limit == 0 || filter.limit > 500 {
            bail!("event limit must be between 1 and 500");
        }
        if let (Some(since_ms), Some(until_ms)) = (filter.since_ms, filter.until_ms)
            && since_ms > until_ms
        {
            bail!("event since_ms must be less than or equal to until_ms");
        }

        let limit = i64::try_from(filter.limit).context("event limit does not fit in i64")?;
        let mut statement = self.connection.prepare(
            "SELECT id, created_at_ms, source, action, profile_slug, profile_id,
                    token_id, token_name, atomic_start, atomic_end, metadata_json
             FROM hoststamp_events
             WHERE (?1 IS NULL OR profile_slug = ?1)
               AND (?2 IS NULL OR action = ?2)
               AND (?3 IS NULL OR source = ?3)
               AND (?4 IS NULL OR token_name = ?4)
               AND (?5 IS NULL OR created_at_ms >= ?5)
               AND (?6 IS NULL OR created_at_ms <= ?6)
             ORDER BY created_at_ms DESC, id DESC
             LIMIT ?7",
        )?;
        let events = statement
            .query_map(
                params![
                    filter.profile_slug.as_ref().map(ProfileSlug::as_str),
                    filter.action.as_deref(),
                    filter.source.as_deref(),
                    filter.token_name.as_deref(),
                    filter.since_ms,
                    filter.until_ms,
                    limit,
                ],
                event_from_row,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(events)
    }

    pub fn backup_snapshot(&mut self) -> Result<BackupSnapshot> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)?;

        let profiles = {
            let mut statement = tx.prepare(
                "SELECT id, slug, access, config_json, config_hash, last_atomic_value,
                        created_at_ms, updated_at_ms, replaced_at_ms, replaced_by_id
                 FROM hoststamp_profiles
                 ORDER BY slug ASC, created_at_ms ASC, updated_at_ms ASC, id ASC",
            )?;
            statement
                .query_map([], profile_from_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };

        let profile_tokens = {
            let mut statement = tx.prepare(
                "SELECT token_id, profile_id, name, created_at_ms, expires_at_ms, last_used_at_ms, revoked_at_ms
                 FROM hoststamp_profile_tokens
                 ORDER BY created_at_ms ASC, token_id ASC",
            )?;
            statement
                .query_map([], profile_token_from_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };

        let events = {
            let mut statement = tx.prepare(
                "SELECT id, created_at_ms, source, action, profile_slug, profile_id,
                        token_id, token_name, atomic_start, atomic_end, metadata_json
                 FROM hoststamp_events
                 ORDER BY created_at_ms ASC, id ASC",
            )?;
            statement
                .query_map([], event_from_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };

        tx.commit()?;
        Ok(BackupSnapshot {
            profiles,
            profile_tokens,
            events,
        })
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

        CREATE TABLE IF NOT EXISTS hoststamp_events (
            id BLOB PRIMARY KEY NOT NULL CHECK(length(id) = 16),
            created_at_ms INTEGER NOT NULL,
            source TEXT NOT NULL,
            action TEXT NOT NULL,
            profile_slug TEXT,
            profile_id BLOB CHECK(profile_id IS NULL OR length(profile_id) = 16),
            token_id TEXT,
            token_name TEXT,
            atomic_start INTEGER,
            atomic_end INTEGER,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            CHECK((atomic_start IS NULL AND atomic_end IS NULL)
                OR (atomic_start IS NOT NULL AND atomic_end IS NOT NULL AND atomic_start <= atomic_end))
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

        CREATE INDEX IF NOT EXISTS idx_hoststamp_events_created_at_ms
            ON hoststamp_events(created_at_ms);

        CREATE INDEX IF NOT EXISTS idx_hoststamp_events_profile_slug
            ON hoststamp_events(profile_slug);

        CREATE INDEX IF NOT EXISTS idx_hoststamp_events_action
            ON hoststamp_events(action);

        CREATE INDEX IF NOT EXISTS idx_hoststamp_events_source
            ON hoststamp_events(source);

        CREATE INDEX IF NOT EXISTS idx_hoststamp_events_token_name
            ON hoststamp_events(token_name);
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

fn validate_event_text(label: &str, value: &str) -> Result<()> {
    if value.trim() != value || value.is_empty() {
        bail!("{label} must not be empty or contain surrounding whitespace");
    }
    if value.len() > 128 {
        bail!("{label} must be 128 characters or fewer");
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
    }) {
        bail!("{label} must use lowercase ASCII letters, digits, dot, hyphen, or underscore");
    }
    Ok(())
}

fn validate_atomic_range(atomic_start: Option<i64>, atomic_end: Option<i64>) -> Result<()> {
    match (atomic_start, atomic_end) {
        (None, None) => Ok(()),
        (Some(start), Some(end)) if start <= end => Ok(()),
        (Some(_), Some(_)) => bail!("event atomic_start must be less than or equal to atomic_end"),
        _ => bail!("event atomic range requires both atomic_start and atomic_end"),
    }
}

fn prune_events(connection: &Connection, limit: i64) -> Result<()> {
    if limit <= 0 {
        bail!("event retention limit must be positive");
    }
    connection.execute(
        "DELETE FROM hoststamp_events
         WHERE id IN (
             SELECT id
             FROM hoststamp_events
             ORDER BY created_at_ms DESC, id DESC
             LIMIT -1 OFFSET ?1
         )",
        params![limit],
    )?;
    Ok(())
}

fn select_profile(connection: &Connection, slug: &ProfileSlug) -> Result<StoredProfile> {
    connection
        .query_row(
            "SELECT id, slug, access, config_json, config_hash, last_atomic_value,
                    created_at_ms, updated_at_ms, replaced_at_ms, replaced_by_id
             FROM hoststamp_profiles
             WHERE slug = ?1 AND replaced_at_ms IS NULL",
            params![slug.as_str()],
            profile_from_row,
        )
        .optional()?
        .with_context(|| format!("profile {:?} does not exist", slug.as_str()))
}

fn select_profile_by_id(connection: &Connection, id: Uuid) -> Result<StoredProfile> {
    connection
        .query_row(
            "SELECT id, slug, access, config_json, config_hash, last_atomic_value,
                    created_at_ms, updated_at_ms, replaced_at_ms, replaced_by_id
             FROM hoststamp_profiles
             WHERE id = ?1",
            params![id.as_bytes().as_slice()],
            profile_from_row,
        )
        .optional()?
        .with_context(|| format!("profile id {id} does not exist"))
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
    stored_profile_from_parts(StoredProfileParts {
        id_blob: row.get(0)?,
        slug_value: row.get(1)?,
        access_value: row.get(2)?,
        config_json: row.get(3)?,
        config_hash_blob: row.get(4)?,
        last_atomic_value: row.get(5)?,
        created_at_ms: row.get(6)?,
        updated_at_ms: row.get(7)?,
        replaced_at_ms: row.get(8)?,
        replaced_by_id_blob: row.get(9)?,
    })
    .map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, error.into())
    })
}

struct StoredProfileParts {
    id_blob: Vec<u8>,
    slug_value: String,
    access_value: String,
    config_json: String,
    config_hash_blob: Vec<u8>,
    last_atomic_value: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    replaced_at_ms: Option<i64>,
    replaced_by_id_blob: Option<Vec<u8>>,
}

fn stored_profile_from_parts(parts: StoredProfileParts) -> Result<StoredProfile> {
    let id = Uuid::from_slice(&parts.id_blob).context("stored profile id is not a UUID")?;
    let slug = parts
        .slug_value
        .parse::<ProfileSlug>()
        .map_err(anyhow::Error::msg)
        .context("stored profile slug is invalid")?;
    let access = parts
        .access_value
        .parse::<ProfileAccess>()
        .map_err(anyhow::Error::msg)
        .context("stored profile access is invalid")?;
    let config = serde_json::from_str::<ProfileConfig>(&parts.config_json)
        .context("stored profile config is invalid")?;
    let config_hash = fixed_hash(parts.config_hash_blob)?;
    let replaced_by_id = parts
        .replaced_by_id_blob
        .map(|blob| Uuid::from_slice(&blob).context("stored replacement profile id is not a UUID"))
        .transpose()?;
    Ok(StoredProfile {
        id,
        slug,
        access,
        config,
        config_hash,
        last_atomic_value: parts.last_atomic_value,
        created_at_ms: parts.created_at_ms,
        updated_at_ms: parts.updated_at_ms,
        replaced_at_ms: parts.replaced_at_ms,
        replaced_by_id,
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

fn select_event(connection: &Connection, id: Uuid) -> Result<StoredEvent> {
    connection
        .query_row(
            "SELECT id, created_at_ms, source, action, profile_slug, profile_id,
                    token_id, token_name, atomic_start, atomic_end, metadata_json
             FROM hoststamp_events
             WHERE id = ?1",
            params![id.as_bytes().as_slice()],
            event_from_row,
        )
        .optional()?
        .with_context(|| format!("event id {id} does not exist"))
}

fn event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredEvent> {
    stored_event_from_parts(StoredEventParts {
        id_blob: row.get(0)?,
        created_at_ms: row.get(1)?,
        source: row.get(2)?,
        action: row.get(3)?,
        profile_slug_value: row.get(4)?,
        profile_id_blob: row.get(5)?,
        token_id: row.get(6)?,
        token_name: row.get(7)?,
        atomic_start: row.get(8)?,
        atomic_end: row.get(9)?,
        metadata_json: row.get(10)?,
    })
    .map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, error.into())
    })
}

struct StoredEventParts {
    id_blob: Vec<u8>,
    created_at_ms: i64,
    source: String,
    action: String,
    profile_slug_value: Option<String>,
    profile_id_blob: Option<Vec<u8>>,
    token_id: Option<String>,
    token_name: Option<String>,
    atomic_start: Option<i64>,
    atomic_end: Option<i64>,
    metadata_json: String,
}

fn stored_event_from_parts(parts: StoredEventParts) -> Result<StoredEvent> {
    let id = Uuid::from_slice(&parts.id_blob).context("stored event id is not a UUID")?;
    let profile_slug = parts
        .profile_slug_value
        .map(|value| {
            value
                .parse::<ProfileSlug>()
                .map_err(anyhow::Error::msg)
                .context("stored event profile slug is invalid")
        })
        .transpose()?;
    let profile_id = parts
        .profile_id_blob
        .map(|blob| Uuid::from_slice(&blob).context("stored event profile id is not a UUID"))
        .transpose()?;
    let metadata = serde_json::from_str(&parts.metadata_json)
        .context("stored event metadata_json is invalid")?;
    Ok(StoredEvent {
        id,
        created_at_ms: parts.created_at_ms,
        source: parts.source,
        action: parts.action,
        profile_slug,
        profile_id,
        token_id: parts.token_id,
        token_name: parts.token_name,
        atomic_start: parts.atomic_start,
        atomic_end: parts.atomic_end,
        metadata,
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

        let history = store.list_profile_history(&slug).expect("history");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].id, original.id);
        assert_eq!(history[0].last_atomic_value, 1);
        assert!(history[0].replaced_at_ms.is_some());
        assert_eq!(history[0].replaced_by_id, Some(replacement.id));
        assert_eq!(history[1].id, replacement.id);
        assert!(history[1].replaced_at_ms.is_none());
        assert_eq!(history[1].replaced_by_id, None);

        let retired = store.load_profile_by_id(original.id).expect("retired");
        assert_eq!(retired.slug, slug);
        assert_eq!(retired.config, seed);
        assert_eq!(retired.last_atomic_value, 1);
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
        let deleted_history = store.list_profile_history(&slug).expect("history");
        assert_eq!(deleted_history.len(), 1);
        assert!(deleted_history[0].replaced_at_ms.is_some());
        assert_eq!(deleted_history[0].replaced_by_id, None);

        let recreated = store.create_profile(&slug, &seed).expect("recreated");
        assert_ne!(recreated.id, created.id);
        assert_eq!(recreated.last_atomic_value, 0);
        let recreated_history = store.list_profile_history(&slug).expect("history");
        assert_eq!(recreated_history.len(), 2);
        assert_eq!(recreated_history[1].id, recreated.id);

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
    fn event_records_round_trip_with_filters() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let url = StorageUrl::Sqlite(tempdir.path().join(DEFAULT_DATABASE_FILE));
        let mut store = ProfileStore::open(&url).expect("store");
        let slug = "team-a".parse::<ProfileSlug>().expect("slug");
        let profile = store
            .create_profile(&slug, &ProfileConfig::default())
            .expect("profile");

        let mut created = NewEvent::new("cli", "profile.create");
        created.profile_slug = Some(slug.clone());
        created.profile_id = Some(profile.id);
        created.metadata = serde_json::json!({ "access": "private" });
        let created = store.record_event(created).expect("created event");

        let mut token = NewEvent::new("api", "profile.token.create");
        token.profile_slug = Some(slug.clone());
        token.profile_id = Some(profile.id);
        token.token_id = Some("token-id".to_owned());
        token.token_name = Some("deploy".to_owned());
        token.metadata = serde_json::json!({ "expires_at_ms": null });
        let token = store.record_event(token).expect("token event");

        let mut generated = NewEvent::new("api", "generate");
        generated.profile_slug = Some(slug.clone());
        generated.profile_id = Some(profile.id);
        generated.atomic_start = Some(1);
        generated.atomic_end = Some(3);
        generated.metadata = serde_json::json!({ "count": 3 });
        let generated = store.record_event(generated).expect("generate event");

        let events = store.list_events(&EventFilter::default()).expect("events");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].id, generated.id);
        assert_eq!(events[1].id, token.id);
        assert_eq!(events[2].id, created.id);

        let profile_events = store
            .list_events(&EventFilter {
                profile_slug: Some(slug.clone()),
                ..EventFilter::default()
            })
            .expect("profile events");
        assert_eq!(profile_events.len(), 3);

        let token_events = store
            .list_events(&EventFilter {
                action: Some("profile.token.create".to_owned()),
                source: Some("api".to_owned()),
                token_name: Some("deploy".to_owned()),
                ..EventFilter::default()
            })
            .expect("token events");
        assert_eq!(token_events.len(), 1);
        assert_eq!(token_events[0].id, token.id);
        assert_eq!(token_events[0].token_id.as_deref(), Some("token-id"));

        let recent_events = store
            .list_events(&EventFilter {
                since_ms: Some(generated.created_at_ms),
                limit: 2,
                ..EventFilter::default()
            })
            .expect("recent events");
        assert!(!recent_events.is_empty());
        assert!(recent_events.len() <= 2);
    }

    #[test]
    fn backup_snapshot_includes_retained_rows() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let url = StorageUrl::Sqlite(tempdir.path().join(DEFAULT_DATABASE_FILE));
        let mut store = ProfileStore::open(&url).expect("store");
        let slug = "team-a".parse::<ProfileSlug>().expect("slug");
        let original = store
            .create_profile(&slug, &ProfileConfig::default())
            .expect("profile");
        store
            .create_profile_token(original.id, "token-id", "deploy", [7_u8; 32], None)
            .expect("token");

        let replacement_config = ProfileConfig::from(&GenerateOptions {
            word1_lengths: Some(vec![4]),
            ..GenerateOptions::default()
        });
        let replacement = store
            .replace_profile_config(&slug, &replacement_config)
            .expect("replacement");

        let mut created = NewEvent::new("cli", "profile.create");
        created.profile_slug = Some(slug.clone());
        created.profile_id = Some(original.id);
        let created = store.record_event(created).expect("created event");
        let mut backup = NewEvent::new("cli", "backup.export");
        backup.metadata = serde_json::json!({ "profile_count": 2 });
        let backup = store.record_event(backup).expect("backup event");
        for (timestamp, id) in [(1_i64, created.id), (2_i64, backup.id)] {
            store
                .connection
                .execute(
                    "UPDATE hoststamp_events SET created_at_ms = ?1 WHERE id = ?2",
                    params![timestamp, id.as_bytes().as_slice()],
                )
                .expect("timestamp");
        }

        let active_profiles = store.list_profiles().expect("active profiles");
        assert_eq!(active_profiles.len(), 1);
        assert_eq!(active_profiles[0].id, replacement.id);

        let snapshot = store.backup_snapshot().expect("snapshot");
        assert_eq!(snapshot.profiles.len(), 2);
        assert_eq!(snapshot.profiles[0].id, original.id);
        assert_eq!(snapshot.profiles[1].id, replacement.id);

        assert_eq!(snapshot.profile_tokens.len(), 1);
        assert_eq!(snapshot.profile_tokens[0].token_id, "token-id");
        assert_eq!(snapshot.profile_tokens[0].profile_id, original.id);

        assert_eq!(
            snapshot
                .events
                .iter()
                .map(|event| event.id)
                .collect::<Vec<_>>(),
            vec![created.id, backup.id]
        );
    }

    #[test]
    fn event_validation_rejects_invalid_inputs() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let url = StorageUrl::Sqlite(tempdir.path().join(DEFAULT_DATABASE_FILE));
        let mut store = ProfileStore::open(&url).expect("store");

        let invalid_source = store
            .record_event(NewEvent::new("CLI", "profile.create"))
            .expect_err("uppercase source should fail");
        assert!(invalid_source.to_string().contains("event source"));

        let mut invalid_range = NewEvent::new("cli", "generate");
        invalid_range.atomic_start = Some(2);
        invalid_range.atomic_end = Some(1);
        let invalid_range = store
            .record_event(invalid_range)
            .expect_err("inverted range should fail");
        assert!(invalid_range.to_string().contains("atomic_start"));

        let invalid_limit = store
            .list_events(&EventFilter {
                limit: 0,
                ..EventFilter::default()
            })
            .expect_err("zero limit should fail");
        assert!(invalid_limit.to_string().contains("event limit"));
    }

    #[test]
    fn event_pruning_keeps_newest_rows() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let url = StorageUrl::Sqlite(tempdir.path().join(DEFAULT_DATABASE_FILE));
        let mut store = ProfileStore::open(&url).expect("store");

        let mut ids = Vec::new();
        for index in 0..5 {
            let mut event = NewEvent::new("cli", "generate");
            event.metadata = serde_json::json!({ "index": index });
            let id = store.record_event(event).expect("event").id;
            store
                .connection
                .execute(
                    "UPDATE hoststamp_events SET created_at_ms = ?1 WHERE id = ?2",
                    params![index, id.as_bytes().as_slice()],
                )
                .expect("timestamp");
            ids.push(id);
        }

        prune_events(&store.connection, 3).expect("prune");
        let events = store
            .list_events(&EventFilter {
                limit: 10,
                ..EventFilter::default()
            })
            .expect("events");

        assert_eq!(events.len(), 3);
        assert!(events.iter().any(|event| event.id == ids[4]));
        assert!(events.iter().any(|event| event.id == ids[3]));
        assert!(events.iter().any(|event| event.id == ids[2]));
        assert!(!events.iter().any(|event| event.id == ids[1]));
        assert!(!events.iter().any(|event| event.id == ids[0]));
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
