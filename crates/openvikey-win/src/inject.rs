//! Injector batching for Win32 and Electron profiles (no sleep).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::policy::OVK_EXTRA;
use crate::sync::InjectCommand;

/// Virtual-key code for Backspace.
const VK_BACK: u16 = 0x08;
/// `KEYEVENTF_KEYUP`
const KEYEVENTF_KEYUP: u32 = 0x0002;
/// `KEYEVENTF_UNICODE`
const KEYEVENTF_UNICODE: u32 = 0x0004;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SynthesizedEvent {
    Backspace,
    Utf16(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectProfile {
    Win32,
    Electron,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InjectError {
    #[error("partial inject: sent {sent}, expected {expected}")]
    Partial { sent: u32, expected: usize },
}

pub trait InputSender {
    /// Sends a batch of synthesized events. Returns how many were accepted.
    ///
    /// # Errors
    ///
    /// Returns transport/send failures from the underlying sender.
    fn send(&mut self, events: &[SynthesizedEvent]) -> Result<u32, InjectError>;
}

pub struct SendingGuard {
    flag: Arc<AtomicBool>,
}

impl SendingGuard {
    pub fn enter(flag: Arc<AtomicBool>) -> Self {
        flag.store(true, Ordering::SeqCst);
        Self { flag }
    }
}

impl Drop for SendingGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

pub struct ProfilingInjector<S: InputSender> {
    pub profile: InjectProfile,
    pub sender: S,
    pub sending: Arc<AtomicBool>,
}

impl<S: InputSender> ProfilingInjector<S> {
    /// Replace prior graphemes with NFC text via profile-specific batching.
    ///
    /// # Errors
    ///
    /// Returns [`InjectError::Partial`] when the sender accepts fewer events than sent.
    pub fn replace(
        &mut self,
        backspace_graphemes: usize,
        text_nfc: &str,
    ) -> Result<(), InjectError> {
        let mut events = Vec::new();
        for _ in 0..backspace_graphemes {
            events.push(SynthesizedEvent::Backspace);
        }
        for unit in text_nfc.encode_utf16() {
            events.push(SynthesizedEvent::Utf16(unit));
        }
        self.dispatch(&events)
    }

    /// Append a delimiter character as Utf16 units.
    ///
    /// # Errors
    ///
    /// Returns [`InjectError::Partial`] when the sender accepts fewer events than sent.
    pub fn append_delimiter(&mut self, delimiter: char) -> Result<(), InjectError> {
        let mut buf = [0u16; 2];
        let encoded: Vec<SynthesizedEvent> = delimiter
            .encode_utf16(&mut buf)
            .iter()
            .copied()
            .map(SynthesizedEvent::Utf16)
            .collect();
        self.dispatch(&encoded)
    }

    fn dispatch(&mut self, events: &[SynthesizedEvent]) -> Result<(), InjectError> {
        if events.is_empty() {
            return Ok(());
        }
        let _guard = SendingGuard::enter(Arc::clone(&self.sending));
        match self.profile {
            InjectProfile::Win32 => {
                let n = self.sender.send(events)?;
                if n as usize != events.len() {
                    return Err(InjectError::Partial {
                        sent: n,
                        expected: events.len(),
                    });
                }
                Ok(())
            }
            InjectProfile::Electron => {
                let split = events
                    .iter()
                    .position(|e| matches!(e, SynthesizedEvent::Utf16(_)));
                match split {
                    None | Some(0) => {
                        let n = self.sender.send(events)?;
                        if n as usize != events.len() {
                            return Err(InjectError::Partial {
                                sent: n,
                                expected: events.len(),
                            });
                        }
                        Ok(())
                    }
                    Some(i) => {
                        let n = self.sender.send(&events[..i])?;
                        if n as usize != i {
                            return Err(InjectError::Partial {
                                sent: n,
                                expected: i,
                            });
                        }
                        let n = self.sender.send(&events[i..])?;
                        if n as usize != events.len() - i {
                            return Err(InjectError::Partial {
                                sent: n,
                                expected: events.len() - i,
                            });
                        }
                        Ok(())
                    }
                }
            }
        }
    }
}

/// Map synthesized events to `(wVk_or_wScan, dwFlags)` INPUT intents (down+up pairs).
///
/// Backspace → VK_BACK down/up. Utf16 → UNICODE down/up.
#[must_use]
pub fn to_win32_events(events: &[SynthesizedEvent]) -> Vec<(u16, u32)> {
    let mut out = Vec::with_capacity(events.len().saturating_mul(2));
    for event in events {
        match *event {
            SynthesizedEvent::Backspace => {
                out.push((VK_BACK, 0));
                out.push((VK_BACK, KEYEVENTF_KEYUP));
            }
            SynthesizedEvent::Utf16(unit) => {
                out.push((unit, KEYEVENTF_UNICODE));
                out.push((unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
            }
        }
    }
    out
}

/// Apply [`InjectCommand`]s through a profiled injector (live or test sender).
pub trait CommandInjector: Send {
    /// # Errors
    ///
    /// Returns [`InjectError`] when a batch is only partially accepted.
    fn apply_commands(
        &mut self,
        cmds: &[InjectCommand],
        profile: InjectProfile,
    ) -> Result<(), InjectError>;

    fn sending_flag(&self) -> &Arc<AtomicBool>;
}

impl<S: InputSender + Send> CommandInjector for ProfilingInjector<S> {
    fn apply_commands(
        &mut self,
        cmds: &[InjectCommand],
        profile: InjectProfile,
    ) -> Result<(), InjectError> {
        self.profile = profile;
        for cmd in cmds {
            match cmd {
                InjectCommand::Replace {
                    backspace_graphemes,
                    text_nfc,
                } => self.replace(*backspace_graphemes, text_nfc)?,
                InjectCommand::AppendDelimiter { delimiter } => {
                    self.append_delimiter(*delimiter)?;
                }
            }
        }
        Ok(())
    }

    fn sending_flag(&self) -> &Arc<AtomicBool> {
        &self.sending
    }
}

/// Win32 `SendInput` transport; stamps [`OVK_EXTRA`] on every event.
#[derive(Debug, Default, Clone, Copy)]
pub struct SendInputSender;

impl InputSender for SendInputSender {
    fn send(&mut self, events: &[SynthesizedEvent]) -> Result<u32, InjectError> {
        #[cfg(windows)]
        {
            send_input_win32(events)
        }
        #[cfg(not(windows))]
        {
            let _ = events;
            Err(InjectError::Partial {
                sent: 0,
                expected: events.len(),
            })
        }
    }
}

#[cfg(windows)]
fn send_input_win32(events: &[SynthesizedEvent]) -> Result<u32, InjectError> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, SendInput, VIRTUAL_KEY,
    };

    let intents = to_win32_events(events);
    if intents.is_empty() {
        return Ok(0);
    }
    let inputs: Vec<INPUT> = intents
        .iter()
        .map(|&(code, flags)| {
            let unicode = flags & KEYEVENTF_UNICODE != 0;
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: if unicode {
                            VIRTUAL_KEY(0)
                        } else {
                            VIRTUAL_KEY(code)
                        },
                        wScan: if unicode { code } else { 0 },
                        dwFlags: KEYBD_EVENT_FLAGS(flags),
                        time: 0,
                        dwExtraInfo: OVK_EXTRA,
                    },
                },
            }
        })
        .collect();
    let sent = unsafe {
        SendInput(
            &inputs,
            i32::try_from(std::mem::size_of::<INPUT>()).unwrap_or(i32::MAX),
        )
    };
    // `SendInput` counts INPUT structs; each synthesized event is a down+up pair.
    let expected_inputs = u32::try_from(intents.len()).unwrap_or(u32::MAX);
    if sent != expected_inputs {
        return Err(InjectError::Partial {
            sent: sent / 2,
            expected: events.len(),
        });
    }
    Ok(u32::try_from(events.len()).unwrap_or(u32::MAX))
}
