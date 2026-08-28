use super::*;

#[test]
fn project_names_reject_path_separators_and_dots() {
    assert!(validate_project_name("spirit").is_ok());
    assert!(validate_project_name("my-project.v2").is_ok());
    assert!(validate_project_name("").is_err());
    assert!(validate_project_name("a/b").is_err());
    assert!(validate_project_name("a\\b").is_err());
    assert!(validate_project_name(".").is_err());
    assert!(validate_project_name("..").is_err());
}

#[test]
fn home_prefixed_paths_expand() {
    let home = dirs::home_dir().expect("a home directory");
    assert_eq!(expand_home("~"), home);
    assert_eq!(expand_home("~/code"), home.join("code"));
    assert_eq!(expand_home("/tmp/code"), PathBuf::from("/tmp/code"));
    assert_eq!(expand_home("relative"), PathBuf::from("relative"));
}
