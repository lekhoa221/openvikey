//! Explicit per-user COM and TSF registration.

use std::path::Path;

use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, S_OK};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, CLSID_TF_InputProcessorProfiles, GUID_TFCAT_TIP_KEYBOARD, ITfCategoryMgr,
    ITfInputProcessorProfileMgr, ITfInputProcessorProfiles, TF_INPUTPROCESSORPROFILE,
    TF_IPPMF_DISABLEPROFILE, TF_IPPMF_ENABLEPROFILE, TF_IPPMF_FORSESSION,
    TF_PROFILETYPE_INPUTPROCESSOR,
};
use windows::core::{Error, IUnknown, Interface, Result};
use windows_registry::CURRENT_USER;

use crate::{CLSID_OPENVIKEY_TSF, GUID_OPENVIKEY_PROFILE};

const COM_CLASS_KEY: &str = r"Software\Classes\CLSID\{741B179E-BF99-4EA2-BDDF-B14AD06E7A60}";
const INPROC_KEY: &str =
    r"Software\Classes\CLSID\{741B179E-BF99-4EA2-BDDF-B14AD06E7A60}\InprocServer32";
const VIETNAMESE_LANG_ID: u16 = 0x042A;
const ENGLISH_US_LANG_ID: u16 = 0x0409;
const DEVELOPMENT_LANG_IDS: [u16; 2] = [ENGLISH_US_LANG_ID, VIETNAMESE_LANG_ID];

fn at_step(error: &Error, step: &str) -> Error {
    Error::new(error.code(), format!("{step}: {error}"))
}

/// Register the DLL for the current user and add development language profiles.
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
            for language_id in DEVELOPMENT_LANG_IDS {
                let _ = profiles.UnregisterProfile(
                    &CLSID_OPENVIKEY_TSF,
                    language_id,
                    &GUID_OPENVIKEY_PROFILE,
                    0,
                );
            }
        }
    }

    if CURRENT_USER.open(COM_CLASS_KEY).is_ok() {
        CURRENT_USER.remove_tree(COM_CLASS_KEY)?;
    }
    Ok(())
}

/// Enable and activate the development profile for the current desktop session.
pub fn activate_profile_for_session() -> Result<()> {
    let language_id = active_keyboard_profile()?.langid;
    let language_profiles = language_profiles()?;
    unsafe {
        language_profiles.EnableLanguageProfile(
            &CLSID_OPENVIKEY_TSF,
            language_id,
            &GUID_OPENVIKEY_PROFILE,
            true,
        )?;
        let profiles = profile_manager()?;
        let hr = (Interface::vtable(&profiles).ActivateProfile)(
            Interface::as_raw(&profiles),
            TF_PROFILETYPE_INPUTPROCESSOR,
            language_id,
            &CLSID_OPENVIKEY_TSF,
            &GUID_OPENVIKEY_PROFILE,
            HKL::default(),
            TF_IPPMF_FORSESSION | TF_IPPMF_ENABLEPROFILE,
        );
        if hr == S_OK {
            let active = active_keyboard_profile()?;
            if active.clsid == CLSID_OPENVIKEY_TSF && active.guidProfile == GUID_OPENVIKEY_PROFILE {
                Ok(())
            } else {
                Err(Error::new(
                    E_FAIL,
                    "Windows enabled OpenViKey but did not select it as the active keyboard profile; select OpenViKey from the language switcher",
                ))
            }
        } else {
            Err(Error::new(
                if hr.is_err() { hr } else { E_FAIL },
                format!("activate development TSF profile returned {hr:?}"),
            ))
        }
    }
}

/// Deactivate and return the development profile to disabled registry state.
pub fn deactivate_profile_for_session() -> Result<()> {
    let profiles = profile_manager()?;
    let language_profiles = language_profiles()?;
    let mut first_error = None;
    for language_id in DEVELOPMENT_LANG_IDS {
        if let Err(error) = unsafe {
            profiles.DeactivateProfile(
                TF_PROFILETYPE_INPUTPROCESSOR,
                language_id,
                &CLSID_OPENVIKEY_TSF,
                &GUID_OPENVIKEY_PROFILE,
                HKL::default(),
                TF_IPPMF_FORSESSION | TF_IPPMF_DISABLEPROFILE,
            )
        } {
            first_error.get_or_insert(error);
        }
        if let Err(error) = unsafe {
            language_profiles.EnableLanguageProfile(
                &CLSID_OPENVIKEY_TSF,
                language_id,
                &GUID_OPENVIKEY_PROFILE,
                false,
            )
        } {
            first_error.get_or_insert(error);
        }
    }
    if let Some(error) = first_error {
        Err(error)
    } else {
        Ok(())
    }
}

/// Query the keyboard profile TSF currently considers active.
pub fn active_keyboard_profile() -> Result<TF_INPUTPROCESSORPROFILE> {
    let profiles = profile_manager()?;
    let mut profile = TF_INPUTPROCESSORPROFILE::default();
    unsafe {
        profiles.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &raw mut profile)?;
    }
    Ok(profile)
}

/// Query one registered OpenViKey development profile.
pub fn development_profile(language_id: u16) -> Result<TF_INPUTPROCESSORPROFILE> {
    let profiles = profile_manager()?;
    let mut profile = TF_INPUTPROCESSORPROFILE::default();
    unsafe {
        profiles.GetProfile(
            TF_PROFILETYPE_INPUTPROCESSOR,
            language_id,
            &CLSID_OPENVIKEY_TSF,
            &GUID_OPENVIKEY_PROFILE,
            HKL::default(),
            &raw mut profile,
        )?;
    }
    Ok(profile)
}

fn profile_manager() -> Result<ITfInputProcessorProfileMgr> {
    unsafe {
        CoCreateInstance(
            &CLSID_TF_InputProcessorProfiles,
            None::<&IUnknown>,
            CLSCTX_INPROC_SERVER,
        )
    }
}

fn language_profiles() -> Result<ITfInputProcessorProfiles> {
    unsafe {
        CoCreateInstance(
            &CLSID_TF_InputProcessorProfiles,
            None::<&IUnknown>,
            CLSCTX_INPROC_SERVER,
        )
    }
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
    for language_id in DEVELOPMENT_LANG_IDS {
        unsafe {
            profiles
                .RegisterProfile(
                    &CLSID_OPENVIKEY_TSF,
                    language_id,
                    &GUID_OPENVIKEY_PROFILE,
                    &description,
                    &[],
                    0,
                    HKL::default(),
                    0,
                    true,
                    0,
                )
                .map_err(|error| at_step(&error, "register development TSF profile"))?;
        }
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
