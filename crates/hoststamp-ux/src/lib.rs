// SPDX-License-Identifier: FSL-1.1-ALv2

use axum::{
    http::{HeaderMap, HeaderValue, header},
    response::{Html, IntoResponse, Response},
};

const INDEX_HTML: &str = include_str!("../static/index.html");

pub async fn index() -> Response {
    let mut response = Html(INDEX_HTML).into_response();
    set_security_headers(response.headers_mut());
    response
}

fn set_security_headers(headers: &mut HeaderMap) {
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=()"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; base-uri 'none'; connect-src 'self'; form-action 'none'; frame-ancestors 'none'; img-src 'self' data:; object-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'",
        ),
    );
}
