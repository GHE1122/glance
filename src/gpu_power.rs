//! The power each graphics card draws, as its maker's driver reports it:
//! NVIDIA's through NVML, AMD's through ADL, Intel's through its control
//! library (IGCL), each shipped with the driver. Windows' own adapter data
//! gives power only as a share of an unstated maximum. NVIDIA's and AMD's
//! integrated graphics are left out: what they draw is part of the CPU
//! package's power, shown with the CPU. Intel's library also says how warm
//! its graphics are, their clock and their fan, which Windows' adapter data
//! does not have for them.
//!
//! Glance may run elevated, so the libraries are loaded from System32 only,
//! where the drivers put them and no ordinary program can.

use std::ffi::c_void;

use crate::reading::GpuLimit;

use windows::core::{s, PCSTR, PCWSTR};
use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
use windows::Win32::System::Memory::{GetProcessHeap, HeapAlloc, HEAP_ZERO_MEMORY};

/// Where a card sits on the PCI bus: bus, device and function numbers.
pub type PciAddress = (u32, u32, u32);

/// How one card's power is read.
#[derive(Clone, Copy)]
pub enum Reader {
    Nvidia(usize),
    Amd(usize),
    Intel(usize),
}

/// What a card's maker's library says of it besides its power.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct More {
    pub temp: Option<f32>,
    pub clock_mhz: Option<f32>,
    pub fan_rpm: Option<u32>,
}

impl Reader {
    /// Where a card's power comes from, for a report.
    pub fn describe(self) -> &'static str {
        match self {
            Reader::Nvidia(_) => "NVML",
            Reader::Amd(_) => "ADL",
            Reader::Intel(_) => "IGCL",
        }
    }
}

/// The vendors' libraries, those the machine has.
pub struct GpuPower {
    nvml: Option<Nvml>,
    adl: Option<Adl>,
    igcl: Option<Igcl>,
}

// The libraries' handles are used from one thread at a time (the sampler's).
unsafe impl Send for GpuPower {}

impl GpuPower {
    pub fn open() -> Self {
        GpuPower { nvml: Nvml::open(), adl: Adl::open(), igcl: Igcl::open() }
    }

    /// How to read the discrete card at `address`, if its driver says.
    pub fn reader(&self, address: PciAddress) -> Option<Reader> {
        let nvidia = self.nvml.as_ref().and_then(|nvml| nvml.devices.iter().position(|(at, _)| *at == address));
        let amd = || self.adl.as_ref().and_then(|adl| adl.adapters.iter().position(|(at, _)| *at == address));
        let intel = || self.igcl.as_ref().and_then(|igcl| igcl.devices.iter().position(|device| device.at == address));
        nvidia.map(Reader::Nvidia).or_else(|| amd().map(Reader::Amd)).or_else(|| intel().map(Reader::Intel))
    }

    /// What else the card's maker's library says of it now (only Intel's is asked).
    pub fn more(&self, reader: Reader) -> Option<More> {
        match reader {
            Reader::Intel(index) => self.igcl.as_ref()?.more(index),
            _ => None,
        }
    }

    /// What Intel's library found, for a report: each of its graphics, where it sits, and what it reads of it.
    pub fn describe_intel(&self) -> Option<String> {
        let igcl = self.igcl.as_ref()?;
        Some(igcl.devices.iter().enumerate().map(|(index, device)| format!("{:?}: {}", device.at, igcl.told(index))).collect::<Vec<_>>().join("; "))
    }

    /// The card's power now, in watts.
    pub fn read(&self, reader: Reader) -> Option<f32> {
        match reader {
            Reader::Nvidia(index) => self.nvml.as_ref()?.power(index),
            Reader::Amd(index) => self.adl.as_ref()?.power(index),
            Reader::Intel(index) => self.igcl.as_ref()?.power(index),
        }
    }

    /// What holds the card's clock back now, where its driver says (only
    /// NVIDIA's does).
    pub fn limit(&self, reader: Reader) -> Option<GpuLimit> {
        match reader {
            Reader::Nvidia(index) => self.nvml.as_ref()?.limit(index),
            Reader::Amd(_) | Reader::Intel(_) => None,
        }
    }
}

