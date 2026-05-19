// SPDX-License-Identifier: FSL-1.1-ALv2

use crate::{
    SERVICE_NAME,
    generator::{self, Dictionary, GenerateOptions, GenerateOverrides},
    ux,
};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::{net::TcpListener, signal};

#[derive(Debug, Clone, Copy)]
pub struct AppState {
    generate: GenerateOptions,
}

#[derive(Debug, Serialize)]
pub struct Health {
    pub status: &'static str,
    pub service: &'static str,
}

#[derive(Debug, Serialize)]
pub struct GenerateResponse {
    pub hostname: String,
}

#[derive(Debug, Deserialize)]
pub struct GenerateQuery {
    pub words: Option<usize>,
    pub word_length: Option<usize>,
    pub dictionary: Option<Dictionary>,
    pub suffix_hash: Option<bool>,
    pub suffix_len: Option<usize>,
}

pub fn app(generate_options: GenerateOptions) -> Router {
    Router::new()
        .route("/", get(ux::index))
        .route("/healthz", get(healthz))
        .route("/api/health", get(healthz))
        .route("/api/generate", get(generate_one))
        .fallback(not_found)
        .with_state(AppState {
            generate: generate_options,
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
    let options = state.generate.with_overrides(GenerateOverrides {
        words: query.words,
        word_length: query.word_length,
        dictionary: query.dictionary,
        suffix_hash: query.suffix_hash,
        suffix_len: query.suffix_len,
    });
    let hostname = generator::generate_hostname(options)
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;

    Ok(Json(GenerateResponse { hostname }))
}

async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "not found")
}

pub async fn serve(addr: SocketAddr, generate: GenerateOptions) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    serve_with_shutdown_with_options(listener, generate, shutdown_signal()).await
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
    let bound = listener.local_addr()?;
    tracing::info!(%bound, "hoststamp listening");
    axum::serve(listener, app(generate))
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
