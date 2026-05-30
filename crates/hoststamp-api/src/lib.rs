// SPDX-License-Identifier: FSL-1.1-ALv2

use anyhow::anyhow;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
};
use hoststamp_core::{
    SERVICE_NAME,
    auth::{
        ApiAuthConfig, constant_time_eq, generate_profile_token, parse_profile_token,
        profile_token_hash, verify_profile_token_hash,
    },
    generator::{self, GenerateOptions, GenerateOverrides, ProfileGeneratedHostname},
    profile::{ProfileAccess, ProfileConfig, ProfileSlug},
    storage::{ProfileStore, StoredProfile, StoredProfileToken, config_hash},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fmt, net::SocketAddr, str::FromStr, sync::Arc};
use tokio::{net::TcpListener, signal, sync::Mutex};
use uuid::Uuid;

const PROFILE_EXPORT_FORMAT: &str = "hoststamp-profile-v1";
pub const MAX_REQUEST_BODY_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    All,
    Api,
    Ux,
}

#[derive(Clone)]
pub struct AppState {
    generate: GenerateOptions,
    atomic: Option<AtomicContext>,
    auth: ApiAuthConfig,
}

#[derive(Clone)]
pub struct AtomicContext {
    pub store: Arc<Mutex<ProfileStore>>,
    pub profile_slug: ProfileSlug,
}

impl AtomicContext {
    pub fn new(store: ProfileStore, profile_slug: ProfileSlug) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
            profile_slug,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Health {
    pub status: &'static str,
    pub service: &'static str,
}

#[derive(Debug, Serialize)]
pub struct GenerateResponse {
    pub hostnames: Vec<GeneratedHostname>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeneratedHostname {
    pub hostname: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atomic_value: Option<i64>,
}

impl GeneratedHostname {
    pub fn plain(hostname: String) -> Self {
        Self {
            hostname,
            profile: None,
            atomic_value: None,
        }
    }

