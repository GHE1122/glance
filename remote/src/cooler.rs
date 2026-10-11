//! The little display on a Thermalright cooler (its "digital" coolers: lit
//! figures for temperatures, driven over USB by the maker's own program,
//! TRCC): put out with the lights, and given back to TRCC with them.
//!
//! The display has no "off": it shows the last frame it was sent, a colour
//! for each of its LEDs, and TRCC sends one several times a second. So
//! TRCC is stopped (through the task it is started by at sign-in, which
//! its user may end and run without being an administrator), and one
//! frame of no light is sent; to light it again, the task is run.

use windows::core::{BSTR, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA,
    SP_DEVICE_INTERFACE_DETAIL_DATA_W,
};
use windows::Win32::Devices::HumanInterfaceDevice::HidD_GetHidGuid;
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::System::TaskScheduler::{IRegisteredTask, ITaskService, TaskScheduler};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::System::IO::{GetOverlappedResultEx, OVERLAPPED};

/// The display, as its USB device names itself in its path.
const DEVICE: &str = "vid_0416&pid_8001";
/// The task TRCC's installer makes to start it at sign-in.
const TASK: &str = "TRCCAppStartup";
/// What every packet begins with, and the two things one may say.
const MAGIC: [u8; 4] = [0xDA, 0xDB, 0xDC, 0xDD];
const WHO: u8 = 1;
const FRAME: u8 = 2;
/// A report: what is written at once (after a nought, its number).
const REPORT: usize = 64;
/// How long the display is waited on to take or to answer (ms).
const WAIT: u32 = 500;

/// How many LEDs the display has, by the model it says it is (the sixth
/// byte of its answer), as those who have read TRCC's own tables have
/// them; none for a model not among them, which is left alone.
fn leds(model: u8) -> Option<usize> {
    match model {
        1..=3 => Some(30),
        16..=31 => Some(84),
        32 => Some(64),
        48..=49 => Some(93),
        128 => Some(31),
        _ => None,
    }
}

/// A packet as the reports it is written in: what it says, then what it
/// carries, the last report filled out with noughts.
fn packet(says: u8, carries: &[u8]) -> Vec<[u8; REPORT + 1]> {
    let mut whole = MAGIC.to_vec();
    whole.extend_from_slice(&[0; 8]);
    whole.extend_from_slice(&[says, 0, 0, 0]);
    whole.extend_from_slice(&(carries.len() as u16).to_le_bytes());
    whole.extend_from_slice(&[0, 0]);
    whole.extend_from_slice(carries);
    whole
        .chunks(REPORT)
        .map(|chunk| {
            let mut report = [0u8; REPORT + 1];
            report[1..1 + chunk.len()].copy_from_slice(chunk);
            report
        })
        .collect()
}

