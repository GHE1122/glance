//! The phone remote, as Glance's settings turn it on and off and pair
//! phones with it.
//!
//! The remote itself is another program, `glance-remote.exe` beside this
//! one: it runs as the user (never elevated), answers a paired phone on the
//! local network, and is the only one of the two to open a port. Glance
//! starts it (through a task of the user's own, which also starts it at
//! sign-in), lets it through the firewall, and asks it for what the
//! settings show: a pairing offered as a QR code, and the phones paired.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, SetHandleInformation, HANDLE, HANDLE_FLAGS, HANDLE_FLAG_INHERIT, HLOCAL};
use windows::Win32::Security::AppLocker::{SaferCloseLevel, SaferComputeTokenFromLevel, SaferCreateLevel, SAFER_COMPUTE_TOKEN_FROM_LEVEL_FLAGS, SAFER_LEVELID_NORMALUSER, SAFER_LEVEL_OPEN, SAFER_SCOPEID_USER};
use windows::Win32::Security::Authorization::ConvertStringSidToSidW;
use windows::Win32::Security::{GetLengthSid, SetTokenInformation, TokenIntegrityLevel, PSID, SAFER_LEVEL_HANDLE, SECURITY_ATTRIBUTES, SID_AND_ATTRIBUTES, TOKEN_MANDATORY_LABEL};
use windows::Win32::Storage::FileSystem::ReadFile;
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::System::SystemServices::SE_GROUP_INTEGRITY;
use windows::Win32::System::Threading::{
    CreateProcessAsUserW, DeleteProcThreadAttributeList, GetExitCodeProcess, InitializeProcThreadAttributeList, UpdateProcThreadAttribute, WaitForSingleObject, CREATE_NO_WINDOW, EXTENDED_STARTUPINFO_PRESENT,
    INFINITE, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

use crate::elevation;

/// The port the remote answers on (its own default: see its configuration).
const PORT: u16 = 47631;
/// The firewall rule that lets phones on the local network reach it.
const RULE: &str = "Glance Remote";
/// A child program started without a window of its own.
const NO_WINDOW: u32 = 0x0800_0000;
/// Where the app is to be had: the page a phone is sent to by a QR code.
pub const APP_PAGE: &str = "https://glancepc.com/app";

/// The remote's program, if this Glance came with it.
pub fn helper() -> Option<PathBuf> {
    let beside = std::env::current_exe().ok()?.parent()?.join("glance-remote.exe");
    beside.is_file().then_some(beside)
}

/// What the remote says, asked with `arguments`. It is asked as the user
/// is without an administrator's rights, as it runs: what it reads and
/// keeps is in the user's own folders, which anything of the user's may
/// have changed, and nothing there is to be acted on with Glance's rights.
fn ask(arguments: &[&str]) -> Option<String> {
    let helper = helper()?;
    if elevation::is_elevated() {
        return said_plainly(&helper, arguments);
    }
    let said = Command::new(helper).args(arguments).creation_flags(NO_WINDOW).output().ok()?;
    said.status.success().then(|| String::from_utf8_lossy(&said.stdout).into_owned())
}

/// A handle closed when this is let go.
struct Open(HANDLE);

impl Drop for Open {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// This program's own rights with an administrator's taken from them:
/// those of the same user's ordinary programs (and their standing, which
/// is "medium"), to start another program with.
fn plain_rights() -> Option<Open> {
    unsafe {
        let mut level = SAFER_LEVEL_HANDLE::default();
        SaferCreateLevel(SAFER_SCOPEID_USER, SAFER_LEVELID_NORMALUSER, SAFER_LEVEL_OPEN, &mut level, None).ok()?;
        let mut token = HANDLE::default();
        let computed = SaferComputeTokenFromLevel(level, None, &mut token, SAFER_COMPUTE_TOKEN_FROM_LEVEL_FLAGS(0), None);
        let _ = SaferCloseLevel(level);
        computed.ok()?;
        let token = Open(token);
        let mut medium = PSID::default();
        ConvertStringSidToSidW(w!("S-1-16-8192"), &mut medium).ok()?;
        let label = TOKEN_MANDATORY_LABEL { Label: SID_AND_ATTRIBUTES { Sid: medium, Attributes: SE_GROUP_INTEGRITY as u32 } };
        let set = SetTokenInformation(token.0, TokenIntegrityLevel, (&label as *const TOKEN_MANDATORY_LABEL).cast(), size_of::<TOKEN_MANDATORY_LABEL>() as u32 + GetLengthSid(medium));
        let _ = LocalFree(Some(HLOCAL(medium.0)));
        set.ok()?;
        Some(token)
    }
}

/// An argument as a command line carries it, for the program started to
/// read back what was meant (as Windows' programs split their command lines).
fn quoted(argument: &str) -> String {
    if !argument.is_empty() && !argument.contains([' ', '\t', '"']) {
        return argument.to_string();
    }
    let mut quoted = String::from('"');
    let mut slashes = 0;
    for letter in argument.chars() {
        match letter {
            '\\' => slashes += 1,
            // (The backslashes before a quote are doubled, and the quote itself escaped.)
            '"' => {
                quoted.extend(std::iter::repeat_n('\\', slashes + 1));
                slashes = 0;
            }
            _ => slashes = 0,
        }
        quoted.push(letter);
    }
    // (As are those before the quote that closes it.)
    quoted.extend(std::iter::repeat_n('\\', slashes));
    quoted.push('"');
    quoted
}

/// What `program` says, run with `arguments` and without an
/// administrator's rights (see `plain_rights`), if it ends well. It is
/// given nothing of this program's but somewhere to say it.
fn said_plainly(program: &Path, arguments: &[&str]) -> Option<String> {
    let rights = plain_rights()?;
    let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let name = wide(&program.display().to_string());
    let mut line = wide(&std::iter::once(quoted(&program.display().to_string())).chain(arguments.iter().map(|argument| quoted(argument))).collect::<Vec<_>>().join(" "));
    unsafe {
        let (mut heard, mut said) = (HANDLE::default(), HANDLE::default());
        let passed_on = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, bInheritHandle: true.into(), ..Default::default() };
        CreatePipe(&mut heard, &mut said, Some(&passed_on), 0).ok()?;
        let (heard, said) = (Open(heard), Open(said));
        // (This end stays here.)
        SetHandleInformation(heard.0, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)).ok()?;
        // Of all this program holds open, the program started is given that one end alone.
        let mut size = 0;
        let _ = InitializeProcThreadAttributeList(None, 1, None, &mut size);
        let mut room = vec![0u64; size.div_ceil(8)];
        let list = LPPROC_THREAD_ATTRIBUTE_LIST(room.as_mut_ptr().cast());
        InitializeProcThreadAttributeList(Some(list), 1, None, &mut size).ok()?;
        let given = [said.0];
        let mut process = PROCESS_INFORMATION::default();
        let started = UpdateProcThreadAttribute(list, 0, PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize, Some(given.as_ptr().cast()), size_of_val(&given), None, None).and_then(|()| {
            let mut startup = STARTUPINFOEXW::default();
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdOutput = said.0;
            startup.lpAttributeList = list;
            CreateProcessAsUserW(Some(rights.0), PCWSTR(name.as_ptr()), Some(PWSTR(line.as_mut_ptr())), None, None, true, CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT, None, None, &startup.StartupInfo, &mut process)
        });
        DeleteProcThreadAttributeList(list);
        started.ok()?;
        let (process, _thread) = (Open(process.hProcess), Open(process.hThread));
        // (Its end is its alone now: what it says ends when it does.)
        drop(said);
        let mut all = Vec::new();
        let mut some = [0u8; 4096];
        loop {
            let mut read = 0;
            if ReadFile(heard.0, Some(&mut some), Some(&mut read), None).is_err() || read == 0 {
                break;
            }
            all.extend_from_slice(&some[..read as usize]);
        }
        WaitForSingleObject(process.0, INFINITE);
        let mut ended = 1;
        GetExitCodeProcess(process.0, &mut ended).ok()?;
        (ended == 0).then(|| String::from_utf8_lossy(&all).into_owned())
    }
}

