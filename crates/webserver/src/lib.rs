pub mod config;
#[cfg(unix)]
pub mod local_ipc;
pub mod passphrase;

use axum::http::header;
use axum::response::{Html, IntoResponse, Response};

/// Whether a client said it can take brotli.
///
/// `br;q=0` is a refusal, not an offer — the same header syntax says both.
fn accepts_br(accept_encoding: Option<&str>) -> bool {
    let Some(header) = accept_encoding else {
        return false;
    };
    header.split(',').any(|part| {
        let mut params = part.split(';').map(str::trim);
        if params.next() != Some("br") {
            return false;
        }
        // Any q but zero is an offer: `q=0.1` is "if you must", `q=0` is "no".
        !params.any(|p| {
            p.strip_prefix("q=")
                .and_then(|q| q.trim().parse::<f32>().ok())
                .is_some_and(|q| q <= 0.0)
        })
    })
}

/// Serve brotli-compressed HTML with ETag support. If the client accepts `br`
/// encoding, the raw compressed bytes are sent; otherwise they are decompressed.
/// Returns 304 if the client's `If-None-Match` matches `etag`.
pub fn html_response(
    html_br: &'static [u8],
    etag: &str,
    if_none_match: Option<&[u8]>,
    accept_encoding: Option<&str>,
) -> Response {
    if let Some(inm) = if_none_match
        && inm == etag.as_bytes()
    {
        return (
            axum::http::StatusCode::NOT_MODIFIED,
            [(axum::http::header::ETAG, etag)],
        )
            .into_response();
    }
    if accepts_br(accept_encoding) {
        (
            [
                (header::ETAG, etag.to_owned()),
                (header::CONTENT_ENCODING, "br".to_owned()),
                (header::CONTENT_TYPE, "text/html".to_owned()),
            ],
            html_br,
        )
            .into_response()
    } else {
        let mut decompressed = Vec::new();
        let _ = brotli::BrotliDecompress(&mut std::io::Cursor::new(html_br), &mut decompressed);
        (
            [(header::ETAG, etag.to_owned())],
            Html(String::from_utf8_lossy(&decompressed).into_owned()),
        )
            .into_response()
    }
}

/// The reserved preview path prefix, mirroring `PREVIEW_PREFIX` in
/// js/core/src/preview.ts. Kept here so the edge and the worker cannot
/// disagree about which paths belong to a preview.
pub const PREVIEW_PATH_PREFIX: &str = "/x/";

/// `Service-Worker-Allowed`, which widens a worker's scope from the directory
/// it was served from to the whole origin. Without it a worker at `/sw.js`
/// could only claim `/`-rooted requests by accident of path.
const SERVICE_WORKER_ALLOWED: axum::http::HeaderName =
    axum::http::HeaderName::from_static("service-worker-allowed");

/// Serve a brotli-compressed JavaScript asset, decompressing when the client
/// cannot take `br`.
///
/// Used for the preview service worker (docs/design/net.md § Client: service
/// worker), which needs three things a route for it must not get wrong: a
/// JavaScript content type, `Service-Worker-Allowed: /` so its scope covers
/// the whole origin rather than its own directory, and `no-cache` so a stale
/// worker cannot outlive an upgrade of the app it serves.
pub fn service_worker_response(
    js_br: &'static [u8],
    etag: &str,
    if_none_match: Option<&[u8]>,
    accept_encoding: Option<&str>,
) -> Response {
    if let Some(inm) = if_none_match
        && inm == etag.as_bytes()
    {
        return (
            axum::http::StatusCode::NOT_MODIFIED,
            [
                (axum::http::header::ETAG, etag),
                (SERVICE_WORKER_ALLOWED, "/"),
            ],
        )
            .into_response();
    }
    let common = [
        (header::ETAG, etag.to_owned()),
        (
            header::CONTENT_TYPE,
            "text/javascript; charset=utf-8".to_owned(),
        ),
        (header::CACHE_CONTROL, "no-cache".to_owned()),
        (SERVICE_WORKER_ALLOWED, "/".to_owned()),
    ];
    if accepts_br(accept_encoding) {
        let mut resp = (common, js_br).into_response();
        resp.headers_mut().insert(
            header::CONTENT_ENCODING,
            header::HeaderValue::from_static("br"),
        );
        resp
    } else {
        let mut decompressed = Vec::new();
        let _ = brotli::BrotliDecompress(&mut std::io::Cursor::new(js_br), &mut decompressed);
        (common, decompressed).into_response()
    }
}

