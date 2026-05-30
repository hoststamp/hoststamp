// SPDX-License-Identifier: FSL-1.1-ALv2

use anyhow::{Context, Result, bail};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use std::fmt;
use subtle::ConstantTimeEq;
use uuid::Uuid;

pub const ADMIN_TOKEN_ENV: &str = "HOSTSTAMP_ADMIN_TOKEN";
pub const API_AUTH_REQUIRED_ENV: &str = "HOSTSTAMP_API_AUTH_REQUIRED";
pub const PROFILE_TOKEN_HASH_KEY_ENV: &str = "HOSTSTAMP_TOKEN_HASH_KEY";
pub const PROFILE_TOKEN_PREFIX: &str = "hspt";

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: String) -> Result<Self> {
        if value.trim().is_empty() {
            bail!("secret value must not be empty");
        }
        Ok(Self(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

#[derive(Debug, Clone, Default)]
pub struct ApiAuthConfig {
    pub required: bool,
    pub admin_token: Option<SecretString>,
    pub token_hash_key: Option<SecretString>,
}

#[derive(Debug, Clone)]
pub struct GeneratedProfileToken {
    pub token_id: String,
    pub secret: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedProfileToken<'a> {
    pub token_id: &'a str,
    pub secret: &'a str,
}

pub fn generate_profile_token() -> GeneratedProfileToken {
    let token_id = Uuid::now_v7().simple().to_string();
    let secret = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let token = format!("{PROFILE_TOKEN_PREFIX}_{token_id}_{secret}");

    GeneratedProfileToken {
        token_id,
        secret,
        token,
    }
}

pub fn parse_profile_token(token: &str) -> Option<PresentedProfileToken<'_>> {
    let mut parts = token.splitn(3, '_');
    let prefix = parts.next()?;
    let token_id = parts.next()?;
    let secret = parts.next()?;
    if prefix != PROFILE_TOKEN_PREFIX || token_id.is_empty() || secret.is_empty() {
        return None;
    }
    if Uuid::parse_str(token_id).is_err() {
        return None;
    }
    Some(PresentedProfileToken { token_id, secret })
}

pub fn profile_token_hash(key: &SecretString, secret: &str) -> Result<[u8; 32]> {
    let mut mac = HmacSha256::new_from_slice(key.expose().as_bytes())
        .context("failed to initialize profile token HMAC")?;
    mac.update(secret.as_bytes());
    Ok(mac.finalize().into_bytes().into())
}

pub fn verify_profile_token_hash(
    key: &SecretString,
    secret: &str,
    expected_hash: &[u8],
) -> Result<bool> {
    let mut mac = HmacSha256::new_from_slice(key.expose().as_bytes())
        .context("failed to initialize profile token HMAC")?;
    mac.update(secret.as_bytes());
    Ok(mac.verify_slice(expected_hash).is_ok())
}

pub fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let max_len = left.len().max(right.len());
    let mut padded_left = vec![0; max_len];
    let mut padded_right = vec![0; max_len];
    padded_left[..left.len()].copy_from_slice(left);
    padded_right[..right.len()].copy_from_slice(right);

    padded_left.ct_eq(&padded_right).into() && left.len() == right.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_profile_tokens_round_trip() {
        let generated = generate_profile_token();
        let parsed = parse_profile_token(&generated.token).expect("parsed");

        assert_eq!(parsed.token_id, generated.token_id);
        assert_eq!(parsed.secret, generated.secret);
    }

    #[test]
    fn hashes_and_verifies_profile_token_secret() {
        let key = SecretString::new("secret-key".to_owned()).expect("key");
        let hash = profile_token_hash(&key, "token-secret").expect("hash");

        assert!(verify_profile_token_hash(&key, "token-secret", &hash).expect("verify"));
        assert!(!verify_profile_token_hash(&key, "other", &hash).expect("verify"));
    }

    #[test]
    fn constant_time_eq_checks_value_and_length() {
        assert!(constant_time_eq("admin-secret", "admin-secret"));
        assert!(!constant_time_eq("admin-secret", "admin-other"));
        assert!(!constant_time_eq("admin-secret", "admin-secret-extra"));
    }

    #[test]
    fn rejects_invalid_profile_token_shape() {
        for value in ["", "token", "hspt_missing", "wrong_id_secret"] {
            assert!(parse_profile_token(value).is_none(), "{value}");
        }
    }
}
