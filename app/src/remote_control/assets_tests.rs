use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};

use super::{serve, serve_document, serve_pairing_page};

fn header(response: &axum::response::Response, name: axum::http::HeaderName) -> String {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn a_known_asset_is_served_with_its_content_type() {
    let response = serve("dev", "styles.css");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(header(&response, CONTENT_TYPE).starts_with("text/css"));
}

#[test]
fn dev_builds_are_never_cached_and_versioned_builds_are_immutable() {
    assert_eq!(
        header(&serve("dev", "styles.css"), CACHE_CONTROL),
        "no-cache"
    );
    assert_eq!(
        header(&serve("v1-2-3", "styles.css"), CACHE_CONTROL),
        "public, max-age=31536000, immutable"
    );
}

#[test]
fn traversal_attempts_are_rejected() {
    for path in [
        "../secret",
        "ui/../../secret",
        "/etc/passwd",
        "..\\secret",
        "",
    ] {
        assert_eq!(
            serve("dev", path).status(),
            StatusCode::NOT_FOUND,
            "{path} must not resolve"
        );
    }
}

#[test]
fn an_unknown_asset_is_a_not_found() {
    assert_eq!(serve("dev", "nope.js").status(), StatusCode::NOT_FOUND);
}

#[test]
fn agent_icons_are_routed_to_the_bundled_directory() {
    assert_eq!(
        serve("dev", "agent_icons/../../secret").status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        serve("dev", "agent_icons/nested/name.png").status(),
        StatusCode::NOT_FOUND
    );
}

#[test]
fn the_index_document_has_its_build_id_substituted() {
    let response = serve_document("v9", "index.html");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, CACHE_CONTROL), "no-store");
    assert!(header(&response, CONTENT_TYPE).starts_with("text/html"));
}

#[test]
fn the_pairing_page_escapes_its_error_message() {
    let response = serve_pairing_page("dev", Some("<script>alert(1)</script>"));
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, CACHE_CONTROL), "no-store");
}
