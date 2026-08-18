//! Explicit per-user COM and TSF registration.

use std::path::Path;

use windows::Win32::Foundation::E_INVALIDARG;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, CLSID_TF_InputProcessorProfiles, GUID_TFCAT_TIP_KEYBOARD, ITfCategoryMgr,
    ITfInputProcessorProfileMgr,
};
use windows::core::{Error, IUnknown, Result};
use windows_registry::CURRENT_USER;

use crate::{CLSID_OPENVIKEY_TSF, GUID_OPENVIKEY_PROFILE};

const COM_CLASS_KEY: &str = r"Software\Classes\CLSID\{741B179E-BF99-4EA2-BDDF-B14AD06E7A60}";
const INPROC_KEY: &str =
    r"Software\Classes\CLSID\{741B179E-BF99-4EA2-BDDF-B14AD06E7A60}\InprocServer32";
const VIETNAMESE_LANG_ID: u16 = 0x042A;

fn at_step(error: &Error, step: &str) -> Error {
    Error::new(error.code(), format!("{step}: {error}"))
}

/// Register the DLL for the current user and add its Vietnamese TSF profile.
pub fn register_server(dll_path: &Path) -> Result<()> {
    if !dll_path.is_absolute() {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    let path = dll_path
        .to_str()
        .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;

    let inproc = CURRENT_USER
        .create(INPROC_KEY)
        .map_err(|error| at_step(&error, "create per-user COM key"))?;
    inproc
        .set_string("", path)
        .map_err(|error| at_step(&error, "write InprocServer32 path"))?;
    inproc
        .set_string("ThreadingModel", "Apartment")
        .map_err(|error| at_step(&error, "write COM threading model"))?;

    let registration_result = unsafe { register_tsf_profile() };
    if let Err(error) = registration_result {
        let _ = unregister_server();
        return Err(error);
    }
    Ok(())
}

/// Remove the current-user TSF profile and its exact COM class subtree.
pub fn unregister_server() -> Result<()> {
    unsafe {
        if let Ok(categories) = CoCreateInstance::<_, ITfCategoryMgr>(
            &CLSID_TF_CategoryMgr,
            None::<&IUnknown>,
            CLSCTX_INPROC_SERVER,
        ) {
            let _ = categories.UnregisterCategory(
                &CLSID_OPENVIKEY_TSF,
                &GUID_TFCAT_TIP_KEYBOARD,
                &CLSID_OPENVIKEY_TSF,
            );
        }
        if let Ok(profiles) = CoCreateInstance::<_, ITfInputProcessorProfileMgr>(
            &CLSID_TF_InputProcessorProfiles,
            None::<&IUnknown>,
            CLSCTX_INPROC_SERVER,
        ) {
            let _ = profiles.UnregisterProfile(
                &CLSID_OPENVIKEY_TSF,
                VIETNAMESE_LANG_ID,
                &GUID_OPENVIKEY_PROFILE,
                0,
            );
        }
    }

    if CURRENT_USER.open(COM_CLASS_KEY).is_ok() {
        CURRENT_USER.remove_tree(COM_CLASS_KEY)?;
    }
    Ok(())
}

unsafe fn register_tsf_profile() -> Result<()> {
    let profiles: ITfInputProcessorProfileMgr = unsafe {
        CoCreateInstance(
            &CLSID_TF_InputProcessorProfiles,
            None::<&IUnknown>,
            CLSCTX_INPROC_SERVER,
        )?
    };
    let description: Vec<u16> = "OpenViKey read-only context".encode_utf16().collect();
    unsafe {
        profiles
            .RegisterProfile(
                &CLSID_OPENVIKEY_TSF,
                VIETNAMESE_LANG_ID,
                &GUID_OPENVIKEY_PROFILE,
                &description,
                &[],
                0,
                HKL::default(),
                0,
                false,
                0,
            )
            .map_err(|error| at_step(&error, "register Vietnamese TSF profile"))?;
    }

    let categories: ITfCategoryMgr = unsafe {
        CoCreateInstance(
            &CLSID_TF_CategoryMgr,
            None::<&IUnknown>,
            CLSCTX_INPROC_SERVER,
        )?
    };
    unsafe {
        categories
            .RegisterCategory(
                &CLSID_OPENVIKEY_TSF,
                &GUID_TFCAT_TIP_KEYBOARD,
                &CLSID_OPENVIKEY_TSF,
            )
            .map_err(|error| at_step(&error, "register TSF keyboard category"))
    }
}
