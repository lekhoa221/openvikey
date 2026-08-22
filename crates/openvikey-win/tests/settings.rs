use openvikey_core::types::{InputMethod, TonePlacement};
use openvikey_win::settings::{
    SETTINGS_VERSION, SettingsLoadError, SettingsV1, default_settings_path, load_settings,
    mutate_settings, save_settings,
};

fn test_root(name: &str) -> std::path::PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("openvikey-{name}-{}-{unique}", std::process::id()))
}

#[test]
fn missing_settings_start_as_vni_without_registration() {
    let root = test_root("missing-settings");
    let path = default_settings_path(Some(&root));

    let settings = load_settings(&path).unwrap();

    assert_eq!(settings.version, SETTINGS_VERSION);
    assert_eq!(settings.input_method, InputMethod::Vni);
    assert_eq!(settings.tone_placement, TonePlacement::Modern);
    assert!(settings.show_suggestions);
    assert!(settings.show_hotkey_hints);
    assert!(settings.learning_enabled);
    assert!(!settings.allow_terminal);
    assert!(!settings.start_with_windows);
}

#[test]
fn settings_round_trip_through_versioned_file() {
    let root = test_root("round-trip");
    let path = default_settings_path(Some(&root));
    let settings = SettingsV1 {
        input_method: InputMethod::Telex,
        show_suggestions: false,
        show_hotkey_hints: false,
        allow_terminal: true,
        start_with_windows: true,
        ..SettingsV1::default()
    };

    save_settings(&path, &settings).unwrap();

    assert_eq!(load_settings(&path).unwrap(), settings);
}

#[test]
fn unknown_settings_version_is_not_silently_overwritten() {
    let root = test_root("unknown-version");
    let path = default_settings_path(Some(&root));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = br#"{"version":999}"#;
    std::fs::write(&path, original).unwrap();

    let error = load_settings(&path).unwrap_err();
    assert!(matches!(error, SettingsLoadError::UnsupportedVersion(999)));

    let mutation_error =
        mutate_settings(&path, |settings| settings.show_suggestions = false).unwrap_err();
    assert!(matches!(
        mutation_error,
        SettingsLoadError::UnsupportedVersion(999)
    ));
    assert_eq!(std::fs::read(&path).unwrap(), original);
}
