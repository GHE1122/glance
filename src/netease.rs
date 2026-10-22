//! InfLink-rs for NetEase Cloud Music, installed on the user's word: the plugin that has NetEase say how far into a
//! track it is, which its lyrics are timed by. NetEase takes plugins only through BetterNCM: its loader, put in
//! NetEase's folder as `msimg32.dll` (which NetEase loads from there), loads the plugins in BetterNCM's own data
//! folder. Both are other projects' (GPL-3.0, their sources served beside them: see `scripts\site.ps1`): taken from a
//! copy on Glance's site, which is reached where GitHub is not, then from where each project publishes it, as it built
//! it, and checked by digest. A BetterNCM already there is left as it is; the loader is taken back only if Glance put
//! it there (see `settings::Settings::netease_loader`). With InfLink-rs goes a plugin of Glance's own, the icon row,
//! which mends what BetterNCM's button does to NetEase's title bar (see `ICON_ROW`): put in by itself as Glance
//! starts where InfLink-rs is there without it (see `mend`).
//!
//! NetEase installed where this user may not write (Program Files), its loader is put there by Glance run elevated,
//! the user asked to let it (`ASKED`): that run finds NetEase from what the machine lists alone, and takes and checks
//! the loader itself, trusting nothing this user could have written.

use std::path::{Path, PathBuf};

