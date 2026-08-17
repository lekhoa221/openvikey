//! Hot-path modules must not log keystrokes via println/eprintln.

#[test]
fn no_println_on_hot_path() {
    for path in ["src/hook.rs", "src/inject.rs", "src/host.rs"] {
        let src = std::fs::read_to_string(path).unwrap();
        assert!(!src.contains("println!"), "{path}");
        assert!(!src.contains("eprintln!"), "{path}");
    }
}