/// Answer a reserved preview path when no worker is installed
/// (docs/design/net.md § Reserve the prefix server-side).
///
/// Without this the edge's SPA fallback would render the yas UI inside the
/// preview frame — a failure mode that looks like anything but "the worker did
/// not intercept this".
/// The routes every origin serving the yas UI must answer the same way,
/// before its own WebSocket upgrade and before its SPA fallback: the preview
/// service worker, and the preview path prefix.
///
/// One function rather than a copy per server, because the copies diverged:
/// the edge had them and the local `yas web` server did not, so there
/// `/sw.js` fell through to `index.html` and the browser rejected it on its
/// MIME type. That is not a stray console line — the preview worker never
/// installs, so web previews do not work on that origin at all.
///
/// Returns `None` for paths the caller still owns.
pub fn try_ui_route(
    path: &str,
    sw_js_br: &'static [u8],
    sw_etag: &str,
    if_none_match: Option<&[u8]>,
    accept_encoding: Option<&str>,
) -> Option<Response> {
    if path == "/sw.js" {
        return Some(service_worker_response(
            sw_js_br,
            sw_etag,
            if_none_match,
            accept_encoding,
        ));
    }
    // `/x/…` reaching a SPA fallback renders the yas UI inside a preview
    // frame, which reads as anything but the failure it is.
    if path.starts_with(PREVIEW_PATH_PREFIX) {
        return Some(preview_unavailable_response());
    }
    None
}

