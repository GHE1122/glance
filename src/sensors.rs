//! Sensors only a driver can reach, read through PawnIO: the CPU's own
//! temperature and power. Needs administrator rights and the PawnIO driver;
//! without either there are simply no readings.

use crate::reading::CpuSensors;
use std::time::Instant;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows::Win32::System::SystemInformation::{
    GetLogicalProcessorInformationEx, RelationProcessorCore, RelationProcessorPackage, GROUP_AFFINITY, PROCESSOR_RELATIONSHIP,
    LOGICAL_PROCESSOR_RELATIONSHIP, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentThread, ReleaseMutex, SetThreadGroupAffinity, WaitForSingleObject};

use crate::pawnio::Module;

/// The modules, compiled by the PawnIO project and shipped beside the program.
const AMD_FAMILY_17: &[u8] = include_bytes!("../pawnio-modules/AMDFamily17.bin");

// AMD Zen register map (families 17h, 19h, 1Ah).
/// Tctl, the control temperature: CUR_TEMP in bits 31–21, in eighths of a
/// degree; bit 19 selects the range that starts at −49 °C.
const THM_TCON_CUR_TMP: u64 = 0x0005_9800;
/// The block the CCDs' temperatures are reported in, one word per CCD from
/// an offset that depends on the processor (`ccd_layout`): bits 10–0 in
/// eighths of a degree from −49 °C, bit 11 set while the reading is valid
/// (a CCD absent, or one the processor reports nothing for, has it clear).
const REPORTED_TEMP_BASE: u64 = 0x0005_9800;
const CCD_TEMP_VALID: u64 = 1 << 11;
const CCD_TEMP_MASK: u64 = 0x7FF;

/// Where the CCDs' temperatures are (their offset in the reported block)
/// and how many CCDs there can be, by family and model, as Linux's k10temp
/// has them; none for a processor that reports none (mobile Zen 5 among
/// them).
fn ccd_layout(family: u32, model: u32) -> Option<(u64, u64)> {
    Some(match (family, model) {
        (0x17 | 0x18, 0x01 | 0x08 | 0x11 | 0x18) => (0x154, 4),
        (0x17 | 0x18, 0x31 | 0x47 | 0x60 | 0x68 | 0x71) => (0x154, 8),
        (0x17 | 0x18, 0xA0..=0xAF) => (0x300, 8),
        (0x19, 0x00..=0x01 | 0x08 | 0x21 | 0x50..=0x5F) => (0x154, 8),
        (0x19, 0x40..=0x4F) => (0x300, 8),
        (0x19, 0x60..=0x7F) => (0x308, 8),
        (0x19, 0x10..=0x1F | 0xA0..=0xAF) => (0x300, 12),
        (0x1A, 0x00..=0x1F) => (0x1F0, 16),
        (0x1A, 0x40..=0x4F) => (0x308, 8),
        _ => return None,
    })
}
const MSR_PWR_UNIT: u64 = 0xC001_0299;
const MSR_PKG_ENERGY_STAT: u64 = 0xC001_029B;


/// A lock that every hardware monitor on Windows agrees to take before
/// touching a shared piece of hardware: the PCI configuration registers
/// (SMN reads go through a pair of them) or the ISA bus (Super I/O chips).
/// Two programs must not interleave their accesses.
pub struct NamedLock(HANDLE);

/// How long to wait for another program's use of a bus to end (ms): longer
/// than its transfers take, short enough not to hold the readings up.
pub const LOCK_WAIT: u32 = 50;

/// The PCI bus lock's name.
const PCI_LOCK: PCWSTR = w!("Global\\Access_PCI");

impl NamedLock {
    /// Waits up to `timeout_ms` for the lock; `None` if another program has it.
    pub fn acquire(name: PCWSTR, timeout_ms: u32) -> Option<Self> {
        Self::taken(name, timeout_ms).ok()
    }

    /// The same, saying what stood in the way: the lock not to be opened
    /// at all (another program made it for itself alone), or held by
    /// another program all the while.
    pub fn taken(name: PCWSTR, timeout_ms: u32) -> Result<Self, String> {
        let mutex = unsafe { CreateMutexW(None, false, name) }.map_err(|error| format!("its lock could not be opened ({error})"))?;
        let waited = unsafe { WaitForSingleObject(mutex, timeout_ms) };
        if waited == WAIT_OBJECT_0 || waited == WAIT_ABANDONED {
            Ok(NamedLock(mutex))
        } else {
            let _ = unsafe { CloseHandle(mutex) };
            Err(format!("held by another program for {timeout_ms} ms and more"))
        }
    }
}