/// A function the library exports, as `F`.
unsafe fn export<F>(library: HMODULE, name: PCSTR) -> Option<F> {
    let address = unsafe { GetProcAddress(library, name) }?;
    Some(unsafe { std::mem::transmute_copy(&address) })
}

fn system_library(name: PCWSTR) -> Option<HMODULE> {
    unsafe { LoadLibraryExW(name, None, LOAD_LIBRARY_SEARCH_SYSTEM32) }.ok()
}

// ---- NVIDIA: NVML (nvml.h) ----

const NVML_SUCCESS: i32 = 0;

// The reasons NVML gives for a clock below the most it may run at
// (nvmlClocksEventReason*): the power limit, the GPU's own temperature and
// the board's slowdown (for heat or for its power supply, said apart by
// the two after it where the driver can).
const SW_POWER_CAP: u64 = 0x4;
const HW_SLOWDOWN: u64 = 0x8;
const SW_THERMAL_SLOWDOWN: u64 = 0x20;
const HW_THERMAL_SLOWDOWN: u64 = 0x40;
const HW_POWER_BRAKE_SLOWDOWN: u64 = 0x80;

/// What holds a clock back, from NVML's reasons; heat before power where
/// both do (the hotter card is the one to look at).
fn limit_of(reasons: u64) -> GpuLimit {
    if reasons & (SW_THERMAL_SLOWDOWN | HW_THERMAL_SLOWDOWN) != 0 {
        GpuLimit::Thermal
    } else if reasons & (SW_POWER_CAP | HW_POWER_BRAKE_SLOWDOWN) != 0 {
        GpuLimit::Power
    } else if reasons & HW_SLOWDOWN != 0 {
        GpuLimit::Hardware
    } else {
        GpuLimit::Free
    }
}

/// nvmlPciInfo_t.
#[repr(C)]
struct NvmlPciInfo {
    bus_id_legacy: [u8; 16],
    domain: u32,
    bus: u32,
    device: u32,
    pci_device_id: u32,
    pci_sub_system_id: u32,
    bus_id: [u8; 32],
}

type NvmlDevice = *mut c_void;

struct Nvml {
    library: HMODULE,
    power_usage: unsafe extern "C" fn(NvmlDevice, *mut u32) -> i32,
    /// The reasons the clock is held back; absent from drivers before NVML
    /// had them.
    clock_reasons: Option<unsafe extern "C" fn(NvmlDevice, *mut u64) -> i32>,
    shutdown: unsafe extern "C" fn() -> i32,
    /// Every card, by where it sits (a card's GPU is function 0).
    devices: Vec<(PciAddress, NvmlDevice)>,
}

impl Nvml {
    fn open() -> Option<Self> {
        let library = system_library(windows::core::w!("nvml.dll"))?;
        let opened = unsafe { Self::start(library) };
        if opened.is_none() {
            let _ = unsafe { FreeLibrary(library) };
        }
        opened
    }

    unsafe fn start(library: HMODULE) -> Option<Self> {
        let init: unsafe extern "C" fn() -> i32 = unsafe { export(library, s!("nvmlInit_v2")) }?;
        let shutdown: unsafe extern "C" fn() -> i32 = unsafe { export(library, s!("nvmlShutdown")) }?;
        let count: unsafe extern "C" fn(*mut u32) -> i32 = unsafe { export(library, s!("nvmlDeviceGetCount_v2")) }?;
        let handle: unsafe extern "C" fn(u32, *mut NvmlDevice) -> i32 = unsafe { export(library, s!("nvmlDeviceGetHandleByIndex_v2")) }?;
        let pci: unsafe extern "C" fn(NvmlDevice, *mut NvmlPciInfo) -> i32 = unsafe { export(library, s!("nvmlDeviceGetPciInfo_v3")) }?;
        let power_usage = unsafe { export(library, s!("nvmlDeviceGetPowerUsage")) }?;
        // Named "throttle" reasons before driver 535 ("clocks event" since).
        let clock_reasons = unsafe { export(library, s!("nvmlDeviceGetCurrentClocksEventReasons")) }
            .or_else(|| unsafe { export(library, s!("nvmlDeviceGetCurrentClocksThrottleReasons")) });
        if unsafe { init() } != NVML_SUCCESS {
            return None;
        }
        let mut devices = Vec::new();
        let mut n = 0u32;
        if unsafe { count(&mut n) } == NVML_SUCCESS {
            for index in 0..n {
                let mut device = std::ptr::null_mut();
                let mut info: NvmlPciInfo = unsafe { std::mem::zeroed() };
                if unsafe { handle(index, &mut device) } == NVML_SUCCESS && unsafe { pci(device, &mut info) } == NVML_SUCCESS {
                    devices.push(((info.bus, info.device, 0), device));
                }
            }
        }
        Some(Nvml { library, power_usage, clock_reasons, shutdown, devices })
    }

