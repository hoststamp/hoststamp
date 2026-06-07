// SPDX-License-Identifier: FSL-1.1-ALv2

use axum::{body::Body, http};
use hoststamp_api as server;
use hoststamp_core::{
    auth::{self, ApiAuthConfig, SecretString},
    generator::{self, GenerateOptions, is_base36_suffix},
    profile::{ProfileAccess, ProfileConfig, ProfileSlug},
    storage::{ProfileStore, StorageUrl, config_hash},
};
use http_body_util::BodyExt;
use rusqlite::Connection;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::{net::TcpListener, sync::oneshot};
use tower::ServiceExt;

#[cfg(debug_assertions)]
const UX_STATIC_DIR_ENV: &str = "HOSTSTAMP_UX_STATIC_DIR";

#[cfg(debug_assertions)]
static UX_STATIC_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(debug_assertions)]
struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

#[cfg(debug_assertions)]
impl EnvVarGuard {
    fn set(key: &'static str, value: &std::path::Path) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: UX route tests that mutate this variable hold UX_STATIC_ENV_LOCK.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }

    fn remove(key: &'static str) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: UX route tests that mutate this variable hold UX_STATIC_ENV_LOCK.
        unsafe {
            std::env::remove_var(key);
        }
        Self { key, previous }
    }
}

#[cfg(debug_assertions)]
impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => {
                // SAFETY: UX route tests that mutate this variable hold UX_STATIC_ENV_LOCK.
                unsafe {
                    std::env::set_var(self.key, value);
                }
            }
            None => {
                // SAFETY: UX route tests that mutate this variable hold UX_STATIC_ENV_LOCK.
                unsafe {
                    std::env::remove_var(self.key);
                }
            }
        }
    }
}

fn hostname_from_item(item: &serde_json::Value) -> &str {
    item["hostname"].as_str().expect("hostname")
}

fn hex_hash(hash: &[u8; 32]) -> String {
    hash.iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn future_timestamp_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_millis();
    i64::try_from(millis).expect("timestamp") + 60_000
}

async fn response_text(response: http::Response<Body>) -> String {
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    String::from_utf8(body.to_vec()).expect("utf8")
}

async fn response_json(response: http::Response<Body>) -> serde_json::Value {
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&body).expect("json")
}

fn json_request(method: http::Method, uri: &str, body: serde_json::Value) -> http::Request<Body> {
    http::Request::builder()
        .method(method)
        .uri(uri)
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("request")
}

fn admin_json_request(
    method: http::Method,
    uri: &str,
    body: serde_json::Value,
) -> http::Request<Body> {
    let mut request = json_request(method, uri, body);
    request.headers_mut().insert(
        http::header::AUTHORIZATION,
        "Bearer admin-secret".parse().expect("header"),
    );
    request
}

fn admin_get_request(uri: &str) -> http::Request<Body> {
    http::Request::builder()
        .uri(uri)
        .header(http::header::AUTHORIZATION, "Bearer admin-secret")
        .body(Body::empty())
        .expect("request")
}

fn auth_config() -> ApiAuthConfig {
    ApiAuthConfig {
        required: true,
        admin_token: Some(SecretString::new("admin-secret".to_owned()).expect("admin")),
        token_hash_key: Some(SecretString::new("hash-key".to_owned()).expect("hash key")),
    }
}

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
    #[cfg(debug_assertions)]
    let _env_lock = UX_STATIC_ENV_LOCK.lock().await;
    #[cfg(debug_assertions)]
    let _env = EnvVarGuard::remove(UX_STATIC_DIR_ENV);

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
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(response.headers()["x-frame-options"], "DENY");
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    assert!(
        response.headers()["content-security-policy"]
            .to_str()
            .expect("csp")
            .contains("frame-ancestors 'none'")
    );
    assert!(
        response.headers()["content-security-policy"]
            .to_str()
            .expect("csp")
            .contains("script-src 'self'")
    );

    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let html = std::str::from_utf8(&body).expect("utf8");

    assert!(html.contains("Hoststamp"));
    assert!(html.contains("/assets/app.css"));
    assert!(html.contains("/assets/app.js"));
    assert!(!html.contains("<style>"));
    assert!(!html.contains("<script>"));
}

