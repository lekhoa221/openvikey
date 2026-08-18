use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use openvikey_win::inject::{
    InjectError, InjectProfile, InputSender, ProfilingInjector, SynthesizedEvent, to_win32_events,
};

fn inj(profile: InjectProfile) -> ProfilingInjector<VecSender> {
    ProfilingInjector {
        profile,
        sender: VecSender { batches: vec![] },
        sending: Arc::new(AtomicBool::new(false)),
    }
}

struct VecSender {
    batches: Vec<Vec<SynthesizedEvent>>,
}
impl InputSender for VecSender {
    fn send(&mut self, events: &[SynthesizedEvent]) -> Result<u32, InjectError> {
        self.batches.push(events.to_vec());
        Ok(u32::try_from(events.len()).expect("batch len fits u32"))
    }
}

struct PartialSender;
impl InputSender for PartialSender {
    fn send(&mut self, events: &[SynthesizedEvent]) -> Result<u32, InjectError> {
        Ok(u32::try_from(events.len().saturating_sub(1)).expect("batch len fits u32"))
    }
}

#[test]
fn win32_replace_one_batch_contents() {
    let mut injector = inj(InjectProfile::Win32);
    injector.replace(1, "à").unwrap();
    assert_eq!(injector.sender.batches.len(), 1);
    assert_eq!(injector.sender.batches[0][0], SynthesizedEvent::Backspace);
    assert!(!injector.sending.load(Ordering::SeqCst));
}

#[test]
fn electron_replace_two_batches() {
    let mut injector = inj(InjectProfile::Electron);
    injector.replace(1, "à").unwrap();
    assert_eq!(injector.sender.batches.len(), 2);
    assert_eq!(
        injector.sender.batches[0],
        vec![SynthesizedEvent::Backspace]
    );
}

#[test]
fn electron_zero_backspace_one_batch() {
    let mut injector = inj(InjectProfile::Electron);
    injector.replace(0, "a").unwrap();
    assert_eq!(injector.sender.batches.len(), 1);
}

#[test]
fn partial_send_is_error_and_clears_sending() {
    let mut injector = ProfilingInjector {
        profile: InjectProfile::Win32,
        sender: PartialSender,
        sending: Arc::new(AtomicBool::new(false)),
    };
    let err = injector.replace(1, "a").unwrap_err();
    assert!(matches!(err, InjectError::Partial { .. }));
    assert!(!injector.sending.load(Ordering::SeqCst));
}

const KEYEVENTF_KEYUP: u32 = 0x0002;
const KEYEVENTF_UNICODE: u32 = 0x0004;
const VK_BACK: u16 = 0x08;

#[test]
fn to_win32_backspace_two_input_intents() {
    let intents = to_win32_events(&[SynthesizedEvent::Backspace]);
    assert_eq!(intents.len(), 2);
    assert_eq!(intents[0], (VK_BACK, 0));
    assert_eq!(intents[1], (VK_BACK, KEYEVENTF_KEYUP));
}

#[test]
fn to_win32_utf16_a_two_unicode_intents() {
    let intents = to_win32_events(&[SynthesizedEvent::Utf16(u16::from(b'a'))]);
    assert_eq!(intents.len(), 2);
    assert_eq!(intents[0], (u16::from(b'a'), KEYEVENTF_UNICODE));
    assert_eq!(
        intents[1],
        (u16::from(b'a'), KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)
    );
}
