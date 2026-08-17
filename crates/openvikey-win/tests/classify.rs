use openvikey_win::classify::profile_for_exe;
use openvikey_win::inject::InjectProfile;

#[test]
fn cursor_is_electron() {
    assert_eq!(
        profile_for_exe(r"C:\Users\x\Cursor.exe"),
        InjectProfile::Electron
    );
    assert_eq!(profile_for_exe("cursor.EXE"), InjectProfile::Electron);
}

#[test]
fn notepad_is_win32() {
    assert_eq!(profile_for_exe("notepad.exe"), InjectProfile::Win32);
}