#[tokio::test]
async fn local_ux_assets_are_served() {
    #[cfg(debug_assertions)]
    let _env_lock = UX_STATIC_ENV_LOCK.lock().await;
    #[cfg(debug_assertions)]
    let _env = EnvVarGuard::remove(UX_STATIC_DIR_ENV);

    let app = server::app(GenerateOptions::default());

    let css = app
        .clone()
        .oneshot(
            http::Request::builder()
                .uri("/assets/app.css")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(css.status(), http::StatusCode::OK);
    assert_eq!(css.headers()["x-content-type-options"], "nosniff");
    assert!(
        css.headers()[http::header::CONTENT_TYPE]
            .to_str()
            .expect("content type")
            .starts_with("text/css")
    );
    assert!(response_text(css).await.contains(":root"));

    let js = app
        .oneshot(
            http::Request::builder()
                .uri("/assets/app.js")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(js.status(), http::StatusCode::OK);
    assert_eq!(js.headers()["x-content-type-options"], "nosniff");
    assert!(
        js.headers()[http::header::CONTENT_TYPE]
            .to_str()
            .expect("content type")
            .starts_with("text/javascript")
    );
    let js = response_text(js).await;
    assert!(js.contains("const state"));
    assert!(js.contains("/api/health"));
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn all_and_ux_modes_serve_dev_reload_routes_when_static_dir_is_enabled() {
    let _env_lock = UX_STATIC_ENV_LOCK.lock().await;
    let tempdir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        tempdir.path().join("index.html"),
        "<!doctype html><html><body>Hoststamp</body></html>",
    )
    .expect("write index");
    std::fs::write(
        tempdir.path().join("app.css"),
        ":root { color-scheme: light; }",
    )
    .expect("write css");
    std::fs::write(tempdir.path().join("app.js"), "const state = {};").expect("write js");
    std::fs::write(
        tempdir.path().join("dev-reload.js"),
        "function checkForUpdate() {}",
    )
    .expect("write reload");
    let _env = EnvVarGuard::set(UX_STATIC_DIR_ENV, tempdir.path());

    for mode in [server::AppMode::All, server::AppMode::Ux] {
        let app = server::app_with_mode(
            GenerateOptions::default(),
            None,
            ApiAuthConfig::default(),
            mode,
        );

        let root = app
            .clone()
            .oneshot(
                http::Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(root.status(), http::StatusCode::OK);
        assert!(response_text(root).await.contains("/assets/dev-reload.js"));

        let version = app
            .clone()
            .oneshot(
                http::Request::builder()
                    .uri("/assets/dev-version")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(version.status(), http::StatusCode::OK);
        assert!(
            version.headers()[http::header::CONTENT_TYPE]
                .to_str()
                .expect("content type")
                .starts_with("text/plain")
        );
        let version = response_text(version).await;
        let version_parts = version.split('.').collect::<Vec<_>>();
        assert_eq!(version_parts.len(), 4);
        assert!(
            version_parts
                .iter()
                .all(|part| !part.is_empty() && part.parse::<u128>().is_ok())
        );

        let reload = app
            .oneshot(
                http::Request::builder()
                    .uri("/assets/dev-reload.js")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(reload.status(), http::StatusCode::OK);
        assert!(
            reload.headers()[http::header::CONTENT_TYPE]
                .to_str()
                .expect("content type")
                .starts_with("text/javascript")
        );
        assert!(response_text(reload).await.contains("checkForUpdate"));
    }
}

#[tokio::test]
async fn api_mode_does_not_serve_local_ux() {
    let app = server::app_with_mode(
        GenerateOptions::default(),
        None,
        ApiAuthConfig::default(),
        server::AppMode::Api,
    );

    let response = app
        .clone()
        .oneshot(
            http::Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), http::StatusCode::NOT_FOUND);

    let asset = app
        .oneshot(
            http::Request::builder()
                .uri("/assets/app.css")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(asset.status(), http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn ux_mode_does_not_serve_api_routes() {
    #[cfg(debug_assertions)]
    let _env_lock = UX_STATIC_ENV_LOCK.lock().await;
    #[cfg(debug_assertions)]
    let _env = EnvVarGuard::remove(UX_STATIC_DIR_ENV);

    let app = server::app_with_mode(
        GenerateOptions::default(),
        None,
        ApiAuthConfig::default(),
        server::AppMode::Ux,
    );

    let root = app
        .clone()
        .oneshot(
            http::Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(root.status(), http::StatusCode::OK);

    let api = app
        .oneshot(
            http::Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(api.status(), http::StatusCode::NOT_FOUND);
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
            .method(http::Method::POST)
            .uri("/api/generate")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);

    assert_eq!(
        response.headers()["content-type"],
        "text/plain; charset=utf-8"
    );
    let body = response_text(response).await;
    let hostnames = body.lines().collect::<Vec<_>>();
    assert_eq!(hostnames.len(), 1);
    let hostname = hostnames[0];
    let parts = hostname.split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 2);
    assert!(parts.iter().all(|part| part.chars().count() == 4));
}

#[tokio::test]
async fn oversized_json_bodies_are_rejected() {
    let body = format!(
        "{{\"padding\":\"{}\"}}",
        "x".repeat(server::MAX_REQUEST_BODY_BYTES)
    );
    let response = server::app_with_auth(GenerateOptions::default(), None, auth_config())
        .oneshot(
            http::Request::builder()
                .method(http::Method::POST)
                .uri("/api/profiles/import")
                .header(http::header::AUTHORIZATION, "Bearer admin-secret")
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), http::StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn generate_endpoint_rejects_get_requests() {
    let response = server::app(GenerateOptions::default())
        .oneshot(
            http::Request::builder()
                .uri("/api/generate")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), http::StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()["allow"], "POST");

    let body = response_text(response).await;
    assert!(body.contains("use POST /api/generate"));
}

#[tokio::test]
async fn random_endpoint_allows_query_overrides() {
    let response = server::app(GenerateOptions::default())
    .oneshot(
        http::Request::builder()
            .uri(
                "/api/random?word1_lengths=5&word2_lengths=5&suffix_enabled=true&suffix_min_length=10",
            )
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);

    let body = response_text(response).await;
    let hostnames = body.lines().collect::<Vec<_>>();
    assert_eq!(hostnames.len(), 1);
    let hostname = hostnames[0];
    let parts = hostname.split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0].chars().count(), 5);
    assert_eq!(parts[1].chars().count(), 5);
    assert!(parts[2].len() >= 10);
    assert!(is_base36_suffix(parts[2]));
}

#[tokio::test]
async fn random_endpoint_accepts_category_query_overrides() {
    let response = server::app(GenerateOptions::default())
        .oneshot(
            http::Request::builder()
                .uri(
                    "/api/random?word1_categories=diceware&word2_categories=diceware&word1_lengths=10&word2_lengths=10&suffix_enabled=false",
                )
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);

    let body = response_text(response).await;
    let hostnames = body.lines().collect::<Vec<_>>();
    assert_eq!(hostnames.len(), 1);
    let hostname = hostnames[0];
    let parts = hostname.split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 2);
    assert!(parts.iter().all(|part| part.chars().count() == 10));
    assert_ne!(parts[0], parts[1]);
}

#[tokio::test]
async fn random_endpoint_accepts_any_length_query() {
    let response = server::app(GenerateOptions::default())
        .oneshot(
            http::Request::builder()
                .uri("/api/random?word1_lengths=any&word2_lengths=any&suffix_enabled=false")
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
                .method(http::Method::POST)
                .uri("/api/generate?count=3")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);

    let body = response_text(response).await;
    let hostnames = body.lines().collect::<Vec<_>>();

    assert_eq!(hostnames.len(), 3);
}

#[tokio::test]
async fn random_endpoint_honors_count_query() {
    let response = server::app(GenerateOptions::default())
        .oneshot(
            http::Request::builder()
                .uri("/api/random?count=3")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);

    let body = response_text(response).await;
    let hostnames = body.lines().collect::<Vec<_>>();

    assert_eq!(hostnames.len(), 3);
}

#[tokio::test]
async fn generate_endpoint_supports_profile_backed_suffix_context() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let atomic_config = ProfileConfig::from(&GenerateOptions::default());
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&slug, &atomic_config)
        .expect("profile");

    let response = server::app_with_atomic(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug.clone())),
    )
    .oneshot(
        http::Request::builder()
            .method(http::Method::POST)
            .uri("/api/generate?count=2")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);

    assert_eq!(response.headers()["x-hoststamp-profile"], "_");
    assert_eq!(response.headers()["x-hoststamp-atomic-values"], "1,2");
    let body = response_text(response).await;
    let hostnames = body.lines().collect::<Vec<_>>();

    assert_eq!(hostnames.len(), 2);
    let mut store = ProfileStore::open(&database_url).expect("store");
    assert_eq!(
        store
            .load_or_seed_profile(&slug, &ProfileConfig::default())
            .expect("profile")
            .last_atomic_value,
        2
    );
}

#[tokio::test]
async fn generate_endpoint_accepts_profile_query() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let default_slug = ProfileSlug::default_profile();
    let other_slug = "team-a".parse::<ProfileSlug>().expect("slug");
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&default_slug, &ProfileConfig::default())
        .expect("default profile");
    store
        .create_profile(&other_slug, &ProfileConfig::default())
        .expect("other profile");

    let response = server::app_with_atomic(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, default_slug.clone())),
    )
    .oneshot(
        http::Request::builder()
            .method(http::Method::POST)
            .uri("/api/generate?format=json&profile=team-a")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);
    assert_eq!(response.headers()["x-hoststamp-profile"], "team-a");
    assert_eq!(response.headers()["x-hoststamp-atomic-values"], "1");
    let body = response_json(response).await;
    assert_eq!(body["hostnames"][0]["profile"], "team-a");
    assert_eq!(body["hostnames"][0]["atomic_value"], 1);

    let store = ProfileStore::open(&database_url).expect("store");
    assert_eq!(
        store
            .load_profile(&default_slug)
            .expect("default profile")
            .last_atomic_value,
        0
    );
    assert_eq!(
        store
            .load_profile(&other_slug)
            .expect("other profile")
            .last_atomic_value,
        1
    );
}