    fn power(&self, index: usize) -> Option<f32> {
        let mut milliwatts = 0u32;
        let read = unsafe { (self.power_usage)(self.devices.get(index)?.1, &mut milliwatts) };
        (read == NVML_SUCCESS).then(|| milliwatts as f32 / 1000.0)
    }

    fn limit(&self, index: usize) -> Option<GpuLimit> {
        let mut reasons = 0u64;
        let read = unsafe { (self.clock_reasons?)(self.devices.get(index)?.1, &mut reasons) };
        (read == NVML_SUCCESS).then(|| limit_of(reasons))
    }
}

impl Drop for Nvml {
    fn drop(&mut self) {
        unsafe {
            (self.shutdown)();
            let _ = FreeLibrary(self.library);
        }
    }
}

// ---- AMD: ADL (adl_sdk.h, adl_structures.h, adl_defines.h) ----

const ADL_OK: i32 = 0;
const ADL_MAX_PATH: usize = 256;
const ADL_ASIC_DISCRETE: i32 = 1 << 0;
const ADL_PMLOG_MAX_SENSORS: usize = 256;
/// The whole board's power, on newer cards; the chip's alone, on the rest.
const PMLOG_BOARD_POWER: usize = 73;
const PMLOG_ASIC_POWER: usize = 23;
/// AMD's PCI vendor id 1002h, as ADL writes it: the hex digits read as a
/// decimal number (AMD's sample compares with 1002; ADL also lists other
/// makers' cards, an NVIDIA one as 10).
const AMD_VENDOR: i32 = 1002;

/// AdapterInfo, as Windows builds lay it out.
#[repr(C)]
struct AdapterInfo {
    size: i32,
    adapter_index: i32,
    udid: [u8; ADL_MAX_PATH],
    bus_number: i32,
    device_number: i32,
    function_number: i32,
    vendor_id: i32,
    adapter_name: [u8; ADL_MAX_PATH],
    display_name: [u8; ADL_MAX_PATH],
    present: i32,
    exist: i32,
    driver_path: [u8; ADL_MAX_PATH],
    driver_path_ext: [u8; ADL_MAX_PATH],
    pnp_string: [u8; ADL_MAX_PATH],
    os_display_index: i32,
}

/// ADLPMLogDataOutput.
#[repr(C)]
struct PmLogData {
    size: i32,
    /// Each sensor: whether it is supported, and its value.
    sensors: [(i32, i32); ADL_PMLOG_MAX_SENSORS],
}

type AdlContext = *mut c_void;

struct Adl {
    library: HMODULE,
    context: AdlContext,
    query: unsafe extern "C" fn(AdlContext, i32, *mut PmLogData) -> i32,
    destroy: unsafe extern "C" fn(AdlContext) -> i32,
    /// Every discrete AMD card, by where it sits, with ADL's index for it.
    adapters: Vec<(PciAddress, i32)>,
}

/// What ADL allocates with: memory it is given and never frees here (the
/// calls made return nothing allocated).
unsafe extern "system" fn adl_allocate(size: i32) -> *mut c_void {
    unsafe { HeapAlloc(GetProcessHeap().unwrap_or_default(), HEAP_ZERO_MEMORY, size.max(0) as usize) }
}

impl Adl {
    fn open() -> Option<Self> {
        let library = system_library(windows::core::w!("atiadlxx.dll"))?;
        let opened = unsafe { Self::start(library) };
        if opened.is_none() {
            let _ = unsafe { FreeLibrary(library) };
        }
        opened
    }