use windows::core::{w, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Registry::{RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};

/// A file of another project's: where it is taken from (Glance's copy first), and its SHA-256.
pub struct Payload {
    sources: [&'static str; 2],
    digest: &'static str,
}

/// BetterNCM 1.3.4's loader, for 64-bit NetEase (3.x) and for 32-bit (2.x).
const LOADER_64: Payload = Payload {
    sources: ["https://glancepc.com/dl/BetterNCMII-1.3.4.dll", "https://github.com/std-microblock/BetterNCM/releases/download/1.3.4/BetterNCMII.dll"],
    digest: "a7c77af418d7940e63faa58ea036fba1f4baad497947109ea52ed78c8e86608f",
};
const LOADER_32: Payload = Payload {
    sources: ["https://glancepc.com/dl/BetterNCMII86-1.3.4.dll", "https://github.com/std-microblock/BetterNCM/releases/download/1.3.4/BetterNCMII86.dll"],
    digest: "b7be606f353c5ca36235ff1f4dfccde62dc459a4b07a506698d663a1953864f2",
};
/// InfLink-rs 3.4.0.
const PLUGIN: Payload = Payload {
    sources: ["https://glancepc.com/dl/InfLink-rs-3.4.0.plugin", "https://github.com/apoint123/inflink-rs/releases/download/v3.4.0/InfLink-rs.plugin"],
    digest: "7d0b7b863b072a99b88eb614c788099315e690920889aaca22685d3663661468",
};
/// Glance's own plugin for BetterNCM (MIT, its source in `resources\netease`), put in and taken out with InfLink-rs:
/// BetterNCM 1.3.4's button, on NetEase 3.1, pushes NetEase's icons at the top right onto a second row, and it puts
/// them back on one (see its `main.js`). Its files, packed as BetterNCM takes a plugin (see `pack`) as it is put in.
const ICON_ROW: [(&str, &str); 2] = [("manifest.json", include_str!("../resources/netease/manifest.json")), ("main.js", include_str!("../resources/netease/main.js"))];
/// What the loader and the plugins are called where they are put.
const LOADER_FILE: &str = "msimg32.dll";
const PLUGIN_FILE: &str = "InfLink-rs.plugin";
const ICON_ROW_FILE: &str = "Glance-Icon-Row.plugin";
/// The versions of NetEase InfLink-rs was tested on, as its project says: 3.1.20 to 3.1.41, and of the 2s, 2.10.13.
const TESTED: [([u32; 3], [u32; 3]); 2] = [([3, 1, 20], [3, 1, 41]), ([2, 10, 13], [2, 10, 13])];
/// What Glance is run elevated with to put the loader in NetEase's folder, or take it back (see `run_asked`).
pub const ASKED: &str = "--netease-loader";
/// The most that is taken of what a place serves (each file is a megabyte or two).
const MOST: usize = 8 << 20;

/// NetEase Cloud Music as it is installed: its folder, its version, and whether it is a 64-bit program.
#[derive(Clone)]
pub struct NetEase {
    pub folder: PathBuf,
    pub version: [u32; 3],
    wide: bool,
}

impl NetEase {
    /// Whether InfLink-rs was tested on this version.
    pub fn tested(&self) -> bool {
        tested(self.version)
    }

    /// Its version as it is written ("3.1.41").
    pub fn version(&self) -> String {
        self.version.map(|part| part.to_string()).join(".")
    }

    fn loader(&self) -> &'static Payload {
        if self.wide { &LOADER_64 } else { &LOADER_32 }
    }

    fn program(&self) -> PathBuf {
        self.folder.join("cloudmusic.exe")
    }

    /// Whether a plugin loader is in its folder (BetterNCM's, put there by Glance or not).
    pub fn has_loader(&self) -> bool {
        self.folder.join(LOADER_FILE).is_file()
    }
}

/// Whether InfLink-rs was tested on NetEase `version` (see `TESTED`).
fn tested(version: [u32; 3]) -> bool {
    TESTED.iter().any(|(low, high)| (*low..=*high).contains(&version))
}

/// NetEase as Windows lists it among what is installed (its entry's icon is its program): the machine's entries,
/// then (unless `machine_only`) this user's. None where it is not installed, or its program is not where it says.
pub fn found(machine_only: bool) -> Option<NetEase> {
    const ENTRIES: [PCWSTR; 2] = [w!(r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\网易云音乐"), w!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\网易云音乐")];
    let roots: &[HKEY] = if machine_only { &[HKEY_LOCAL_MACHINE] } else { &[HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] };
    let program = roots.iter().flat_map(|&root| ENTRIES.map(|entry| (root, entry))).find_map(|(root, entry)| {
        let icon = registry_string(root, entry, w!("DisplayIcon"))?;
        // ("D:\App\CloudMusic\cloudmusic.exe", maybe quoted, maybe with ",0" after it.)
        let path = PathBuf::from(icon.split(',').next()?.trim().trim_matches('"'));
        path.file_name().is_some_and(|name| name.eq_ignore_ascii_case("cloudmusic.exe")).then_some(path).filter(|path| path.is_file())
    })?;
    // (Its header is in its first few kilobytes.)
    let mut header = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(std::fs::File::open(&program).ok()?, 4096), &mut header).ok()?;
    let wide = machine(&header)? == MACHINE_AMD64;
    Some(NetEase { folder: program.parent()?.to_path_buf(), version: file_version(&program)?, wide })
}

/// The 64-bit x86 machine, as a program's header names it.
const MACHINE_AMD64: u16 = 0x8664;

/// The machine a Windows program `bytes` is made for, as its header says.
fn machine(bytes: &[u8]) -> Option<u16> {
    // ("MZ", and at 0x3C where the PE header is: "PE\0\0" and the machine after it.)
    if bytes.get(..2)? != b"MZ" {
        return None;
    }
    let header = u32::from_le_bytes(bytes.get(0x3C..0x40)?.try_into().ok()?) as usize;
    if bytes.get(header..header.checked_add(4)?)? != b"PE\0\0" {
        return None;
    }
    Some(u16::from_le_bytes(bytes.get(header + 4..header + 6)?.try_into().ok()?))
}

fn registry_string(root: HKEY, key: PCWSTR, value: PCWSTR) -> Option<String> {
    let mut data = [0u16; 1024];
    let mut size = size_of_val(&data) as u32;
    unsafe { RegGetValueW(root, key, value, RRF_RT_REG_SZ, None, Some(data.as_mut_ptr().cast()), Some(&mut size)) }.ok().ok()?;
    let length = data.iter().position(|&c| c == 0).unwrap_or(data.len());
    Some(String::from_utf16_lossy(&data[..length]))
}