#[tokio::test]
async fn capacity_endpoint_reports_selected_profile_space() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let default_slug = ProfileSlug::default_profile();
    let other_slug = "team-a".parse::<ProfileSlug>().expect("slug");
    let config = ProfileConfig::from(&GenerateOptions {
        word1_lengths: Some(vec![4]),
        ..GenerateOptions::default()
    });
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&default_slug, &ProfileConfig::default())
        .expect("default profile");
    store
        .create_profile(&other_slug, &config)
        .expect("other profile");

    let response = server::app_with_atomic(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, default_slug)),
    )
    .oneshot(
        http::Request::builder()
            .uri("/api/capacity?profile=team-a")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["word1_count"], 65);
    assert_eq!(body["suffix_min_length"], 5);
    assert_eq!(body["suffix_enabled"], true);
}

#[tokio::test]
async fn capacity_endpoint_uses_server_defaults_without_profile_storage() {
    let response = server::app(GenerateOptions {
        word1_lengths: Some(vec![4]),
        suffix_enabled: false,
        ..GenerateOptions::default()
    })
    .oneshot(
        http::Request::builder()
            .uri("/api/capacity")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["word1_count"], 65);
    assert_eq!(body["suffix_enabled"], false);
}

#[tokio::test]
async fn capacity_endpoint_seeds_default_profile_when_auth_is_optional() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let store = ProfileStore::open(&database_url).expect("store");

    let response = server::app_with_atomic(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug.clone())),
    )
    .oneshot(
        http::Request::builder()
            .uri("/api/capacity")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);

    let store = ProfileStore::open(&database_url).expect("store");
    assert!(store.load_profile(&slug).is_ok());
}

#[tokio::test]
async fn generate_endpoint_requires_auth_for_private_profiles() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&slug, &ProfileConfig::default())
        .expect("profile");

    let response = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
        auth_config(),
    )
    .oneshot(
        http::Request::builder()
            .method(http::Method::POST)
            .uri("/api/generate")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["www-authenticate"], "Bearer");
}

#[tokio::test]
async fn generate_endpoint_rejects_malformed_bearer_tokens() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&slug, &ProfileConfig::default())
        .expect("profile");
    let app = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
        auth_config(),
    );

    let invalid_shape = app
        .clone()
        .oneshot(
            http::Request::builder()
                .method(http::Method::POST)
                .uri("/api/generate")
                .header(http::header::AUTHORIZATION, "Bearer not-a-profile-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(invalid_shape.status(), http::StatusCode::UNAUTHORIZED);

    let extra_parts = app
        .oneshot(
            http::Request::builder()
                .method(http::Method::POST)
                .uri("/api/generate")
                .header(http::header::AUTHORIZATION, "Bearer one two")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(extra_parts.status(), http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn generate_endpoint_accepts_admin_token_for_private_profiles() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&slug, &ProfileConfig::default())
        .expect("profile");

    let response = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
        auth_config(),
    )
    .oneshot(
        http::Request::builder()
            .method(http::Method::POST)
            .header(http::header::AUTHORIZATION, "Bearer admin-secret")
            .uri("/api/generate")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);
}

#[tokio::test]
async fn generate_endpoint_accepts_case_insensitive_bearer_scheme() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&slug, &ProfileConfig::default())
        .expect("profile");

    let response = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
        auth_config(),
    )
    .oneshot(
        http::Request::builder()
            .method(http::Method::POST)
            .header(http::header::AUTHORIZATION, "bearer   admin-secret")
            .uri("/api/generate")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);
}

#[tokio::test]
async fn generate_endpoint_accepts_profile_token_for_matching_profile() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_path = tempdir.path().join("hoststamp.db");
    let database_url = StorageUrl::Sqlite(database_path.clone());
    let slug = ProfileSlug::default_profile();
    let mut store = ProfileStore::open(&database_url).expect("store");
    let profile = store
        .load_or_seed_profile(&slug, &ProfileConfig::default())
        .expect("profile");
    let generated = auth::generate_profile_token();
    let hash_key = SecretString::new("hash-key".to_owned()).expect("key");
    let token_hash = auth::profile_token_hash(&hash_key, &generated.secret).expect("hash");
    store
        .create_profile_token(profile.id, &generated.token_id, "deploy", token_hash, None)
        .expect("token");

    let response = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
        ApiAuthConfig {
            required: true,
            admin_token: Some(SecretString::new("admin-secret".to_owned()).expect("admin")),
            token_hash_key: Some(hash_key),
        },
    )
    .oneshot(
        http::Request::builder()
            .method(http::Method::POST)
            .header(
                http::header::AUTHORIZATION,
                format!("Bearer {}", generated.token),
            )
            .uri("/api/generate")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);
    let last_used_at_ms: Option<i64> = Connection::open(database_path)
        .expect("connection")
        .query_row(
            "SELECT last_used_at_ms FROM hoststamp_profile_tokens WHERE token_id = ?1",
            [&generated.token_id],
            |row| row.get(0),
        )
        .expect("last used");
    assert!(last_used_at_ms.is_some());
}

#[tokio::test]
async fn generate_endpoint_rejects_expired_profile_token() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_path = tempdir.path().join("hoststamp.db");
    let database_url = StorageUrl::Sqlite(database_path.clone());
    let slug = ProfileSlug::default_profile();
    let mut store = ProfileStore::open(&database_url).expect("store");
    let profile = store
        .load_or_seed_profile(&slug, &ProfileConfig::default())
        .expect("profile");
    let generated = auth::generate_profile_token();
    let hash_key = SecretString::new("hash-key".to_owned()).expect("key");
    let token_hash = auth::profile_token_hash(&hash_key, &generated.secret).expect("hash");
    store
        .create_profile_token(
            profile.id,
            &generated.token_id,
            "deploy",
            token_hash,
            Some(future_timestamp_ms()),
        )
        .expect("token");
    drop(store);
    let connection = Connection::open(&database_path).expect("connection");
    connection
        .execute(
            "UPDATE hoststamp_profile_tokens SET expires_at_ms = 1 WHERE token_id = ?1",
            [&generated.token_id],
        )
        .expect("expire token");
    drop(connection);
    let store = ProfileStore::open(&database_url).expect("store");

    let response = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
        ApiAuthConfig {
            required: true,
            admin_token: Some(SecretString::new("admin-secret".to_owned()).expect("admin")),
            token_hash_key: Some(hash_key),
        },
    )
    .oneshot(
        http::Request::builder()
            .method(http::Method::POST)
            .header(
                http::header::AUTHORIZATION,
                format!("Bearer {}", generated.token),
            )
            .uri("/api/generate")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn generate_endpoint_rejects_wrong_profile_token_with_generic_error() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let other_slug: ProfileSlug = "other".parse().expect("slug");
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&slug, &ProfileConfig::default())
        .expect("profile");
    let other_profile = store
        .create_profile(&other_slug, &ProfileConfig::default())
        .expect("other profile");
    let generated = auth::generate_profile_token();
    let hash_key = SecretString::new("hash-key".to_owned()).expect("key");
    let token_hash = auth::profile_token_hash(&hash_key, &generated.secret).expect("hash");
    store
        .create_profile_token(
            other_profile.id,
            &generated.token_id,
            "deploy",
            token_hash,
            None,
        )
        .expect("token");

    let response = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
        ApiAuthConfig {
            required: true,
            admin_token: Some(SecretString::new("admin-secret".to_owned()).expect("admin")),
            token_hash_key: Some(hash_key),
        },
    )
    .oneshot(
        http::Request::builder()
            .method(http::Method::POST)
            .header(
                http::header::AUTHORIZATION,
                format!("Bearer {}", generated.token),
            )
            .uri("/api/generate")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::UNAUTHORIZED);
    let body = response_text(response).await;
    assert_eq!(body, "authorization bearer token is invalid");
}

