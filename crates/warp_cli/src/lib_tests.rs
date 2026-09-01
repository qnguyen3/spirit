use super::*;

#[test]
fn identifies_worker_subcommands() {
    #[cfg(unix)]
    assert!(is_worker_invocation(&terminal_server_subcommand()));
    #[cfg(feature = "plugin_host")]
    assert!(is_worker_invocation("--plugin-host"));
    assert!(!is_worker_invocation("--prompt"));
}

/// Pins that each pair of constants names the same variable under both prefixes. A typo in
/// either half would otherwise go unnoticed until a consumer read the wrong name.
#[test]
fn oz_and_warp_env_var_constants_name_the_same_variables() {
    for (oz_name, warp_name) in [
        (OZ_RUN_ID_ENV, WARP_RUN_ID_ENV),
        (OZ_PARENT_RUN_ID_ENV, WARP_PARENT_RUN_ID_ENV),
        (OZ_CLI_ENV, WARP_CLI_ENV),
        (OZ_HARNESS_ENV, WARP_HARNESS_ENV),
    ] {
        let suffix = oz_name
            .strip_prefix("OZ_")
            .unwrap_or_else(|| panic!("{oz_name} should be OZ_-prefixed"));
        assert_eq!(
            warp_name,
            format!("WARP_{suffix}"),
            "{warp_name} does not correspond to {oz_name}"
        );
    }
}
