//! What Glance reads, left where the remote-control helper can read it.
//!
//! The helper is another program, not elevated, that a paired phone asks
//! how the PC is. Of temperatures and fans it could say nothing by itself
//! (reading them takes the driver, and the driver takes elevation): Glance
//! leaves a few figures of each sample in a piece of memory shared by name,
//! which whoever is signed in may read and only Glance write.
//!
//! The memory: a count (odd while what follows is being written), the
//! length of what follows, and that: the figures, as JSON.

use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{json, Value};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, LocalFree, ERROR_ALREADY_EXISTS, HLOCAL, INVALID_HANDLE_VALUE};
use windows::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::System::Memory::{CreateFileMappingW, MapViewOfFile, FILE_MAP_WRITE, PAGE_READWRITE};

use crate::reading::{Sample, StaticInfo};

/// The memory's name: this session's own.
#[cfg(not(debug_assertions))]
const NAME: PCWSTR = w!("Local\\Glance.Readings");
#[cfg(debug_assertions)]
const NAME: PCWSTR = w!("Local\\Glance.Readings.Debug");
/// How much of it there is: the count and the length, then the figures.
const SIZE: usize = 4096;
const HEAD: usize = 8;

/// The shared memory, as Glance holds it for as long as it runs.
pub struct Told(*mut u8);

// Written by the one thread that holds it.
unsafe impl Send for Told {}

impl Told {
    /// Makes the memory; none where it cannot be made, or where something
    /// of that name is there already (not Glance's own: left alone).
    pub fn open() -> Option<Told> {
        // Administrators and the system, everything; whoever is signed in
        // here, to read.
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(w!("D:(A;;GA;;;BA)(A;;GA;;;SY)(A;;GR;;;IU)"), SDDL_REVISION_1, &mut descriptor, None) }.ok()?;
        let attributes = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: descriptor.0, bInheritHandle: false.into() };
        let made = unsafe { CreateFileMappingW(INVALID_HANDLE_VALUE, Some(&attributes), PAGE_READWRITE, 0, SIZE as u32, NAME) };
        let there_already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
        let mapping = made.ok()?;
        if there_already {
            let _ = unsafe { CloseHandle(mapping) };
            return None;
        }
        // (The mapping stays open with the view of it.)
        let view = unsafe { MapViewOfFile(mapping, FILE_MAP_WRITE, 0, 0, SIZE) };
        (!view.Value.is_null()).then(|| Told(view.Value.cast()))
    }

    /// Leaves the figures of `sample` for the helper.
    pub fn tell(&mut self, sample: &Sample, info: &StaticInfo) {
        let memory = unsafe { std::slice::from_raw_parts_mut(self.0, SIZE) };
        write(memory, figures(sample, info).to_string().as_bytes());
    }
}

/// Writes `text` into `memory` so that a reader can tell a whole text from
/// one half written: the count is odd for as long as the writing takes.
fn write(memory: &mut [u8], text: &[u8]) {
    if text.len() > memory.len() - HEAD {
        return;
    }
    // (The start of a mapping, or of a test's own aligned words.)
    let count = unsafe { &*(memory.as_ptr() as *const AtomicU32) };
    count.fetch_add(1, Ordering::SeqCst);
    memory[4..HEAD].copy_from_slice(&(text.len() as u32).to_le_bytes());
    memory[HEAD..HEAD + text.len()].copy_from_slice(text);
    count.fetch_add(1, Ordering::SeqCst);
}

/// The fan on the processor and the fastest of the case's own, each in
/// turns a minute, out of the board's fans by name. Only a board whose
/// headers Glance knows by name says which is which.
fn fans(named: &[(String, f32)]) -> (Option<f32>, Option<f32>) {
    let cpu = named.iter().find(|(name, _)| name == "cpu_fan").map(|(_, rpm)| *rpm);
    let case = named.iter().filter(|(name, _)| name.starts_with("system_fan") && !name.ends_with("pump")).map(|(_, rpm)| *rpm).max_by(f32::total_cmp);
    (cpu, case)
}

/// The few figures of a sample the helper is told: how warm (°C) and how
/// busy (%) the processor, the busiest graphics card, the memory and the
/// disks are, and how fast the fans turn; `null` where one is not known.
fn figures(sample: &Sample, info: &StaticInfo) -> Value {
    let whole = |value: Option<f32>| value.map(|value| value.round() as i64);
    let hottest = |temps: &mut dyn Iterator<Item = f32>| temps.max_by(f32::total_cmp);
    let gpu = sample.gpus.iter().max_by(|a, b| a.usage.unwrap_or(0.0).total_cmp(&b.usage.unwrap_or(0.0)));
    let (cpu_fan, case_fan) = sample.board.as_ref().map_or((None, None), |board| fans(&board.fans));
    json!({
        "t": sample.t,
        "cpu": { "temp": whole(sample.cpu_sensors.as_ref().and_then(|sensors| sensors.temp)), "load": whole(sample.cpu) },
        "gpu": { "temp": whole(gpu.and_then(|gpu| gpu.temp)), "load": whole(gpu.and_then(|gpu| gpu.usage)) },
        "ram": { "temp": whole(hottest(&mut sample.dimm_temps.iter().copied())), "load": whole(Some(sample.memory.used as f32 / info.mem_total.max(1) as f32 * 100.0)) },
        "disk": { "temp": whole(hottest(&mut sample.drive_temps.iter().map(|drive| drive.celsius))), "load": whole(sample.disk_active) },
        "cpu_fan": whole(cpu_fan),
        "case_fan": whole(case_fan),
        // Every fan the board reads, as it names each.
        "fans": sample.board.as_ref().map_or(Vec::new(), |board| board.fans.iter().map(|(name, rpm)| json!([name, rpm.round() as i64])).collect()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_written_is_whole_and_counted_twice() {
        let mut words = [0u32; 16];
        let memory = unsafe { std::slice::from_raw_parts_mut(words.as_mut_ptr().cast::<u8>(), 64) };
        write(memory, b"{\"t\":1}");
        assert_eq!(memory[..4], 2u32.to_le_bytes());
        assert_eq!(memory[4..8], 7u32.to_le_bytes());
        assert_eq!(&memory[8..15], b"{\"t\":1}");
        // One too long for the memory is not written at all.
        write(memory, &[b'x'; 57]);
        assert_eq!(memory[..4], 2u32.to_le_bytes());
        assert_eq!(&memory[8..15], b"{\"t\":1}");
    }

    #[test]
    fn the_fans_are_told_apart_only_by_name() {
        let named = |fans: &[(&str, f32)]| fans.iter().map(|(name, rpm)| (name.to_string(), *rpm)).collect::<Vec<_>>();
        assert_eq!(fans(&named(&[("cpu_fan", 1200.0), ("system_fan_1", 800.0), ("system_fan_2", 950.0), ("system_fan_4_pump", 2800.0), ("cpu_opt_fan", 1500.0)])), (Some(1200.0), Some(950.0)));
        // Headers known only by their numbers: which cools what is not known.
        assert_eq!(fans(&named(&[("1", 1200.0), ("2", 800.0)])), (None, None));
        assert_eq!(fans(&[]), (None, None));
    }
}