#[tokio::test]
async fn generate_endpoint_allows_public_profiles_without_token() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&slug, &ProfileConfig::default())
        .expect("profile");
    store
        .set_profile_access(&slug, ProfileAccess::Public)
        .expect("access");

    let response = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
        auth_config(),
    )
    .oneshot(
        http::Request::builder()
            .method(http::Method::POST)
            .uri("/api/generate")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);
}

#[tokio::test]
async fn regenerate_endpoint_recreates_profile_hostname_without_incrementing_counter() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let atomic_config = ProfileConfig::from(&GenerateOptions::default());
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&slug, &atomic_config)
        .expect("profile");

    let app = server::app_with_atomic(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug.clone())),
    );

    let generated_response = app
        .clone()
        .oneshot(
            http::Request::builder()
                .method(http::Method::POST)
                .uri("/api/generate?count=2")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(generated_response.status(), http::StatusCode::OK);
    let generated_body = response_text(generated_response).await;
    let generated = generated_body.lines().collect::<Vec<_>>();
    assert_eq!(generated.len(), 2);

    let regenerated_response = app
        .oneshot(
            http::Request::builder()
                .uri("/api/regenerate?atomic_value=1&count=2&format=json")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(regenerated_response.status(), http::StatusCode::OK);
    assert_eq!(regenerated_response.headers()["x-hoststamp-profile"], "_");
    assert_eq!(
        regenerated_response.headers()["x-hoststamp-atomic-values"],
        "1,2"
    );

    let body = regenerated_response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let hostnames = payload["hostnames"].as_array().expect("hostnames");
    assert_eq!(hostnames.len(), 2);
    assert_eq!(hostname_from_item(&hostnames[0]), generated[0]);
    assert_eq!(hostnames[0]["profile"], "_");
    assert_eq!(hostnames[0]["atomic_value"], 1);
    assert_eq!(hostname_from_item(&hostnames[1]), generated[1]);
    assert_eq!(hostnames[1]["profile"], "_");
    assert_eq!(hostnames[1]["atomic_value"], 2);

    let mut store = ProfileStore::open(&database_url).expect("store");
    assert_eq!(
        store
            .load_or_seed_profile(&slug, &ProfileConfig::default())
            .expect("profile")
            .last_atomic_value,
        2
    );
}

#[tokio::test]
async fn regenerate_endpoint_supports_admin_profile_id_history() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = "team-a".parse::<ProfileSlug>().expect("slug");
    let mut store = ProfileStore::open(&database_url).expect("store");
    let profile = store
        .create_profile(&slug, &ProfileConfig::default())
        .expect("profile");
    assert_eq!(store.increment_atomic_value(&slug).expect("atomic"), 1);
    let options = profile.config.to_generate_options(generator::DEFAULT_COUNT);
    let expected =
        generator::generate_profile_hostname(&options, profile.id, &profile.config_hash, 1)
            .expect("hostname");
    let replacement_config = ProfileConfig::from(&GenerateOptions {
        word1_lengths: Some(vec![4]),
        ..GenerateOptions::default()
    });
    store
        .replace_profile_config(&slug, &replacement_config)
        .expect("replacement");

    let app = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(
            store,
            ProfileSlug::default_profile(),
        )),
        auth_config(),
    );
    let uri = format!(
        "/api/regenerate?format=json&profile_id={}&atomic_value=1",
        profile.id
    );

    let unauthorized = app
        .clone()
        .oneshot(
            http::Request::builder()
                .uri(&uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(unauthorized.status(), http::StatusCode::UNAUTHORIZED);

    let both_profile_selectors = app
        .clone()
        .oneshot(admin_get_request(&format!("{uri}&profile=team-a")))
        .await
        .expect("response");
    assert_eq!(
        both_profile_selectors.status(),
        http::StatusCode::BAD_REQUEST
    );
    let message = response_text(both_profile_selectors).await;
    assert!(message.contains("either profile or profile_id"));

    let regenerated = app
        .oneshot(admin_get_request(&uri))
        .await
        .expect("response");
    assert_eq!(regenerated.status(), http::StatusCode::OK);
    let payload = response_json(regenerated).await;
    let hostnames = payload["hostnames"].as_array().expect("hostnames");
    assert_eq!(hostnames.len(), 1);
    assert_eq!(hostnames[0]["hostname"], expected);
    assert_eq!(hostnames[0]["profile"], "team-a");
    assert_eq!(hostnames[0]["atomic_value"], 1);
}

#[tokio::test]
async fn lookup_endpoint_validates_profile_hostname() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let atomic_config = ProfileConfig::from(&GenerateOptions::default());
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&slug, &atomic_config)
        .expect("profile");

    let app = server::app_with_atomic(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
    );

    let generated_response = app
        .clone()
        .oneshot(
            http::Request::builder()
                .method(http::Method::POST)
                .uri("/api/generate?count=2")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(generated_response.status(), http::StatusCode::OK);
    let generated_body = response_text(generated_response).await;
    let generated = generated_body.lines().collect::<Vec<_>>();
    assert_eq!(generated.len(), 2);

    let lookup = app
        .clone()
        .oneshot(
            http::Request::builder()
                .uri(format!("/api/lookup?hostname={}&format=json", generated[1]))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(lookup.status(), http::StatusCode::OK);
    let payload = response_json(lookup).await;
    assert_eq!(payload["profile"], "_");
    assert_eq!(payload["atomic_value"], 2);
    assert_eq!(payload["valid"], true);

    let plain_format = app
        .clone()
        .oneshot(
            http::Request::builder()
                .uri(format!(
                    "/api/lookup?hostname={}&format=plain",
                    generated[1]
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(plain_format.status(), http::StatusCode::BAD_REQUEST);
    let body = response_text(plain_format).await;
    assert!(body.contains("lookup only supports format=json"));

    let mut parts = generated[1].split('-').collect::<Vec<_>>();
    parts[0] = "zzzzz";
    let tampered = parts.join("-");
    let tampered_lookup = app
        .oneshot(
            http::Request::builder()
                .uri(format!("/api/lookup?hostname={tampered}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(tampered_lookup.status(), http::StatusCode::OK);
    let payload = response_json(tampered_lookup).await;
    assert_eq!(payload["profile"], "_");
    assert_eq!(payload["atomic_value"], 2);
    assert_eq!(payload["valid"], false);
}

#[tokio::test]
async fn lookup_endpoint_rejects_unissued_profile_hostname() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let mut store = ProfileStore::open(&database_url).expect("store");
    let profile = store
        .load_or_seed_profile(&slug, &ProfileConfig::default())
        .expect("profile");
    let options = profile.config.to_generate_options(generator::DEFAULT_COUNT);
    let hostname =
        generator::generate_profile_hostname(&options, profile.id, &profile.config_hash, 1)
            .expect("hostname");

    let response = server::app_with_atomic(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
    )
    .oneshot(
        http::Request::builder()
            .uri(format!("/api/lookup?hostname={hostname}"))
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["atomic_value"], 1);
    assert_eq!(payload["valid"], false);
}

#[tokio::test]
async fn lookup_endpoint_requires_profile_storage() {
    let response = server::app(GenerateOptions::default())
        .oneshot(
            http::Request::builder()
                .uri("/api/lookup?hostname=brief-cobra-db50d")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    let body = response_text(response).await;
    assert!(body.contains("profile storage is required"));
}

#[tokio::test]
async fn lookup_endpoint_requires_auth_for_private_profiles() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&slug, &ProfileConfig::default())
        .expect("profile");
    let app = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
        auth_config(),
    );

    let unauthorized = app
        .clone()
        .oneshot(
            http::Request::builder()
                .uri("/api/lookup?hostname=brief-cobra-db50d")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(unauthorized.status(), http::StatusCode::UNAUTHORIZED);
    assert_eq!(unauthorized.headers()["www-authenticate"], "Bearer");

    let authorized = app
        .oneshot(
            http::Request::builder()
                .header(http::header::AUTHORIZATION, "Bearer admin-secret")
                .uri("/api/lookup?hostname=brief-cobra-db50d")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(authorized.status(), http::StatusCode::OK);
    let payload = response_json(authorized).await;
    assert_eq!(payload["valid"], false);
}

#[tokio::test]
async fn lookup_endpoint_requires_profile_backed_suffixes() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let config = ProfileConfig::from(&GenerateOptions {
        suffix_enabled: false,
        ..GenerateOptions::default()
    });
    let mut store = ProfileStore::open(&database_url).expect("store");
    store.load_or_seed_profile(&slug, &config).expect("profile");

    let response = server::app_with_atomic(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
    )
    .oneshot(
        http::Request::builder()
            .uri("/api/lookup?hostname=brief-cobra-db50d")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    let body = response_text(response).await;
    assert!(body.contains("atomic values are only tracked"));
}

#[tokio::test]
async fn regenerate_endpoint_requires_profile_storage() {
    let response = server::app(GenerateOptions::default())
        .oneshot(
            http::Request::builder()
                .uri("/api/regenerate?atomic_value=1")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    let body = response_text(response).await;
    assert!(body.contains("profile storage is required"));
}

#[tokio::test]
async fn regenerate_endpoint_rejects_values_that_have_not_been_issued() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let atomic_config = ProfileConfig::from(&GenerateOptions::default());
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&slug, &atomic_config)
        .expect("profile");

    let response = server::app_with_atomic(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
    )
    .oneshot(
        http::Request::builder()
            .uri("/api/regenerate?atomic_value=1&count=2")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    let body = response_text(response).await;
    assert!(body.contains("never generated"));
}

#[tokio::test]
async fn regenerate_endpoint_does_not_seed_missing_client_profile() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let atomic_config = ProfileConfig::from(&GenerateOptions::default());
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&slug, &atomic_config)
        .expect("profile");

    let response = server::app_with_atomic(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
    )
    .oneshot(
        http::Request::builder()
            .uri("/api/regenerate?profile=missing&atomic_value=1")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    let body = response_text(response).await;
    assert!(body.contains("profile \"missing\" does not exist"));

    let store = ProfileStore::open(&database_url).expect("store");
    let missing = "missing".parse().expect("slug");
    assert!(store.load_profile(&missing).is_err());
}

#[tokio::test]
async fn regenerate_endpoint_requires_auth_before_reporting_missing_private_profile() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&slug, &ProfileConfig::default())
        .expect("profile");

    let app = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
        auth_config(),
    );

    let unauthorized = app
        .clone()
        .oneshot(
            http::Request::builder()
                .uri("/api/regenerate?profile=missing&atomic_value=1")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(unauthorized.status(), http::StatusCode::UNAUTHORIZED);
    assert_eq!(unauthorized.headers()["www-authenticate"], "Bearer");
    let unauthorized_body = response_text(unauthorized).await;
    assert!(!unauthorized_body.contains("does not exist"));

    let authorized = app
        .oneshot(
            http::Request::builder()
                .header(http::header::AUTHORIZATION, "Bearer admin-secret")
                .uri("/api/regenerate?profile=missing&atomic_value=1")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(authorized.status(), http::StatusCode::BAD_REQUEST);
    let authorized_body = response_text(authorized).await;
    assert!(authorized_body.contains("profile \"missing\" does not exist"));
}

#[tokio::test]
async fn regenerate_endpoint_rejects_invalid_atomic_value() {
    let response = server::app(GenerateOptions::default())
        .oneshot(
            http::Request::builder()
                .uri("/api/regenerate?atomic_value=0")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    let body = response_text(response).await;
    assert!(body.contains("atomic value must be at least 1"));
}

#[tokio::test]
async fn regenerate_endpoint_requires_profile_backed_suffixes() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let config = ProfileConfig::from(&GenerateOptions {
        suffix_enabled: false,
        ..GenerateOptions::default()
    });
    let mut store = ProfileStore::open(&database_url).expect("store");
    store.load_or_seed_profile(&slug, &config).expect("profile");

    let response = server::app_with_atomic(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
    )
    .oneshot(
        http::Request::builder()
            .uri("/api/regenerate?atomic_value=1")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    let body = response_text(response).await;
    assert!(body.contains("atomic values are only tracked"));
}

#[tokio::test]
async fn generate_endpoint_reloads_active_atomic_profile() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let atomic_options = GenerateOptions::default();
    let replacement_options = GenerateOptions {
        word1_lengths: Some(vec![4]),
        ..GenerateOptions::default()
    };
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&slug, &ProfileConfig::from(&atomic_options))
        .expect("profile");

    let app = server::app_with_atomic(
        atomic_options,
        Some(server::AtomicContext::new(store, slug.clone())),
    );

    let mut replacement_store = ProfileStore::open(&database_url).expect("replacement store");
    let replacement = replacement_store
        .replace_profile_config(&slug, &ProfileConfig::from(&replacement_options))
        .expect("replacement");

    let response = app
        .oneshot(
            http::Request::builder()
                .method(http::Method::POST)
                .uri("/api/generate?format=json")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), http::StatusCode::OK);
    assert_eq!(response.headers()["x-hoststamp-profile"], "_");
    assert_eq!(response.headers()["x-hoststamp-atomic-values"], "1");

    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let hostnames = payload["hostnames"].as_array().expect("hostnames");
    let hostname = hostname_from_item(&hostnames[0]);
    assert_eq!(hostnames[0]["profile"], "_");
    assert_eq!(hostnames[0]["atomic_value"], 1);
    let parts = hostname.split('-').collect::<Vec<_>>();

    assert_eq!(parts[0].chars().count(), 4);
    assert_eq!(
        replacement_store
            .load_or_seed_profile(&slug, &ProfileConfig::default())
            .expect("profile")
            .id,
        replacement.id
    );
    assert_eq!(
        replacement_store
            .load_or_seed_profile(&slug, &ProfileConfig::default())
            .expect("profile")
            .last_atomic_value,
        1
    );
}

#[tokio::test]
async fn generate_endpoint_rejects_atomic_profile_config_overrides() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let atomic_config = ProfileConfig::from(&GenerateOptions::default());
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&slug, &atomic_config)
        .expect("profile");

    let response = server::app_with_atomic(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
    )
    .oneshot(
        http::Request::builder()
            .method(http::Method::POST)
            .uri("/api/generate?word1_lengths=4")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn generate_endpoint_returns_internal_error_for_atomic_increment_failures() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let path = tempdir.path().join("hoststamp.db");
    let database_url = StorageUrl::Sqlite(path.clone());
    let slug = ProfileSlug::default_profile();
    let atomic_config = ProfileConfig::from(&GenerateOptions::default());
    let mut setup_store = ProfileStore::open(&database_url).expect("store");
    setup_store
        .load_or_seed_profile(&slug, &atomic_config)
        .expect("profile");
    drop(setup_store);

    Connection::open(&path)
        .expect("connection")
        .execute_batch(
            "
            CREATE TRIGGER fail_atomic_increment
            BEFORE UPDATE OF last_atomic_value ON hoststamp_profiles
            BEGIN
                SELECT RAISE(FAIL, 'forced atomic increment failure');
            END;
            ",
        )
        .expect("trigger");

    let store = ProfileStore::open(&database_url).expect("store");
    let response = server::app_with_atomic(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
    )
    .oneshot(
        http::Request::builder()
            .method(http::Method::POST)
            .uri("/api/generate")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn generate_endpoint_rejects_random_query_overrides() {
    let response = server::app(GenerateOptions::default())
        .oneshot(
            http::Request::builder()
                .method(http::Method::POST)
                .uri("/api/generate?word1_lengths=4")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn admin_profile_endpoints_manage_profiles() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let default_slug = ProfileSlug::default_profile();
    let store = ProfileStore::open(&database_url).expect("store");
    let app = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, default_slug)),
        auth_config(),
    );

    let created = app
        .clone()
        .oneshot(admin_json_request(
            http::Method::POST,
            "/api/profiles",
            json!({ "slug": "team-a" }),
        ))
        .await
        .expect("response");
    assert_eq!(created.status(), http::StatusCode::CREATED);
    let created = response_json(created).await;
    assert_eq!(created["profile"]["slug"], "team-a");
    assert_eq!(created["profile"]["access"], "private");

    let shown = app
        .clone()
        .oneshot(admin_get_request("/api/profiles/team-a"))
        .await
        .expect("response");
    assert_eq!(shown.status(), http::StatusCode::OK);
    let shown = response_json(shown).await;
    assert_eq!(shown["profile"]["slug"], "team-a");

    let listed = app
        .clone()
        .oneshot(admin_get_request("/api/profiles"))
        .await
        .expect("response");
    assert_eq!(listed.status(), http::StatusCode::OK);
    let listed = response_json(listed).await;
    assert_eq!(listed["profiles"].as_array().expect("profiles").len(), 1);

    let access = app
        .clone()
        .oneshot(admin_json_request(
            http::Method::PATCH,
            "/api/profiles/team-a/access",
            json!({ "access": "public" }),
        ))
        .await
        .expect("response");
    assert_eq!(access.status(), http::StatusCode::OK);
    let access = response_json(access).await;
    assert_eq!(access["profile"]["access"], "public");

    let empty_config = app
        .clone()
        .oneshot(admin_json_request(
            http::Method::PATCH,
            "/api/profiles/team-a/config",
            json!({}),
        ))
        .await
        .expect("response");
    assert_eq!(empty_config.status(), http::StatusCode::BAD_REQUEST);
    let message = response_text(empty_config).await;
    assert!(message.contains("requires at least one setting"));

    let no_op_config = app
        .clone()
        .oneshot(admin_json_request(
            http::Method::PATCH,
            "/api/profiles/team-a/config",
            json!({
                "word1_enabled": true,
                "word1_lengths": [5],
                "word2_enabled": true,
                "word2_lengths": "5",
                "suffix_enabled": true,
                "suffix_min_length": 5
            }),
        ))
        .await
        .expect("response");
    assert_eq!(no_op_config.status(), http::StatusCode::OK);

    let unconfirmed_config = app
        .clone()
        .oneshot(admin_json_request(
            http::Method::PATCH,
            "/api/profiles/team-a/config",
            json!({ "word1_lengths": [4] }),
        ))
        .await
        .expect("response");
    assert_eq!(unconfirmed_config.status(), http::StatusCode::BAD_REQUEST);
    let message = response_text(unconfirmed_config).await;
    assert!(message.contains("requires confirmation"));

    let wrong_confirmation = app
        .clone()
        .oneshot(admin_json_request(
            http::Method::PATCH,
            "/api/profiles/team-a/config",
            json!({
                "word1_lengths": [4],
                "confirmation": {
                    "profile": "team-a",
                    "action": "delete"
                }
            }),
        ))
        .await
        .expect("response");
    assert_eq!(wrong_confirmation.status(), http::StatusCode::BAD_REQUEST);
    let message = response_text(wrong_confirmation).await;
    assert!(message.contains("confirmation must include"));

    let config = app
        .clone()
        .oneshot(admin_json_request(
            http::Method::PATCH,
            "/api/profiles/team-a/config",
            json!({
                "word1_lengths": [4],
                "word1_categories": ["diceware"],
                "word2_lengths": "any",
                "word2_categories": ["diceware"],
                "confirmation": {
                    "profile": "team-a",
                    "action": "replace"
                }
            }),
        ))
        .await
        .expect("response");
    assert_eq!(config.status(), http::StatusCode::OK);
    let config = response_json(config).await;
    assert_eq!(config["profile"]["config"]["word1"]["lengths"], json!([4]));
    assert_eq!(
        config["profile"]["config"]["word2"]["lengths"],
        serde_json::Value::Null
    );
    assert_eq!(config["profile"]["last_atomic_value"], 0);

    let history = app
        .clone()
        .oneshot(admin_get_request("/api/profiles/team-a/history"))
        .await
        .expect("response");
    assert_eq!(history.status(), http::StatusCode::OK);
    let history = response_json(history).await;
    let profiles = history["profiles"].as_array().expect("profiles");
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0]["id"], created["profile"]["id"]);
    assert_eq!(profiles[0]["replaced_by_id"], config["profile"]["id"]);
    assert!(profiles[0]["replaced_at_ms"].is_number());
    assert_eq!(profiles[1]["id"], config["profile"]["id"]);
    assert_eq!(profiles[1]["replaced_at_ms"], serde_json::Value::Null);
    assert_eq!(profiles[1]["replaced_by_id"], serde_json::Value::Null);

    let exported = app
        .clone()
        .oneshot(admin_get_request("/api/profiles/team-a/export"))
        .await
        .expect("response");
    assert_eq!(exported.status(), http::StatusCode::OK);
    let exported = response_json(exported).await;
    assert_eq!(exported["format"], "hoststamp-profile-v1");
    assert!(exported["id"].as_str().expect("id").contains('-'));
    assert_eq!(exported["slug"], "team-a");
    assert_eq!(exported["access"], "private");
    assert_eq!(exported["last_atomic_value"], 0);
    assert!(exported["config_hash"].as_str().expect("hash").len() == 64);
    assert_eq!(exported["config"]["word1"]["lengths"], json!([4]));

    let negative_reset = app
        .clone()
        .oneshot(admin_json_request(
            http::Method::POST,
            "/api/profiles/team-a/reset-atomic-value",
            json!({
                "atomic_value": -1,
                "confirmation": {
                    "profile": "team-a",
                    "action": "reset"
                }
            }),
        ))
        .await
        .expect("response");
    assert_eq!(negative_reset.status(), http::StatusCode::BAD_REQUEST);

    let reset = app
        .clone()
        .oneshot(admin_json_request(
            http::Method::POST,
            "/api/profiles/team-a/reset-atomic-value",
            json!({
                "atomic_value": 42,
                "confirmation": {
                    "profile": "team-a",
                    "action": "reset"
                }
            }),
        ))
        .await
        .expect("response");
    assert_eq!(reset.status(), http::StatusCode::OK);
    let reset = response_json(reset).await;
    assert_eq!(reset["profile"]["last_atomic_value"], 42);

    let deleted = app
        .clone()
        .oneshot(admin_json_request(
            http::Method::DELETE,
            "/api/profiles/team-a",
            json!({
                "confirmation": {
                    "profile": "team-a",
                    "action": "delete"
                }
            }),
        ))
        .await
        .expect("response");
    assert_eq!(deleted.status(), http::StatusCode::NO_CONTENT);

    let missing = app
        .oneshot(admin_get_request("/api/profiles/team-a"))
        .await
        .expect("response");
    assert_eq!(missing.status(), http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn admin_profile_import_preserves_exported_identity() {
    let source_dir = tempfile::tempdir().expect("source tempdir");
    let source_database = StorageUrl::Sqlite(source_dir.path().join("hoststamp.db"));
    let slug = "team-a".parse::<ProfileSlug>().expect("slug");
    let mut source_store = ProfileStore::open(&source_database).expect("source store");
    source_store
        .create_profile(&slug, &ProfileConfig::default())
        .expect("profile");
    source_store
        .set_profile_access(&slug, ProfileAccess::Public)
        .expect("access");
    source_store
        .increment_atomic_value(&slug)
        .expect("atomic value 1");
    source_store
        .increment_atomic_value(&slug)
        .expect("atomic value 2");
    let source_app = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(
            source_store,
            ProfileSlug::default_profile(),
        )),
        auth_config(),
    );

    let exported = source_app
        .oneshot(admin_get_request("/api/profiles/team-a/export"))
        .await
        .expect("response");
    assert_eq!(exported.status(), http::StatusCode::OK);
    let mut exported = response_json(exported).await;
    let exported_id = exported["id"].clone();
    assert_eq!(exported["access"], "public");
    assert_eq!(exported["last_atomic_value"], 2);

    let target_dir = tempfile::tempdir().expect("target tempdir");
    let target_database = StorageUrl::Sqlite(target_dir.path().join("hoststamp.db"));
    let target_store = ProfileStore::open(&target_database).expect("target store");
    let target_app = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(
            target_store,
            ProfileSlug::default_profile(),
        )),
        auth_config(),
    );

    let imported = target_app
        .clone()
        .oneshot(admin_json_request(
            http::Method::POST,
            "/api/profiles/import",
            exported.clone(),
        ))
        .await
        .expect("response");
    assert_eq!(imported.status(), http::StatusCode::CREATED);
    let imported = response_json(imported).await;
    assert_eq!(imported["profile"]["id"], exported_id);
    assert_eq!(imported["profile"]["slug"], "team-a");
    assert_eq!(imported["profile"]["access"], "public");
    assert_eq!(imported["profile"]["last_atomic_value"], 2);

    exported["last_atomic_value"] = json!(3);
    let unconfirmed = target_app
        .clone()
        .oneshot(admin_json_request(
            http::Method::POST,
            "/api/profiles/import",
            exported.clone(),
        ))
        .await
        .expect("response");
    assert_eq!(unconfirmed.status(), http::StatusCode::BAD_REQUEST);
    let message = response_text(unconfirmed).await;
    assert!(message.contains("requires confirmation"));

    exported["confirmation"] = json!({
        "profile": "team-a",
        "action": "replace"
    });
    let updated = target_app
        .clone()
        .oneshot(admin_json_request(
            http::Method::POST,
            "/api/profiles/import",
            exported,
        ))
        .await
        .expect("response");
    assert_eq!(updated.status(), http::StatusCode::OK);
    let updated = response_json(updated).await;
    assert_eq!(updated["profile"]["id"], exported_id);
    assert_eq!(updated["profile"]["last_atomic_value"], 3);

    let duplicate_id = json!({
        "format": "hoststamp-profile-v1",
        "id": exported_id,
        "slug": "team-b",
        "access": "public",
        "last_atomic_value": 3,
        "config_hash": hex_hash(&config_hash(&ProfileConfig::default()).expect("hash")),
        "config": ProfileConfig::default()
    });
    let rejected = target_app
        .clone()
        .oneshot(admin_json_request(
            http::Method::POST,
            "/api/profiles/import",
            duplicate_id,
        ))
        .await
        .expect("response");
    assert_eq!(rejected.status(), http::StatusCode::BAD_REQUEST);
    let message = response_text(rejected).await;
    assert!(message.contains("already exists"));

    let stale_config = ProfileConfig::from(&GenerateOptions {
        word1_lengths: Some(vec![4]),
        ..GenerateOptions::default()
    });
    let mismatched = json!({
        "format": "hoststamp-profile-v1",
        "id": exported_id,
        "slug": "team-a",
        "access": "public",
        "last_atomic_value": 3,
        "config_hash": hex_hash(&config_hash(&stale_config).expect("hash")),
        "config": ProfileConfig::default()
    });
    let rejected = target_app
        .oneshot(admin_json_request(
            http::Method::POST,
            "/api/profiles/import",
            mismatched,
        ))
        .await
        .expect("response");
    assert_eq!(rejected.status(), http::StatusCode::BAD_REQUEST);
    let message = response_text(rejected).await;
    assert!(message.contains("config_hash does not match"));
}

