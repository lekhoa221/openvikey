use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn product_startup_has_no_tsf_bridge_or_registration_dependency() {
    let root = workspace_root();
    let main = std::fs::read_to_string(root.join("crates/openvikey-win/src/main.rs")).unwrap();
    let package =
        std::fs::read_to_string(root.join("scripts/build-standalone-preview.ps1")).unwrap();

    assert!(!main.contains("ContextBridgeServer"));
    assert!(!main.contains("openvikey_win_tsf"));
    assert!(!package.contains("openvikey-tsf-register"));
    assert!(!package.contains("openvikey_win_tsf.dll"));
}

#[test]
fn release_product_is_a_windows_gui_subsystem_binary() {
    let main =
        std::fs::read_to_string(workspace_root().join("crates/openvikey-win/src/main.rs")).unwrap();

    assert!(main.contains("windows_subsystem = \"windows\""));
}
