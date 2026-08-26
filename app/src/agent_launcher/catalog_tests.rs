use std::collections::HashSet;

use super::agent_catalog;

#[test]
fn catalog_is_non_empty() {
    assert!(!agent_catalog().is_empty());
}

#[test]
fn binaries_are_single_tokens() {
    for def in agent_catalog() {
        assert!(
            !def.binary.contains(char::is_whitespace),
            "binary {:?} for {} must be a single token",
            def.binary,
            def.display_name
        );
        assert!(!def.binary.is_empty());
    }
}

#[test]
fn commands_start_with_binary_name() {
    for def in agent_catalog() {
        let first_token = def
            .command
            .split_whitespace()
            .next()
            .expect("command must not be empty");
        assert_eq!(
            first_token, def.binary,
            "command for {} must start with its binary name",
            def.display_name
        );
    }
}

#[test]
fn display_names_are_unique() {
    let mut seen = HashSet::new();
    for def in agent_catalog() {
        assert!(
            seen.insert(def.display_name),
            "duplicate display name {:?}",
            def.display_name
        );
    }
}

#[test]
fn install_docs_urls_are_https() {
    for def in agent_catalog() {
        assert!(
            def.install_docs_url.starts_with("https://"),
            "install docs URL for {} must be https",
            def.display_name
        );
    }
}
