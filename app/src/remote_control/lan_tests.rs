use remote_control::hosts::RemoteEndpoint;

use super::{allowed_host_names, lan_endpoints};

#[test]
fn lan_access_off_allows_no_extra_hosts() {
    let endpoints = vec![RemoteEndpoint::new("192.168.1.5", 7777)];
    assert!(allowed_host_names(&endpoints, false).is_empty());
}

#[test]
fn lan_access_on_allows_every_discovered_host_once() {
    let endpoints = vec![
        RemoteEndpoint::new("192.168.1.5", 7777),
        RemoteEndpoint::new("192.168.1.5", 7777),
        RemoteEndpoint::new("spirit.local", 7777),
    ];
    assert_eq!(
        allowed_host_names(&endpoints, true),
        vec!["192.168.1.5".to_owned(), "spirit.local".to_owned()]
    );
}

#[test]
fn discovered_endpoints_all_carry_the_requested_port() {
    for endpoint in lan_endpoints(7777) {
        assert_eq!(endpoint.port, 7777);
        assert!(!endpoint.host.is_empty());
    }
}

#[test]
fn discovery_never_returns_loopback() {
    for endpoint in lan_endpoints(7777) {
        assert_ne!(endpoint.host, "127.0.0.1");
        assert_ne!(endpoint.host, "0.0.0.0");
    }
}
