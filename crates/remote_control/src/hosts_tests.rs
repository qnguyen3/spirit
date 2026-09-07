use super::{AllowedHosts, RemoteEndpoint};

fn loopback() -> AllowedHosts {
    AllowedHosts::loopback(7777)
}

#[test]
fn loopback_names_with_the_configured_port_are_allowed() {
    let hosts = loopback();
    assert!(hosts.allows("127.0.0.1:7777"));
    assert!(hosts.allows("localhost:7777"));
    assert!(hosts.allows("[::1]:7777"));
}

#[test]
fn host_matching_is_case_insensitive_and_ignores_a_trailing_dot() {
    let hosts = loopback();
    assert!(hosts.allows("LOCALHOST:7777"));
    assert!(hosts.allows("localhost.:7777"));
}

#[test]
fn a_different_port_is_rejected() {
    let hosts = loopback();
    assert!(!hosts.allows("127.0.0.1:7778"));
    assert!(!hosts.allows("127.0.0.1"));
}

#[test]
fn port_may_be_omitted_only_on_port_eighty() {
    assert!(AllowedHosts::loopback(80).allows("localhost"));
    assert!(AllowedHosts::loopback(80).allows("localhost:80"));
}

#[test]
fn unknown_hosts_are_rejected() {
    let hosts = loopback();
    assert!(!hosts.allows("evil.example:7777"));
    assert!(!hosts.allows("192.168.1.5:7777"));
    assert!(!hosts.allows(""));
    assert!(!hosts.allows("   "));
}

#[test]
fn lan_extras_are_allowed_once_configured() {
    let hosts = AllowedHosts::with_extra(
        7777,
        vec!["192.168.1.5".to_owned(), "spirit-box.local".to_owned()],
    );
    assert!(hosts.allows("192.168.1.5:7777"));
    assert!(hosts.allows("spirit-box.local:7777"));
    assert!(hosts.allows("SPIRIT-BOX.LOCAL.:7777"));
    assert!(!hosts.allows("192.168.1.6:7777"));
}

#[test]
fn malformed_authorities_are_rejected() {
    let hosts = loopback();
    assert!(!hosts.allows("127.0.0.1:"));
    assert!(!hosts.allows(":7777"));
    assert!(!hosts.allows("127.0.0.1:notaport"));
    assert!(!hosts.allows("[::1"));
}

#[test]
fn origin_must_match_the_host_header() {
    let hosts = loopback();
    assert!(hosts.origin_matches("http://127.0.0.1:7777", "127.0.0.1:7777"));
    assert!(hosts.origin_matches("https://127.0.0.1:7777", "127.0.0.1:7777"));
    assert!(hosts.origin_matches("HTTP://LOCALHOST:7777", "localhost:7777"));
    assert!(!hosts.origin_matches("http://localhost:7777", "127.0.0.1:7777"));
    assert!(!hosts.origin_matches("http://evil.example", "127.0.0.1:7777"));
    assert!(!hosts.origin_matches("null", "127.0.0.1:7777"));
    assert!(!hosts.origin_matches("", "127.0.0.1:7777"));
    assert!(!hosts.origin_matches("http://127.0.0.1:7777/evil", "127.0.0.1:7777"));
}

#[test]
fn origin_is_rejected_when_the_host_itself_is_not_allowed() {
    let hosts = loopback();
    assert!(!hosts.origin_matches("http://evil.example:7777", "evil.example:7777"));
}

#[test]
fn endpoints_render_urls_and_pairing_links() {
    let endpoint = RemoteEndpoint::new("127.0.0.1", 7777);
    assert_eq!(endpoint.url(), "http://127.0.0.1:7777");
    assert_eq!(
        endpoint.pairing_url("abc"),
        "http://127.0.0.1:7777/pair?t=abc"
    );
    assert_eq!(RemoteEndpoint::new("::1", 7777).url(), "http://[::1]:7777");
}
