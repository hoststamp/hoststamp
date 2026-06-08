// SPDX-License-Identifier: FSL-1.1-ALv2

use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
#[cfg(debug_assertions)]
use std::time::UNIX_EPOCH;
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

const INDEX_HTML: &str = include_str!("../static/index.html");
const APP_CSS: &str = include_str!("../static/app.css");
const APP_JS: &str = include_str!("../static/app.js");
const PROFILE_HEALTH_JS: &str = include_str!("../static/profile-health.js");
#[cfg(debug_assertions)]
const DEV_RELOAD_JS: &str = include_str!("../static/dev-reload.js");
#[cfg(debug_assertions)]
const DEV_STATIC_DIR_ENV: &str = "HOSTSTAMP_UX_STATIC_DIR";
const CSP_HEADER: &str = concat!(
    "default-src 'none'; base-uri 'none'; connect-src 'self'; form-action 'none'; ",
    "frame-ancestors 'none'; img-src 'self' data:; object-src 'none'; ",
    "script-src 'self'; style-src 'self'",
);
#[cfg(debug_assertions)]
const DEV_RELOAD_TAG: &str = r#"    <script src="/assets/dev-reload.js"></script>
"#;

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

pub async fn profile_health_script() -> Response {
    static_response(
        static_file("profile-health.js", PROFILE_HEALTH_JS),
        "text/javascript; charset=utf-8",
        None,
    )
}

#[cfg(debug_assertions)]
pub async fn dev_reload_script() -> Response {
    static_response(
        dev_only_file("dev-reload.js", DEV_RELOAD_JS),
        "text/javascript; charset=utf-8",
        None,
    )
}

#[cfg(debug_assertions)]
pub async fn dev_version() -> Response {
    static_response(dev_static_version(), "text/plain; charset=utf-8", None)
}

fn index_page() -> Result<Cow<'static, str>, &'static str> {
    let page = static_file("index.html", INDEX_HTML)?;
    let dev_static_dir = dev_static_dir();
    maybe_inject_dev_reload(page, dev_static_dir.as_deref())
}

fn static_file(filename: &str, embedded: &'static str) -> Result<Cow<'static, str>, &'static str> {
    let dev_static_dir = dev_static_dir();
    static_file_from_dev_dir(dev_static_dir.as_deref(), filename, embedded)
}

#[cfg(debug_assertions)]
fn dev_only_file(
    filename: &str,
    embedded: &'static str,
) -> Result<Cow<'static, str>, &'static str> {
    let Some(dev_static_dir) = dev_static_dir() else {
        return Err("dev asset is only available when local UX static serving is enabled");
    };
    static_file_from_dev_dir(Some(&dev_static_dir), filename, embedded)
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

fn maybe_inject_dev_reload(
    page: Cow<'static, str>,
    dev_static_dir: Option<&Path>,
) -> Result<Cow<'static, str>, &'static str> {
    #[cfg(debug_assertions)]
    {
        if dev_static_dir.is_none() {
            return Ok(page);
        }

        let page = page.into_owned();
        let Some(index) = page.rfind("</body>") else {
            return Err("local UX HTML is missing </body>");
        };
        let mut output = String::with_capacity(page.len() + DEV_RELOAD_TAG.len());
        output.push_str(&page[..index]);
        output.push_str(DEV_RELOAD_TAG);
        output.push_str(&page[index..]);
        Ok(Cow::Owned(output))
    }

    #[cfg(not(debug_assertions))]
    {
        let _ = dev_static_dir;
        Ok(page)
    }
}

#[cfg(debug_assertions)]
fn dev_static_version() -> Result<Cow<'static, str>, &'static str> {
    let Some(dev_static_dir) = dev_static_dir() else {
        return Err("dev asset version is only available when local UX static serving is enabled");
    };
    dev_static_version_from_dir(&dev_static_dir)
}

#[cfg(debug_assertions)]
fn dev_static_version_from_dir(dev_static_dir: &Path) -> Result<Cow<'static, str>, &'static str> {
    let version = [
        "index.html",
        "app.css",
        "profile-health.js",
        "app.js",
        "dev-reload.js",
    ]
    .into_iter()
    .map(|filename| asset_mtime_ms(&dev_static_dir.join(filename)))
    .collect::<Result<Vec<_>, _>>()?
    .into_iter()
    .map(|mtime| mtime.to_string())
    .collect::<Vec<_>>()
    .join(".");

    Ok(Cow::Owned(version))
}

#[cfg(debug_assertions)]
fn asset_mtime_ms(path: &Path) -> Result<u128, &'static str> {
    let modified = std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|_| "failed to read local UX asset metadata")?;
    modified
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|_| "local UX asset metadata is before unix epoch")
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
        let health_js =
            static_file_from_dev_dir(None, "profile-health.js", PROFILE_HEALTH_JS).expect("health");
        let js = static_file_from_dev_dir(None, "app.js", APP_JS).expect("js");
        let reload =
            static_file_from_dev_dir(None, "dev-reload.js", DEV_RELOAD_JS).expect("reload");

        assert!(index.contains("Hoststamp"));
        assert!(index.contains("/assets/app.css"));
        assert!(index.contains("/assets/profile-health.js"));
        assert!(index.contains("/assets/app.js"));
        assert!(index.contains("event-detail"));
        assert!(index.contains("reset-events"));
        assert!(index.contains("profile-health"));
        assert!(index.contains("refresh-profile-health"));
        assert!(css.contains(":root"));
        assert!(css.contains(".event-detail"));
        assert!(css.contains(".health-list"));
        assert!(health_js.contains("HoststampProfileHealth"));
        assert!(health_js.contains("profileHealthWarnings"));
        assert!(js.contains("const state"));
        assert!(js.contains("renderEventDetail"));
        assert!(js.contains("renderProfileHealth"));
        assert!(reload.contains("checkForUpdate"));
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

    #[cfg(debug_assertions)]
    #[test]
    fn dev_reload_script_is_injected_only_when_static_dir_is_enabled() {
        let embedded = Cow::Borrowed("<html><body>Hoststamp</body></html>");
        assert!(
            !maybe_inject_dev_reload(embedded.clone(), None)
                .expect("html")
                .contains("/assets/dev-reload.js")
        );

        let dir = std::env::temp_dir().join(format!("hoststamp-ux-reload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");

        let html = maybe_inject_dev_reload(embedded, Some(&dir)).expect("html");
        std::fs::remove_dir_all(&dir).expect("remove dir");

        assert!(html.contains("/assets/dev-reload.js"));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn dev_static_version_includes_all_assets() {
        let dir = std::env::temp_dir().join(format!("hoststamp-ux-version-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        for filename in [
            "index.html",
            "app.css",
            "profile-health.js",
            "app.js",
            "dev-reload.js",
        ] {
            std::fs::write(dir.join(filename), filename).expect("write asset");
        }

        let version = dev_static_version_from_dir(&dir).expect("version");
        std::fs::remove_dir_all(&dir).expect("remove dir");

        assert_eq!(version.split('.').count(), 5);
    }
}