/// Serve a bundled worker script by exact path.
///
/// A worker cannot be inlined into the single-file UI, so each one ships as
/// its own asset and needs a route here. Without one it 404s and whatever it
/// carries degrades silently — the transport worker would have kept audio off
/// the main thread in development, where a dev server hands out every file,
/// and quietly fallen back to the main thread in production.
///
/// Names are pinned rather than content-hashed for the same reason the service
/// worker's is: a hashed name cannot be named by `include_bytes!`. Revalidation
/// is by ETag, which is what `service_worker_response` already does.
pub fn try_worker_route(
    path: &str,
    workers: &[(&'static str, &'static [u8], &'static str)],
    if_none_match: Option<&[u8]>,
    accept_encoding: Option<&str>,
) -> Option<Response> {
    for (route, body_br, etag) in workers {
        if path == *route {
            return Some(service_worker_response(
                body_br,
                etag,
                if_none_match,
                accept_encoding,
            ));
        }
    }
    None
}

pub fn preview_unavailable_response() -> Response {
    (
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        "yas preview: the preview service worker is not installed for this \
         origin, so this request reached the server instead of a relayed \
         socket. Open the yas UI in a top-level tab first; previews need a \
         secure context (https, or http on localhost).\n",
    )
        .into_response()
}

/// Compute an ETag string from content bytes.
pub fn html_etag(data: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut h);
    format!("\"yas-{:x}\"", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    // ── html_etag ──

    #[test]
    fn etag_deterministic() {
        let a = html_etag(b"<html>hello</html>");
        let b = html_etag(b"<html>hello</html>");
        assert_eq!(a, b);
    }

    #[test]
    fn etag_different_for_different_content() {
        let a = html_etag(b"aaa");
        let b = html_etag(b"bbb");
        assert_ne!(a, b);
    }

    #[test]
    fn etag_format() {
        let tag = html_etag(b"test");
        assert!(
            tag.starts_with("\"yas-"),
            "expected quoted yas- prefix, got {tag}"
        );
        assert!(tag.ends_with('"'));
    }

    // ── html_response ──

    #[tokio::test]
    async fn html_response_200_without_etag_match() {
        let etag = html_etag(b"hello");
        let resp = html_response(b"hello", &etag, None, None);
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get("etag").unwrap().to_str().unwrap(), etag);
    }

    #[tokio::test]
    async fn html_response_304_with_matching_etag() {
        let etag = html_etag(b"hello");
        let resp = html_response(b"hello", &etag, Some(etag.as_bytes()), None);
        assert_eq!(resp.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn html_response_200_with_mismatched_etag() {
        let etag = html_etag(b"hello");
        let resp = html_response(b"hello", &etag, Some(b"\"wrong\""), None);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn accept_encoding_reading() {
        assert!(accepts_br(Some("br")));
        assert!(accepts_br(Some("gzip, deflate, br")));
        assert!(accepts_br(Some("br;q=0.1")), "a low q is still an offer");
        assert!(!accepts_br(None), "a client that said nothing gets bytes");
        assert!(!accepts_br(Some("gzip, deflate")));
        assert!(
            !accepts_br(Some("br;q=0")),
            "q=0 is a refusal, not an offer"
        );
        assert!(
            !accepts_br(Some("brotli")),
            "the token is `br` — a prefix match would take anything"
        );
    }

    // ── service worker + preview routes ──

    /// Every origin serving the yas UI routes these, and the copies used to
    /// diverge: the edge had them, the local `yas web` server did not, so
    /// `/sw.js` there fell through to `index.html` and the browser rejected it
    /// on its MIME type — which meant no preview worker, so no previews at all
    /// on that origin.
    #[tokio::test]
    async fn ui_routes_cover_the_worker_and_the_preview_prefix() {
        let sw = try_ui_route("/sw.js", b"fake-br", "\"e\"", None, None)
            .expect("/sw.js belongs to every UI origin");
        assert_eq!(sw.status(), 200);
        assert!(
            sw.headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("text/javascript"),
            "a worker served as anything else is refused by the browser"
        );
        assert_eq!(sw.headers().get("service-worker-allowed").unwrap(), "/");

        // A preview path must never reach a SPA fallback.
        for path in [PREVIEW_PATH_PREFIX, "/x/local/http/localhost:3000/"] {
            let resp = try_ui_route(path, b"x", "\"e\"", None, None)
                .unwrap_or_else(|| panic!("{path} belongs to every UI origin"));
            assert_eq!(resp.status(), axum::http::StatusCode::SERVICE_UNAVAILABLE);
        }

        // Conditional requests still work through the shared route.
        let cached = try_ui_route("/sw.js", b"x", "\"e\"", Some(b"\"e\""), None).unwrap();
        assert_eq!(cached.status(), axum::http::StatusCode::NOT_MODIFIED);

        // Everything else stays the caller's.
        for path in ["/", "/index.html", "/mux", "/config", "/xylophone"] {
            assert!(
                try_ui_route(path, b"x", "\"e\"", None, None).is_none(),
                "{path} is not a shared route"
            );
        }
    }

    #[tokio::test]
    async fn service_worker_declares_root_scope_and_js_type() {
        // All three matter: a worker served without root scope cannot claim
        // `/`, one served as text/plain is rejected outright, and a cached one
        // outlives the app it serves.
        let resp = service_worker_response(b"fake-br", "\"e\"", None, None);
        assert_eq!(resp.status(), 200);
        let headers = resp.headers();
        assert_eq!(headers.get("service-worker-allowed").unwrap(), "/");
        assert!(
            headers
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("text/javascript")
        );
        assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-cache");
    }

    #[tokio::test]
    async fn service_worker_sends_br_only_when_accepted() {
        let plain = service_worker_response(b"x", "\"e\"", None, None);
        assert!(plain.headers().get(header::CONTENT_ENCODING).is_none());
        let compressed = service_worker_response(b"x", "\"e\"", None, Some("gzip, br"));
        assert_eq!(
            compressed.headers().get(header::CONTENT_ENCODING).unwrap(),
            "br"
        );
    }

    #[tokio::test]
    async fn service_worker_304_keeps_the_scope_header() {
        // A revalidated worker still needs its scope declared, or the browser
        // narrows it on re-registration.
        let resp = service_worker_response(b"x", "\"e\"", Some(b"\"e\""), None);
        assert_eq!(resp.status(), 304);
        assert_eq!(resp.headers().get("service-worker-allowed").unwrap(), "/");
    }

    #[tokio::test]
    async fn preview_miss_is_a_legible_503_not_the_app() {
        let resp = preview_unavailable_response();
        assert_eq!(resp.status(), 503);
        assert!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("text/plain")
        );
    }

    #[test]
    fn preview_prefix_matches_the_client_constant() {
        // js/core/src/preview.ts exports the same string; they must agree or
        // the edge reserves a path the worker never claims.
        assert_eq!(PREVIEW_PATH_PREFIX, "/x/");
    }
}
