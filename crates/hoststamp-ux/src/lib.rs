// SPDX-License-Identifier: FSL-1.1-ALv2

use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

const INDEX_HTML: &str = include_str!("../static/index.html");
const APP_CSS: &str = include_str!("../static/app.css");
const APP_JS: &str = include_str!("../static/app.js");
#[cfg(debug_assertions)]
const DEV_STATIC_DIR_ENV: &str = "HOSTSTAMP_UX_STATIC_DIR";
const CSP_HEADER: &str = concat!(
    "default-src 'none'; base-uri 'none'; connect-src 'self'; form-action 'none'; ",
    "frame-ancestors 'none'; img-src 'self' data:; object-src 'none'; ",
    "script-src 'self'; style-src 'self'",
);

pub async fn index() -> Response {
    static_response(index_page(), "text/html; charset=utf-8", Some(CSP_HEADER))
}

pub async fn stylesheet() -> Response {
    static_response(
        static_file("app.css", APP_CSS),
        "text/css; charset=utf-8",
        None,
    )
}

pub async fn script() -> Response {
    static_response(
        static_file("app.js", APP_JS),
        "text/javascript; charset=utf-8",
        None,
    )
}

fn index_page() -> Result<Cow<'static, str>, &'static str> {
    static_file("index.html", INDEX_HTML)
}

fn static_file(filename: &str, embedded: &'static str) -> Result<Cow<'static, str>, &'static str> {
    let dev_static_dir = dev_static_dir();
    static_file_from_dev_dir(dev_static_dir.as_deref(), filename, embedded)
}

fn static_file_from_dev_dir(
    dev_static_dir: Option<&Path>,
    filename: &str,
    embedded: &'static str,
) -> Result<Cow<'static, str>, &'static str> {
    #[cfg(debug_assertions)]
    if let Some(dir) = dev_static_dir {
        return std::fs::read_to_string(dir.join(filename))
            .map(Cow::Owned)
            .map_err(|_| "failed to read local UX asset");
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = dev_static_dir;
        let _ = filename;
    }

    Ok(Cow::Borrowed(embedded))
}

fn dev_static_dir() -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    {
        std::env::var_os(DEV_STATIC_DIR_ENV).map(PathBuf::from)
    }
    #[cfg(not(debug_assertions))]
    {
        None
    }
}

fn static_response(
    body: Result<Cow<'static, str>, &'static str>,
    content_type: &'static str,
    csp: Option<&'static str>,
) -> Response {
    let mut response = match body {
        Ok(body) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, content_type)],
            body.into_owned(),
        )
            .into_response(),
        Err(message) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            message,
        )
            .into_response(),
    };
    set_security_headers(response.headers_mut(), csp);
    response
}

fn set_security_headers(headers: &mut HeaderMap, csp: Option<&'static str>) {
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
    if let Some(csp) = csp {
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(csp),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_security_policy_allows_only_self_hosted_assets() {
        let mut headers = HeaderMap::new();
        set_security_headers(&mut headers, Some(CSP_HEADER));

        let csp = headers
            .get(header::CONTENT_SECURITY_POLICY)
            .expect("csp")
            .to_str()
            .expect("csp string");

        assert!(!csp.contains("'unsafe-inline'"));
        assert!(!csp.contains("'sha256-"));
        assert!(csp.contains("script-src 'self'"));
        assert!(csp.contains("style-src 'self'"));
    }

    #[test]
    fn embedded_static_files_are_available() {
        let index = static_file_from_dev_dir(None, "index.html", INDEX_HTML).expect("index");
        let css = static_file_from_dev_dir(None, "app.css", APP_CSS).expect("css");
        let js = static_file_from_dev_dir(None, "app.js", APP_JS).expect("js");

        assert!(index.contains("Hoststamp"));
        assert!(index.contains("/assets/app.css"));
        assert!(index.contains("/assets/app.js"));
        assert!(css.contains(":root"));
        assert!(js.contains("const state"));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn dev_static_files_read_from_disk() {
        let dir =
            std::env::temp_dir().join(format!("hoststamp-ux-dev-static-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        std::fs::write(dir.join("app.css"), "body { color: red; }").expect("write css");

        let css = static_file_from_dev_dir(Some(&dir), "app.css", APP_CSS).expect("css");
        std::fs::remove_dir_all(&dir).expect("remove dir");

        assert_eq!(css, "body { color: red; }");
    }
}