#[tokio::test]
async fn admin_profile_import_rejects_invalid_envelopes() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let store = ProfileStore::open(&database_url).expect("store");
    let app = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(
            store,
            ProfileSlug::default_profile(),
        )),
        auth_config(),
    );
    let mut valid = json!({
        "format": "hoststamp-profile-v1",
        "id": "018ff2de-8cf0-71aa-9e9b-8554cc5f4fd7",
        "slug": "team-a",
        "access": "private",
        "last_atomic_value": 0,
        "config_hash": hex_hash(&config_hash(&ProfileConfig::default()).expect("hash")),
        "config": ProfileConfig::default()
    });

    let mut wrong_format = valid.clone();
    wrong_format["format"] = json!("hoststamp-profile-v0");
    let rejected = app
        .clone()
        .oneshot(admin_json_request(
            http::Method::POST,
            "/api/profiles/import",
            wrong_format,
        ))
        .await
        .expect("response");
    assert_eq!(rejected.status(), http::StatusCode::BAD_REQUEST);
    let message = response_text(rejected).await;
    assert!(message.contains("profile import format"));

    let mut invalid_id = valid.clone();
    invalid_id["id"] = json!("not-a-uuid");
    let rejected = app
        .clone()
        .oneshot(admin_json_request(
            http::Method::POST,
            "/api/profiles/import",
            invalid_id,
        ))
        .await
        .expect("response");
    assert_eq!(rejected.status(), http::StatusCode::BAD_REQUEST);
    let message = response_text(rejected).await;
    assert!(message.contains("profile import id is invalid"));

    valid["last_atomic_value"] = json!(-1);
    let rejected = app
        .clone()
        .oneshot(admin_json_request(
            http::Method::POST,
            "/api/profiles/import",
            valid,
        ))
        .await
        .expect("response");
    assert_eq!(rejected.status(), http::StatusCode::BAD_REQUEST);
    let message = response_text(rejected).await;
    assert!(message.contains("last_atomic_value"));

    let stale_config = ProfileConfig {
        word1: hoststamp_core::profile::WordProfileConfig {
            pool_hash: Some("old".to_owned()),
            ..ProfileConfig::default().word1
        },
        ..ProfileConfig::default()
    };
    let stale = json!({
        "format": "hoststamp-profile-v1",
        "id": "018ff2de-8cf0-71aa-9e9b-8554cc5f4fd8",
        "slug": "team-a",
        "access": "private",
        "last_atomic_value": 0,
        "config_hash": hex_hash(&config_hash(&stale_config).expect("hash")),
        "config": stale_config
    });
    let rejected = app
        .oneshot(admin_json_request(
            http::Method::POST,
            "/api/profiles/import",
            stale,
        ))
        .await
        .expect("response");
    assert_eq!(rejected.status(), http::StatusCode::BAD_REQUEST);
    let message = response_text(rejected).await;
    assert!(message.contains("generation engine"));
}

