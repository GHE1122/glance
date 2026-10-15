//! A client for PawnIO, the signed driver that runs small sandboxed modules
//! for hardware access (AMD and Intel CPU registers, Super I/O chips, SMBus).
//! Each module is loaded into its own handle; its functions are then called
//! by name with arrays of 64-bit values in and out.

use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;

const DEVICE_TYPE: u32 = 41394;
const fn ctl_code(function: u32) -> u32 {
    // METHOD_BUFFERED, FILE_ANY_ACCESS
    (DEVICE_TYPE << 16) | (function << 2)
}
const IOCTL_LOAD_BINARY: u32 = ctl_code(0x821);
const IOCTL_EXECUTE_FN: u32 = ctl_code(0x841);
const FN_NAME_LENGTH: usize = 32;

/// Whether the driver has been started again for a device it ran without (see `Module::loaded`).
static REVIVED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The driver's device, opened.
fn open() -> windows::core::Result<HANDLE> {
    unsafe {
        CreateFileW(
            w!(r"\\?\GLOBALROOT\Device\PawnIO"),
            (FILE_GENERIC_READ | FILE_GENERIC_WRITE).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
}

/// Starts the driver's device again, as Device Manager does when a device's settings change (and `pnputil
/// /restart-device`): every PawnIO device there is. Whether one was. Needs elevation.
fn revive() -> bool {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        SetupDiCallClassInstaller, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW, SetupDiGetDeviceInstanceIdW,
        SetupDiSetClassInstallParamsW, DICS_FLAG_GLOBAL, DICS_PROPCHANGE, DIF_PROPERTYCHANGE, DIGCF_ALLCLASSES, DIGCF_PRESENT, SP_CLASSINSTALL_HEADER,
        SP_DEVINFO_DATA, SP_PROPCHANGE_PARAMS,
    };
    let Ok(devices) = (unsafe { SetupDiGetClassDevsW(None, w!("ROOT"), None, DIGCF_ALLCLASSES | DIGCF_PRESENT) }) else { return false };
    let mut revived = false;
    let mut index = 0;
    loop {
        let mut device = SP_DEVINFO_DATA { cbSize: size_of::<SP_DEVINFO_DATA>() as u32, ..Default::default() };
        if unsafe { SetupDiEnumDeviceInfo(devices, index, &mut device) }.is_err() {
            break;
        }
        index += 1;
        let mut id = [0u16; 256];
        if unsafe { SetupDiGetDeviceInstanceIdW(devices, &device, Some(&mut id), None) }.is_err() {
            continue;
        }
        let id = String::from_utf16_lossy(&id[..id.iter().position(|&c| c == 0).unwrap_or(id.len())]);
        if !id.to_ascii_uppercase().starts_with(r"ROOT\PAWNIO\") {
            continue;
        }
        let change = SP_PROPCHANGE_PARAMS {
            ClassInstallHeader: SP_CLASSINSTALL_HEADER { cbSize: size_of::<SP_CLASSINSTALL_HEADER>() as u32, InstallFunction: DIF_PROPERTYCHANGE },
            StateChange: DICS_PROPCHANGE,
            Scope: DICS_FLAG_GLOBAL,
            HwProfile: 0,
        };
        let asked = unsafe { SetupDiSetClassInstallParamsW(devices, Some(&device), Some(&change.ClassInstallHeader), size_of::<SP_PROPCHANGE_PARAMS>() as u32) }.is_ok();
        revived |= asked && unsafe { SetupDiCallClassInstaller(DIF_PROPERTYCHANGE, devices, Some(&device)) }.is_ok();
    }
    let _ = unsafe { SetupDiDestroyDeviceInfoList(devices) };
    revived
}

/// One module loaded into the driver.
pub struct Module {
    handle: HANDLE,
}

// The handle is the driver's; calls on it are independent of the thread.
unsafe impl Send for Module {}

/// How the last module that could not be loaded failed (the driver not opened, or the module not loaded, and why),
/// for a report.
static FAILED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn last_failure() -> Option<String> {
    FAILED.lock().unwrap().clone()
}

impl Module {
    /// Loads a compiled module. Fails if the driver is not installed, the
    /// process may not open it, or the module does not support this machine.
    pub fn load(blob: &[u8]) -> windows::core::Result<Self> {
        let loaded = Self::loaded(blob);
        if let Err((step, error)) = &loaded {
            *FAILED.lock().unwrap() = Some(format!("{step}: {error}"));
        }
        loaded.map_err(|(_, error)| error)
    }

    fn loaded(blob: &[u8]) -> Result<Self, (&'static str, windows::core::Error)> {
        let handle = match open() {
            // Installed and running, and yet no device to open: the driver came up while an earlier copy of it was
            // still held by another program (one that installs PawnIO itself, as G-Helper does), and could not take
            // its name. Started again (once for this process), it takes it.
            Err(error) if error.code() == ERROR_FILE_NOT_FOUND.to_hresult() && !REVIVED.swap(true, std::sync::atomic::Ordering::Relaxed) && revive() => {
                crate::journal::note("sensors: the PawnIO driver ran with no device to open, and was started again");
                open()
            }
            opened => opened,
        }
        .map_err(|error| ("the driver not opened", error))?;
        let module = Module { handle };
        // A synchronous call has to be given somewhere to say how much it
        // returned, even when it returns nothing.
        let mut returned = 0u32;
        unsafe {
            DeviceIoControl(
                handle,
                IOCTL_LOAD_BINARY,
                Some(blob.as_ptr() as *const _),
                blob.len() as u32,
                None,
                0,
                Some(&mut returned),
                None,
            )
        }
        .map_err(|error| ("a module not loaded", error))?;
        Ok(module)
    }

    /// Calls the module's function `name` (as in its source, `ioctl_` included).
    pub fn call(&self, name: &str, input: &[u64], output: &mut [u64]) -> windows::core::Result<usize> {
        assert!(name.len() < FN_NAME_LENGTH, "function name too long: {name}");
        let mut request = vec![0u8; FN_NAME_LENGTH + input.len() * 8];
        request[..name.len()].copy_from_slice(name.as_bytes());
        for (i, value) in input.iter().enumerate() {
            request[FN_NAME_LENGTH + i * 8..FN_NAME_LENGTH + i * 8 + 8].copy_from_slice(&value.to_le_bytes());
        }
        let mut written = 0u32;
        unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_EXECUTE_FN,
                Some(request.as_ptr() as *const _),
                request.len() as u32,
                Some(output.as_mut_ptr() as *mut _),
                (output.len() * 8) as u32,
                Some(&mut written),
                None,
            )
        }?;
        Ok(written as usize / 8)
    }

    /// A function taking one value and returning one.
    pub fn read(&self, name: &str, argument: u64) -> windows::core::Result<u64> {
        let mut out = [0u64; 1];
        self.call(name, &[argument], &mut out)?;
        Ok(out[0])
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs administrator rights and the PawnIO driver; run with --ignored"]
    fn reads_the_cpu_temperature() {
        let blob = std::fs::read(env!("CARGO_MANIFEST_DIR").to_string() + "/pawnio-modules/AMDFamily17.bin").unwrap();
        let module = match Module::load(&blob) {
            Ok(module) => module,
            Err(error) => panic!("could not load the module: {error}"),
        };
        let raw = module.read("ioctl_read_smn", 0x0005_9800).unwrap();
        let celsius = ((raw >> 21) & 0x7FF) as f64 * 0.125 - if raw & (1 << 19) != 0 { 49.0 } else { 0.0 };
        println!("THM_TCON_CUR_TMP = {raw:#x} -> {celsius} °C");
    }
}