impl Drop for NamedLock {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

/// A set of processors, as Windows names one: a processor group and a mask
/// of processors in it.
type Processors = GROUP_AFFINITY;

/// The processors of each CPU package (socket), or of each core: one entry
/// each, holding some of its processors (a package spread over several
/// processor groups is named by its first).
fn topology(relation: LOGICAL_PROCESSOR_RELATIONSHIP) -> Vec<Processors> {
    let mut length = 0u32;
    let _ = unsafe { GetLogicalProcessorInformationEx(relation, None, &mut length) };
    // u64 words keep the records aligned.
    let mut buffer = vec![0u64; (length as usize).div_ceil(8)];
    let records = buffer.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX;
    if unsafe { GetLogicalProcessorInformationEx(relation, Some(records), &mut length) }.is_err() {
        return Vec::new();
    }
    // Each record is as long as its Size says, shorter than the Rust type
    // (whose union holds the largest relation): its fields are read where
    // they lie, never through a reference to the whole type.
    let bytes = unsafe { std::slice::from_raw_parts(buffer.as_ptr() as *const u8, length as usize) };
    let size_at = std::mem::offset_of!(SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX, Size);
    let processor_at = std::mem::offset_of!(SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX, Anonymous);
    let count_at = processor_at + std::mem::offset_of!(PROCESSOR_RELATIONSHIP, GroupCount);
    let mask_at = processor_at + std::mem::offset_of!(PROCESSOR_RELATIONSHIP, GroupMask);
    let read = |at: usize, len: usize| bytes.get(at..at + len);
    let mut found = Vec::new();
    let mut offset = 0usize;
    while let Some(size) = read(offset + size_at, 4).map(|b| u32::from_ne_bytes(b.try_into().unwrap()) as usize) {
        let count = read(offset + count_at, 2).map(|b| u16::from_ne_bytes(b.try_into().unwrap()));
        let mask = read(offset + mask_at, std::mem::size_of::<Processors>())
            .map(|b| unsafe { (b.as_ptr() as *const Processors).read_unaligned() });
        if let (Some(1..), Some(mask)) = (count, mask) {
            found.push(mask);
        }
        if size == 0 {
            break;
        }
        offset += size;
    }
    found
}

/// Runs `read` on one of `processors`: MSRs are per package or per core, and
/// the driver reads them on the processor its caller runs on. The thread
/// goes back to where it may run afterwards.
fn on<R>(processors: &Processors, read: impl FnOnce() -> R) -> Option<R> {
    let thread = unsafe { GetCurrentThread() };
    let mut before = Processors::default();
    if !unsafe { SetThreadGroupAffinity(thread, processors, Some(&mut before)) }.as_bool() {
        return None;
    }
    let result = read();
    let _ = unsafe { SetThreadGroupAffinity(thread, &before, None) };
    Some(result)
}

/// No CPU package draws a kilowatt. A gap between two readings of an energy
/// counter shorter than it takes to wrap at that power holds at most one
/// wrap, which wrapping subtraction undoes; over a longer gap (a stalled or
/// suspended process) whole wraps could hide, so it gives no reading.
const MOST_WATTS: f64 = 1000.0;

/// A package's energy counter: 32 bits of `unit` joules, wrapping around.
struct EnergyCounter {
    unit: f64,
    last: Option<(u32, Instant)>,
    /// Whether it has ever moved.
    counting: bool,
}

impl EnergyCounter {
    fn new(unit: f64) -> Self {
        EnergyCounter { unit, last: None, counting: false }
    }

    /// The power drawn since the previous count, given the count now. A
    /// counter that has never moved is not counting (a running package always
    /// draws power; a hypervisor may return a constant): no reading. One that
    /// has, standing still, says nothing was drawn (the graphics' plane, idle).
    fn power(&mut self, count: u32) -> Option<f32> {
        self.power_at(count, Instant::now())
    }