/// The path the display is opened by, if there is one plugged in.
fn path() -> Option<Vec<u16>> {
    unsafe {
        let hid = HidD_GetHidGuid();
        let devices = SetupDiGetClassDevsW(Some(&hid), PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE).ok()?;
        let mut found = None;
        for index in 0.. {
            let mut interface = SP_DEVICE_INTERFACE_DATA { cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32, ..Default::default() };
            if SetupDiEnumDeviceInterfaces(devices, None, &hid, index, &mut interface).is_err() {
                break;
            }
            // How long its path is, then the path (in room that begins as the structure it is given in).
            let mut needed = 0u32;
            let _ = SetupDiGetDeviceInterfaceDetailW(devices, &interface, None, 0, Some(&mut needed), None);
            let mut room = vec![0u64; (needed as usize).div_ceil(8).max(1)];
            let detail = room.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(devices, &interface, Some(detail), needed, None, None).is_err() {
                continue;
            }
            let start = std::ptr::addr_of!((*detail).DevicePath) as *const u16;
            let length = (0..).take_while(|at| *start.add(*at) != 0).count();
            let path = std::slice::from_raw_parts(start, length);
            if String::from_utf16_lossy(path).to_ascii_lowercase().contains(DEVICE) {
                found = Some(path.iter().copied().chain([0]).collect());
                break;
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(devices);
        found
    }
}

/// The display, open.
struct Display(HANDLE);

impl Drop for Display {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

impl Display {
    fn open() -> Option<Display> {
        let path = path()?;
        let opened = unsafe { CreateFileW(PCWSTR(path.as_ptr()), (GENERIC_READ | GENERIC_WRITE).0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAG_OVERLAPPED, None) };
        opened.ok().map(Display)
    }

    /// Writes a packet: whether the display took all of it in time.
    fn write(&self, says: u8, carries: &[u8]) -> bool {
        packet(says, carries).iter().all(|report| unsafe {
            let mut waiting = OVERLAPPED::default();
            let _ = WriteFile(self.0, Some(report), None, Some(&mut waiting));
            let mut written = 0;
            GetOverlappedResultEx(self.0, &waiting, &mut written, WAIT, false).is_ok() && written as usize == report.len()
        })
    }

    /// The report the display answers with, if it does in time.
    fn read(&self) -> Option<[u8; REPORT + 1]> {
        let mut report = [0u8; REPORT + 1];
        unsafe {
            let mut waiting = OVERLAPPED::default();
            let _ = ReadFile(self.0, Some(&mut report), None, Some(&mut waiting));
            let mut read = 0;
            GetOverlappedResultEx(self.0, &waiting, &mut read, WAIT, false).ok()?;
        }
        Some(report)
    }
}

/// TRCC's task, as Windows' scheduler has it; none where TRCC is not
/// installed (or is started some other way).
fn task() -> Option<IRegisteredTask> {
    crate::actions::com();
    unsafe {
        let scheduler: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_ALL).ok()?;
        let none = VARIANT::default();
        scheduler.Connect(&none, &none, &none, &none).ok()?;
        scheduler.GetFolder(&BSTR::from("\\")).ok()?.GetTask(&BSTR::from(TASK)).ok()
    }
}

/// Puts the display out: whether it was (not, where there is no such
/// display, it is of a model not known, or TRCC is not this user's to stop).
pub fn dark() -> bool {
    let Some(display) = Display::open() else { return false };
    // TRCC running on, a frame of no light would last a moment, and the
    // display is slow to answer anyone else: it is stopped first (one not
    // started by its task is not this user's to stop).
    let task = task();
    if let Some(task) = &task {
        let _ = unsafe { task.Stop(0) };
    }
    let stopped = (0..100).any(|_| {
        let gone = !running();
        if !gone {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        gone
    });
    if !stopped {
        return false;
    }
    // What it is: one of a model not known is given back to TRCC as it was.
    let model = display.write(WHO, &[]).then(|| display.read()).flatten().filter(|answer| answer[1..5] == MAGIC).map(|answer| answer[6]);
    match model.and_then(leds) {
        Some(leds) => display.write(FRAME, &vec![0u8; leds * 3]),
        None => {
            if let Some(task) = &task {
                let _ = unsafe { task.Run(&VARIANT::default()) };
            }
            false
        }
    }
}

/// Whether TRCC is running.
fn running() -> bool {
    crate::actions::programs().iter().any(|(_, name)| name.eq_ignore_ascii_case("TRCC.exe"))
}

/// Gives the display back to TRCC, which lights it as it was set to.
pub fn lit() -> bool {
    path().is_some() && task().is_some_and(|task| unsafe { task.Run(&VARIANT::default()) }.is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_packet_is_cut_into_reports() {
        let who = packet(WHO, &[]);
        assert_eq!(who.len(), 1);
        assert_eq!(who[0][..14], [0, 0xDA, 0xDB, 0xDC, 0xDD, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        // 93 LEDs: 279 bytes carried after 20 of heading, in five reports.
        let frame = packet(FRAME, &[7u8; 279]);
        assert_eq!(frame.len(), 5);
        assert_eq!(frame[0][13], 2);
        assert_eq!(frame[0][17..19], [0x17, 0x01]);
        assert_eq!(frame[0][21], 7);
        assert_eq!(frame[4][1..44], [7u8; 43]);
        assert_eq!(frame[4][44..], [0u8; 21]);
    }

    #[test]
    fn only_models_known_have_a_count() {
        assert_eq!(leds(48), Some(93));
        assert_eq!(leds(20), Some(84));
        assert_eq!(leds(0), None);
        assert_eq!(leds(200), None);
    }
}