/// The version `file`'s version resource says (its first three numbers).
fn file_version(file: &Path) -> Option<[u32; 3]> {
    use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO};
    let path = HSTRING::from(file.as_os_str());
    let size = unsafe { GetFileVersionInfoSizeW(&path, None) };
    if size == 0 {
        return None;
    }
    let mut data = vec![0u8; size as usize];
    unsafe { GetFileVersionInfoW(&path, None, size, data.as_mut_ptr().cast()) }.ok()?;
    let (mut fixed, mut length) = (std::ptr::null_mut(), 0u32);
    if !unsafe { VerQueryValueW(data.as_ptr().cast(), w!("\\"), &mut fixed, &mut length) }.as_bool() || (length as usize) < size_of::<VS_FIXEDFILEINFO>() {
        return None;
    }
    let fixed = unsafe { &*(fixed as *const VS_FIXEDFILEINFO) };
    Some([fixed.dwFileVersionMS >> 16, fixed.dwFileVersionMS & 0xFFFF, fixed.dwFileVersionLS >> 16])
}

/// BetterNCM's data folder, as BetterNCM picks it: where `BETTERNCM_PROFILE` says, else `C:\betterncm`.
fn data_folder(profile: Option<std::ffi::OsString>) -> PathBuf {
    profile.filter(|profile| !profile.is_empty()).map_or_else(|| PathBuf::from(r"C:\betterncm"), PathBuf::from)
}

/// Where BetterNCM's plugins are.
pub fn plugin_folder() -> PathBuf {
    data_folder(std::env::var_os("BETTERNCM_PROFILE")).join("plugins")
}

/// Where the plugin `file` is put.
fn plugin_path(file: &str) -> PathBuf {
    plugin_folder().join(file)
}

/// How much of what `install` puts in is there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Installed {
    /// Not InfLink-rs, or no loader to load it.
    No,
    /// InfLink-rs and a loader, without the icon row (put in by a Glance before it, or not by Glance): `put_icon_row`
    /// puts it in.
    NoIconRow,
    /// All of it.
    Yes,
}

/// What of InfLink-rs is installed for `netease`: a loader in its folder (BetterNCM's, Glance's or not), the plugin
/// in BetterNCM's, and the icon row beside it.
pub fn installed(netease: &NetEase) -> Installed {
    match (netease.has_loader() && plugin_path(PLUGIN_FILE).is_file(), plugin_path(ICON_ROW_FILE).is_file()) {
        (false, _) => Installed::No,
        (true, false) => Installed::NoIconRow,
        (true, true) => Installed::Yes,
    }
}

/// Why installing or taking back did not go through.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Failed {
    /// What was taken was not reached, or was not what it should be.
    Taken,
    /// It could not be put where it goes (the user refused to let Glance do it elevated, or it was not let).
    Put,
}

/// Installs InfLink-rs for `netease`: the plugin and the icon row in BetterNCM's folder, and BetterNCM's loader in
/// NetEase's folder where none is (elevated, the user asked, where this user may not write there). Whether Glance put
/// the loader.
pub fn install(netease: &NetEase) -> Result<bool, Failed> {
    let plugin = taken(&PLUGIN).ok_or(Failed::Taken)?;
    let path = plugin_path(PLUGIN_FILE);
    std::fs::create_dir_all(path.parent().ok_or(Failed::Put)?).map_err(|_| Failed::Put)?;
    std::fs::write(&path, plugin).map_err(|_| Failed::Put)?;
    put_icon_row()?;
    let loader = netease.folder.join(LOADER_FILE);
    if loader.is_file() {
        crate::journal::note("netease: the plugin put in; a loader was there already");
        return Ok(false);
    }
    let bytes = taken(netease.loader()).ok_or(Failed::Taken)?;
    match put_new(&loader, &bytes) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => elevated("put")?,
        Err(_) => return Err(Failed::Put),
    }
    crate::journal::note("netease: the plugin and the loader put in");
    Ok(true)
}

