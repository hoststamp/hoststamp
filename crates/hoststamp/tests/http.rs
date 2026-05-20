// SPDX-License-Identifier: FSL-1.1-ALv2

use axum::{body::Body, http};
use hoststamp::{generator::GenerateOptions, server};
use http_body_util::BodyExt;
use tokio::{net::TcpListener, sync::oneshot};
use tower::ServiceExt;

#[tokio::test]
async fn healthz_returns_ok_payload() {
    let response = server::app(GenerateOptions::default())
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
    let response = server::app(GenerateOptions::default())
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
async fn generate_endpoint_uses_server_defaults() {
    let response = server::app(GenerateOptions {
        word1_lengths: Some(vec![4]),
        word2_lengths: Some(vec![4]),
        suffix_enabled: false,
        ..GenerateOptions::default()
    })
    .oneshot(
        http::Request::builder()
            .uri("/api/generate")
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
    let hostnames = payload["hostnames"].as_array().expect("hostnames");
    assert_eq!(hostnames.len(), 1);
    let hostname = hostnames[0].as_str().expect("hostname");
    let parts = hostname.split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 2);
    assert!(parts.iter().all(|part| part.chars().count() == 4));
}

#[tokio::test]
async fn generate_endpoint_allows_query_overrides() {
    let response = server::app(GenerateOptions {
        word1_lengths: Some(vec![4]),
        word2_lengths: Some(vec![4]),
        suffix_enabled: false,
        suffix_length: 7,
        ..GenerateOptions::default()
    })
    .oneshot(
        http::Request::builder()
            .uri(
                "/api/generate?word1_lengths=5&word2_lengths=5&suffix_enabled=true&suffix_length=10",
            )
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
    let hostnames = payload["hostnames"].as_array().expect("hostnames");
    assert_eq!(hostnames.len(), 1);
    let hostname = hostnames[0].as_str().expect("hostname");
    let parts = hostname.split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0].chars().count(), 5);
    assert_eq!(parts[1].chars().count(), 5);
    assert_eq!(parts[2].len(), 10);
    assert!(parts[2].chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn generate_endpoint_accepts_category_query_overrides() {
    let response = server::app(GenerateOptions::default())
        .oneshot(
            http::Request::builder()
                .uri(
                    "/api/generate?word1_categories=diceware&word2_categories=diceware&word1_lengths=10&word2_lengths=10&suffix_enabled=false",
                )
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
    let hostnames = payload["hostnames"].as_array().expect("hostnames");
    assert_eq!(hostnames.len(), 1);
    let hostname = hostnames[0].as_str().expect("hostname");
    let parts = hostname.split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 2);
    assert!(parts.iter().all(|part| part.chars().count() == 10));
    assert_ne!(parts[0], parts[1]);
}

#[tokio::test]
async fn generate_endpoint_accepts_any_length_query() {
    let response = server::app(GenerateOptions::default())
        .oneshot(
            http::Request::builder()
                .uri("/api/generate?word1_lengths=any&word2_lengths=any&suffix_enabled=false")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);
}

#[tokio::test]
async fn generate_endpoint_honors_count_query() {
    let response = server::app(GenerateOptions::default())
        .oneshot(
            http::Request::builder()
                .uri("/api/generate?count=3")
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
    let hostnames = payload["hostnames"].as_array().expect("hostnames");

    assert_eq!(hostnames.len(), 3);
}

#[tokio::test]
async fn generate_endpoint_returns_bad_request_for_invalid_filter() {
    let response = server::app(GenerateOptions::default())
        .oneshot(
            http::Request::builder()
                .uri("/api/generate?word1_lengths=100")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);

    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let message = std::str::from_utf8(&body).expect("utf8");

    assert!(message.contains("do not contain"));
}

#[tokio::test]
async fn generate_endpoint_rejects_atomic_source() {
    let response = server::app(GenerateOptions::default())
        .oneshot(
            http::Request::builder()
                .uri("/api/generate?suffix_source=atomic")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn fallback_returns_not_found() {
    let response = server::app(GenerateOptions::default())
        .oneshot(
            http::Request::builder()
                .uri("/missing")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn serve_returns_bind_error_when_addr_is_in_use() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");

    let error = server::serve(addr, GenerateOptions::default())
        .await
        .expect_err("bind error");

    assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
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
