use super::*;

#[test]
fn middle_truncate_keeps_both_ends() {
    let short = PathBuf::from("/tmp/repo");
    assert_eq!(middle_truncate(&short, 34), "/tmp/repo");

    let long = PathBuf::from("/Users/someone/very/deeply/nested/checkout/of/a/repository");
    let truncated = middle_truncate(&long, 20);
    assert!(truncated.chars().count() <= 20, "{truncated}");
    assert!(truncated.starts_with("/Users/so"), "{truncated}");
    assert!(truncated.ends_with("epository"), "{truncated}");
    assert!(truncated.contains('\u{2026}'), "{truncated}");
}
