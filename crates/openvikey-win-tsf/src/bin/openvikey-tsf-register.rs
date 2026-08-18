//! Development helper for explicit per-user TSF registration.

use std::path::PathBuf;

use openvikey_win_tsf::registration::{register_server, unregister_server};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let action = args.next().ok_or("expected register or unregister")?;
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
        _ => return Err("expected register or unregister".into()),
    };
    unsafe {
        CoUninitialize();
    }
    result?;
    Ok(())
}