    /// As `power`, the count taken `now`.
    fn power_at(&mut self, count: u32, now: Instant) -> Option<f32> {
        let longest = (1u64 << 32) as f64 * self.unit / MOST_WATTS;
        let power = self.last.and_then(|(before, at)| {
            let seconds = now.duration_since(at).as_secs_f64();
            let counted = count.wrapping_sub(before);
            self.counting |= counted > 0;
            (self.counting && seconds > 0.0 && seconds < longest).then(|| (counted as f64 * self.unit / seconds) as f32)
        });
        self.last = Some((count, now));
        power
    }
}

/// The whole CPU's power: every package's, or none if one cannot be read.
fn total_power(module: &Module, packages: &mut [(Processors, EnergyCounter)], msr: u64) -> Option<f32> {
    let mut total = Some(0.0);
    for (processors, counter) in packages {
        let count = on(processors, || module.read("ioctl_read_msr", msr).ok()).flatten();
        // Every counter is read, to keep each one's baseline current.
        let power = count.and_then(|count| counter.power(count as u32));
        total = total.zip(power).map(|(sum, watts)| sum + watts);
    }
    total
}

pub struct AmdCpu {
    module: Module,
    /// Where its CCDs' temperatures are, and how many there can be.
    ccds: Option<(u64, u64)>,
    /// Each package's processors and energy counter.
    packages: Vec<(Processors, EnergyCounter)>,
    /// The latest temperatures and when they were read, kept while another program has the PCI bus (no longer than
    /// `STALE_AFTER`).
    last: (Option<f32>, Vec<(usize, f32)>, Instant),
    /// How far above the die's own temperature its control temperature (Tctl) reads, on the CPUs that read so
    /// (see `tctl_offset`).
    tctl_offset: f32,
}

/// How far above the die's temperature an AMD CPU's control temperature (Tctl) reads (degrees), from its family and
/// its name: the CPUs that read so, and by how much, as Linux's k10temp driver has them (`tctl_offset_table`), named
/// by a part of their name. Most read none.
fn tctl_offset(family: u32, name: &str) -> f32 {
    const OFFSETS: [(u32, &str, f32); 6] = [
        (0x17, "AMD Ryzen 5 1600X", 20.0),
        (0x17, "AMD Ryzen 7 1700X", 20.0),
        (0x17, "AMD Ryzen 7 1800X", 20.0),
        (0x17, "AMD Ryzen 7 2700X", 10.0),
        (0x17, "AMD Ryzen Threadripper 19", 27.0),
        (0x17, "AMD Ryzen Threadripper 29", 27.0),
    ];
    OFFSETS.iter().find(|(of, part, _)| *of == family && name.contains(part)).map_or(0.0, |(.., offset)| *offset)
}

/// How long the last temperatures stand for the CPU's while another program has the bus.
const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(10);

/// The CPU's name as it says it (CPUID leaves 0x80000002 to 0x80000004).
fn cpu_name() -> String {
    let bytes: Vec<u8> = (0x8000_0002u32..=0x8000_0004)
        .flat_map(|leaf| {
            let r = std::arch::x86_64::__cpuid(leaf);
            [r.eax, r.ebx, r.ecx, r.edx].into_iter().flat_map(u32::to_le_bytes)
        })
        .collect();
    String::from_utf8_lossy(&bytes).trim_matches(char::from(0)).trim().to_string()
}

impl AmdCpu {
    /// The CPU's sensors, if it is an AMD Zen and the driver can be used.
    pub fn open() -> Option<Self> {
        let module = Module::load(AMD_FAMILY_17).ok()?;
        let (family, model) = cpu_family_model();
        let ccds = ccd_layout(family, model);
        let units = module.read("ioctl_read_msr", MSR_PWR_UNIT).ok()?;
        let unit = 1.0 / (1u64 << ((units >> 8) & 0x1F)) as f64;
        let packages = topology(RelationProcessorPackage).into_iter().map(|p| (p, EnergyCounter::new(unit))).collect();
        Some(AmdCpu { module, ccds, packages, last: (None, Vec::new(), Instant::now()), tctl_offset: tctl_offset(family, &cpu_name()) })
    }

    /// One SMN register. Reached through an index and a data register on
    /// the PCI bus, so only under the PCI lock other programs share.
    fn smn(&self, address: u64) -> Option<u64> {
        self.module.read("ioctl_read_smn", address).ok()
    }

