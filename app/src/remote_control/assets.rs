use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use warp_assets::Assets;

use super::http::{content_type_for, text_response};

const WEB_ROOT: &str = "web/remote_control/";
const BUNDLED_ROOT: &str = "bundled/";
const AGENT_ICON_PREFIX: &str = "agent_icons/";
const BUILD_ID_PLACEHOLDER: &str = "{{BUILD_ID}}";
const PAIR_ERROR_PLACEHOLDER: &str = "{{PAIR_ERROR}}";

fn read(path: &str) -> Option<Vec<u8>> {
    Assets::get(path).map(|file| file.data.into_owned())
}

fn is_safe(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.starts_with('\\')
        && !path.split(['/', '\\']).any(|segment| segment == "..")
        && !path.contains('\0')
}

fn embedded_path(path: &str) -> Option<String> {
    if !is_safe(path) {
        return None;
    }
    match path.strip_prefix(AGENT_ICON_PREFIX) {
        Some(name) if is_safe(name) && !name.contains('/') => {
            Some(format!("{BUNDLED_ROOT}png/agent_icons/{name}"))
        }
        Some(_) => None,
        None => Some(format!("{WEB_ROOT}{path}")),
    }
}

fn cache_control_for(build_id: &str) -> &'static str {
    if build_id == "dev" {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    }
}

pub(crate) fn serve(build_id: &str, path: &str) -> Response {
    let Some(embedded) = embedded_path(path) else {
        return not_found();
    };
    let Some(body) = read(&embedded) else {
        return not_found();
    };
    text_response(
        StatusCode::OK,
        content_type_for(path),
        cache_control_for(build_id),
        body,
    )
}

pub(crate) fn serve_document(build_id: &str, name: &str) -> Response {
    let Some(body) = read(&format!("{WEB_ROOT}{name}")) else {
        return not_found();
    };
    let rendered = String::from_utf8_lossy(&body).replace(BUILD_ID_PLACEHOLDER, build_id);
    text_response(
        StatusCode::OK,
        HeaderValue::from_static("text/html; charset=utf-8"),
        "no-store",
        rendered.into_bytes(),
    )
}

pub(crate) fn serve_pairing_page(build_id: &str, error: Option<&str>) -> Response {
    let Some(body) = read(&format!("{WEB_ROOT}pair.html")) else {
        return not_found();
    };
    let rendered = String::from_utf8_lossy(&body)
        .replace(BUILD_ID_PLACEHOLDER, build_id)
        .replace(PAIR_ERROR_PLACEHOLDER, &escape_html(error.unwrap_or("")));
    text_response(
        StatusCode::OK,
        HeaderValue::from_static("text/html; charset=utf-8"),
        "no-store",
        rendered.into_bytes(),
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn not_found() -> Response {
    text_response(
        StatusCode::NOT_FOUND,
        HeaderValue::from_static("text/plain; charset=utf-8"),
        "no-store",
        b"not found".to_vec(),
    )
}

#[cfg(test)]
#[path = "assets_tests.rs"]
mod tests;
