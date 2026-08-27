use super::plugin_chip_key;

#[test]
fn chip_key_is_bare_agent_prefix_for_local_sessions() {
    assert_eq!(plugin_chip_key("claude", &None), "claude");
}

#[test]
fn chip_key_is_scoped_per_host_for_remote_sessions() {
    assert_eq!(
        plugin_chip_key("claude", &Some("user@example.com".to_owned())),
        "claude@user@example.com"
    );
}

#[test]
fn chip_keys_for_different_hosts_do_not_collide() {
    let first = plugin_chip_key("codex", &Some("host-a".to_owned()));
    let second = plugin_chip_key("codex", &Some("host-b".to_owned()));
    assert_ne!(first, second);
    assert_ne!(first, plugin_chip_key("codex", &None));
}