    pub fn read(&mut self) -> CpuSensors {
        let mut sensors = CpuSensors::default();
        // The temperatures are the first socket's: the module reaches SMN
        // through the PCI root at 0/0/0 only, whichever processor asks
        // (power, read from MSRs, covers every socket).
        // While another program has the bus, the last temperatures stand.
        if let Some(_lock) = NamedLock::acquire(PCI_LOCK, LOCK_WAIT) {
            let mut ccds = Vec::new();
            // (The die's: the control temperature less what it reads above it, on the CPUs that do.)
            let temp = self.smn(THM_TCON_CUR_TMP).map(|raw| {
                let offset = if raw & (1 << 19) != 0 { 49.0 } else { 0.0 };
                ((raw >> 21) & 0x7FF) as f32 * 0.125 - offset - self.tctl_offset
            });
            let (offset, count) = self.ccds.unwrap_or_default();
            for ccd in 0..count {
                let Some(raw) = self.smn(REPORTED_TEMP_BASE + offset + ccd * 4) else { continue };
                // A register out of reach reads all ones, which would pass
                // for valid.
                if raw & 0xFFFF_FFFF != 0xFFFF_FFFF && raw & CCD_TEMP_VALID != 0 {
                    ccds.push((ccd as usize, (raw & CCD_TEMP_MASK) as f32 * 0.125 - 49.0));
                }
            }
            self.last = (temp, ccds, Instant::now());
        }
        // (Read too long ago, none: they would show a temperature frozen as if live.)
        let (temp, ccds, at) = &self.last;
        if at.elapsed() < STALE_AFTER {
            (sensors.temp, sensors.ccds) = (*temp, ccds.clone());
        }
        sensors.power = total_power(&self.module, &mut self.packages, MSR_PKG_ENERGY_STAT);
        sensors
    }
}

/// An Intel CPU's sensors, read from its model-specific registers through
/// PawnIO's IntelMSR module: its temperature (the hottest package's, or
/// core's) and its power. Registers per Intel's Software Developer's Manual,
/// vol. 4.
pub struct IntelCpu {
    module: Module,
    /// Where temperatures are read: each package, on CPUs that report a
    /// package temperature, else each core; with the temperature it throttles
    /// at, which readings count down from.
    thermal: Vec<(Processors, f32)>,
    /// Which register holds those readings.
    thermal_status: u64,
    /// Each package's processors and energy counter, if the CPU has RAPL.
    packages: Vec<(Processors, EnergyCounter)>,
    /// The first package's graphics plane's (PP1: the graphics built into it) energy counter, where the CPU has one.
    graphics: Option<(Processors, EnergyCounter)>,
}

const INTEL_MSR: &[u8] = include_bytes!("../pawnio-modules/IntelMSR.bin");
const IA32_THERM_STATUS: u64 = 0x19C;
const IA32_TEMPERATURE_TARGET: u64 = 0x1A2;
const IA32_PACKAGE_THERM_STATUS: u64 = 0x1B1;
const MSR_RAPL_POWER_UNIT: u64 = 0x606;
const MSR_PKG_ENERGY_STATUS: u64 = 0x611;
const MSR_PP1_ENERGY_STATUS: u64 = 0x641;

impl IntelCpu {
    /// The CPU's sensors, if it is an Intel one, the driver can be used (the
    /// module declines to load on other CPUs) and it reports any.
    pub fn open() -> Option<Self> {
        let module = Module::load(INTEL_MSR).ok()?;
        let packages = topology(RelationProcessorPackage);
        // CPUID leaf 6: EAX bit 0, digital thermal sensors; bit 6, package
        // thermal management (Sandy Bridge on).
        let power_management = std::arch::x86_64::__cpuid(6).eax;
        let (places, thermal_status) = if power_management & (1 << 6) != 0 {
            (packages.clone(), IA32_PACKAGE_THERM_STATUS)
        } else if power_management & 1 != 0 {
            (topology(RelationProcessorCore), IA32_THERM_STATUS)
        } else {
            (Vec::new(), IA32_THERM_STATUS)
        };
        // A place whose CPU does not report its throttling point (zero, or no
        // register) has nothing to count down from.
        let thermal = places
            .into_iter()
            .filter_map(|place| {
                let target = on(&place, || module.read("ioctl_read_msr", IA32_TEMPERATURE_TARGET).ok()).flatten()?;
                let tj_max = ((target >> 16) & 0xFF) as f32;
                (tj_max > 0.0).then_some((place, tj_max))
            })
            .collect::<Vec<_>>();
        let unit = module.read("ioctl_read_msr", MSR_RAPL_POWER_UNIT).ok().map(|units| energy_unit(units, cpu_family_model_intel()));
        // (The graphics' plane, on CPUs with graphics built in: a register a CPU without one refuses to be read.)
        let graphics = unit.zip(packages.first().cloned()).filter(|(_, first)| on(first, || module.read("ioctl_read_msr", MSR_PP1_ENERGY_STATUS).ok()).flatten().is_some()).map(|(unit, first)| (first, EnergyCounter::new(unit)));
        let packages: Vec<(Processors, EnergyCounter)> = match unit {
            Some(unit) => packages.into_iter().map(|p| (p, EnergyCounter::new(unit))).collect(),
            None => Vec::new(),
        };
        if thermal.is_empty() && packages.is_empty() {
            return None;
        }
        Some(IntelCpu { module, thermal, thermal_status, packages, graphics })
    }

