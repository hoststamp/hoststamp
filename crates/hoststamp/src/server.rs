// SPDX-License-Identifier: FSL-1.1-ALv2

use crate::{
    SERVICE_NAME,
    generator::{self, GenerateOptions, GenerateOverrides, SuffixHash, SuffixSource},
    profile::{ProfileConfig, ProfileSlug},
    storage::ProfileStore,
    ux,
};
use anyhow::anyhow;
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
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
    pub hostnames: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct GenerateQuery {
    pub word1_enabled: Option<bool>,
    pub word1_lengths: Option<String>,
    pub word1_categories: Option<String>,
    pub word2_enabled: Option<bool>,
    pub word2_lengths: Option<String>,
    pub word2_categories: Option<String>,
    pub suffix_enabled: Option<bool>,
    pub suffix_length: Option<usize>,
    pub suffix_source: Option<SuffixSource>,
    pub suffix_hash: Option<SuffixHash>,
    pub count: Option<usize>,
}

pub fn app(generate_options: GenerateOptions) -> Router {
    app_with_atomic(generate_options, None)
}

pub fn app_with_atomic(generate_options: GenerateOptions, atomic: Option<AtomicContext>) -> Router {
    Router::new()
        .route("/", get(ux::index))
        .route("/healthz", get(healthz))
        .route("/api/health", get(healthz))
        .route("/api/generate", get(generate_one))
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

async fn generate_one(
    State(state): State<AppState>,
    Query(query): Query<GenerateQuery>,
) -> Result<Json<GenerateResponse>, (StatusCode, String)> {
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
        suffix_length: query.suffix_length,
        suffix_source: query.suffix_source,
        suffix_hash: query.suffix_hash,
        count: query.count,
    };
    let hostnames = generate_with_state(overrides, &state)
        .await
        .map_err(generate_error_response)?;

    Ok(Json(GenerateResponse { hostnames }))
}

async fn generate_with_state(
    overrides: GenerateOverrides,
    state: &AppState,
) -> Result<Vec<String>, GenerateError> {
    let options = match &state.atomic {
        Some(atomic) => {
            let mut store = atomic.store.lock().await;
            let profile = store
                .load_or_seed_profile(&atomic.profile_slug, &ProfileConfig::default())
                .map_err(GenerateError::Internal)?;
            let base = profile.config.to_generate_options(state.generate.count);
            let options = base.with_overrides(overrides);

            if options.suffix_enabled && options.suffix_source == SuffixSource::Atomic {
                if ProfileConfig::from(&options) != profile.config {
                    return Err(GenerateError::BadRequest(
                        "atomic profile config overrides require interactive CLI confirmation"
                            .to_owned(),
                    ));
                }

                let profile_id = profile.id;
                let profile_slug = profile.slug;
                let suffix_hash = options.suffix_hash;
                let suffix_length = options.suffix_length;
                return generator::generate_many_with_atomic_suffix(options, || {
                    let atomic_value = store
                        .increment_atomic_value(&profile_slug)
                        .map_err(AtomicStorageError)?;
                    generator::compute_atomic_suffix(
                        profile_id,
                        atomic_value,
                        suffix_hash,
                        suffix_length,
                    )
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

    if options.suffix_enabled && options.suffix_source == SuffixSource::Atomic {
        return Err(GenerateError::BadRequest(
            "suffix source 'atomic' requires a profile database".to_owned(),
        ));
    }

    generator::generate_many(options).map_err(|error| GenerateError::BadRequest(error.to_string()))
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
