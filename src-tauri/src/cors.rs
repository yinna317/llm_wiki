/// Shared CORS policy for the local HTTP servers.
///
/// Two server stacks consume this module:
///   - the legacy clip server (tiny_http) — `local_cors_headers` /
///     `request_origin` keep their tiny_http signatures;
///   - the axum API/UI server — `cors_header_pairs` /
///     `origin_from_header_map` work on `http` crate types.
///
/// The policy itself (which origins may talk to us) lives in
/// `is_allowed_browser_origin` and is deliberately shared so both servers
/// expose exactly the same surface.
use axum::http::HeaderMap;

const CORS_ALLOW_METHODS: &str = "GET, POST, PATCH, OPTIONS";

pub fn request_origin(request: &tiny_http::Request) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv("Origin"))
        .map(|header| header.value.as_str().to_string())
}

pub fn origin_from_header_map(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

pub fn is_allowed_browser_origin(origin: &str) -> bool {
    origin.starts_with("chrome-extension://")
        || origin.starts_with("moz-extension://")
        || origin == "http://localhost"
        || origin.starts_with("http://localhost:")
        || origin == "http://127.0.0.1"
        || origin.starts_with("http://127.0.0.1:")
        || origin == "http://[::1]"
        || origin.starts_with("http://[::1]:")
        || origin == "tauri://localhost"
        || origin == "http://tauri.localhost"
        || origin == "https://tauri.localhost"
}

/// Header name/value pairs implementing the CORS policy. Transport-agnostic;
/// each server converts them to its own header type.
pub fn cors_header_pairs(origin: Option<&str>, allow_headers: &str) -> Vec<(String, String)> {
    let mut headers = vec![
        (
            "Access-Control-Allow-Methods".to_string(),
            CORS_ALLOW_METHODS.to_string(),
        ),
        (
            "Access-Control-Allow-Headers".to_string(),
            allow_headers.to_string(),
        ),
        ("Content-Type".to_string(), "application/json".to_string()),
    ];
    if let Some(origin) = origin.filter(|origin| is_allowed_browser_origin(origin)) {
        headers.push((
            "Access-Control-Allow-Origin".to_string(),
            origin.to_string(),
        ));
        headers.push(("Vary".to_string(), "Origin".to_string()));
        headers.push((
            "Access-Control-Allow-Private-Network".to_string(),
            "true".to_string(),
        ));
    }
    headers
}

pub fn local_cors_headers(origin: Option<&str>, allow_headers: &str) -> Vec<tiny_http::Header> {
    cors_header_pairs(origin, allow_headers)
        .into_iter()
        .filter_map(|(name, value)| tiny_http::Header::from_bytes(name, value).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_value(headers: &[tiny_http::Header], name: &str) -> Option<String> {
        headers
            .iter()
            .find(|header| header.field.as_str().to_string().eq_ignore_ascii_case(name))
            .map(|header| header.value.as_str().to_string())
    }

    #[test]
    fn allowed_browser_origins_are_narrowly_scoped() {
        for origin in [
            "chrome-extension://abc",
            "moz-extension://abc",
            "http://localhost",
            "http://localhost:19827",
            "http://127.0.0.1:5500",
            "http://[::1]:3000",
            "tauri://localhost",
            "http://tauri.localhost",
            "https://tauri.localhost",
        ] {
            assert!(is_allowed_browser_origin(origin), "{origin}");
        }

        for origin in [
            "",
            "HTTP://LOCALHOST",
            "http://localhost.evil.com",
            "http://127.0.0.1.evil.com",
            "https://localhost",
            "http://evil.com",
            "https://evil.com",
        ] {
            assert!(!is_allowed_browser_origin(origin), "{origin}");
        }
    }

    #[test]
    fn cors_headers_reflect_allowed_origin_only() {
        let allowed = local_cors_headers(Some("chrome-extension://abc"), "Content-Type");
        assert_eq!(
            header_value(&allowed, "Access-Control-Allow-Origin").as_deref(),
            Some("chrome-extension://abc")
        );
        assert_eq!(
            header_value(&allowed, "Access-Control-Allow-Private-Network").as_deref(),
            Some("true")
        );
        assert_eq!(
            header_value(&allowed, "Access-Control-Allow-Methods").as_deref(),
            Some("GET, POST, PATCH, OPTIONS")
        );
        assert_eq!(
            header_value(&allowed, "Access-Control-Allow-Headers").as_deref(),
            Some("Content-Type")
        );
        assert_eq!(header_value(&allowed, "Vary").as_deref(), Some("Origin"));

        let denied = local_cors_headers(Some("https://evil.com"), "Content-Type");
        assert!(header_value(&denied, "Access-Control-Allow-Origin").is_none());
        assert!(header_value(&denied, "Access-Control-Allow-Private-Network").is_none());
        assert!(header_value(&denied, "Vary").is_none());

        let missing = local_cors_headers(None, "Content-Type");
        assert!(header_value(&missing, "Access-Control-Allow-Origin").is_none());
    }

    #[test]
    fn header_map_origin_is_read_case_insensitively() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::ORIGIN,
            "http://localhost:1420".parse().unwrap(),
        );
        assert_eq!(
            origin_from_header_map(&headers).as_deref(),
            Some("http://localhost:1420")
        );
        assert_eq!(origin_from_header_map(&HeaderMap::new()), None);
    }
}
