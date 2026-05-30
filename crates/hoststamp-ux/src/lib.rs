// SPDX-License-Identifier: FSL-1.1-ALv2

use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use std::{borrow::Cow, path::Path};

const INDEX_HTML: &str = include_str!("../static/index.html");
#[cfg(debug_assertions)]
const DEV_INDEX_HTML_ENV: &str = "HOSTSTAMP_UX_INDEX_HTML";
const CSP_HEADER: &str = concat!(
    "default-src 'none'; base-uri 'none'; connect-src 'self'; form-action 'none'; ",
    "frame-ancestors 'none'; img-src 'self' data:; object-src 'none'; ",
    "script-src 'self' 'sha256-yT/t4RDWzrHZcAsiRlPVDqR9781wBW/EgMovsmPp/Kw='; ",
    "style-src 'self' 'sha256-IRGhWzfmFUZX7kCvya+dyjsPPydI8oJ5vFJ/XRqDQOA='",
);
#[cfg(debug_assertions)]
const DEV_CSP_HEADER: &str = concat!(
    "default-src 'none'; base-uri 'none'; connect-src 'self'; form-action 'none'; ",
    "frame-ancestors 'none'; img-src 'self' data:; object-src 'none'; ",
    "script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'",
);

pub async fn index() -> Response {
    match index_page() {
        Ok(page) => {
            let mut response = Html(page.html).into_response();
            set_security_headers(response.headers_mut(), page.csp);
            response
        }
        Err(message) => {
            let mut response = (StatusCode::INTERNAL_SERVER_ERROR, message).into_response();
            set_security_headers(response.headers_mut(), CSP_HEADER);
            response
        }
    }
}

struct IndexPage {
    html: Cow<'static, str>,
    csp: &'static str,
}

fn index_page() -> Result<IndexPage, &'static str> {
    #[cfg(debug_assertions)]
    {
        let dev_path = std::env::var_os(DEV_INDEX_HTML_ENV);
        index_page_from_dev_path(dev_path.as_deref().map(Path::new))
    }

    #[cfg(not(debug_assertions))]
    {
        index_page_from_dev_path(None)
    }
}

fn index_page_from_dev_path(dev_path: Option<&Path>) -> Result<IndexPage, &'static str> {
    #[cfg(debug_assertions)]
    if let Some(path) = dev_path {
        return std::fs::read_to_string(path)
            .map(|html| IndexPage {
                html: Cow::Owned(html),
                csp: DEV_CSP_HEADER,
            })
            .map_err(|_| "failed to read local UX asset");
    }
    #[cfg(not(debug_assertions))]
    let _ = dev_path;

    Ok(IndexPage {
        html: Cow::Borrowed(INDEX_HTML),
        csp: CSP_HEADER,
    })
}

fn set_security_headers(headers: &mut HeaderMap, csp: &'static str) {
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
        HeaderValue::from_static(csp),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_security_policy_pins_inline_assets_by_hash() {
        let mut headers = HeaderMap::new();
        set_security_headers(&mut headers, CSP_HEADER);

        let csp = headers
            .get(header::CONTENT_SECURITY_POLICY)
            .expect("csp")
            .to_str()
            .expect("csp string");

        assert!(!csp.contains("'unsafe-inline'"));
        assert!(csp.contains("script-src 'self' 'sha256-"));
        assert!(csp.contains("style-src 'self' 'sha256-"));
    }

    #[test]
    fn embedded_index_uses_strict_csp() {
        let page = index_page_from_dev_path(None).expect("index page");

        assert!(page.html.contains("Hoststamp"));
        assert_eq!(page.csp, CSP_HEADER);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn dev_index_reads_from_disk_with_relaxed_csp() {
        let path = std::env::temp_dir().join(format!(
            "hoststamp-ux-dev-index-{}.html",
            std::process::id()
        ));
        std::fs::write(&path, "<!doctype html><title>Dev Hoststamp</title>").expect("write html");

        let page = index_page_from_dev_path(Some(&path)).expect("index page");
        std::fs::remove_file(&path).expect("remove html");

        assert!(page.html.contains("Dev Hoststamp"));
        assert_eq!(page.csp, DEV_CSP_HEADER);
        assert!(page.csp.contains("'unsafe-inline'"));
    }
}