/// Puts the icon row in BetterNCM's folder (it is Glance's own: nothing is taken for it). It works once NetEase
/// starts again.
pub fn put_icon_row() -> Result<(), Failed> {
    let path = plugin_path(ICON_ROW_FILE);
    std::fs::create_dir_all(path.parent().ok_or(Failed::Put)?).map_err(|_| Failed::Put)?;
    std::fs::write(&path, pack(&ICON_ROW)).map_err(|_| Failed::Put)
}

/// The icon row put in beside an InfLink-rs installed without it (by a Glance before it, or not by Glance), as Glance
/// starts: nothing taken, nothing else touched. It works once NetEase starts again. Nothing where NetEase, or InfLink-rs,
/// is not there.
pub fn mend() {
    if let Some(netease) = found(false) {
        mend_for(&netease);
    }
}

/// As `mend`, for `netease`.
fn mend_for(netease: &NetEase) {
    if installed(netease) == Installed::NoIconRow {
        let put = put_icon_row();
        crate::journal::note(format!("netease: the icon row put in beside InfLink-rs as Glance started: {put:?}"));
    }
}

/// `files` (each a name and what it holds) packed as BetterNCM takes a plugin: a zip, each file in it stored as it is.
fn pack(files: &[(&str, &str)]) -> Vec<u8> {
    // (A header for each file and the file, then a directory of them, then where the directory is. Each file is dated
    // the first day a zip can say, 1980-01-01: the same files pack to the same bytes.)
    const DAY: u16 = 1 << 5 | 1;
    let (mut packed, mut directory) = (Vec::new(), Vec::new());
    for (name, text) in files {
        let (name, bytes) = (name.as_bytes(), text.as_bytes());
        // (What both of its headers say: the version needed (1.0), no flags, stored, its time and day, its CRC and
        // sizes, its name's length, nothing extra.)
        let mut said: Vec<u8> = [10, 0, 0, 0, DAY].into_iter().flat_map(u16::to_le_bytes).collect();
        said.extend([crc32(bytes), bytes.len() as u32, bytes.len() as u32].into_iter().flat_map(u32::to_le_bytes));
        said.extend([name.len() as u16, 0].into_iter().flat_map(u16::to_le_bytes));
        let at = packed.len() as u32;
        packed.extend(0x0403_4B50u32.to_le_bytes());
        packed.extend(&said);
        packed.extend(name);
        packed.extend(bytes);
        // (In the directory: made by zip 2.0, what the header says, no comment, on the first disk, no attributes, and
        // where its header is.)
        directory.extend(0x0201_4B50u32.to_le_bytes());
        directory.extend(20u16.to_le_bytes());
        directory.extend(&said);
        directory.extend([0u16, 0, 0].into_iter().flat_map(u16::to_le_bytes));
        directory.extend([0, at].into_iter().flat_map(u32::to_le_bytes));
        directory.extend(name);
    }
    // (Its end: the first disk, how many files, the directory's size and where it is, no comment.)
    let (count, size, at) = (files.len() as u16, directory.len() as u32, packed.len() as u32);
    packed.extend(directory);
    packed.extend(0x0605_4B50u32.to_le_bytes());
    packed.extend([0, 0, count, count].into_iter().flat_map(u16::to_le_bytes));
    packed.extend([size, at].into_iter().flat_map(u32::to_le_bytes));
    packed.extend(0u16.to_le_bytes());
    packed
}

/// The CRC-32 of `bytes`, as a zip checks a file by.
fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(!0u32, |crc, &byte| (0..8).fold(crc ^ u32::from(byte), |crc, _| (crc >> 1) ^ (0xEDB8_8320 & (crc & 1).wrapping_neg())))
}