    unsafe fn start(library: HMODULE) -> Option<Self> {
        type Allocate = unsafe extern "system" fn(i32) -> *mut c_void;
        let create: unsafe extern "C" fn(Allocate, i32, *mut AdlContext) -> i32 = unsafe { export(library, s!("ADL2_Main_Control_Create")) }?;
        let destroy: unsafe extern "C" fn(AdlContext) -> i32 = unsafe { export(library, s!("ADL2_Main_Control_Destroy")) }?;
        let count: unsafe extern "C" fn(AdlContext, *mut i32) -> i32 = unsafe { export(library, s!("ADL2_Adapter_NumberOfAdapters_Get")) }?;
        let infos: unsafe extern "C" fn(AdlContext, *mut AdapterInfo, i32) -> i32 = unsafe { export(library, s!("ADL2_Adapter_AdapterInfo_Get")) }?;
        let asic: unsafe extern "C" fn(AdlContext, i32, *mut i32, *mut i32) -> i32 =
            unsafe { export(library, s!("ADL2_Adapter_ASICFamilyType_Get")) }?;
        let query = unsafe { export(library, s!("ADL2_New_QueryPMLogData_Get")) }?;
        let mut context = std::ptr::null_mut();
        // Only the adapters that are there.
        if unsafe { create(adl_allocate, 1, &mut context) } != ADL_OK {
            return None;
        }
        let mut adapters = Vec::new();
        let mut n = 0;
        if unsafe { count(context, &mut n) } == ADL_OK && n > 0 {
            let mut list: Vec<AdapterInfo> = (0..n).map(|_| unsafe { std::mem::zeroed() }).collect();
            let bytes = (list.len() * size_of::<AdapterInfo>()) as i32;
            if unsafe { infos(context, list.as_mut_ptr(), bytes) } == ADL_OK {
                for info in &list {
                    let address = (info.bus_number as u32, info.device_number as u32, info.function_number as u32);
                    // ADL lists a card once for each of its outputs.
                    if info.vendor_id != AMD_VENDOR || adapters.iter().any(|(at, _)| *at == address) {
                        continue;
                    }
                    let (mut types, mut valid) = (0, 0);
                    let read = unsafe { asic(context, info.adapter_index, &mut types, &mut valid) };
                    if read == ADL_OK && valid & ADL_ASIC_DISCRETE != 0 && types & ADL_ASIC_DISCRETE != 0 {
                        adapters.push((address, info.adapter_index));
                    }
                }
            }
        }
        Some(Adl { library, context, query, destroy, adapters })
    }

    fn power(&self, index: usize) -> Option<f32> {
        let adapter = self.adapters.get(index)?.1;
        let mut data = PmLogData { size: size_of::<PmLogData>() as i32, sensors: [(0, 0); ADL_PMLOG_MAX_SENSORS] };
        if unsafe { (self.query)(self.context, adapter, &mut data) } != ADL_OK {
            return None;
        }
        [PMLOG_BOARD_POWER, PMLOG_ASIC_POWER]
            .into_iter()
            .map(|sensor| data.sensors[sensor])
            .find(|(supported, _)| *supported != 0)
            .map(|(_, watts)| watts as f32)
    }
}

impl Drop for Adl {
    fn drop(&mut self) {
        unsafe {
            (self.destroy)(self.context);
            let _ = FreeLibrary(self.library);
        }
    }
}

// ---- Intel: IGCL (igcl_api.h) ----

const CTL_SUCCESS: u32 = 0;
/// The version of the library this is written for: 1.1.
const CTL_VERSION: u32 = 1 << 16 | 1;
const INTEL_VENDOR: u32 = 0x8086;
type CtlHandle = *mut c_void;

#[repr(C)]
struct CtlInitArgs {
    size: u32,
    version: u8,
    app_version: u32,
    flags: u32,
    supported_version: u32,
    application: [u32; 4],
}

