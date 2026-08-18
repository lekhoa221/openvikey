//! Source greps: hook/mouse path must stay free of blocking lock/sleep/serialize/recv.

use std::fs;
use std::path::Path;

fn assert_forbidden(path: &str, src: &str) {
    for needle in ["lock(", "sleep", "model_payload", "to_payload", "recv("] {
        assert!(
            !src.contains(needle),
            "{path} must not contain `{needle}`"
        );
    }
}

#[test]
fn hook_and_mouse_forbid_blocking_and_serialize() {
    for rel in ["src/hook.rs", "src/mouse.rs", "src/ll.rs"] {
        let path = Path::new(rel);
        if !path.exists() {
            continue;
        }
        let src = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        assert_forbidden(rel, &src);
    }
    assert!(
        Path::new("src/hook.rs").exists(),
        "src/hook.rs must exist for this task"
    );
}

#[test]
fn try_lock_lives_in_host_not_hook() {
    let hook = fs::read_to_string("src/hook.rs").expect("src/hook.rs");
    assert!(
        !hook.contains("try_lock"),
        "try_lock must live in host.rs, not hook.rs"
    );
    let mouse = fs::read_to_string("src/mouse.rs").expect("src/mouse.rs");
    assert!(
        !mouse.contains("try_lock"),
        "try_lock must live in host.rs, not mouse.rs"
    );
    let ll = fs::read_to_string("src/ll.rs").expect("src/ll.rs");
    assert!(
        !ll.contains("try_lock"),
        "try_lock must live in host.rs, not ll.rs"
    );
    let host = fs::read_to_string("src/host.rs").expect("src/host.rs");
    assert!(
        host.contains("try_lock"),
        "host.rs must own try_lock for the LL session path"
    );
}

#[test]
fn hook_has_no_key_mpsc() {
    let hook = fs::read_to_string("src/hook.rs").expect("src/hook.rs");
    assert!(!hook.contains("mpsc"), "no key mpsc in hook.rs");
    assert!(!hook.contains("std::sync::mpsc"), "no key mpsc in hook.rs");
}

#[test]
fn main_has_no_unsafe() {
    let main = fs::read_to_string("src/main.rs").expect("src/main.rs");
    assert!(
        !main.contains("unsafe"),
        "main.rs must not contain unsafe (allowlisted modules only)"
    );
}

#[test]
fn live_keyboard_path_uses_one_lock_helper() {
    let hook = fs::read_to_string("src/hook.rs").expect("src/hook.rs");
    assert!(
        hook.contains("handle_runtime_key("),
        "live keyboard callback must call the one-lock host helper"
    );
    assert!(
        !hook.contains("sync_runtime_locked"),
        "focus+key must share one try_lock; hook.rs must not call sync_runtime_locked"
    );
    let host = fs::read_to_string("src/host.rs").expect("src/host.rs");
    assert!(
        host.contains("pub fn handle_runtime_key("),
        "one-lock helper handle_runtime_key must exist in host.rs"
    );
}
