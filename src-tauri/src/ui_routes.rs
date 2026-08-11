/// Browser-mode (`llm-wiki serve`) HTTP endpoints.
///
/// These routes only answer while UI mode is enabled — in desktop mode they
/// all 404, keeping the external API surface identical to what it was before
/// browser mode existed. Everything here is token-gated regardless of the
/// `allowUnauthenticated` API setting, because the command bridge and app
/// state are write-capable.
use std::convert::Infallible;
use std::path::{Component, Path};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path as AxumPath, Request, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response, Sse};
use serde_json::{json, Map, Value};
use tokio_stream::StreamExt;

use crate::api_server::{self, ApiState, UiMode};
use crate::{cmd_bridge as bridge, events};

const MAX_RAW_FILE_BYTES: usize = 64 * 1024 * 1024;
const EVENTS_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

fn ui_auth_query(request: &Request) -> &str {
    request.uri().query().unwrap_or("")
}

/// Token check for UI endpoints: the serve session token or the configured
/// API token, via `?token=`, `X-LLM-Wiki-Token` or `Bearer`. Unlike the
/// read-only API endpoints there is no unauthenticated escape hatch.
fn ui_authorized(state: &ApiState, request: &Request) -> bool {
    let headers = api_server::lowercased_headers(request.headers());
    api_server::is_token_authorized(&state.app, ui_auth_query(request), &headers)
}

fn not_found() -> Response {
    api_server::error_response(404, "Not found", None)
}

fn unauthorized() -> Response {
    api_server::error_response(401, "Unauthorized", None)
}

/// `GET /api/v1/ui-info` — unauthenticated bootstrap probe so the frontend
/// can confirm it is talking to a serve-mode backend.
pub async fn ui_info() -> Response {
    api_server::json_response(
        200,
        json!({
            "ok": true,
            "ui": api_server::ui_mode().is_some(),
            "version": env!("CARGO_PKG_VERSION"),
        }),
        None,
    )
}

/// `GET /api/v1/events` — SSE stream carrying every app event that desktop
/// mode would deliver through Tauri's webview channel (`agent-event`,
/// `claude-cli:*`, `codex-cli:*`, `file-sync://*`).
pub async fn events_sse(State(state): State<ApiState>, request: Request) -> Response {
    if api_server::ui_mode().is_none() {
        return not_found();
    }
    if !ui_authorized(&state, &request) {
        return unauthorized();
    }
    let receiver = events::subscribe();
    let stream = tokio_stream::wrappers::BroadcastStream::new(receiver).filter_map(|item| {
        match item {
            // Lagged consumers simply miss intermediate events; every stream
            // has a terminal frame carrying full state, so skipping is safe.
            Ok((event, payload)) => Some(Ok::<_, Infallible>(
                axum::response::sse::Event::default()
                    .event(event)
                    .data(payload.to_string()),
            )),
            Err(_) => None,
        }
    });
    let mut response = Sse::new(stream)
        .keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(EVENTS_KEEPALIVE_INTERVAL)
                .text("keepalive"),
        )
        .into_response();
    let origin = crate::cors::origin_from_header_map(request.headers());
    api_server::apply_cors(response.headers_mut(), origin.as_deref(), false);
    response
}

/// `POST /api/v1/cmd/{name}` — the browser-mode replacement for Tauri's
/// `invoke`. The request body is the same argument object the frontend
/// passes to `invoke(name, args)` (camelCase keys), and the JSON result is
/// the same value the command would have returned.
pub async fn cmd_bridge(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
    request: Request,
) -> Response {
    if api_server::ui_mode().is_none() {
        return not_found();
    }
    if !ui_authorized(&state, &request) {
        return unauthorized();
    }
    let body = match api_server::read_body(request.into_body(), api_server::MAX_CHAT_BODY_BYTES).await {
        Ok(body) => body,
        Err(err) => return api_server::error_response(400, &err, None),
    };
    let args: Map<String, Value> = if body.trim().is_empty() {
        Map::new()
    } else {
        match serde_json::from_str::<Value>(&body) {
            Ok(Value::Object(map)) => map,
            _ => {
                return api_server::error_response(400, "Command args must be a JSON object", None)
            }
        }
    };
    let result = bridge::dispatch(&state.app, &name, args).await;
    match result {
        Ok(value) => api_server::json_response(200, value, None),
        Err(message) => api_server::error_response(400, &message, None),
    }
}