    pub fn profile_backed(profile: &ProfileSlug, generated: ProfileGeneratedHostname) -> Self {
        Self {
            hostname: generated.hostname,
            profile: Some(profile.as_str().to_owned()),
            atomic_value: Some(generated.atomic_value),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerateQuery {
    pub format: Option<GenerateFormat>,
    pub profile: Option<String>,
    pub count: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityQuery {
    pub profile: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RandomQuery {
    pub format: Option<GenerateFormat>,
    pub word1_enabled: Option<bool>,
    pub word1_lengths: Option<String>,
    pub word1_categories: Option<String>,
    pub word2_enabled: Option<bool>,
    pub word2_lengths: Option<String>,
    pub word2_categories: Option<String>,
    pub suffix_enabled: Option<bool>,
    pub suffix_min_length: Option<usize>,
    pub count: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegenerateQuery {
    pub format: Option<GenerateFormat>,
    pub profile: Option<String>,
    pub atomic_value: i64,
    pub count: Option<usize>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GenerateFormat {
    Plain,
    Json,
}

#[derive(Debug, Serialize)]
pub struct ProfilesResponse {
    pub profiles: Vec<ProfileResponse>,
}

#[derive(Debug, Serialize)]
pub struct ProfileEnvelope {
    pub profile: ProfileResponse,
}

#[derive(Debug, Serialize)]
pub struct ProfileResponse {
    pub id: String,
    pub slug: String,
    pub access: ProfileAccess,
    pub last_atomic_value: i64,
    pub config_hash: String,
    pub config: ProfileConfig,
}

impl From<StoredProfile> for ProfileResponse {
    fn from(profile: StoredProfile) -> Self {
        Self {
            id: profile.id.to_string(),
            slug: profile.slug.as_str().to_owned(),
            access: profile.access,
            last_atomic_value: profile.last_atomic_value,
            config_hash: hex_string(&profile.config_hash),
            config: profile.config,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ProfileTokensResponse {
    pub tokens: Vec<ProfileTokenResponse>,
}

#[derive(Debug, Serialize)]
pub struct CreatedProfileTokenResponse {
    pub token: ProfileTokenResponse,
    pub profile_token: String,
}

#[derive(Debug, Serialize)]
pub struct ProfileTokenEnvelope {
    pub token: ProfileTokenResponse,
}

#[derive(Debug, Serialize)]
pub struct ProfileTokenResponse {
    pub token_id: String,
    pub profile_id: String,
    pub name: String,
    pub created_at_ms: i64,
    pub expires_at_ms: Option<i64>,
    pub last_used_at_ms: Option<i64>,
    pub revoked_at_ms: Option<i64>,
}

impl From<StoredProfileToken> for ProfileTokenResponse {
    fn from(token: StoredProfileToken) -> Self {
        Self {
            token_id: token.token_id,
            profile_id: token.profile_id.to_string(),
            name: token.name,
            created_at_ms: token.created_at_ms,
            expires_at_ms: token.expires_at_ms,
            last_used_at_ms: token.last_used_at_ms,
            revoked_at_ms: token.revoked_at_ms,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ProfileExport {
    pub format: &'static str,
    pub id: String,
    pub slug: String,
    pub access: ProfileAccess,
    pub last_atomic_value: i64,
    pub config_hash: String,
    pub config: ProfileConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportProfileRequest {
    pub format: String,
    pub id: String,
    pub slug: String,
    pub access: ProfileAccess,
    pub last_atomic_value: i64,
    pub config_hash: String,
    pub config: ProfileConfig,
    pub confirmation: Option<AdminConfirmation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateProfileRequest {
    pub slug: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateProfileAccessRequest {
    pub access: ProfileAccess,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateProfileTokenRequest {
    pub name: String,
    pub expires_at_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteProfileRequest {
    pub confirmation: AdminConfirmation,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetAtomicValueRequest {
    pub atomic_value: i64,
    pub confirmation: AdminConfirmation,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateProfileConfigRequest {
    pub word1_enabled: Option<bool>,
    pub word1_lengths: Option<Value>,
    pub word1_categories: Option<Vec<String>>,
    pub word2_enabled: Option<bool>,
    pub word2_lengths: Option<Value>,
    pub word2_categories: Option<Vec<String>>,
    pub suffix_enabled: Option<bool>,
    pub suffix_min_length: Option<usize>,
    pub confirmation: Option<AdminConfirmation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminConfirmation {
    pub profile: String,
    pub action: String,
}

pub fn app(generate_options: GenerateOptions) -> Router {
    app_with_atomic(generate_options, None)
}

pub fn app_with_atomic(generate_options: GenerateOptions, atomic: Option<AtomicContext>) -> Router {
    app_with_auth(generate_options, atomic, ApiAuthConfig::default())
}

pub fn app_with_auth(
    generate_options: GenerateOptions,
    atomic: Option<AtomicContext>,
    auth: ApiAuthConfig,
) -> Router {
    app_with_mode(generate_options, atomic, auth, AppMode::All)
}

pub fn app_with_mode(
    generate_options: GenerateOptions,
    atomic: Option<AtomicContext>,
    auth: ApiAuthConfig,
    mode: AppMode,
) -> Router {
    let mut router = Router::new().route("/healthz", get(healthz));
    if matches!(mode, AppMode::All | AppMode::Ux) {
        router = router
            .route("/", get(hoststamp_ux::index))
            .route("/assets/app.css", get(hoststamp_ux::stylesheet))
            .route("/assets/app.js", get(hoststamp_ux::script));
    }
    if matches!(mode, AppMode::All | AppMode::Api) {
        router = router
            .route("/api/health", get(healthz))
            .route(
                "/api/generate",
                post(generate_one).get(generate_method_not_allowed),
            )
            .route("/api/capacity", get(capacity_one))
            .route("/api/regenerate", get(regenerate_one))
            .route("/api/random", get(random_one))
            .route(
                "/api/profiles",
                get(admin_list_profiles).post(admin_create_profile),
            )
            .route(
                "/api/profiles/{slug}",
                get(admin_show_profile).delete(admin_delete_profile),
            )
            .route("/api/profiles/{slug}/export", get(admin_export_profile))
            .route("/api/profiles/import", post(admin_import_profile))
            .route(
                "/api/profiles/{slug}/config",
                patch(admin_update_profile_config),
            )
            .route(
                "/api/profiles/{slug}/access",
                patch(admin_update_profile_access),
            )
            .route(
                "/api/profiles/{slug}/tokens",
                get(admin_list_profile_tokens).post(admin_create_profile_token),
            )
            .route(
                "/api/profiles/{slug}/tokens/{token_id}",
                delete(admin_revoke_profile_token),
            )
            .route(
                "/api/profiles/{slug}/reset-atomic-value",
                post(admin_reset_atomic_value),
            );
    }

    router
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(AppState {
            generate: generate_options,
            atomic,
            auth,
        })
}

pub fn health_payload() -> Health {
    Health {
        status: "ok",
        service: SERVICE_NAME,
    }
}

async fn healthz() -> Json<Health> {
    Json(health_payload())
}

async fn generate_method_not_allowed() -> Response {
    let mut response = (
        StatusCode::METHOD_NOT_ALLOWED,
        "use POST /api/generate for profile-backed generation",
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static("POST"));
    response
}

async fn generate_one(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<GenerateQuery>,
) -> Result<Response, Response> {
    let overrides = GenerateOverrides {
        count: query.count,
        ..GenerateOverrides::default()
    };
    let profile = query
        .profile
        .as_deref()
        .map(ProfileSlug::from_str)
        .transpose()
        .map_err(bad_request_response)?;
    let hostnames = generate_with_state(overrides, profile.as_ref(), &state, &headers)
        .await
        .map_err(generate_error_response)?;

    Ok(generate_response(
        query.format.unwrap_or(GenerateFormat::Plain),
        hostnames,
    ))
}

async fn capacity_one(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CapacityQuery>,
) -> Result<Json<generator::CapacityReport>, Response> {
    let profile = query
        .profile
        .as_deref()
        .map(ProfileSlug::from_str)
        .transpose()
        .map_err(bad_request_response)?;
    let options = capacity_options(profile.as_ref(), &state, &headers)
        .await
        .map_err(generate_error_response)?;
    let report = generator::capacity_report(&options)
        .map_err(|error| bad_request_response(error.to_string()))?;
    Ok(Json(report))
}

async fn random_one(Query(query): Query<RandomQuery>) -> Result<Response, Response> {
    let overrides = random_overrides(&query).map_err(bad_request_response)?;
    let options = GenerateOptions::default().with_overrides(overrides);
    let hostnames = generator::generate_many(options)
        .map(|hostnames| {
            hostnames
                .into_iter()
                .map(GeneratedHostname::plain)
                .collect::<Vec<_>>()
        })
        .map_err(|error| GenerateError::BadRequest(error.to_string()))
        .map_err(generate_error_response)?;

    Ok(generate_response(
        query.format.unwrap_or(GenerateFormat::Plain),
        hostnames,
    ))
}

async fn regenerate_one(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<RegenerateQuery>,
) -> Result<Response, Response> {
    if query.atomic_value < generator::ATOMIC_MIN_VALUE {
        return Err(bad_request_response(format!(
            "atomic value must be at least {}",
            generator::ATOMIC_MIN_VALUE
        )));
    }

    let hostnames = regenerate_with_state(&query, &state, &headers)
        .await
        .map_err(generate_error_response)?;

    Ok(generate_response(
        query.format.unwrap_or(GenerateFormat::Plain),
        hostnames,
    ))
}

async fn admin_list_profiles(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ProfilesResponse>, Response> {
    authorize_admin_request(&headers, &state.auth)?;
    let store = admin_store(&state)?;
    let store = store.lock().await;
    let profiles = store
        .list_profiles()
        .map_err(admin_internal_response)?
        .into_iter()
        .map(Into::into)
        .collect();

    Ok(Json(ProfilesResponse { profiles }))
}

async fn admin_show_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<ProfileEnvelope>, Response> {
    let slug = parse_slug(&slug)?;
    authorize_admin_request(&headers, &state.auth)?;
    let store = admin_store(&state)?;
    let store = store.lock().await;
    let profile = store
        .load_profile(&slug)
        .map_err(admin_bad_request_response)?;

    Ok(Json(ProfileEnvelope {
        profile: profile.into(),
    }))
}

async fn admin_create_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateProfileRequest>,
) -> Result<(StatusCode, Json<ProfileEnvelope>), Response> {
    let slug = parse_slug(&request.slug)?;
    authorize_admin_request(&headers, &state.auth)?;
    let store = admin_store(&state)?;
    let mut store = store.lock().await;
    let profile = store
        .create_profile(&slug, &ProfileConfig::default())
        .map_err(admin_bad_request_response)?;

    Ok((
        StatusCode::CREATED,
        Json(ProfileEnvelope {
            profile: profile.into(),
        }),
    ))
}

async fn admin_export_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<ProfileExport>, Response> {
    let slug = parse_slug(&slug)?;
    authorize_admin_request(&headers, &state.auth)?;
    let store = admin_store(&state)?;
    let store = store.lock().await;
    let profile = store
        .load_profile(&slug)
        .map_err(admin_bad_request_response)?;

    Ok(Json(ProfileExport {
        format: PROFILE_EXPORT_FORMAT,
        id: profile.id.to_string(),
        slug: profile.slug.as_str().to_owned(),
        access: profile.access,
        last_atomic_value: profile.last_atomic_value,
        config_hash: hex_string(&profile.config_hash),
        config: profile.config,
    }))
}

async fn admin_import_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ImportProfileRequest>,
) -> Result<(StatusCode, Json<ProfileEnvelope>), Response> {
    authorize_admin_request(&headers, &state.auth)?;
    if request.format != PROFILE_EXPORT_FORMAT {
        return Err(admin_bad_request_response(format!(
            "profile import format must be {PROFILE_EXPORT_FORMAT:?}"
        )));
    }
    let id = Uuid::parse_str(&request.id).map_err(|error| {
        admin_bad_request_response(format!("profile import id is invalid: {error}"))
    })?;
    let slug = parse_slug(&request.slug)?;
    if request.last_atomic_value < 0 {
        return Err(admin_bad_request_response(
            "profile import last_atomic_value must be at least 0",
        ));
    }
    let options = request.config.to_generate_options(generator::DEFAULT_COUNT);
    generator::validate_generate_options(&options).map_err(admin_bad_request_response)?;
    let expected_config_hash = hex_string(
        &config_hash(&request.config)
            .map_err(|error| admin_bad_request_response(error.to_string()))?,
    );
    if request.config_hash != expected_config_hash {
        return Err(admin_bad_request_response(
            "profile import config_hash does not match config",
        ));
    }
    if !request.config.uses_current_generation_contract() {
        return Err(admin_bad_request_response(
            "profile import config was created with a generation engine, dictionary/blocklist versions, or resolved word pools that do not match this binary",
        ));
    }

    let store = admin_store(&state)?;
    let mut store = store.lock().await;
    let existing = store.load_profile(&slug).ok();
    if existing.as_ref().is_some_and(|profile| {
        profile.id != id
            || profile.access != request.access
            || profile.config != request.config
            || profile.last_atomic_value != request.last_atomic_value
    }) {
        let confirmation = request.confirmation.as_ref().ok_or_else(|| {
            AdminError::BadRequest("profile import replacement requires confirmation".to_owned())
        })?;
        confirm_admin_action(confirmation, &slug, "replace")?;
    }
    let status = if existing.is_some() {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    let profile = store
        .import_profile(
            &slug,
            id,
            request.access,
            &request.config,
            request.last_atomic_value,
        )
        .map_err(admin_bad_request_response)?;

    Ok((
        status,
        Json(ProfileEnvelope {
            profile: profile.into(),
        }),
    ))
}

async fn admin_delete_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(request): Json<DeleteProfileRequest>,
) -> Result<StatusCode, Response> {
    let slug = parse_slug(&slug)?;
    authorize_admin_request(&headers, &state.auth)?;
    confirm_admin_action(&request.confirmation, &slug, "delete")?;
    let store = admin_store(&state)?;
    let mut store = store.lock().await;
    store
        .delete_profile(&slug)
        .map_err(admin_bad_request_response)?;

    Ok(StatusCode::NO_CONTENT)
}

async fn admin_update_profile_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(request): Json<UpdateProfileConfigRequest>,
) -> Result<Json<ProfileEnvelope>, Response> {
    let slug = parse_slug(&slug)?;
    authorize_admin_request(&headers, &state.auth)?;
    let store = admin_store(&state)?;
    let mut store = store.lock().await;
    let profile = store
        .load_profile(&slug)
        .map_err(admin_bad_request_response)?;
    let desired_config = request.apply(profile.config.clone())?;
    let options = desired_config.to_generate_options(generator::DEFAULT_COUNT);
    generator::validate_generate_options(&options).map_err(admin_bad_request_response)?;

    if desired_config == profile.config {
        return Ok(Json(ProfileEnvelope {
            profile: profile.into(),
        }));
    }

    let confirmation = request.confirmation.as_ref().ok_or_else(|| {
        AdminError::BadRequest("profile config replacement requires confirmation".to_owned())
    })?;
    confirm_admin_action(confirmation, &slug, "replace")?;
    let profile = store
        .replace_profile_config(&slug, &desired_config)
        .map_err(admin_bad_request_response)?;

    Ok(Json(ProfileEnvelope {
        profile: profile.into(),
    }))
}

async fn admin_update_profile_access(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(request): Json<UpdateProfileAccessRequest>,
) -> Result<Json<ProfileEnvelope>, Response> {
    let slug = parse_slug(&slug)?;
    authorize_admin_request(&headers, &state.auth)?;
    let store = admin_store(&state)?;
    let mut store = store.lock().await;
    let profile = store
        .set_profile_access(&slug, request.access)
        .map_err(admin_bad_request_response)?;

    Ok(Json(ProfileEnvelope {
        profile: profile.into(),
    }))
}

async fn admin_list_profile_tokens(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<ProfileTokensResponse>, Response> {
    let slug = parse_slug(&slug)?;
    authorize_admin_request(&headers, &state.auth)?;
    let store = admin_store(&state)?;
    let store = store.lock().await;
    let profile = store
        .load_profile(&slug)
        .map_err(admin_bad_request_response)?;
    let tokens = store
        .list_profile_tokens(profile.id)
        .map_err(admin_internal_response)?
        .into_iter()
        .map(Into::into)
        .collect();

    Ok(Json(ProfileTokensResponse { tokens }))
}

async fn admin_create_profile_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(request): Json<CreateProfileTokenRequest>,
) -> Result<(StatusCode, Json<CreatedProfileTokenResponse>), Response> {
    let slug = parse_slug(&slug)?;
    authorize_admin_request(&headers, &state.auth)?;
    let store = admin_store(&state)?;
    let mut store = store.lock().await;
    let hash_key = state.auth.token_hash_key.as_ref().ok_or_else(|| {
        admin_bad_request_response(format!(
            "{} is required to create profile tokens",
            hoststamp_core::auth::PROFILE_TOKEN_HASH_KEY_ENV
        ))
    })?;
    let profile = store
        .load_profile(&slug)
        .map_err(admin_bad_request_response)?;
    let generated = generate_profile_token();
    let token_hash =
        profile_token_hash(hash_key, &generated.secret).map_err(admin_internal_response)?;
    let token = store
        .create_profile_token(
            profile.id,
            &generated.token_id,
            &request.name,
            token_hash,
            request.expires_at_ms,
        )
        .map_err(admin_bad_request_response)?;

    Ok((
        StatusCode::CREATED,
        Json(CreatedProfileTokenResponse {
            token: token.into(),
            profile_token: generated.token,
        }),
    ))
}

async fn admin_revoke_profile_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((slug, token_id)): Path<(String, String)>,
) -> Result<Json<ProfileTokenEnvelope>, Response> {
    let slug = parse_slug(&slug)?;
    authorize_admin_request(&headers, &state.auth)?;
    let store = admin_store(&state)?;
    let mut store = store.lock().await;
    let profile = store
        .load_profile(&slug)
        .map_err(admin_bad_request_response)?;
    let token = store
        .revoke_profile_token(profile.id, &token_id)
        .map_err(admin_bad_request_response)?;

    Ok(Json(ProfileTokenEnvelope {
        token: token.into(),
    }))
}

async fn admin_reset_atomic_value(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(request): Json<ResetAtomicValueRequest>,
) -> Result<Json<ProfileEnvelope>, Response> {
    let slug = parse_slug(&slug)?;
    if request.atomic_value < 0 {
        return Err(admin_bad_request_response(
            "atomic value must be at least 0",
        ));
    }
    authorize_admin_request(&headers, &state.auth)?;
    confirm_admin_action(&request.confirmation, &slug, "reset")?;
    let store = admin_store(&state)?;
    let mut store = store.lock().await;
    let profile = store
        .reset_atomic_value(&slug, request.atomic_value)
        .map_err(admin_bad_request_response)?;

    Ok(Json(ProfileEnvelope {
        profile: profile.into(),
    }))
}

fn random_overrides(query: &RandomQuery) -> Result<GenerateOverrides, String> {
    let word1_categories = query
        .word1_categories
        .as_deref()
        .map(generator::parse_categories)
        .transpose()
        .map_err(|error| error.to_string())?;
    let word2_categories = query
        .word2_categories
        .as_deref()
        .map(generator::parse_categories)
        .transpose()
        .map_err(|error| error.to_string())?;
    let word1_lengths = query
        .word1_lengths
        .as_deref()
        .map(generator::parse_lengths)
        .transpose()
        .map_err(|error| error.to_string())?;
    let word2_lengths = query
        .word2_lengths
        .as_deref()
        .map(generator::parse_lengths)
        .transpose()
        .map_err(|error| error.to_string())?;

    let overrides = GenerateOverrides {
        word1_enabled: query.word1_enabled,
        word1_lengths,
        word1_categories,
        word2_enabled: query.word2_enabled,
        word2_lengths,
        word2_categories,
        suffix_enabled: query.suffix_enabled,
        suffix_min_length: query.suffix_min_length,
        count: query.count,
    };

    Ok(overrides)
}

impl UpdateProfileConfigRequest {
    fn apply(&self, config: ProfileConfig) -> Result<ProfileConfig, AdminError> {
        if self.is_empty() {
            return Err(AdminError::BadRequest(
                "profile config update requires at least one setting".to_owned(),
            ));
        }

        let mut options = config.to_generate_options(generator::DEFAULT_COUNT);
        if let Some(enabled) = self.word1_enabled {
            options.word1_enabled = enabled;
        }
        if let Some(value) = &self.word1_lengths {
            options.word1_lengths = parse_lengths_value(value)?;
        }
        if let Some(categories) = &self.word1_categories {
            options.word1_categories = categories.clone();
        }
        if let Some(enabled) = self.word2_enabled {
            options.word2_enabled = enabled;
        }
        if let Some(value) = &self.word2_lengths {
            options.word2_lengths = parse_lengths_value(value)?;
        }
        if let Some(categories) = &self.word2_categories {
            options.word2_categories = categories.clone();
        }
        if let Some(enabled) = self.suffix_enabled {
            options.suffix_enabled = enabled;
        }
        if let Some(min_length) = self.suffix_min_length {
            options.suffix_min_length = min_length;
        }

        ProfileConfig::try_from_options(&options)
            .map_err(|error| AdminError::BadRequest(error.to_string()))
    }

    fn is_empty(&self) -> bool {
        self.word1_enabled.is_none()
            && self.word1_lengths.is_none()
            && self.word1_categories.is_none()
            && self.word2_enabled.is_none()
            && self.word2_lengths.is_none()
            && self.word2_categories.is_none()
            && self.suffix_enabled.is_none()
            && self.suffix_min_length.is_none()
    }
}

fn parse_lengths_value(value: &Value) -> Result<Option<Vec<usize>>, AdminError> {
    match value {
        Value::Null => Ok(None),
        Value::String(value) => generator::parse_lengths(value)
            .map_err(|error| AdminError::BadRequest(error.to_string())),
        Value::Array(values) => {
            let mut lengths = Vec::with_capacity(values.len());
            for value in values {
                let length = value.as_u64().ok_or_else(|| {
                    AdminError::BadRequest("length values must be positive integers".to_owned())
                })?;
                if length == 0 {
                    return Err(AdminError::BadRequest(
                        "length must be at least 1".to_owned(),
                    ));
                }
                let length = usize::try_from(length).map_err(|_| {
                    AdminError::BadRequest("length value does not fit on this platform".to_owned())
                })?;
                lengths.push(length);
            }
            if lengths.is_empty() {
                return Err(AdminError::BadRequest(
                    "length list must not be empty (use null or \"any\" for no length filter)"
                        .to_owned(),
                ));
            }
            Ok(Some(lengths))
        }
        _ => Err(AdminError::BadRequest(
            "lengths must be null, \"any\", a comma-separated string, or an integer array"
                .to_owned(),
        )),
    }
}

async fn regenerate_with_state(
    query: &RegenerateQuery,
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Vec<GeneratedHostname>, GenerateError> {
    let Some(atomic) = &state.atomic else {
        return Err(GenerateError::BadRequest(
            "profile storage is required for API regeneration".to_owned(),
        ));
    };
    let count = query.count.unwrap_or(generator::DEFAULT_COUNT);
    generator::validate_count(count)
        .map_err(|error| GenerateError::BadRequest(error.to_string()))?;
    let count_offset = i64::try_from(count - 1)
        .map_err(|_| GenerateError::BadRequest("count is too large".to_owned()))?;
    let final_atomic_value = query
        .atomic_value
        .checked_add(count_offset)
        .ok_or_else(|| GenerateError::BadRequest("atomic value range is too large".to_owned()))?;

    let profile_slug = match query.profile.as_deref() {
        Some(profile) => profile.parse().map_err(GenerateError::BadRequest)?,
        None => atomic.profile_slug.clone(),
    };

    let mut store = atomic.store.lock().await;
    let profile = match store.load_profile(&profile_slug) {
        Ok(profile) => profile,
        Err(error) => {
            authorize_missing_profile_request(headers, &state.auth)?;
            return Err(GenerateError::BadRequest(error.to_string()));
        }
    };
    authorize_profile_request(headers, &state.auth, &mut store, &profile)?;
    if !profile.config.suffix.enabled {
        return Err(GenerateError::BadRequest(format!(
            "profile {:?} cannot regenerate hostnames because suffixes are disabled; atomic values are only tracked when suffixes are enabled",
            profile.slug.as_str()
        )));
    }
    if final_atomic_value > profile.last_atomic_value {
        return Err(GenerateError::BadRequest(format!(
            "profile {:?} has issued {} atomic values; requested range {}..={} includes values that were never generated",
            profile.slug.as_str(),
            profile.last_atomic_value,
            query.atomic_value,
            final_atomic_value
        )));
    }
    if !profile.config.uses_current_generation_contract() {
        return Err(GenerateError::BadRequest(format!(
            "profile {:?} was created with a generation engine, dictionary/blocklist versions, or resolved word pools that do not match this binary; profile-backed generation cannot run safely across generation contract changes",
            profile.slug.as_str()
        )));
    }

    let options = profile.config.to_generate_options(generator::DEFAULT_COUNT);
    generator::validate_generate_options(&options)
        .map_err(|error| GenerateError::BadRequest(error.to_string()))?;
    (query.atomic_value..=final_atomic_value)
        .map(|atomic_value| {
            let hostname = generator::generate_profile_hostname(
                &options,
                profile.id,
                &profile.config_hash,
                atomic_value,
            )
            .map_err(|error| GenerateError::BadRequest(error.to_string()))?;
            Ok(GeneratedHostname::profile_backed(
                &profile.slug,
                ProfileGeneratedHostname {
                    hostname,
                    atomic_value,
                },
            ))
        })
        .collect()
}

fn generate_response(format: GenerateFormat, hostnames: Vec<GeneratedHostname>) -> Response {
    let mut response = match format {
        GenerateFormat::Plain => {
            let mut body = hostnames
                .iter()
                .map(|generated| generated.hostname.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            body.push('\n');
            ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
        }
        GenerateFormat::Json => Json(GenerateResponse {
            hostnames: hostnames.clone(),
        })
        .into_response(),
    };
    add_generate_metadata_headers(response.headers_mut(), &hostnames);
    response
}

fn add_generate_metadata_headers(headers: &mut HeaderMap, hostnames: &[GeneratedHostname]) {
    let Some(profile) = shared_profile(hostnames) else {
        return;
    };
    if let Ok(value) = HeaderValue::from_str(profile) {
        headers.insert("x-hoststamp-profile", value);
    }

    let atomic_values = hostnames
        .iter()
        .filter_map(|generated| generated.atomic_value)
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    if atomic_values.is_empty() {
        return;
    }
    if let Ok(value) = HeaderValue::from_str(&atomic_values.join(",")) {
        headers.insert("x-hoststamp-atomic-values", value);
    }
}

fn shared_profile(hostnames: &[GeneratedHostname]) -> Option<&str> {
    let first = hostnames.first()?.profile.as_deref()?;
    hostnames
        .iter()
        .all(|generated| generated.profile.as_deref() == Some(first))
        .then_some(first)
}

async fn generate_with_state(
    overrides: GenerateOverrides,
    profile_override: Option<&ProfileSlug>,
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Vec<GeneratedHostname>, GenerateError> {
    let options = match &state.atomic {
        Some(atomic) => {
            let mut store = atomic.store.lock().await;
            let profile_slug = profile_override.unwrap_or(&atomic.profile_slug);
            let profile = if state.auth.required || profile_override.is_some() {
                store
                    .load_profile(profile_slug)
                    .map_err(|error| GenerateError::BadRequest(error.to_string()))?
            } else {
                store
                    .load_or_seed_profile(profile_slug, &ProfileConfig::default())
                    .map_err(GenerateError::Internal)?
            };
            authorize_profile_request(headers, &state.auth, &mut store, &profile)?;
            let base = profile.config.to_generate_options(state.generate.count);
            let options = base.with_overrides(overrides);

            if options.suffix_enabled {
                generator::validate_generate_options(&options)
                    .map_err(|error| GenerateError::BadRequest(error.to_string()))?;

                let profile_id = profile.id;
                let profile_slug = profile.slug;
                let config_hash = profile.config_hash;
                let profile_slug_for_output = profile_slug.clone();
                return generator::generate_profile_many(options, profile_id, &config_hash, || {
                    store
                        .increment_atomic_value(&profile_slug)
                        .map_err(AtomicStorageError)
                        .map_err(Into::into)
                })
                .map(|hostnames| {
                    hostnames
                        .into_iter()
                        .map(|hostname| {
                            GeneratedHostname::profile_backed(&profile_slug_for_output, hostname)
                        })
                        .collect()
                })
                .map_err(|error| {
                    if error.downcast_ref::<AtomicStorageError>().is_some() {
                        GenerateError::Internal(error)
                    } else {
                        GenerateError::BadRequest(error.to_string())
                    }
                });
            }

            options
        }
        None => state.generate.with_overrides(overrides),
    };

    generator::generate_many(options)
        .map(|hostnames| {
            hostnames
                .into_iter()
                .map(GeneratedHostname::plain)
                .collect()
        })
        .map_err(|error| GenerateError::BadRequest(error.to_string()))
}

async fn capacity_options(
    profile_override: Option<&ProfileSlug>,
    state: &AppState,
    headers: &HeaderMap,
) -> Result<GenerateOptions, GenerateError> {
    let Some(atomic) = &state.atomic else {
        return Ok(state.generate.clone());
    };

    let mut store = atomic.store.lock().await;
    let profile_slug = profile_override.unwrap_or(&atomic.profile_slug);
    let profile = if state.auth.required || profile_override.is_some() {
        store
            .load_profile(profile_slug)
            .map_err(|error| GenerateError::BadRequest(error.to_string()))?
    } else {
        store
            .load_or_seed_profile(profile_slug, &ProfileConfig::default())
            .map_err(GenerateError::Internal)?
    };
    authorize_profile_request(headers, &state.auth, &mut store, &profile)?;
    Ok(profile.config.to_generate_options(state.generate.count))
}

fn authorize_profile_request(
    headers: &HeaderMap,
    auth: &ApiAuthConfig,
    store: &mut ProfileStore,
    profile: &StoredProfile,
) -> Result<(), GenerateError> {
    if !auth.required || profile.access == ProfileAccess::Public {
        return Ok(());
    }

    let Some(token) = bearer_token(headers) else {
        return Err(GenerateError::Unauthorized(
            "authorization bearer token is required".to_owned(),
        ));
    };

    if auth
        .admin_token
        .as_ref()
        .is_some_and(|admin_token| constant_time_eq(token, admin_token.expose()))
    {
        return Ok(());
    }

    let Some(parsed) = parse_profile_token(token) else {
        return Err(GenerateError::Unauthorized(
            "authorization bearer token is invalid".to_owned(),
        ));
    };
    let Some(hash_key) = auth.token_hash_key.as_ref() else {
        return Err(GenerateError::Unauthorized(
            "authorization bearer token is invalid".to_owned(),
        ));
    };
    let Some(record) = store
        .load_profile_token_auth(parsed.token_id)
        .map_err(GenerateError::Internal)?
    else {
        return Err(GenerateError::Unauthorized(
            "authorization bearer token is invalid".to_owned(),
        ));
    };
    if record.profile_id != profile.id {
        return Err(GenerateError::Unauthorized(
            "authorization bearer token is invalid".to_owned(),
        ));
    }
    if verify_profile_token_hash(hash_key, parsed.secret, &record.token_hash)
        .map_err(GenerateError::Internal)?
    {
        store
            .mark_profile_token_used(record.profile_id, parsed.token_id)
            .map_err(GenerateError::Internal)?;
        return Ok(());
    }

    Err(GenerateError::Unauthorized(
        "authorization bearer token is invalid".to_owned(),
    ))
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let mut parts = value.split_whitespace();
    let scheme = parts.next()?;
    let token = parts.next()?;
    if parts.next().is_some() || !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    Some(token)
}

fn authorize_missing_profile_request(
    headers: &HeaderMap,
    auth: &ApiAuthConfig,
) -> Result<(), GenerateError> {
    if !auth.required {
        return Ok(());
    }

    let Some(token) = bearer_token(headers) else {
        return Err(GenerateError::Unauthorized(
            "authorization bearer token is required".to_owned(),
        ));
    };

    if auth
        .admin_token
        .as_ref()
        .is_some_and(|admin_token| constant_time_eq(token, admin_token.expose()))
    {
        return Ok(());
    }

    Err(GenerateError::Unauthorized(
        "authorization bearer token is invalid".to_owned(),
    ))
}

fn authorize_admin_request(headers: &HeaderMap, auth: &ApiAuthConfig) -> Result<(), AdminError> {
    let Some(admin_token) = auth.admin_token.as_ref() else {
        return Err(AdminError::Unavailable(
            "admin API requires a configured admin token",
        ));
    };

    let Some(token) = bearer_token(headers) else {
        return Err(AdminError::Unauthorized(
            "admin authorization bearer token is required",
        ));
    };

    if constant_time_eq(token, admin_token.expose()) {
        return Ok(());
    }

    Err(AdminError::Unauthorized(
        "admin authorization bearer token is invalid",
    ))
}

fn admin_store(state: &AppState) -> Result<Arc<Mutex<ProfileStore>>, AdminError> {
    state
        .atomic
        .as_ref()
        .map(|atomic| Arc::clone(&atomic.store))
        .ok_or_else(|| {
            AdminError::BadRequest(
                "profile storage is required for API profile administration".to_owned(),
            )
        })
}

fn parse_slug(value: &str) -> Result<ProfileSlug, AdminError> {
    value
        .parse()
        .map_err(|error: String| AdminError::BadRequest(error))
}

fn confirm_admin_action(
    confirmation: &AdminConfirmation,
    slug: &ProfileSlug,
    action: &str,
) -> Result<(), AdminError> {
    if confirmation.profile.trim() != slug.as_str() || confirmation.action.trim() != action {
        return Err(AdminError::BadRequest(format!(
            "confirmation must include profile {:?} and action {:?}",
            slug.as_str(),
            action
        )));
    }
    Ok(())
}

fn admin_bad_request_response(error: impl ToString) -> Response {
    (StatusCode::BAD_REQUEST, error.to_string()).into_response()
}

fn admin_internal_response(error: impl Into<anyhow::Error>) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        anyhow!("profile administration error: {}", error.into()).to_string(),
    )
        .into_response()
}

fn unauthorized_response(message: &str) -> Response {
    let mut response = (StatusCode::UNAUTHORIZED, message.to_owned()).into_response();
    response
        .headers_mut()
        .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    response
}

fn hex_string(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

fn bad_request_response(error: impl ToString) -> Response {
    (StatusCode::BAD_REQUEST, error.to_string()).into_response()
}

enum GenerateError {
    BadRequest(String),
    Unauthorized(String),
    Internal(anyhow::Error),
}

enum AdminError {
    BadRequest(String),
    Unauthorized(&'static str),
    Unavailable(&'static str),
}

impl From<AdminError> for Response {
    fn from(error: AdminError) -> Self {
        match error {
            AdminError::BadRequest(message) => admin_bad_request_response(message),
            AdminError::Unauthorized(message) => unauthorized_response(message),
            AdminError::Unavailable(message) => {
                (StatusCode::SERVICE_UNAVAILABLE, message).into_response()
            }
        }
    }
}

#[derive(Debug)]
struct AtomicStorageError(anyhow::Error);

impl fmt::Display for AtomicStorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "atomic profile storage error: {}", self.0)
    }
}

impl std::error::Error for AtomicStorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

fn generate_error_response(error: GenerateError) -> Response {
    match error {
        GenerateError::BadRequest(message) => (StatusCode::BAD_REQUEST, message).into_response(),
        GenerateError::Unauthorized(message) => {
            let mut response = (StatusCode::UNAUTHORIZED, message).into_response();
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
            response
        }
        GenerateError::Internal(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            anyhow!("profile storage error: {error}").to_string(),
        )
            .into_response(),
    }
}

async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "not found")
}

pub async fn serve(addr: SocketAddr, generate: GenerateOptions) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    serve_with_shutdown_with_options(listener, generate, shutdown_signal()).await
}

pub async fn serve_with_atomic(
    addr: SocketAddr,
    generate: GenerateOptions,
    atomic: AtomicContext,
    auth: ApiAuthConfig,
) -> std::io::Result<()> {
    serve_with_atomic_and_mode(addr, generate, atomic, auth, AppMode::All).await
}

pub async fn serve_with_atomic_and_mode(
    addr: SocketAddr,
    generate: GenerateOptions,
    atomic: AtomicContext,
    auth: ApiAuthConfig,
    mode: AppMode,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    serve_with_shutdown_with_options_and_atomic(
        listener,
        generate,
        Some(atomic),
        auth,
        mode,
        shutdown_signal(),
    )
    .await
}

pub async fn serve_with_shutdown<F>(listener: TcpListener, shutdown: F) -> std::io::Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    serve_with_shutdown_with_options(listener, GenerateOptions::default(), shutdown).await
}

pub async fn serve_with_shutdown_with_options<F>(
    listener: TcpListener,
    generate: GenerateOptions,
    shutdown: F,
) -> std::io::Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    serve_with_shutdown_with_options_and_atomic(
        listener,
        generate,
        None,
        ApiAuthConfig::default(),
        AppMode::All,
        shutdown,
    )
    .await
}

pub async fn serve_with_shutdown_with_options_and_atomic<F>(
    listener: TcpListener,
    generate: GenerateOptions,
    atomic: Option<AtomicContext>,
    auth: ApiAuthConfig,
    mode: AppMode,
    shutdown: F,
) -> std::io::Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let bound = listener.local_addr()?;
    tracing::info!(%bound, "hoststamp listening");
    axum::serve(listener, app_with_mode(generate, atomic, auth, mode))
        .with_graceful_shutdown(shutdown)
        .await
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = signal::ctrl_c().await {
            tracing::warn!(%error, "failed to install Ctrl+C handler");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(error) => {
                tracing::warn!(%error, "failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("received Ctrl+C, shutting down"),
        _ = terminate => tracing::info!("received SIGTERM, shutting down"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn parse_lengths_value_accepts_supported_shapes() {
        assert_eq!(
            parse_lengths_value(&Value::Null).unwrap_or_else(|_| panic!("null")),
            None
        );
        assert_eq!(
            parse_lengths_value(&Value::String("4,5".to_owned()))
                .unwrap_or_else(|_| panic!("string")),
            Some(vec![4, 5])
        );
        assert_eq!(
            parse_lengths_value(&serde_json::json!([4, 5])).unwrap_or_else(|_| panic!("array")),
            Some(vec![4, 5])
        );
    }

    #[test]
    fn parse_lengths_value_rejects_invalid_shapes() {
        let non_integer = parse_lengths_value(&serde_json::json!([4, "5"]))
            .expect_err("strings are invalid in arrays");
        assert!(matches!(non_integer, AdminError::BadRequest(_)));

        let zero = parse_lengths_value(&serde_json::json!([0])).expect_err("zero is invalid");
        assert!(matches!(zero, AdminError::BadRequest(_)));

        let empty = parse_lengths_value(&serde_json::json!([])).expect_err("empty is invalid");
        assert!(matches!(empty, AdminError::BadRequest(_)));

        let object = parse_lengths_value(&serde_json::json!({ "length": 4 }))
            .expect_err("objects are invalid");
        assert!(matches!(object, AdminError::BadRequest(_)));
    }

    #[test]
    fn update_profile_config_request_can_clear_word_lengths() {
        let config = UpdateProfileConfigRequest {
            word1_enabled: None,
            word1_lengths: Some(Value::Null),
            word1_categories: None,
            word2_enabled: None,
            word2_lengths: None,
            word2_categories: None,
            suffix_enabled: None,
            suffix_min_length: None,
            confirmation: None,
        }
        .apply(ProfileConfig::default())
        .unwrap_or_else(|_| panic!("profile config"));

        assert_eq!(config.word1.lengths, None);
    }

    #[test]
    fn generate_metadata_headers_skip_missing_atomic_values() {
        let slug = "team-a".parse::<ProfileSlug>().expect("slug");
        let mut headers = HeaderMap::new();
        add_generate_metadata_headers(
            &mut headers,
            &[
                GeneratedHostname {
                    hostname: "alpha-bravo".to_owned(),
                    profile: Some(slug.as_str().to_owned()),
                    atomic_value: None,
                },
                GeneratedHostname {
                    hostname: "charlie-delta".to_owned(),
                    profile: Some(slug.as_str().to_owned()),
                    atomic_value: None,
                },
            ],
        );

        assert_eq!(headers["x-hoststamp-profile"], "team-a");
        assert!(!headers.contains_key("x-hoststamp-atomic-values"));
    }

    #[test]
    fn response_helpers_map_internal_errors() {
        let admin = admin_internal_response(anyhow!("database unavailable"));
        assert_eq!(admin.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let generated = generate_error_response(GenerateError::Internal(anyhow!("database busy")));
        assert_eq!(generated.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let error = AtomicStorageError(anyhow!("locked"));
        assert_eq!(error.source().expect("source").to_string(), "locked");
    }

    #[test]
    fn profile_authorization_rejects_unusable_profile_tokens() {
        let mut store = ProfileStore::open(&hoststamp_core::storage::StorageUrl::Sqlite(
            ":memory:".into(),
        ))
        .unwrap_or_else(|_| panic!("store"));
        let slug = ProfileSlug::default_profile();
        let profile = store
            .load_or_seed_profile(&slug, &ProfileConfig::default())
            .unwrap_or_else(|_| panic!("profile"));
        let generated = generate_profile_token();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", generated.token)
                .parse()
                .expect("header"),
        );

        let missing_hash_key = authorize_profile_request(
            &headers,
            &ApiAuthConfig {
                required: true,
                admin_token: None,
                token_hash_key: None,
            },
            &mut store,
            &profile,
        )
        .expect_err("hash key should be required");
        assert!(matches!(missing_hash_key, GenerateError::Unauthorized(_)));

        let hash_key =
            hoststamp_core::auth::SecretString::new("hash-key".to_owned()).expect("hash key");
        store
            .create_profile_token(profile.id, &generated.token_id, "deploy", [0; 32], None)
            .unwrap_or_else(|_| panic!("token"));
        let bad_secret = authorize_profile_request(
            &headers,
            &ApiAuthConfig {
                required: true,
                admin_token: None,
                token_hash_key: Some(hash_key),
            },
            &mut store,
            &profile,
        )
        .expect_err("bad secret should be rejected");
        assert!(matches!(bad_secret, GenerateError::Unauthorized(_)));
    }

    #[test]
    fn missing_profile_authorization_rejects_non_admin_tokens() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Bearer profile-token".parse().expect("header"),
        );
        let auth = ApiAuthConfig {
            required: true,
            admin_token: Some(
                hoststamp_core::auth::SecretString::new("admin-secret".to_owned()).expect("admin"),
            ),
            token_hash_key: None,
        };

        let error = authorize_missing_profile_request(&headers, &auth)
            .expect_err("profile token should not authorize missing profiles");

        assert!(matches!(error, GenerateError::Unauthorized(_)));
    }
}
