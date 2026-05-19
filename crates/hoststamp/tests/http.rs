// SPDX-License-Identifier: FSL-1.1-ALv2

use axum::{body::Body, http};
use hoststamp::server;
use http_body_util::BodyExt;
use tokio::{net::TcpListener, sync::oneshot};
use tower::ServiceExt;

#[tokio::test]
async fn healthz_returns_ok_payload() {
    let response = server::app()
        .oneshot(
            http::Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);

    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&body).expect("json");

    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["service"], "hoststamp");
}

#[tokio::test]
async fn root_serves_local_ux() {
    let response = server::app()
        .oneshot(
            http::Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);

    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let html = std::str::from_utf8(&body).expect("utf8");

    assert!(html.contains("Hoststamp"));
    assert!(html.contains("/api/health"));
}

#[tokio::test]
async fn serve_with_shutdown_handles_live_health_request() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    let server = tokio::spawn(server::serve_with_shutdown(listener, async {
        let _ = shutdown_rx.await;
    }));

    let response = reqwest::get(format!("http://{addr}/healthz"))
        .await
        .expect("response");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let payload: serde_json::Value = response.json().await.expect("json");
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["service"], "hoststamp");

    shutdown_tx.send(()).expect("shutdown");
    server.await.expect("join").expect("server");
}