/// A QR code: its cells, row by row (true, dark), with no margin.
pub struct Code(pub Vec<Vec<bool>>);

fn code(rows: &mut dyn Iterator<Item = &str>) -> Option<Code> {
    let cells: Vec<Vec<bool>> = rows.filter(|row| !row.is_empty()).map(|row| row.bytes().map(|cell| cell == b'1').collect()).collect();
    (!cells.is_empty() && cells.iter().all(|row| row.len() == cells.len())).then_some(Code(cells))
}

/// A pairing offered to one phone, as the QR code it scans: with the app,
/// or (`web`) with its camera, which opens the remote's page in its browser.
pub fn offer(web: bool) -> Option<Code> {
    let said = ask(if web { &["pair", "web", "cells"] } else { &["pair", "cells"] })?;
    // (The link itself, then the code's rows.)
    code(&mut said.lines().skip(1))
}

/// The QR code that sends a phone to where the app is to be had.
pub fn app_code() -> Option<Code> {
    code(&mut ask(&["qr", APP_PAGE])?.lines())
}

/// Whether there is an OpenRGB for the remote to set the lights by.
pub fn lights() -> bool {
    ask(&["lights", "there"]).is_some_and(|said| said.trim() == "there")
}

/// Fetches OpenRGB for the remote (some twenty megabytes, from its own
/// project), or takes the one fetched away: whether there is one then.
/// Takes as long as the fetching does.
pub fn set_lights(on: bool) -> bool {
    ask(&["lights", if on { "fetch" } else { "forget" }]).is_some_and(|said| said.lines().last().is_some_and(|last| last.trim() == "there"))
}

/// What the remote finds on this PC (its lights, its monitors, what else
/// it can set), as lines for a report; none where this Glance has no remote.
pub fn report() -> Option<String> {
    ask(&["report"])
}

