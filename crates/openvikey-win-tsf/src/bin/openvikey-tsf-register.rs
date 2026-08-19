//! Development helper for explicit per-user TSF registration.

use std::path::PathBuf;

use openvikey_win_tsf::registration::{
    activate_profile_for_session, active_keyboard_profile, deactivate_profile_for_session,
    development_profile, register_server, unregister_server,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let action = args.next().ok_or("expected a TSF profile action")?;
    let dll_path = args.next().map(PathBuf::from);

    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    }
    let result = match action.to_string_lossy().as_ref() {
        "register" => {
            let path = dll_path.ok_or("register requires an absolute DLL path")?;
            register_server(&path)
        }
        "unregister" => unregister_server(),
        "activate" => activate_profile_for_session(),
        "deactivate" => deactivate_profile_for_session(),
        "status" => {
            let profile = active_keyboard_profile()?;
            println!(
                "type={} langid=0x{:04X} clsid={:?} profile={:?} flags=0x{:08X}",
                profile.dwProfileType,
                profile.langid,
                profile.clsid,
                profile.guidProfile,
                profile.dwFlags
            );
            for language_id in [0x0409, 0x042A] {
                match development_profile(language_id) {
                    Ok(profile) => println!(
                        "openvikey langid=0x{:04X} flags=0x{:08X}",
                        profile.langid, profile.dwFlags
                    ),
                    Err(error) => {
                        println!("openvikey langid=0x{language_id:04X} unavailable: {error}");
                    }
                }
            }
            Ok(())
        }
        "cycle20" => {
            let path = dll_path.ok_or("cycle20 requires an absolute DLL path")?;
            for _ in 0..20 {
                register_server(&path)?;
                unregister_server()?;
            }
            Ok(())
        }
        _ => {
            return Err(
                "expected register, unregister, activate, deactivate, status, or cycle20".into(),
            );
        }
    };
    unsafe {
        CoUninitialize();
    }
    result?;
    Ok(())
}
