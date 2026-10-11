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

/// Makes the light under the keys as bright as `level` (0 to `MOST`):
/// whether a keyboard took it.
pub fn light(level: u8) -> bool {
    let level = level.min(MOST);
    // The keyboard is several devices to Windows: the one that has feature reports and takes this one is it.
    KEYBOARDS.iter().flat_map(|keyboard| crate::cooler::paths(keyboard)).any(|path| unsafe {
        let Ok(part) = CreateFileW(PCWSTR(path.as_ptr()), (GENERIC_READ | GENERIC_WRITE).0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, Default::default(), None) else { return false };
        let mut parsed = PHIDP_PREPARSED_DATA::default();
        let mut length = 0;
        if HidD_GetPreparsedData(part, &mut parsed) {
            let mut caps = HIDP_CAPS::default();
            if HidP_GetCaps(parsed, &mut caps).is_ok() {
                length = caps.FeatureReportByteLength as usize;
            }
            let _ = HidD_FreePreparsedData(parsed);
        }
        let mut report = vec![0u8; length];
        let took = length >= 5 && {
            report[0] = REPORT;
            report[1..4].copy_from_slice(&BRIGHTNESS);
            report[4] = level;
            HidD_SetFeature(part, report.as_ptr().cast(), length as u32)
        };
        let _ = CloseHandle(part);
        took
    })
}