/// `GET /api/v1/app-state` — full app-state.json contents (the browser-mode
/// backing store for `@tauri-apps/plugin-store`).
pub async fn get_app_state(State(state): State<ApiState>, request: Request) -> Response {
    if api_server::ui_mode().is_none() {
        return not_found();
    }
    if !ui_authorized(&state, &request) {
        return unauthorized();
    }
    let value = api_server::load_app_state(&state.app).unwrap_or_else(|| json!({}));
    api_server::json_response(200, value, None)
}

/// `PUT /api/v1/app-state` — replace app-state.json with the given object.
/// Written atomically (tmp file + rename) so a crash mid-save can't corrupt
/// the store the desktop app also reads.
pub async fn put_app_state(State(state): State<ApiState>, request: Request) -> Response {
    if api_server::ui_mode().is_none() {
        return not_found();
    }
    if !ui_authorized(&state, &request) {
        return unauthorized();
    }
    let body = match api_server::read_body(request.into_body(), api_server::MAX_CHAT_BODY_BYTES).await {
        Ok(body) => body,
        Err(err) => return api_server::error_response(400, &err, None),
    };
    let value = match serde_json::from_str::<Value>(&body) {
        Ok(value @ Value::Object(_)) => value,
        _ => return api_server::error_response(400, "App state must be a JSON object", None),
    };
    let Some(path) = api_server::app_state_path(&state.app) else {
        return api_server::error_response(500, "App data directory unavailable", None);
    };
    if let Some(parent) = path.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            return api_server::error_response(500, &format!("Failed to create app data dir: {err}"), None);
        }
    }
    let tmp = path.with_extension("json.tmp");
    let serialized = value.to_string();
    if let Err(err) = std::fs::write(&tmp, serialized) {
        return api_server::error_response(500, &format!("Failed to write app state: {err}"), None);
    }
    if let Err(err) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return api_server::error_response(500, &format!("Failed to save app state: {err}"), None);
    }
    api_server::invalidate_config_cache();
    api_server::json_response(200, json!({ "ok": true }), None)
}