/// Takes InfLink-rs back from `netease`: the plugin and the icon row, and the loader if Glance put it (`loader_ours`)
/// and it is still the one Glance put. NetEase is closed first (a loader it holds cannot be taken), and started again
/// after if it ran.
pub fn remove(netease: &NetEase, loader_ours: bool) -> Result<(), Failed> {
    let ran = close(netease);
    for path in [plugin_path(PLUGIN_FILE), plugin_path(ICON_ROW_FILE)] {
        if path.is_file() {
            std::fs::remove_file(&path).map_err(|_| Failed::Put)?;
        }
    }
    let loader = netease.folder.join(LOADER_FILE);
    let result = if loader_ours && std::fs::read(&loader).is_ok_and(|bytes| is(&bytes, netease.loader())) {
        match std::fs::remove_file(&loader) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => elevated("take"),
            Err(_) => Err(Failed::Put),
        }
    } else {
        Ok(())
    };
    if ran {
        start(netease);
    }
    crate::journal::note(format!("netease: taken back ({result:?})"));
    result
}

/// Writes `bytes` as `path`, a file there is not yet (none there is written through).
fn put_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes).inspect_err(|_| {
        let _ = std::fs::remove_file(path);
    })
}

/// What `payload` is, from the first of its places that serves it as it should be.
fn taken(payload: &Payload) -> Option<Vec<u8>> {
    payload.sources.iter().find_map(|url| crate::update::get(url).filter(|bytes| bytes.len() <= MOST && is(bytes, payload)))
}

/// Whether `bytes` are `payload`: its digest.
fn is(bytes: &[u8], payload: &Payload) -> bool {
    use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
    let mut digest = [0u8; 32];
    unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, bytes, &mut digest) }.is_ok() && digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>() == payload.digest
}

/// Runs this program elevated (`ASKED`) to `what` ("put" or "take") the loader, the user asked to let it.
fn elevated(what: &str) -> Result<(), Failed> {
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    let exe = std::env::current_exe().map_err(|_| Failed::Put)?;
    let (file, arguments) = (HSTRING::from(exe.as_os_str()), HSTRING::from(format!("{ASKED} {what}")));
    let mut started = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(arguments.as_ptr()),
        nShow: windows::Win32::UI::WindowsAndMessaging::SW_HIDE.0,
        ..Default::default()
    };
    if unsafe { ShellExecuteExW(&mut started) }.is_err() || started.hProcess.is_invalid() {
        return Err(Failed::Put);
    }
    let mut code = 1u32;
    unsafe {
        WaitForSingleObject(started.hProcess, INFINITE);
        let _ = GetExitCodeProcess(started.hProcess, &mut code);
        let _ = CloseHandle(started.hProcess);
    }
    match code {
        0 => Ok(()),
        2 => Err(Failed::Taken),
        _ => Err(Failed::Put),
    }
}

/// Run as `ASKED` (elevated, see `elevated`): puts the loader in NetEase's folder ("put", where none is) or takes the
/// one Glance put back ("take", only if it is that one), NetEase found from what the machine lists alone; ends saying
/// how that went (0 done, 2 the loader not had as it should be, 1 not done). None: not so run.
pub fn run_asked(arguments: &[String]) -> Option<i32> {
    let [flag, what] = arguments else { return None };
    if flag != ASKED || !crate::elevation::is_elevated() {
        return None;
    }
    let Some(netease) = found(true) else { return Some(1) };
    let loader = netease.folder.join(LOADER_FILE);
    Some(match what.as_str() {
        "put" if loader.exists() => 0,
        "put" => match taken(netease.loader()) {
            Some(bytes) => i32::from(put_new(&loader, &bytes).is_err()),
            None => 2,
        },
        "take" if std::fs::read(&loader).is_ok_and(|bytes| is(&bytes, netease.loader())) => i32::from(std::fs::remove_file(&loader).is_err()),
        "take" => 0,
        _ => 1,
    })
}

/// Whether NetEase runs (from `netease`'s folder).
pub fn running(netease: &NetEase) -> bool {
    let processes = processes(netease);
    let any = !processes.is_empty();
    for process in processes {
        let _ = unsafe { CloseHandle(process) };
    }
    any
}