    pub fn read(&mut self) -> CpuSensors {
        let mut sensors = CpuSensors::default();
        for (place, tj_max) in &self.thermal {
            let Some(Ok(status)) = on(place, || self.module.read("ioctl_read_msr", self.thermal_status)) else { continue };
            // A core's reading is good only while bit 31 says so (the
            // package register has no such bit).
            if self.thermal_status == IA32_THERM_STATUS && status & (1 << 31) == 0 {
                continue;
            }
            // How far below the throttling point it is: bits 22–16, widened
            // to 23–16 on newer CPUs (always zero above on older ones).
            let celsius = tj_max - ((status >> 16) & 0xFF) as f32;
            sensors.temp = Some(sensors.temp.map_or(celsius, |hottest: f32| hottest.max(celsius)));
        }
        if !self.packages.is_empty() {
            sensors.power = total_power(&self.module, &mut self.packages, MSR_PKG_ENERGY_STATUS);
        }
        if let Some((first, counter)) = &mut self.graphics {
            let count = on(first, || self.module.read("ioctl_read_msr", MSR_PP1_ENERGY_STATUS).ok()).flatten();
            sensors.graphics_power = count.and_then(|count| counter.power(count as u32));
        }
        sensors
    }
}

/// Joules per count of RAPL's energy counters, from MSR_RAPL_POWER_UNIT's
/// energy status units (bits 12–8): 1/2^ESU joules, except on Silvermont
/// and Airmont Atoms, where they count 2^ESU microjoules.
fn energy_unit(units: u64, (family, model): (u32, u32)) -> f64 {
    let esu = ((units >> 8) & 0x1F) as i32;
    let microjoule_atoms = [0x37, 0x4A, 0x4D, 0x5A, 0x5D, 0x4C];
    if family == 6 && microjoule_atoms.contains(&model) {
        2f64.powi(esu) * 1e-6
    } else {
        2f64.powi(-esu)
    }
}

/// The CPU's own sensors, whichever maker's.
pub enum CpuReader {
    Amd(AmdCpu),
    Intel(IntelCpu),
}

impl CpuReader {
    pub fn open() -> Option<Self> {
        AmdCpu::open().map(CpuReader::Amd).or_else(|| IntelCpu::open().map(CpuReader::Intel))
    }

    pub fn read(&mut self) -> CpuSensors {
        match self {
            CpuReader::Amd(cpu) => cpu.read(),
            CpuReader::Intel(cpu) => cpu.read(),
        }
    }

    /// Which way the CPU is read, for a report.
    pub fn describe(&self) -> &'static str {
        match self {
            CpuReader::Amd(_) => "AMD (SMN temperatures, RAPL energy)",
            CpuReader::Intel(_) => "Intel (MSR temperatures and energy)",
        }
    }
}

/// The CPU's family and model, as AMD defines them from CPUID leaf 1.
fn cpu_family_model() -> (u32, u32) {
    let leaf = std::arch::x86_64::__cpuid(1);
    let base_family = (leaf.eax >> 8) & 0xF;
    let base_model = (leaf.eax >> 4) & 0xF;
    let family = if base_family == 0xF { base_family + ((leaf.eax >> 20) & 0xFF) } else { base_family };
    let model = if base_family == 0xF { base_model | (((leaf.eax >> 16) & 0xF) << 4) } else { base_model };
    (family, model)
}

