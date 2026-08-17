//! Hidden passphrase input via Win32 console (no crossterm).

use std::io::{self, Write};

use zeroize::Zeroizing;

/// Apply one virtual-key / character to the passphrase buffer.
///
/// Returns `true` when Enter (`0x0D`) finishes input.
pub fn apply_passphrase_key(buffer: &mut String, vk: u16, ch: Option<char>) -> bool {
    match vk {
        0x0D => true,
        0x08 => {
            buffer.pop();
            false
        }
        _ => {
            if let Some(c) = ch {
                buffer.push(c);
            }
            false
        }
    }
}

/// Prompt on stderr and read a passphrase with echo disabled.
///
/// Uses `ReadConsoleW` + `SetConsoleMode` without `ENABLE_ECHO_INPUT`.
/// Allocates a console if stdin is not attached to one.
pub fn read_hidden_passphrase() -> io::Result<Zeroizing<String>> {
    eprint!("Passphrase: ");
    io::stderr().flush()?;
    #[cfg(windows)]
    {
        read_hidden_passphrase_windows()
    }
    #[cfg(not(windows))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "hidden passphrase requires Windows console APIs",
        ))
    }
}

#[cfg(windows)]
fn read_hidden_passphrase_windows() -> io::Result<Zeroizing<String>> {
    use windows::Win32::System::Console::{
        AllocConsole, GetConsoleMode, GetStdHandle, SetConsoleMode, CONSOLE_MODE,
        ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT, STD_INPUT_HANDLE,
    };

    let stdin = unsafe { GetStdHandle(STD_INPUT_HANDLE) }.map_err(|e| io_from_windows(&e))?;
    if stdin.is_invalid() {
        unsafe { AllocConsole() }.map_err(|e| io_from_windows(&e))?;
    }
    let stdin = unsafe { GetStdHandle(STD_INPUT_HANDLE) }.map_err(|e| io_from_windows(&e))?;
    if stdin.is_invalid() {
        return Err(io::Error::other("stdin console handle is invalid"));
    }

    let mut mode = CONSOLE_MODE(0);
    unsafe { GetConsoleMode(stdin, &raw mut mode) }.map_err(|e| io_from_windows(&e))?;
    let previous = mode;
    let without_echo = CONSOLE_MODE(
        mode.0 & !(ENABLE_ECHO_INPUT.0 | ENABLE_LINE_INPUT.0 | ENABLE_PROCESSED_INPUT.0),
    );
    unsafe { SetConsoleMode(stdin, without_echo) }.map_err(|e| io_from_windows(&e))?;

    let result = read_chars_into_buffer(stdin);
    let _ = unsafe { SetConsoleMode(stdin, previous) };
    result
}

#[cfg(windows)]
fn read_chars_into_buffer(
    stdin: windows::Win32::Foundation::HANDLE,
) -> io::Result<Zeroizing<String>> {
    use windows::Win32::System::Console::ReadConsoleW;

    let mut buffer = Zeroizing::new(String::new());
    loop {
        let mut unit = [0u16; 1];
        let mut read = 0u32;
        unsafe {
            ReadConsoleW(
                stdin,
                unit.as_mut_ptr().cast::<core::ffi::c_void>(),
                1,
                &raw mut read,
                None,
            )
        }
        .map_err(|e| io_from_windows(&e))?;
        if read == 0 {
            continue;
        }
        let ch = char::decode_utf16([unit[0]])
            .next()
            .and_then(Result::ok)
            .unwrap_or('\u{FFFD}');
        let (vk, opt) = match ch {
            '\r' | '\n' => (0x0Du16, None),
            '\u{0008}' => (0x08, None),
            c => (0, Some(c)),
        };
        if apply_passphrase_key(&mut buffer, vk, opt) {
            break;
        }
    }
    eprintln!();
    Ok(buffer)
}

#[cfg(windows)]
fn io_from_windows(error: &windows::core::Error) -> io::Error {
    io::Error::other(error.to_string())
}
