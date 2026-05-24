// SPDX-License-Identifier: FSL-1.1-ALv2

use axum::{body::Body, http};
use hoststamp::{
    auth::{self, ApiAuthConfig, SecretString},
    generator::{GenerateOptions, is_base36_suffix},
    profile::{ProfileAccess, ProfileConfig, ProfileSlug},
    server,
    storage::{ProfileStore, StorageUrl},
};
use http_body_util::BodyExt;
use rusqlite::Connection;
use tokio::{net::TcpListener, sync::oneshot};
use tower::ServiceExt;

fn hostname_from_item(item: &serde_json::Value) -> &str {
    item["hostname"].as_str().expect("hostname")
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
        .create_profile_token(profile.id, &generated.token_id, "deploy", token_hash)
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
        .create_profile_token(other_profile.id, &generated.token_id, "deploy", token_hash)
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
                .uri("/api/regenerate?atomic_value=1&format=json")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(regenerated_response.status(), http::StatusCode::OK);
    assert_eq!(regenerated_response.headers()["x-hoststamp-profile"], "_");
    assert_eq!(
        regenerated_response.headers()["x-hoststamp-atomic-values"],
        "1"
    );

    let body = regenerated_response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let hostnames = payload["hostnames"].as_array().expect("hostnames");
    assert_eq!(hostname_from_item(&hostnames[0]), generated[0]);
    assert_eq!(hostnames[0]["profile"], "_");
    assert_eq!(hostnames[0]["atomic_value"], 1);

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
            .uri("/api/regenerate?atomic_value=1")
            .body(Body::empty())
            .expect("request"),
    )
    .await
    .expect("response");

    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    let body = response_text(response).await;
    assert!(body.contains("was never generated"));
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
