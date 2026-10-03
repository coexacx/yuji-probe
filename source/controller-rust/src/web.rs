//! Embedded pages for the Nginx + Rust distribution. No filesystem URL mapping.
use crate::core::*;
use axum::{
    body::Body,
    http::{Request, header},
    response::{IntoResponse, Response},
};
include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));
pub const CSP: &str = "default-src 'none'; script-src 'self'; worker-src 'self'; frame-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' blob:; font-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";
pub fn secure(mut response: Response) -> Response {
    let h = response.headers_mut();
    for (name, value) in [
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        (
            "permissions-policy",
            "camera=(), microphone=(), geolocation=()",
        ),
        ("content-security-policy", CSP),
    ] {
        h.insert(
            header::HeaderName::from_static(name),
            header::HeaderValue::from_static(value),
        );
    }
    h.entry(header::CACHE_CONTROL)
        .or_insert(header::HeaderValue::from_static("no-store"));
    response
}
pub fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub fn host_matches(req: &Request<Body>, origin: &str) -> bool {
    req.headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case(origin.trim_start_matches("https://")))
}
pub fn response(path: &str, head: bool, name: &str) -> Response {
    if path == "/" {
        let html = VIEW.replace("__SITE_NAME__", &escape(name));
        let length = html.len();
        let mut response = (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            if head { String::new() } else { html },
        )
            .into_response();
        response.headers_mut().insert(
            header::CONTENT_LENGTH,
            header::HeaderValue::from_str(&length.to_string()).expect("numeric content length"),
        );
        return response;
    }
    if let Some((_, mime, body)) = ASSETS.iter().find(|(url, _, _)| *url == path) {
        let mut r = Response::new(if head {
            Body::empty()
        } else {
            Body::from(*body)
        });
        r.headers_mut()
            .insert(header::CONTENT_TYPE, header::HeaderValue::from_static(mime));
        r.headers_mut().insert(
            header::CONTENT_LENGTH,
            header::HeaderValue::from_str(&body.len().to_string()).expect("numeric content length"),
        );
        r.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
        if path.starts_with("/assets/download-service-") {
            r.headers_mut().insert(
                header::HeaderName::from_static("service-worker-allowed"),
                header::HeaderValue::from_static("/"),
            );
        }
        return r;
    }
    ApiError::new(404, "页面不存在").into_response()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn site_names_are_escaped_in_text_and_attributes() {
        assert_eq!(escape("<script>\"'&"), "&lt;script&gt;&quot;&#39;&amp;");
    }
    #[tokio::test]
    async fn head_keeps_the_representation_length_without_sending_the_body() {
        for path in ["/", ASSETS[0].0] {
            let get = response(path, false, "羽迹 & probe");
            let expected = get.headers()[header::CONTENT_LENGTH].clone();
            let bytes = axum::body::to_bytes(get.into_body(), usize::MAX)
                .await
                .unwrap();
            assert_eq!(
                expected.to_str().unwrap().parse::<usize>().unwrap(),
                bytes.len()
            );
            let head = response(path, true, "羽迹 & probe");
            assert_eq!(head.headers()[header::CONTENT_LENGTH], expected);
            assert!(
                axum::body::to_bytes(head.into_body(), usize::MAX)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
    }
    #[test]
    fn embedded_assets_are_public_and_bound_to_known_paths() {
        assert!(ASSETS.len() > 200);
        assert!(
            ASSETS
                .iter()
                .all(|(path, _, _)| path.starts_with("/assets/") || *path == "/favicon.svg")
        );
        for path in [
            "/storage/control/auth.json",
            "/app/view.html",
            "/bin/probe-linux-amd64",
            "/assets/../app.key",
            "/source/Cargo.toml",
            "/install",
        ] {
            assert_eq!(response(path, false, "probe").status(), 404);
        }
    }
}
