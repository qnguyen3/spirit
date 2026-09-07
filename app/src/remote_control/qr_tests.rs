use remote_control::hosts::RemoteEndpoint;

use super::{pairing_svg, render_svg};

#[test]
fn a_pairing_url_renders_as_an_svg_document() {
    let endpoint = RemoteEndpoint::new("192.168.1.5", 7777);
    let svg = pairing_svg(&endpoint, "abc123").expect("a short URL always encodes");
    assert!(svg.contains("<svg"));
    assert!(svg.contains("</svg>"));
    assert!(svg.contains("#000000"));
    assert!(svg.contains("#ffffff"));
}

#[test]
fn the_rendered_code_is_at_least_the_minimum_size() {
    let svg = render_svg("http://127.0.0.1:7777/pair?t=abc").expect("encodes");
    let width = svg
        .split("width=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .and_then(|value| value.parse::<u32>().ok())
        .expect("the SVG declares a width");
    assert!(width >= 220, "width was {width}");
}

#[test]
fn different_urls_produce_different_codes() {
    let first = render_svg("http://127.0.0.1:7777/pair?t=aaa").expect("encodes");
    let second = render_svg("http://127.0.0.1:7777/pair?t=bbb").expect("encodes");
    assert_ne!(first, second);
}

#[test]
fn a_realistic_pairing_url_still_encodes() {
    let endpoint = RemoteEndpoint::new("192.168.1.200", 7777);
    let token = "a".repeat(43);
    assert!(pairing_svg(&endpoint, &token).is_some());
}

#[test]
fn a_payload_beyond_the_format_capacity_is_refused_rather_than_panicking() {
    assert!(render_svg(&"x".repeat(8000)).is_none());
}