#[repr(C)]
struct CtlAdapterProperties {
    size: u32,
    version: u8,
    device_id: *mut c_void,
    device_id_size: u32,
    device_type: i32,
    supported: u32,
    driver_version: u64,
    firmware_version: [u64; 3],
    pci_vendor_id: u32,
    pci_device_id: u32,
    rev_id: u32,
    eus_per_sub_slice: u32,
    sub_slices_per_slice: u32,
    slices: u32,
    name: [u8; 100],
    graphics_adapter_properties: u32,
    frequency: u32,
    pci_subsys_id: u16,
    pci_subsys_vendor_id: u16,
    bdf: [u8; 3],
    xe_cores: u32,
    reserved: [u8; 108],
}

/// A value the library tells, of some type and in some unit.
#[repr(C)]
#[derive(Clone, Copy)]
struct CtlItem {
    supported: bool,
    units: i32,
    kind: i32,
    /// A union of every type a value may have, the widest of them 8 bytes.
    value: u64,
}

impl CtlItem {
    /// The value as a number, whatever type it is told in; none where the library does not tell it.
    fn number(&self) -> Option<f64> {
        if !self.supported {
            return None;
        }
        let bytes = self.value.to_ne_bytes();
        Some(match self.kind {
            0 => bytes[0] as i8 as f64,
            1 => bytes[0] as f64,
            2 => i16::from_ne_bytes([bytes[0], bytes[1]]) as f64,
            3 => u16::from_ne_bytes([bytes[0], bytes[1]]) as f64,
            4 => i32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64,
            5 => u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64,
            6 => self.value as i64 as f64,
            7 => self.value as f64,
            8 => f32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64,
            9 => f64::from_ne_bytes(bytes),
            _ => return None,
        })
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CtlPsu {
    supported: bool,
    kind: i32,
    energy: CtlItem,
    voltage: CtlItem,
}

/// ctl_power_telemetry_t, as the header has it at version 1.
#[repr(C)]
struct CtlTelemetry {
    size: u32,
    version: u8,
    time_stamp: CtlItem,
    gpu_energy: CtlItem,
    gpu_voltage: CtlItem,
    gpu_clock: CtlItem,
    gpu_temperature: CtlItem,
    global_activity: CtlItem,
    render_activity: CtlItem,
    media_activity: CtlItem,
    gpu_limited: [bool; 5],
    vram_energy: CtlItem,
    vram_voltage: CtlItem,
    vram_clock: CtlItem,
    vram_effective_clock: CtlItem,
    vram_read: CtlItem,
    vram_written: CtlItem,
    vram_temperature: CtlItem,
    vram_limited: [bool; 5],
    card_energy: CtlItem,
    psu: [CtlPsu; 5],
    fans: [CtlItem; 5],
    gpu_vr_temperature: CtlItem,
    vram_vr_temperature: CtlItem,
    sa_vr_temperature: CtlItem,
    gpu_effective_clock: CtlItem,
    gpu_over_voltage: CtlItem,
    gpu_power_percent: CtlItem,
    gpu_temperature_percent: CtlItem,
    vram_read_bandwidth: CtlItem,
    vram_write_bandwidth: CtlItem,
}

struct IntelDevice {
    at: PciAddress,
    handle: CtlHandle,
    /// The energy counter as last read, and when (the library's own clock, seconds): the power is what was
    /// drawn between two readings.
    last: std::cell::Cell<Option<(f64, f64)>>,
}

struct Igcl {
    library: HMODULE,
    api: CtlHandle,
    close: unsafe extern "C" fn(CtlHandle) -> u32,
    telemetry: unsafe extern "C" fn(CtlHandle, *mut CtlTelemetry) -> u32,
    /// Intel's graphics, by where each sits.
    devices: Vec<IntelDevice>,
}

impl Igcl {
    fn open() -> Option<Self> {
        let library = system_library(windows::core::w!("ControlLib.dll"))?;
        let opened = unsafe { Self::start(library) };
        if opened.is_none() {
            let _ = unsafe { FreeLibrary(library) };
        }
        opened
    }

    unsafe fn start(library: HMODULE) -> Option<Self> {
        let init: unsafe extern "C" fn(*mut CtlInitArgs, *mut CtlHandle) -> u32 = unsafe { export(library, s!("ctlInit")) }?;
        let close: unsafe extern "C" fn(CtlHandle) -> u32 = unsafe { export(library, s!("ctlClose")) }?;
        let enumerate: unsafe extern "C" fn(CtlHandle, *mut u32, *mut CtlHandle) -> u32 = unsafe { export(library, s!("ctlEnumerateDevices")) }?;
        let properties: unsafe extern "C" fn(CtlHandle, *mut CtlAdapterProperties) -> u32 = unsafe { export(library, s!("ctlGetDeviceProperties")) }?;
        let telemetry = unsafe { export(library, s!("ctlPowerTelemetryGet")) }?;
        let mut args = CtlInitArgs { size: size_of::<CtlInitArgs>() as u32, version: 0, app_version: CTL_VERSION, flags: 0, supported_version: 0, application: [0; 4] };
        let mut api: CtlHandle = std::ptr::null_mut();
        if unsafe { init(&mut args, &mut api) } != CTL_SUCCESS || api.is_null() {
            return None;
        }
        let mut found = Igcl { library, api, close, telemetry, devices: Vec::new() };
        let mut count = 0u32;
        if unsafe { enumerate(api, &mut count, std::ptr::null_mut()) } != CTL_SUCCESS || count == 0 {
            return Some(found);
        }
        let mut handles = vec![std::ptr::null_mut(); count as usize];
        if unsafe { enumerate(api, &mut count, handles.as_mut_ptr()) } != CTL_SUCCESS {
            return Some(found);
        }
        for handle in handles.into_iter().take(count as usize) {
            // (The adapter's own identifier, as Windows has it, is asked for with the rest: 8 bytes for it.)
            let mut luid = [0u8; 8];
            let mut about: CtlAdapterProperties = unsafe { std::mem::zeroed() };
            about.size = size_of::<CtlAdapterProperties>() as u32;
            about.version = 2;
            about.device_id = luid.as_mut_ptr().cast();
            about.device_id_size = luid.len() as u32;
            if unsafe { properties(handle, &mut about) } == CTL_SUCCESS && about.pci_vendor_id == INTEL_VENDOR {
                found.devices.push(IntelDevice { at: (about.bdf[0] as u32, about.bdf[1] as u32, about.bdf[2] as u32), handle, last: std::cell::Cell::new(None) });
            }
        }
        Some(found)
    }

    fn read(&self, index: usize) -> Option<CtlTelemetry> {
        let mut read: CtlTelemetry = unsafe { std::mem::zeroed() };
        read.size = size_of::<CtlTelemetry>() as u32;
        read.version = 1;
        (unsafe { (self.telemetry)(self.devices.get(index)?.handle, &mut read) } == CTL_SUCCESS).then_some(read)
    }

    /// The watts drawn since it was last asked: the energy counter's rise over the time between.
    fn power(&self, index: usize) -> Option<f32> {
        let read = self.read(index)?;
        let now = (read.time_stamp.number()?, read.gpu_energy.number()?);
        let before = self.devices[index].last.replace(Some(now))?;
        let (seconds, joules) = (now.0 - before.0, now.1 - before.1);
        (seconds > 0.0 && joules >= 0.0).then(|| (joules / seconds) as f32)
    }

    fn more(&self, index: usize) -> Option<More> {
        let read = self.read(index)?;
        Some(More {
            temp: read.gpu_temperature.number().filter(|celsius| *celsius > 0.0).map(|celsius| celsius as f32),
            clock_mhz: read.gpu_clock.number().filter(|mhz| *mhz > 0.0).map(|mhz| mhz as f32),
            fan_rpm: read.fans.iter().filter_map(CtlItem::number).find(|rpm| *rpm > 0.0).map(|rpm| rpm as u32),
        })
    }

    /// Which of its readings the library gives for a device, for a report.
    fn told(&self, index: usize) -> String {
        let Some(read) = self.read(index) else { return "telemetry not read".into() };
        let has = |item: &CtlItem| if item.supported { "yes" } else { "no" };
        format!("temperature {}, clock {}, energy {}, fans {}", has(&read.gpu_temperature), has(&read.gpu_clock), has(&read.gpu_energy), read.fans.iter().filter(|fan| fan.supported).count())
    }
}

impl Drop for Igcl {
    fn drop(&mut self) {
        unsafe {
            (self.close)(self.api);
            let _ = FreeLibrary(self.library);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn lays_out_intels_structures() {
        use super::*;
        // As igcl_api.h lays them out, on 64-bit Windows.
        assert_eq!(size_of::<CtlItem>(), 24);
        assert_eq!(size_of::<CtlPsu>(), 8 + 2 * 24);
        assert_eq!(size_of::<CtlInitArgs>(), 4 + 4 + 4 + 4 + 4 + 16);
        assert_eq!(size_of::<CtlTelemetry>(), 8 + 8 * 24 + 8 + 7 * 24 + 8 + 24 + 5 * 56 + 5 * 24 + 9 * 24);
        assert_eq!(std::mem::offset_of!(CtlAdapterProperties, pci_vendor_id), 8 + 8 + 4 + 4 + 4 + 4 + 8 + 24);
        assert_eq!(std::mem::offset_of!(CtlAdapterProperties, bdf), 64 + 6 * 4 + 100 + 4 + 4 + 2 + 2);
        // (And as a program compiled against the header itself says they are.)
        assert_eq!((size_of::<CtlTelemetry>(), size_of::<CtlAdapterProperties>()), (1024, 320));
        assert_eq!(std::mem::offset_of!(CtlTelemetry, time_stamp), 8);
        assert_eq!(std::mem::offset_of!(CtlTelemetry, gpu_energy), 32);
        assert_eq!(std::mem::offset_of!(CtlTelemetry, gpu_clock), 80);
        assert_eq!(std::mem::offset_of!(CtlTelemetry, gpu_temperature), 104);
        assert_eq!(std::mem::offset_of!(CtlTelemetry, fans), 688);
        // A value in whichever type it is told.
        let item = |kind: i32, value: u64| CtlItem { supported: true, units: 0, kind, value };
        assert_eq!(item(9, 61.5f64.to_bits()).number(), Some(61.5));
        assert_eq!(item(8, 2400.0f32.to_bits() as u64).number(), Some(2400.0));
        assert_eq!(item(5, 1800).number(), Some(1800.0));
        assert_eq!(CtlItem { supported: false, ..item(9, 0) }.number(), None);
    }

    #[test]
    fn lays_out_the_vendors_structures() {
        // As their headers lay them out, on 64-bit Windows.
        assert_eq!(size_of::<super::NvmlPciInfo>(), 16 + 5 * 4 + 32);
        assert_eq!(size_of::<super::AdapterInfo>(), 4 * 2 + 256 + 4 * 4 + 256 * 2 + 4 * 2 + 256 * 3 + 4);
        assert_eq!(size_of::<super::PmLogData>(), 4 + 256 * 8);
    }

    #[test]
    fn names_what_holds_a_clock_back() {
        use super::*;
        assert_eq!(limit_of(0), GpuLimit::Free);
        // Idle, and clocks the user set: nothing holding it back.
        assert_eq!(limit_of(0x1 | 0x2), GpuLimit::Free);
        assert_eq!(limit_of(SW_POWER_CAP), GpuLimit::Power);
        assert_eq!(limit_of(HW_SLOWDOWN | HW_POWER_BRAKE_SLOWDOWN), GpuLimit::Power);
        assert_eq!(limit_of(HW_SLOWDOWN | HW_THERMAL_SLOWDOWN), GpuLimit::Thermal);
        assert_eq!(limit_of(SW_POWER_CAP | SW_THERMAL_SLOWDOWN), GpuLimit::Thermal);
        assert_eq!(limit_of(HW_SLOWDOWN), GpuLimit::Hardware);
    }

    /// Prints what this machine's cards draw.
    #[test]
    fn reads_this_machines_cards() {
        let power = super::GpuPower::open();
        if let Some(nvml) = &power.nvml {
            for (index, (at, _)) in nvml.devices.iter().enumerate() {
                println!("NVIDIA at {at:?}: {:?} W, held back by {:?}", nvml.power(index), nvml.limit(index));
            }
        }
        if let Some(adl) = &power.adl {
            println!("AMD discrete cards: {:?}", adl.adapters.iter().map(|(at, _)| at).collect::<Vec<_>>());
        }
    }
}
