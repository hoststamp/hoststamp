// SPDX-License-Identifier: FSL-1.1-ALv2

use crate::{
    SERVICE_NAME,
    generator::{self, GenerateOptions, GenerateOverrides, ProfileGeneratedHostname},
    profile::{ProfileConfig, ProfileSlug},
    storage::ProfileStore,
    ux,
};
use anyhow::anyhow;
use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::{fmt, net::SocketAddr, sync::Arc};
use tokio::{net::TcpListener, signal, sync::Mutex};

#[derive(Clone)]
pub struct AppState {
    generate: GenerateOptions,
    atomic: Option<AtomicContext>,
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
    pub count: Option<usize>,
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
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GenerateFormat {
    Plain,
    Json,
}

pub fn app(generate_options: GenerateOptions) -> Router {
    app_with_atomic(generate_options, None)
}

pub fn app_with_atomic(generate_options: GenerateOptions, atomic: Option<AtomicContext>) -> Router {
    Router::new()
        .route("/", get(ux::index))
        .route("/healthz", get(healthz))
        .route("/api/health", get(healthz))
        .route(
            "/api/generate",
            post(generate_one).get(generate_method_not_allowed),
        )
        .route("/api/regenerate", get(regenerate_one))
        .route("/api/random", get(random_one))
        .fallback(not_found)
        .with_state(AppState {
            generate: generate_options,
            atomic,
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
    Query(query): Query<GenerateQuery>,
) -> Result<Response, (StatusCode, String)> {
    let overrides = GenerateOverrides {
        count: query.count,
        ..GenerateOverrides::default()
    };
    let hostnames = generate_with_state(overrides, &state)
        .await
        .map_err(generate_error_response)?;

    Ok(generate_response(
        query.format.unwrap_or(GenerateFormat::Plain),
        hostnames,
    ))
}

async fn random_one(Query(query): Query<RandomQuery>) -> Result<Response, (StatusCode, String)> {
    let overrides = random_overrides(&query)?;
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
    Query(query): Query<RegenerateQuery>,
) -> Result<Response, (StatusCode, String)> {
    if query.atomic_value < generator::ATOMIC_MIN_VALUE {
        return Err(bad_request(format!(
            "atomic value must be at least {}",
            generator::ATOMIC_MIN_VALUE
        )));
    }

    let hostname = regenerate_with_state(&query, &state)
        .await
        .map_err(generate_error_response)?;

    Ok(generate_response(
        query.format.unwrap_or(GenerateFormat::Plain),
        vec![hostname],
    ))
}

fn random_overrides(query: &RandomQuery) -> Result<GenerateOverrides, (StatusCode, String)> {
    let word1_categories = query
        .word1_categories
        .as_deref()
        .map(generator::parse_categories)
        .transpose()
        .map_err(bad_request)?;
    let word2_categories = query
        .word2_categories
        .as_deref()
        .map(generator::parse_categories)
        .transpose()
        .map_err(bad_request)?;
    let word1_lengths = query
        .word1_lengths
        .as_deref()
        .map(generator::parse_lengths)
        .transpose()
        .map_err(bad_request)?;
    let word2_lengths = query
        .word2_lengths
        .as_deref()
        .map(generator::parse_lengths)
        .transpose()
        .map_err(bad_request)?;

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

async fn regenerate_with_state(
    query: &RegenerateQuery,
    state: &AppState,
) -> Result<GeneratedHostname, GenerateError> {
    let Some(atomic) = &state.atomic else {
        return Err(GenerateError::BadRequest(
            "profile storage is required for API regeneration".to_owned(),
        ));
    };

    let profile_slug = match query.profile.as_deref() {
        Some(profile) => profile.parse().map_err(GenerateError::BadRequest)?,
        None => atomic.profile_slug.clone(),
    };

    let store = atomic.store.lock().await;
    let profile = store
        .load_profile(&profile_slug)
        .map_err(|error| GenerateError::BadRequest(error.to_string()))?;
    if !profile.config.suffix.enabled {
        return Err(GenerateError::BadRequest(format!(
            "profile {:?} cannot regenerate hostnames because suffixes are disabled; atomic values are only tracked when suffixes are enabled",
            profile.slug.as_str()
        )));
    }
    if query.atomic_value > profile.last_atomic_value {
        return Err(GenerateError::BadRequest(format!(
            "profile {:?} has issued {} atomic values; {} was never generated",
            profile.slug.as_str(),
            profile.last_atomic_value,
            query.atomic_value
        )));
    }
    if !profile.config.uses_current_dictionary() {
        return Err(GenerateError::BadRequest(format!(
            "profile {:?} was created with dictionary artifact {}, but this binary uses {}; profile-backed generation cannot run safely across dictionary changes",
            profile.slug.as_str(),
            profile.config.dictionary_fingerprint,
            crate::dictionary::artifact_sha256()
        )));
    }

    let options = profile.config.to_generate_options(generator::DEFAULT_COUNT);
    generator::validate_generate_options(&options)
        .map_err(|error| GenerateError::BadRequest(error.to_string()))?;
    let hostname = generator::generate_profile_hostname(
        &options,
        profile.id,
        &profile.config_hash,
        query.atomic_value,
    )
    .map_err(|error| GenerateError::BadRequest(error.to_string()))?;

    Ok(GeneratedHostname::profile_backed(
        &profile.slug,
        ProfileGeneratedHostname {
            hostname,
            atomic_value: query.atomic_value,
        },
    ))
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
    state: &AppState,
) -> Result<Vec<GeneratedHostname>, GenerateError> {
    let options = match &state.atomic {
        Some(atomic) => {
            let mut store = atomic.store.lock().await;
            let profile = store
                .load_or_seed_profile(&atomic.profile_slug, &ProfileConfig::default())
                .map_err(GenerateError::Internal)?;
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

fn bad_request(error: impl ToString) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, error.to_string())
}

enum GenerateError {
    BadRequest(String),
    Internal(anyhow::Error),
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

fn generate_error_response(error: GenerateError) -> (StatusCode, String) {
    match error {
        GenerateError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
        GenerateError::Internal(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            anyhow!("profile storage error: {error}").to_string(),
        ),
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
) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    serve_with_shutdown_with_options_and_atomic(listener, generate, Some(atomic), shutdown_signal())
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
    serve_with_shutdown_with_options_and_atomic(listener, generate, None, shutdown).await
}

pub async fn serve_with_shutdown_with_options_and_atomic<F>(
    listener: TcpListener,
    generate: GenerateOptions,
    atomic: Option<AtomicContext>,
    shutdown: F,
) -> std::io::Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let bound = listener.local_addr()?;
    tracing::info!(%bound, "hoststamp listening");
    axum::serve(listener, app_with_atomic(generate, atomic))
        .with_graceful_shutdown(shutdown)
        .await
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("received Ctrl+C, shutting down"),
        _ = terminate => tracing::info!("received SIGTERM, shutting down"),
    }
}
