// SPDX-License-Identifier: FSL-1.1-ALv2

use axum::{
    http::{HeaderMap, HeaderValue, header},
    response::{Html, IntoResponse, Response},
};

const INDEX_HTML: &str = include_str!("../static/index.html");
const CSP_HEADER: &str = concat!(
    "default-src 'none'; base-uri 'none'; connect-src 'self'; form-action 'none'; ",
    "frame-ancestors 'none'; img-src 'self' data:; object-src 'none'; ",
    "script-src 'self' 'sha256-yT/t4RDWzrHZcAsiRlPVDqR9781wBW/EgMovsmPp/Kw='; ",
    "style-src 'self' 'sha256-IRGhWzfmFUZX7kCvya+dyjsPPydI8oJ5vFJ/XRqDQOA='",
);

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
        HeaderValue::from_static(CSP_HEADER),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_security_policy_pins_inline_assets_by_hash() {
        let mut headers = HeaderMap::new();
        set_security_headers(&mut headers);

        let csp = headers
            .get(header::CONTENT_SECURITY_POLICY)
            .expect("csp")
            .to_str()
            .expect("csp string");

        assert!(!csp.contains("'unsafe-inline'"));
        assert!(csp.contains("script-src 'self' 'sha256-"));
        assert!(csp.contains("style-src 'self' 'sha256-"));
    }
}
