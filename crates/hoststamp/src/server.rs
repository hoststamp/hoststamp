// SPDX-License-Identifier: FSL-1.1-ALv2

use crate::{SERVICE_NAME, ux};
use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::get};
use serde::Serialize;
use std::net::SocketAddr;
use tokio::{net::TcpListener, signal};

#[derive(Debug, Serialize)]
pub struct Health {
    pub status: &'static str,
    pub service: &'static str,
}

pub fn app() -> Router {
    Router::new()
        .route("/", get(ux::index))
        .route("/healthz", get(healthz))
        .route("/api/health", get(healthz))
        .fallback(not_found)
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

async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "not found")
}

pub async fn serve(addr: SocketAddr) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    serve_with_shutdown(listener, shutdown_signal()).await
}

pub async fn serve_with_shutdown<F>(listener: TcpListener, shutdown: F) -> std::io::Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let bound = listener.local_addr()?;
    tracing::info!(%bound, "hoststamp listening");
    axum::serve(listener, app())
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