#[tokio::test]
async fn admin_endpoints_require_admin_token_when_auth_is_required() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let mut store = ProfileStore::open(&database_url).expect("store");
    let profile = store
        .load_or_seed_profile(&slug, &ProfileConfig::default())
        .expect("profile");
    let generated = auth::generate_profile_token();
    let hash_key = SecretString::new("hash-key".to_owned()).expect("key");
    let token_hash = auth::profile_token_hash(&hash_key, &generated.secret).expect("hash");
    store
        .create_profile_token(profile.id, &generated.token_id, "deploy", token_hash, None)
        .expect("token");

    let app = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
        ApiAuthConfig {
            required: true,
            admin_token: Some(SecretString::new("admin-secret".to_owned()).expect("admin")),
            token_hash_key: Some(hash_key),
        },
    );

    let missing = app
        .clone()
        .oneshot(
            http::Request::builder()
                .uri("/api/profiles")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(missing.status(), http::StatusCode::UNAUTHORIZED);
    assert_eq!(missing.headers()["www-authenticate"], "Bearer");

    let profile_token = app
        .clone()
        .oneshot(
            http::Request::builder()
                .uri("/api/profiles")
                .header(
                    http::header::AUTHORIZATION,
                    format!("Bearer {}", generated.token),
                )
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(profile_token.status(), http::StatusCode::UNAUTHORIZED);

    let admin = app
        .oneshot(
            http::Request::builder()
                .uri("/api/profiles")
                .header(http::header::AUTHORIZATION, "Bearer admin-secret")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(admin.status(), http::StatusCode::OK);
}

#[tokio::test]
async fn admin_endpoints_require_configured_admin_token() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let store = ProfileStore::open(&database_url).expect("store");

    let response = server::app_with_atomic(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
    )
    .oneshot(
        http::Request::builder()
            .uri("/api/profiles")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::SERVICE_UNAVAILABLE);
    let body = response_text(response).await;
    assert!(body.contains("configured admin token"));
}

#[tokio::test]
async fn admin_endpoints_require_profile_storage() {
    let response = server::app_with_auth(GenerateOptions::default(), None, auth_config())
        .oneshot(admin_get_request("/api/profiles"))
        .await
        .expect("response");

    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    let body = response_text(response).await;
    assert!(body.contains("profile storage is required"));
}

#[tokio::test]
async fn admin_profile_token_creation_requires_hash_key() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&slug, &ProfileConfig::default())
        .expect("profile");

    let response = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
        ApiAuthConfig {
            required: true,
            admin_token: Some(SecretString::new("admin-secret".to_owned()).expect("admin")),
            token_hash_key: None,
        },
    )
    .oneshot(admin_json_request(
        http::Method::POST,
        "/api/profiles/_/tokens",
        json!({ "name": "deploy" }),
    ))
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    let body = response_text(response).await;
    assert!(body.contains(auth::PROFILE_TOKEN_HASH_KEY_ENV));
}