/// A phone paired with: what the remote tells it from the others by, and what it calls itself.
pub struct Phone {
    pub id: String,
    pub name: String,
}

/// The phones paired with, as the remote keeps them (pairings offered and not yet taken are not among them).
pub fn phones() -> Vec<Phone> {
    let kept = std::env::var_os("APPDATA").map(|roaming| PathBuf::from(roaming).join("dev.weiyi.glance.remote").join("config.json"));
    let config: Option<serde_json::Value> = kept.and_then(|path| std::fs::read_to_string(path).ok()).and_then(|text| serde_json::from_str(&text).ok());
    let Some(list) = config.as_ref().and_then(|config| config.get("phones")).and_then(|phones| phones.as_array()) else { return Vec::new() };
    let text = |phone: &serde_json::Value, name: &str| phone.get(name).and_then(|value| value.as_str()).unwrap_or_default().to_string();
    list.iter().filter(|phone| phone.get("offered_till").is_none_or(|till| till.is_null())).map(|phone| Phone { id: text(phone, "id"), name: text(phone, "name") }).filter(|phone| !phone.id.is_empty()).collect()
}

/// Unpairs the phone the remote tells from the others by `id`.
pub fn unpair(id: &str) {
    let _ = ask(&["unpair", id]);
}

/// Windows' own `netsh`, from where Windows keeps it (and from nowhere
/// else that a program of that name might have been put).
fn netsh() -> PathBuf {
    let mut folder = [0u16; 260];
    let length = unsafe { GetSystemDirectoryW(Some(&mut folder)) } as usize;
    PathBuf::from(String::from_utf16_lossy(&folder[..length.min(folder.len())])).join("netsh.exe")
}

fn firewall(arguments: &[&str]) -> bool {
    Command::new(netsh()).args(["advfirewall", "firewall"]).args(arguments).creation_flags(NO_WINDOW).output().is_ok_and(|done| done.status.success())
}

/// The firewall closed to the remote again: for the uninstaller.
pub fn forget() {
    firewall(&["delete", "rule", &format!("name={RULE}")]);
}

/// The remote running, if it is on: asked of it as Glance starts.
pub fn keep() {
    if let Some(helper) = helper() {
        elevation::run_remote(&helper);
    }
}

/// Whether the remote is on: started at sign-in, by this account's task.
pub fn on() -> bool {
    helper().is_some_and(|helper| elevation::remote_enabled(&helper))
}

/// Turns the remote on (its task made and run, the firewall opened to the
/// local network for it alone) or off (stopped, its task and its rule
/// gone): whether it is on then.
pub fn turn(on: bool) -> bool {
    let Some(helper) = helper() else { return false };
    let name = format!("name={RULE}");
    // (Whatever rule of that name there was goes first: one, and current.)
    firewall(&["delete", "rule", &name]);
    if on {
        // (What phones ask of it, and their looking for it when its address has changed.)
        for protocol in ["protocol=TCP", "protocol=UDP"] {
            firewall(&["add", "rule", &name, "dir=in", "action=allow", protocol, &format!("localport={PORT}"), "remoteip=localsubnet", "profile=any", &format!("program={}", helper.display())]);
        }
    }
    elevation::set_remote(&helper, on)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_carried_as_windows_reads_them_back() {
        assert_eq!(quoted("pair"), "pair");
        assert_eq!(quoted(""), "\"\"");
        assert_eq!(quoted("a b"), "\"a b\"");
        assert_eq!(quoted("C:\\Program Files\\"), "\"C:\\Program Files\\\\\"");
        assert_eq!(quoted("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert!(netsh().is_file());
    }

    /// The remote asked as the user is without an administrator's rights
    /// (to be run elevated, where that is another thing than this
    /// program's own rights): it answers, and what it keeps stays the user's.
    #[test]
    #[ignore]
    fn the_remote_is_asked_without_an_administrators_rights() {
        let helper = std::env::current_exe().unwrap().ancestors().find_map(|folder| Some(folder.join("run").join("glance-remote.exe")).filter(|helper| helper.is_file())).expect("a remote built beside");
        let said = said_plainly(&helper, &["qr", "a b"]).expect("an answer");
        assert!(code(&mut said.lines()).is_some(), "{said}");
        assert!(said_plainly(&helper, &["report"]).is_some_and(|said| said.contains("version")));
        // And what is started so stands as the user's ordinary programs do ("medium", not "high").
        let standing = said_plainly(&netsh().with_file_name("whoami.exe"), &["/groups"]).expect("an answer");
        assert!(standing.contains("S-1-16-8192") && !standing.contains("S-1-16-12288"), "{standing}");
    }

    #[test]
    fn a_code_is_square_or_none() {
        let read = code(&mut "101\n010\n111\n".lines()).unwrap();
        assert_eq!(read.0, [[true, false, true], [false, true, false], [true, true, true]]);
        assert!(code(&mut "101\n01\n".lines()).is_none());
        assert!(code(&mut "".lines()).is_none());
    }
}
