//! What the settings copy for a problem report: this Glance, the system,
//! the hardware Glance found to read and how, and which readings it is
//! getting. Nothing that names the user: no paths, no network addresses.

use windows::core::w;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;

use crate::metrics::reg_string;
use crate::reading::{Sample, StaticInfo};

/// The report, in English (it is for an issue on GitHub).
pub fn report() -> String {
    let app = crate::app();
    let info = &app.info;
    let windows_key = w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let bios = w!(r"HARDWARE\DESCRIPTION\System\BIOS");
    let yes = |b: bool| if b { "yes" } else { "no" };
    let mut lines = vec![
        format!("Glance {}", env!("CARGO_PKG_VERSION")),
        windows(windows_key),
        format!("Running elevated: {}", yes(crate::elevation::is_elevated())),
        format!("Installed in a protected folder: {}", yes(crate::elevation::may_start_unasked())),
        format!("PawnIO driver: {}", crate::elevation::pawnio_version().unwrap_or_else(|| "not installed".into())),
        format!("CPU: {} ({} threads)", info.cpu_name, info.threads),
        format!("Motherboard: {} {}", reg_string(bios, w!("BaseBoardManufacturer")), info.board),
        format!("Memory: {}", info.memory_modules.clone().unwrap_or_else(|| "unknown".into())),
    ];
    lines.extend(info.gpus.iter().map(|gpu| format!("GPU: {}", gpu.name)));
    lines.extend(info.found.iter().cloned());
    if let Some(later) = crate::superio::found_later() {
        lines.push(format!("Motherboard chip, looked for again: {later}"));
    }
    let log = crate::journal::recent(15);
    lines.push(format!("Recent log ({} lines):", log.len()));
    lines.extend(log.into_iter().map(|line| format!("  {line}")));
    // What the phone remote finds, where it is there: the lights, the monitors, what else it sets.
    if let Some(remote) = crate::phone::report() {
        lines.push("Phone remote:".into());
        lines.extend(remote.lines().map(|line| format!("  {line}")));
    }
    lines.push("Latest readings:".into());
    match app.controller.history.lock().unwrap().back() {
        Some(sample) => lines.extend(readings(sample, info).into_iter().map(|line| format!("  {line}"))),
        None => lines.push("  none yet".into()),
    }
    lines.join("\n")
}

/// Where a report is sent: Glance's own site.
const REPORTS: &str = "https://glancepc.com/api/report";

/// Sends `text` (the report, as the user was shown it) to Glance's site:
/// the short name it was given there, to quote when asking about it; none
/// if it did not get there.
pub fn send(text: &str, pictures: &[Vec<u8>]) -> Option<String> {
    let pictures: Vec<String> = pictures.iter().map(|picture| base64(picture)).collect();
    let sent = serde_json::json!({ "version": env!("CARGO_PKG_VERSION"), "text": text, "pictures": pictures }).to_string();
    let answer: serde_json::Value = serde_json::from_slice(&crate::update::post(REPORTS, sent.as_bytes())?).ok()?;
    answer.get("id")?.as_str().map(str::to_string)
}

/// `bytes` in base64 (RFC 4648), as JSON carries a picture.
fn base64(bytes: &[u8]) -> String {
    const LETTERS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for three in bytes.chunks(3) {
        let bits = three.iter().enumerate().fold(0u32, |bits, (at, &byte)| bits | (byte as u32) << (16 - 8 * at));
        for at in 0..4 {
            text.push(if at <= three.len() { LETTERS[(bits >> (18 - 6 * at) & 63) as usize] as char } else { '=' });
        }
    }
    text
}

/// Windows' name, release and build. Windows 11 keeps "Windows 10" in its
/// product name; its builds, from 22000, tell it apart.
fn windows(key: windows::core::PCWSTR) -> String {
    let build = reg_string(key, w!("CurrentBuild"));
    let mut name = reg_string(key, w!("ProductName"));
    if build.parse::<u32>().is_ok_and(|build| build >= 22000) {
        name = name.replace("Windows 10", "Windows 11");
    }
    format!("Windows: {name} {} (build {build}.{})", reg_string(key, w!("DisplayVersion")), crate::metrics::reg_dword(key, w!("UBR")))
}

/// Which readings arrive, and what they are now.
fn readings(sample: &Sample, info: &StaticInfo) -> Vec<String> {
    let value = |reading: Option<f32>, unit: &str| reading.map_or("not read".to_string(), |v| format!("{v:.1} {unit}"));
    let cpu = sample.cpu_sensors.clone().unwrap_or_default();
    let mut lines = vec![
        format!("CPU temperature: {}", value(cpu.temp, "°C")),
        format!("CPU package power: {}", value(cpu.power, "W")),
        format!("CPU chiplet temperatures: {}", cpu.ccds.len()),
    ];
    for (reading, gpu) in sample.gpus.iter().zip(&info.gpus) {
        lines.push(format!(
            "{}: temperature {}, hot spot {}, clock {}, fan {}, power {}",
            gpu.name,
            value(reading.temp, "°C"),
            value(reading.hot_spot, "°C"),
            value(reading.clock_mhz, "MHz"),
            reading.fan_rpm.map_or("not read".into(), |rpm| format!("{rpm} RPM")),
            value(reading.power, "W"),
        ));
    }
    let board = sample.board.clone().unwrap_or_default();
    lines.push(format!("Motherboard: {} temperatures, {} fans", board.temps.len(), board.fans.len()));
    lines.extend(board.temps.iter().map(|(name, temp)| format!("  {name}: {temp:.1} °C")));
    lines.extend(board.fans.iter().map(|(name, rpm)| format!("  {name}: {rpm:.0} RPM")));
    lines.push(format!("Memory module temperatures: {}", sample.dimm_temps.len()));
    lines.push(format!("Drive temperatures: {}", sample.drive_temps.len()));
    lines.push(format!("Battery: {}", if sample.battery.is_some() { "present" } else { "none" }));
    lines
}

/// Puts `text` on the clipboard, as the window `owner` holds it.
pub fn copy(owner: HWND, text: &str) -> bool {
    let units: Vec<u16> = text.encode_utf16().chain([0]).collect();
    let bytes = units.len() * 2;
    unsafe {
        if OpenClipboard(Some(owner)).is_err() {
            return false;
        }
        let placed = (|| {
            EmptyClipboard().ok()?;
            let memory = GlobalAlloc(GMEM_MOVEABLE, bytes).ok()?;
            let target = GlobalLock(memory) as *mut u16;
            if target.is_null() {
                let _ = GlobalFree(Some(memory));
                return None;
            }
            std::ptr::copy_nonoverlapping(units.as_ptr(), target, units.len());
            let _ = GlobalUnlock(memory);
            // The clipboard owns the memory once it takes it.
            if SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(memory.0))).is_err() {
                let _ = GlobalFree(Some(HGLOBAL(memory.0)));
                return None;
            }
            Some(())
        })();
        let _ = CloseClipboard();
        placed.is_some()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_is_as_the_standard_has_it() {
        for (bytes, text) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(super::base64(bytes.as_bytes()), text);
        }
    }

    #[test]
    #[ignore = "sends a report to Glance's site"]
    fn sends_a_report() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let picture = std::mem::take(&mut crate::ui::picture::from_file(concat!(env!("CARGO_MANIFEST_DIR"), "/website/apple-touch-icon.png"), (84, 52)).expect("a picture to send").jpeg);
        let id = super::send("Glance (a test of sending from this program; nothing is wrong)", &[picture]);
        println!("sent as {id:?}");
        assert!(id.is_some_and(|id| id.len() == 6));
    }
}
