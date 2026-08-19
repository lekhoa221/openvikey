//! Serialized command boundary between native controls and the live product runtime.

use std::path::PathBuf;

use openvikey_core::model::ModelInspectionRow;
use openvikey_core::types::{InputMethod, TonePlacement};

use crate::policy::Mode;
use crate::settings::{AppPolicyV1, HotkeySettingsV1, StartupMode};

#[derive(Debug, Clone)]
pub struct GeneralSettingsDraft {
    pub mode: Mode,
    pub input_method: InputMethod,
    pub tone_placement: TonePlacement,
    pub mode_on_start: StartupMode,
    pub show_suggestions: bool,
    pub allow_terminal: bool,
    pub start_with_windows: bool,
}

#[derive(Debug, Clone)]
pub enum UiCommand {
    ApplyGeneral(GeneralSettingsDraft),
    SetAppPolicies(Vec<AppPolicyV1>),
    SetHotkeys(HotkeySettingsV1),
    ForgetRule(ModelInspectionRow),
    ForgetLastRule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiCommandOutcome {
    Applied,
    Changed(bool),
}

fn settings_path() -> PathBuf {
    crate::settings::default_settings_path(std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
}

/// Execute one UI command on the UI/message thread and report any durable-setting failure.
pub fn execute(command: UiCommand) -> Result<UiCommandOutcome, String> {
    match command {
        UiCommand::ApplyGeneral(draft) => {
            let path = settings_path();
            let mut settings =
                crate::settings::load_settings(&path).map_err(|error| error.to_string())?;
            let at_ms = now_ms();
            crate::host::set_mode_runtime(draft.mode, at_ms);
            crate::host::set_input_method_runtime(draft.input_method, at_ms);
            crate::host::set_tone_placement_runtime(draft.tone_placement, at_ms);
            crate::host::set_show_suggestions_runtime(draft.show_suggestions);
            crate::host::set_allow_terminal_runtime(draft.allow_terminal);
            crate::startup::set_enabled(draft.start_with_windows)
                .map_err(|error| error.to_string())?;
            settings.input_method = draft.input_method;
            settings.tone_placement = draft.tone_placement;
            settings.mode_on_start = draft.mode_on_start;
            settings.last_mode_viet = draft.mode == Mode::Viet;
            settings.show_suggestions = draft.show_suggestions;
            settings.allow_terminal = draft.allow_terminal;
            settings.start_with_windows = draft.start_with_windows;
            crate::settings::save_settings(&path, &settings).map_err(|error| error.to_string())?;
            Ok(UiCommandOutcome::Applied)
        }
        UiCommand::SetAppPolicies(policies) => {
            let path = settings_path();
            crate::settings::mutate_settings(&path, |settings| {
                settings.app_policies.clone_from(&policies);
            })
            .map_err(|error| error.to_string())?;
            crate::host::set_app_policies_runtime(policies);
            Ok(UiCommandOutcome::Applied)
        }
        UiCommand::SetHotkeys(hotkeys) => {
            crate::host::set_hotkeys_runtime(hotkeys)?;
            Ok(UiCommandOutcome::Applied)
        }
        UiCommand::ForgetRule(row) => Ok(UiCommandOutcome::Changed(
            crate::host::forget_rule_runtime(&row),
        )),
        UiCommand::ForgetLastRule => Ok(UiCommandOutcome::Changed(
            crate::host::forget_last_rule_runtime(),
        )),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(0)
        })
}
