use std::path::PathBuf;

#[test]
fn injected_data_dir_is_used_once_before_initialization() {
    let path =
        std::env::temp_dir().join(format!("streamtube-data-dir-test-{}", std::process::id()));
    assert!(streamtube_core::store::set_data_dir(path.clone()));
    assert_eq!(streamtube_core::store::data_dir(), PathBuf::from(&path));
    assert!(!streamtube_core::store::set_data_dir(path.join("other")));
    let _ = std::fs::remove_dir_all(path);
}
