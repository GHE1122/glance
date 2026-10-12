//! The light under a laptop's keys, where the laptop's maker has a way to
//! set it that is known: for now, ASUS's (ROG, TUF, Zephyrus), whose
//! keyboards are USB devices of their own taking the brightness of their
//! light as a feature report, the way ASUS's own software sets it.
//!
//! How bright it is cannot be asked of the keyboard: what is known is
//! what was last set here.

use windows::core::PCWSTR;
use windows::Win32::Devices::HumanInterfaceDevice::{HidD_FreePreparsedData, HidD_GetPreparsedData, HidD_SetFeature, HidP_GetCaps, HIDP_CAPS, PHIDP_PREPARSED_DATA};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};

/// ASUS's laptop keyboards, as their USB devices name themselves.
const KEYBOARDS: [&str; 5] = ["vid_0b05&pid_19b6", "vid_0b05&pid_1a30", "vid_0b05&pid_1866", "vid_0b05&pid_1869", "vid_0b05&pid_1854"];
/// The brightest the light goes: from 0 (out) to this.
pub const MOST: u8 = 3;
/// The report the brightness goes in, and what it begins with.
const REPORT: u8 = 0x5A;
const BRIGHTNESS: [u8; 3] = [0xBA, 0xC5, 0xC4];

/// Whether this PC has such a keyboard.
pub fn there() -> bool {
    KEYBOARDS.iter().any(|keyboard| !crate::cooler::paths(keyboard).is_empty())
}

/// The report the light's colour goes in (ASUS's "Aura"): the colour for the whole keyboard, lit steadily;
/// then that it is to be set, and applied.
const AURA: u8 = 0x5D;
/// What the keys' light does, as the keyboard numbers it; and the part of the keyboard that takes its colour,
/// as Windows names what a part is for.
pub const STEADY: u8 = 0;
pub const BREATHING: u8 = 1;
pub const RAINBOW: u8 = 3;
const AURA_PART: (u16, u16) = (0xFF31, 0x0079);

/// Whether the keys' light can be given a colour: whether the keyboard has the part that takes one.
pub fn coloured() -> bool {
    KEYBOARDS.iter().flat_map(|keyboard| crate::cooler::paths(keyboard)).any(|path| unsafe {
        let Ok(part) = CreateFileW(PCWSTR(path.as_ptr()), (GENERIC_READ | GENERIC_WRITE).0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, Default::default(), None) else { return false };
        let mut parsed = PHIDP_PREPARSED_DATA::default();
        let mut is = false;
        if HidD_GetPreparsedData(part, &mut parsed) {
            let mut caps = HIDP_CAPS::default();
            is = HidP_GetCaps(parsed, &mut caps).is_ok() && (caps.UsagePage, caps.Usage) == AURA_PART;
            let _ = HidD_FreePreparsedData(parsed);
        }
        let _ = CloseHandle(part);
        is
    })
}

/// Sends one feature report (what it begins with: the rest of its length is nothing) to each part of the
/// keyboard: whether a part took it. The keyboard is several devices to Windows: the one that has feature
/// reports of this kind is the one that takes it.
fn send(begins: &[u8]) -> bool {
    let mut took = false;
    for path in KEYBOARDS.iter().flat_map(|keyboard| crate::cooler::paths(keyboard)) {
        unsafe {
            let Ok(part) = CreateFileW(PCWSTR(path.as_ptr()), (GENERIC_READ | GENERIC_WRITE).0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, Default::default(), None) else { continue };
            let length = feature_length(part);
            if length >= begins.len() {
                let mut report = vec![0u8; length];
                report[..begins.len()].copy_from_slice(begins);
                took |= HidD_SetFeature(part, report.as_ptr().cast(), length as u32);
            }
            let _ = CloseHandle(part);
        }
    }
    took
}

/// How long a part's feature reports are; 0 for a part that has none.
unsafe fn feature_length(part: windows::Win32::Foundation::HANDLE) -> usize {
    let mut parsed = PHIDP_PREPARSED_DATA::default();
    let mut length = 0;
    unsafe {
        if HidD_GetPreparsedData(part, &mut parsed) {
            let mut caps = HIDP_CAPS::default();
            if HidP_GetCaps(parsed, &mut caps).is_ok() {
                length = caps.FeatureReportByteLength as usize;
            }
            let _ = HidD_FreePreparsedData(parsed);
        }
    }
    length
}

/// Makes the light under the keys as bright as `level` (0 to `MOST`):
/// whether a keyboard took it.
pub fn light(level: u8) -> bool {
    send(&[REPORT, BRIGHTNESS[0], BRIGHTNESS[1], BRIGHTNESS[2], level.min(MOST)])
}

/// Lights the keys in a colour (red, green, blue), doing what `does`
/// (`STEADY`, `BREATHING`, `RAINBOW`, at the keyboard's middle speed):
/// whether a keyboard took it (one with a light of one colour does not).
pub fn colour(rgb: [u8; 3], does: u8) -> bool {
    send(&[AURA, 0xB3, 0x00, does, rgb[0], rgb[1], rgb[2], 0xEB, 0x00, 0x00, 0x00, 0x00, 0x00]) && send(&[AURA, 0xB5, 0x00, 0x00, 0x00]) && send(&[AURA, 0xB4])
}

/// Each part of the keyboard, for a report: what it is for (its usage) and how long its reports are.
pub fn parts() -> Vec<String> {
    let mut found = Vec::new();
    for path in KEYBOARDS.iter().flat_map(|keyboard| crate::cooler::paths(keyboard)) {
        unsafe {
            let Ok(part) = CreateFileW(PCWSTR(path.as_ptr()), (GENERIC_READ | GENERIC_WRITE).0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, Default::default(), None) else {
                found.push("a part that would not open".to_string());
                continue;
            };
            let mut parsed = PHIDP_PREPARSED_DATA::default();
            if HidD_GetPreparsedData(part, &mut parsed) {
                let mut caps = HIDP_CAPS::default();
                if HidP_GetCaps(parsed, &mut caps).is_ok() {
                    found.push(format!("usage {:04X}:{:04X}, feature reports of {} bytes, output of {}", caps.UsagePage, caps.Usage, caps.FeatureReportByteLength, caps.OutputReportByteLength));
                }
                let _ = HidD_FreePreparsedData(parsed);
            }
            let _ = CloseHandle(part);
        }
    }
    found
}