/// The CPU's family and model, as Intel defines them from CPUID leaf 1: the
/// extended model counts for families 6 and 15.
fn cpu_family_model_intel() -> (u32, u32) {
    let leaf = std::arch::x86_64::__cpuid(1);
    let base_family = (leaf.eax >> 8) & 0xF;
    let base_model = (leaf.eax >> 4) & 0xF;
    let family = if base_family == 0xF { base_family + ((leaf.eax >> 20) & 0xFF) } else { base_family };
    let model = if base_family == 6 || base_family == 0xF { base_model | (((leaf.eax >> 16) & 0xF) << 4) } else { base_model };
    (family, model)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CPUs whose control temperature reads above their die's are read less by as much, by their name and
    /// family as Linux's k10temp has them; the others as they read.
    #[test]
    fn some_cpus_read_their_control_temperature_above_the_die() {
        assert_eq!(tctl_offset(0x17, "AMD Ryzen 7 1800X Eight-Core Processor"), 20.0);
        assert_eq!(tctl_offset(0x17, "AMD Ryzen 7 2700X Eight-Core Processor"), 10.0);
        assert_eq!(tctl_offset(0x17, "AMD Ryzen Threadripper 2950X 16-Core Processor"), 27.0);
        assert_eq!(tctl_offset(0x17, "AMD Ryzen 7 2700 Eight-Core Processor"), 0.0);
        assert_eq!(tctl_offset(0x19, "AMD Ryzen 9 7950X 16-Core Processor"), 0.0);
        assert_eq!(tctl_offset(0x1A, "AMD Ryzen 7 1800X"), 0.0);
    }

    #[test]
    fn finds_this_machines_processors() {
        let packages = topology(RelationProcessorPackage);
        let cores = topology(RelationProcessorCore);
        assert!(!packages.is_empty() && cores.len() >= packages.len());
        assert!(packages.iter().all(|p| p.Mask != 0));
        // The thread can be moved to each and back.
        assert!(packages.iter().all(|p| on(p, || ()).is_some()));
    }

    #[test]
    fn scales_rapl_energy() {
        // ESU 14 on a Core CPU: 2^-14 J; ESU 5 on a Silvermont Atom: 32 µJ.
        assert_eq!(energy_unit(14 << 8, (6, 0x97)), 1.0 / 16384.0);
        assert!((energy_unit(5 << 8, (6, 0x37)) - 32e-6).abs() < 1e-12);
    }

    #[test]
    fn turns_energy_counts_into_power() {
        // (Each count taken at a moment given, not read off the clock: a busy machine moved it between the two.)
        let unit = 1.0 / 16384.0;
        let second = std::time::Duration::from_secs(1);
        let start = Instant::now();
        let mut counter = EnergyCounter::new(unit);
        // Across a wrap: 100 J in one second.
        counter.last = Some((u32::MAX - 16384 * 50 + 1, start));
        let watts = counter.power_at(16384 * 50, start + second).unwrap();
        assert!((watts - 100.0).abs() < 0.01, "{watts}");
        // Standing still once it has counted: nothing drawn.
        counter.last = Some((7, start));
        assert_eq!(counter.power_at(7, start + second), Some(0.0));
        // A counter that has never moved is not counting.
        let mut still = EnergyCounter::new(unit);
        still.last = Some((7, start));
        assert_eq!(still.power_at(7, start + second), None);
        // A gap the counter could wrap in more than once gives nothing.
        counter.last = Some((0, start));
        assert_eq!(counter.power_at(5, start + 400 * second), None);
    }

    /// Each step of opening this CPU's sensors, and what it gave: for a CPU whose sensors do not open.
    #[test]
    #[ignore = "needs administrator rights and the PawnIO driver; run with --ignored"]
    fn opens_this_cpu_step_by_step() {
        println!("family/model {:x?}", cpu_family_model());
        let module = Module::load(AMD_FAMILY_17);
        println!("module: {:?}", module.as_ref().err());
        let Ok(module) = module else { return };
        println!("power unit: {:x?}", module.read("ioctl_read_msr", MSR_PWR_UNIT));
        println!("energy: {:x?}", module.read("ioctl_read_msr", MSR_PKG_ENERGY_STAT));
        println!("Tctl: {:x?}", module.read("ioctl_read_smn", THM_TCON_CUR_TMP));
    }

    /// Needs administrator rights and the PawnIO driver.
    #[test]
    #[ignore = "needs administrator rights and the PawnIO driver; run with --ignored"]
    fn reads_this_cpu() {
        println!("family/model {:x?}", cpu_family_model());
        let mut cpu = AmdCpu::open().expect("PawnIO and an AMD Zen CPU, elevated");
        cpu.read();
        std::thread::sleep(std::time::Duration::from_millis(500));
        println!("{:?}", cpu.read());
    }
}