#[tokio::test]
async fn admin_delete_authorizes_before_checking_confirmation() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&slug, &ProfileConfig::default())
        .expect("profile");

    let response = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
        auth_config(),
    )
    .oneshot(json_request(
        http::Method::DELETE,
        "/api/profiles/_",
        json!({
            "confirmation": {
                "profile": "_",
                "action": "wrong"
            }
        }),
    ))
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_profile_token_endpoints_manage_tokens() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database_url = StorageUrl::Sqlite(tempdir.path().join("hoststamp.db"));
    let slug = ProfileSlug::default_profile();
    let mut store = ProfileStore::open(&database_url).expect("store");
    store
        .load_or_seed_profile(&slug, &ProfileConfig::default())
        .expect("profile");
    let app = server::app_with_auth(
        GenerateOptions::default(),
        Some(server::AtomicContext::new(store, slug)),
        auth_config(),
    );

    let expires_at_ms = future_timestamp_ms();
    let request = admin_json_request(
        http::Method::POST,
        "/api/profiles/_/tokens",
        json!({ "name": "deploy", "expires_at_ms": expires_at_ms }),
    );
    let created = app.clone().oneshot(request).await.expect("response");
    assert_eq!(created.status(), http::StatusCode::CREATED);
    let created = response_json(created).await;
    let token_id = created["token"]["token_id"].as_str().expect("token id");
    assert_eq!(created["token"]["name"], "deploy");
    assert_eq!(created["token"]["expires_at_ms"], expires_at_ms);
    assert!(
        created["profile_token"]
            .as_str()
            .expect("profile token")
            .starts_with("hspt_")
    );

    let listed = app
        .clone()
        .oneshot(admin_get_request("/api/profiles/_/tokens"))
        .await
        .expect("response");
    assert_eq!(listed.status(), http::StatusCode::OK);
    let listed = response_json(listed).await;
    assert_eq!(listed["tokens"].as_array().expect("tokens").len(), 1);
    assert_eq!(listed["tokens"][0]["expires_at_ms"], expires_at_ms);
    assert!(listed.get("profile_token").is_none());

    let expired = app
        .clone()
        .oneshot(admin_json_request(
            http::Method::POST,
            "/api/profiles/_/tokens",
            json!({ "name": "expired", "expires_at_ms": 1 }),
        ))
        .await
        .expect("response");
    assert_eq!(expired.status(), http::StatusCode::BAD_REQUEST);
    let message = response_text(expired).await;
    assert!(message.contains("expiration must be in the future"));

    let duplicate = app
        .clone()
        .oneshot(admin_json_request(
            http::Method::POST,
            "/api/profiles/_/tokens",
            json!({ "name": "deploy" }),
        ))
        .await
        .expect("response");
    assert_eq!(duplicate.status(), http::StatusCode::BAD_REQUEST);
    let message = response_text(duplicate).await;
    assert!(message.contains("already exists"));

    let invalid_name = app
        .clone()
        .oneshot(admin_json_request(
            http::Method::POST,
            "/api/profiles/_/tokens",
            json!({ "name": "Deploy" }),
        ))
        .await
        .expect("response");
    assert_eq!(invalid_name.status(), http::StatusCode::BAD_REQUEST);
    let message = response_text(invalid_name).await;
    assert!(message.contains("lowercase ASCII"));

    let revoked = app
        .oneshot(
            http::Request::builder()
                .method(http::Method::DELETE)
                .uri(format!("/api/profiles/_/tokens/{token_id}"))
                .header(http::header::AUTHORIZATION, "Bearer admin-secret")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(revoked.status(), http::StatusCode::OK);
    let revoked = response_json(revoked).await;
    assert!(revoked["token"]["revoked_at_ms"].is_i64());
}

#[tokio::test]
async fn random_endpoint_returns_bad_request_for_invalid_filter() {
    let response = server::app(GenerateOptions::default())
        .oneshot(
            http::Request::builder()
                .uri("/api/random?word1_lengths=100")
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