/// `GET /api/v1/files/raw?path=<abs>` — browser-mode replacement for
/// `convertFileSrc` / the asset protocol. Reads are whitelisted to files
/// inside known project directories.
pub async fn file_raw(State(state): State<ApiState>, request: Request) -> Response {
    if api_server::ui_mode().is_none() {
        return not_found();
    }
    if !ui_authorized(&state, &request) {
        return unauthorized();
    }
    let params: std::collections::BTreeMap<String, String> = request
        .uri()
        .query()
        .unwrap_or("")
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((percent_decode(key), percent_decode(value)))
        })
        .collect();
    let Some(path) = params.get("path") else {
        return api_server::error_response(400, "path is required", None);
    };
    let path = Path::new(path);
    if !api_server::is_known_project_path(&state.app, path) {
        return api_server::error_response(403, "Path is outside known project directories", None);
    }
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(_) => return not_found(),
    };
    if !metadata.is_file() || metadata.len() > MAX_RAW_FILE_BYTES as u64 {
        return api_server::error_response(404, "Not found", None);
    }
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(_) => return not_found(),
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_str(mime.as_ref())
            .unwrap_or_else(|_| axum::http::HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, max-age=60"),
    );
    response
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `POST /api/v1/http-proxy` — browser-mode replacement for
/// tauri-plugin-http. LLM providers whose CORS policy rejects browser
/// origins (MiniMax, Volcengine Ark, most on-prem gateways) are called
/// server-side instead. The response body is streamed back so SSE-style
/// LLM streaming keeps working.
pub async fn http_proxy(State(state): State<ApiState>, request: Request) -> Response {
    if api_server::ui_mode().is_none() {
        return not_found();
    }
    if !ui_authorized(&state, &request) {
        return unauthorized();
    }
    let body = match api_server::read_body(request.into_body(), api_server::MAX_CHAT_BODY_BYTES).await {
        Ok(body) => body,
        Err(err) => return api_server::error_response(400, &err, None),
    };
    let payload: Value = match serde_json::from_str(&body) {
        Ok(payload) => payload,
        Err(_) => return api_server::error_response(400, "Invalid JSON body", None),
    };
    let Some(url) = payload.get("url").and_then(Value::as_str) else {
        return api_server::error_response(400, "url is required", None);
    };
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return api_server::error_response(400, "Only http(s) URLs can be proxied", None);
    }
    let method = match payload
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("POST")
        .parse::<Method>()
    {
        Ok(method) => method,
        Err(_) => return api_server::error_response(400, "Unsupported method", None),
    };
    let accept_invalid_certs = payload
        .get("dangerAcceptInvalidCerts")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let client = match proxy_client(accept_invalid_certs) {
        Ok(client) => client,
        Err(err) => return api_server::error_response(500, &err, None),
    };
    let mut outbound = client.request(method, url);
    if let Some(headers) = payload.get("headers").and_then(Value::as_object) {
        for (name, value) in headers {
            if is_hop_by_hop(name) || name.eq_ignore_ascii_case("host") {
                continue;
            }
            let Some(value) = value.as_str() else { continue };
            let (Ok(name), Ok(value)) = (
                axum::http::header::HeaderName::from_bytes(name.as_bytes()),
                axum::http::HeaderValue::from_str(value),
            ) else {
                continue;
            };
            outbound = outbound.header(name, value);
        }
    }
    if let Some(body) = payload.get("body").and_then(Value::as_str) {
        outbound = outbound.body(body.to_string());
    }
    let upstream = match outbound.send().await {
        Ok(upstream) => upstream,
        Err(err) => {
            return api_server::error_response(502, &format!("Proxy request failed: {err}"), None)
        }
    };
    let status = StatusCode::from_u16(upstream.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    // Capture content-type before the response is consumed into the byte
    // stream; everything else hop-related is dropped.
    let content_type = upstream
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .cloned();
    let mut response = Response::new(Body::from_stream(
        upstream
            .bytes_stream()
            .map(|chunk| chunk.map_err(std::io::Error::other)),
    ));
    *response.status_mut() = status;
    if let Some(content_type) = content_type {
        response
            .headers_mut()
            .insert(axum::http::header::CONTENT_TYPE, content_type);
    }
    response
}

fn proxy_client(accept_invalid_certs: bool) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(accept_invalid_certs)
        .build()
        .map_err(|err| format!("Failed to build proxy client: {err}"))
}

fn is_hop_by_hop(name: &str) -> bool {
    [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ]
    .iter()
    .any(|hop| name.eq_ignore_ascii_case(hop))
}

/// Static frontend hosting for UI mode. Non-API GET paths resolve inside
/// the bundled `frontend_dir`; extensionless paths fall back to
/// `index.html` so client-side routing works on reload.
pub async fn serve_static(mode: &UiMode, path: &str) -> Response {
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    if Path::new(rel)
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
    {
        return api_server::error_response(403, "Forbidden", None);
    }
    let candidate = mode.frontend_dir.join(rel);
    let (file_path, bytes) = match tokio::fs::read(&candidate).await {
        Ok(bytes) => (candidate, bytes),
        Err(_) => {
            // SPA fallback: only extensionless paths (client-side routes)
            // rewrite to index.html; real asset misses stay 404.
            if rel.contains('.') {
                return not_found();
            }
            let index = mode.frontend_dir.join("index.html");
            match tokio::fs::read(&index).await {
                Ok(bytes) => (index, bytes),
                Err(_) => return not_found(),
            }
        }
    };
    let mime = mime_guess::from_path(&file_path).first_or_octet_stream();
    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_str(mime.as_ref())
            .unwrap_or_else(|_| axum::http::HeaderValue::from_static("application/octet-stream")),
    );
    // index.html must revalidate so deploys show up; hashed assets can be
    // cached briefly.
    let cache = if file_path.ends_with("index.html") {
        "no-cache"
    } else {
        "private, max-age=300"
    };
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static(cache),
    );
    response
}

#[cfg(test)]
mod tests {
    #[test]
    fn hop_by_hop_headers_are_dropped_case_insensitively() {
        assert!(super::is_hop_by_hop("Connection"));
        assert!(super::is_hop_by_hop("transfer-encoding"));
        assert!(!super::is_hop_by_hop("authorization"));
        assert!(!super::is_hop_by_hop("x-api-key"));
    }
}