/// Starts NetEase again: closes it (every process of it), then starts it as this user. A plugin newly put in is
/// loaded as it starts.
pub fn restart(netease: &NetEase) {
    close(netease);
    start(netease);
}

/// Ends NetEase's processes and waits for them to be gone; whether it ran.
fn close(netease: &NetEase) -> bool {
    use windows::Win32::System::Threading::{TerminateProcess, WaitForSingleObject};
    let processes = processes(netease);
    let ran = !processes.is_empty();
    for process in processes {
        unsafe {
            let _ = TerminateProcess(process, 0);
            WaitForSingleObject(process, 5000);
            let _ = CloseHandle(process);
        }
    }
    ran
}

fn start(netease: &NetEase) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    let program = HSTRING::from(netease.program().as_os_str());
    let folder = HSTRING::from(netease.folder.as_os_str());
    unsafe { ShellExecuteW(None, w!("open"), &program, PCWSTR::null(), &folder, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL) };
}

/// NetEase's processes (`cloudmusic.exe` from its folder), opened to be ended and waited for.
fn processes(netease: &NetEase) -> Vec<HANDLE> {
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE};
    let mut found = Vec::new();
    let Ok(folder) = std::fs::canonicalize(&netease.folder) else { return found };
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return found };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let length = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..length]).eq_ignore_ascii_case("cloudmusic.exe") {
                if let Ok(process) = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, false, entry.th32ProcessID) {
                    let mut path = vec![0u16; 1024];
                    let mut size = path.len() as u32;
                    let named = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(path.as_mut_ptr()), &mut size).is_ok();
                    let file = PathBuf::from(String::from_utf16_lossy(&path[..size as usize]));
                    if named && file.parent().and_then(|parent| std::fs::canonicalize(parent).ok()).is_some_and(|parent| parent == folder) {
                        found.push(process);
                    } else {
                        let _ = CloseHandle(process);
                    }
                }
            }
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tested are the versions InfLink-rs says it was tested on, ends included, and no other.
    #[test]
    fn tested_as_inflink_says() {
        assert!(tested([3, 1, 20]) && tested([3, 1, 33]) && tested([3, 1, 41]) && tested([2, 10, 13]));
        assert!(!tested([3, 1, 19]) && !tested([3, 1, 42]) && !tested([3, 2, 0]) && !tested([2, 10, 12]) && !tested([2, 9, 13]));
    }

    /// A program's machine is read from its header, where the header says it is; not from what is no program.
    #[test]
    fn reads_a_programs_machine() {
        let mut program = vec![0u8; 0x100];
        program[..2].copy_from_slice(b"MZ");
        program[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        program[0x80..0x84].copy_from_slice(b"PE\0\0");
        program[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        assert_eq!(machine(&program), Some(MACHINE_AMD64));
        program[0x84..0x86].copy_from_slice(&0x014Cu16.to_le_bytes());
        assert_eq!(machine(&program), Some(0x014C));
        // (Its header said to be past its end, or not a program's.)
        program[0x3C..0x40].copy_from_slice(&0xFFF0u32.to_le_bytes());
        assert_eq!(machine(&program), None);
        program[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        program[0x80] = b'X';
        assert_eq!(machine(&program), None);
        assert_eq!(machine(b"MZ"), None);
        assert_eq!(machine(b""), None);
    }

    /// BetterNCM's folder is where its variable says, else `C:\betterncm`; an empty variable says nothing.
    #[test]
    fn finds_betterncms_folder_as_it_does() {
        assert_eq!(data_folder(None), PathBuf::from(r"C:\betterncm"));
        assert_eq!(data_folder(Some(r"D:\ncm".into())), PathBuf::from(r"D:\ncm"));
        assert_eq!(data_folder(Some("".into())), PathBuf::from(r"C:\betterncm"));
    }

    /// The digests are of what each payload is.
    #[test]
    fn checks_by_digest() {
        let empty = Payload { sources: ["", ""], digest: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" };
        assert!(is(b"", &empty));
        assert!(!is(b"a", &empty));
    }

    /// The CRC-32 a zip checks a file by: the standard's own check value, and nothing's.
    #[test]
    fn checks_as_a_zip_does() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    /// The icon row packed is a zip its files are read back from as they are, found through its directory, each its
    /// header's name, size and CRC; packed again, the same bytes.
    #[test]
    fn packs_the_icon_row_as_a_zip() {
        let packed = pack(&ICON_ROW);
        assert_eq!(packed, pack(&ICON_ROW));
        let half = |at: usize| u16::from_le_bytes(packed[at..at + 2].try_into().unwrap()) as usize;
        let word = |at: usize| u32::from_le_bytes(packed[at..at + 4].try_into().unwrap());
        // (Its end, the last 22 bytes: how many files, and where the directory is.)
        let end = packed.len() - 22;
        assert_eq!(word(end), 0x0605_4B50);
        assert_eq!(half(end + 10), ICON_ROW.len());
        let mut entry = word(end + 16) as usize;
        for (name, text) in ICON_ROW {
            assert_eq!(word(entry), 0x0201_4B50);
            let header = word(entry + 42) as usize;
            assert_eq!(&packed[entry + 46..entry + 46 + half(entry + 28)], name.as_bytes());
            assert_eq!(word(header), 0x0403_4B50);
            // (Stored, its sizes and CRC as the directory has them.)
            assert_eq!(half(header + 8), 0);
            assert_eq!(packed[header + 14..header + 26], packed[entry + 16..entry + 28]);
            let (size, length) = (word(header + 22) as usize, half(header + 26));
            assert_eq!(&packed[header + 30..header + 30 + length], name.as_bytes());
            let bytes = &packed[header + 30 + length..header + 30 + length + size];
            assert_eq!(bytes, text.as_bytes());
            assert_eq!(word(header + 14), crc32(bytes));
            entry += 46 + half(entry + 28);
        }
        assert_eq!(entry, end);
    }

    /// The icon row's manifest is as BetterNCM 1.3.4 reads one: its first form, for NetEase 3 only, its name its
    /// folder's (BetterNCM drops what is not a letter, a digit or `-`, and spaces become `-`), its script among its files.
    #[test]
    fn the_icon_rows_manifest_is_as_betterncm_reads_it() {
        let manifest: serde_json::Value = serde_json::from_str(ICON_ROW[0].1).unwrap();
        assert_eq!(ICON_ROW[0].0, "manifest.json");
        assert_eq!(manifest["manifest_version"], 1);
        assert_eq!(manifest["ncm3-compatible"], true);
        assert_eq!(manifest["ncm-version-req"], ">=3.0.0");
        let name = manifest["name"].as_str().unwrap();
        assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == ' '));
        assert_eq!(format!("{}.plugin", name.replace(' ', "-")), ICON_ROW_FILE);
        let script = manifest["injects"]["Main"][0]["file"].as_str().unwrap();
        assert!(ICON_ROW.iter().any(|(file, _)| *file == script));
    }

    /// The tests that read `BETTERNCM_PROFILE` take turns (see `Scratch`).
    static PROFILE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn turn() -> std::sync::MutexGuard<'static, ()> {
        PROFILE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A NetEase and a BetterNCM folder of a test's own: BetterNCM's where `BETTERNCM_PROFILE` says while it lasts,
    /// the test's turn. Gone after, and the variable as it was.
    struct Scratch {
        folder: PathBuf,
        netease: NetEase,
        was: Option<std::ffi::OsString>,
        _turn: std::sync::MutexGuard<'static, ()>,
    }

    impl Scratch {
        /// The folders, in one called `name`: NetEase's made, BetterNCM's not.
        fn new(name: &str) -> Scratch {
            let turn = turn();
            let folder = std::env::temp_dir().join(format!("glance-test-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&folder);
            std::fs::create_dir_all(folder.join("netease")).unwrap();
            let was = std::env::var_os("BETTERNCM_PROFILE");
            std::env::set_var("BETTERNCM_PROFILE", folder.join("betterncm"));
            let netease = NetEase { folder: folder.join("netease"), version: [3, 1, 41], wide: true };
            Scratch { folder, netease, was, _turn: turn }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            match self.was.take() {
                Some(was) => std::env::set_var("BETTERNCM_PROFILE", was),
                None => std::env::remove_var("BETTERNCM_PROFILE"),
            }
            let _ = std::fs::remove_dir_all(&self.folder);
        }
    }

    /// The icon row goes with InfLink-rs: InfLink-rs and a loader without it are installed but for it, it put in as
    /// Glance starts makes it whole, and taking back takes it with the plugin (another's loader kept).
    #[test]
    fn the_icon_row_goes_with_inflink() {
        let scratch = Scratch::new("netease-icon-row");
        let netease = &scratch.netease;
        std::fs::write(netease.folder.join(LOADER_FILE), b"another's").unwrap();
        std::fs::create_dir_all(plugin_folder()).unwrap();
        std::fs::write(plugin_path(PLUGIN_FILE), b"InfLink-rs").unwrap();
        assert_eq!(installed(netease), Installed::NoIconRow);
        // (As Glance starts: put in by itself.)
        mend_for(netease);
        assert_eq!(installed(netease), Installed::Yes);
        assert_eq!(std::fs::read(scratch.folder.join(r"betterncm\plugins").join(ICON_ROW_FILE)).unwrap(), pack(&ICON_ROW));
        assert_eq!(remove(netease, true), Ok(()));
        assert!(!plugin_path(PLUGIN_FILE).exists() && !plugin_path(ICON_ROW_FILE).exists() && netease.has_loader());
        assert_eq!(installed(netease), Installed::No);
    }

    /// Installed into a NetEase and a BetterNCM folder of the test's own, from where the files are published: the
    /// plugin, the icon row and the loader put in, each as it should be, and taken back; a loader that was there
    /// already is kept, whatever is asked.
    #[test]
    #[ignore = "downloads the plugin and its loader"]
    fn installs_and_takes_back() {
        let scratch = Scratch::new("netease");
        let (netease, plugins) = (&scratch.netease, scratch.folder.join(r"betterncm\plugins"));
        assert_eq!(installed(netease), Installed::No);
        assert_eq!(install(netease), Ok(true));
        assert_eq!(installed(netease), Installed::Yes);
        assert!(is(&std::fs::read(netease.folder.join(LOADER_FILE)).unwrap(), &LOADER_64));
        assert!(is(&std::fs::read(plugins.join(PLUGIN_FILE)).unwrap(), &PLUGIN));
        assert_eq!(std::fs::read(plugins.join(ICON_ROW_FILE)).unwrap(), pack(&ICON_ROW));
        assert_eq!(remove(netease, true), Ok(()));
        assert!(!netease.has_loader() && !plugins.join(PLUGIN_FILE).exists() && !plugins.join(ICON_ROW_FILE).exists());

        std::fs::write(netease.folder.join(LOADER_FILE), b"another's").unwrap();
        assert_eq!(install(netease), Ok(false));
        assert_eq!(std::fs::read(netease.folder.join(LOADER_FILE)).unwrap(), b"another's");
        assert_eq!(remove(netease, true), Ok(()));
        assert!(netease.has_loader() && !plugins.join(PLUGIN_FILE).exists() && !plugins.join(ICON_ROW_FILE).exists());
    }

    /// NetEase on this PC, as it is found, and InfLink-rs on it (read only).
    #[test]
    #[ignore = "reads this PC's NetEase"]
    fn finds_netease_here() {
        let _turn = turn();
        let netease = found(false).expect("NetEase");
        println!("{:?} {} wide {} tested {} installed {:?} running {}", netease.folder, netease.version(), netease.wide, netease.tested(), installed(&netease), running(&netease));
    }
}
