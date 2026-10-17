//! Starting Glance as the user from an elevated process, through the
//! desktop's own shell: the running Explorer's automation object is asked to
//! run it, so it starts with Explorer's token (the user's, unelevated), as if
//! it had been double-clicked. An elevated Glance never shows its windows:
//! it hands the start over this way and becomes the sensors' process.

use windows::core::{Interface, BSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TokenUser, TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, IDispatch, IServiceProvider, CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::{IShellBrowser, IShellDispatch2, IShellFolderViewDual, IShellWindows, ShellWindows, SID_STopLevelBrowser, SVGIO_BACKGROUND, SWC_DESKTOP, SWFO_NEEDDISPATCH};
use windows::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};

/// The argument the started Glance is given: it was started so, and never
/// hands its start over again (were it elevated after all).
pub const HANDED_OFF: &str = "--handed-off";

/// Why the start could not be handed over.
#[derive(Debug, PartialEq)]
pub enum Refused {
    /// No desktop shell to ask (Explorer not running).
    NoShell,
    /// The shell runs elevated itself (UAC off): there is no unelevated token to start with.
    ShellElevated,
    /// The shell is another account's, or another session's.
    ShellNotOurs,
    /// Asking it failed.
    Failed(String),
}

/// Asks the desktop's shell to start `program` with `arguments`, as the user.
pub fn start_as_user(program: &str, arguments: &str) -> Result<(), Refused> {
    let program = program.to_string();
    let arguments = arguments.to_string();
    // On a thread of its own, in an apartment of its own (the caller's may be another kind).
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let done = ask_shell(&program, &arguments);
        CoUninitialize();
        done
    })
    .join()
    .unwrap_or(Err(Refused::Failed("the thread asking the shell failed".into())))
}

unsafe fn ask_shell(program: &str, arguments: &str) -> Result<(), Refused> {
    let failed = |what: &str, error: windows::core::Error| Refused::Failed(format!("{what}: {error}"));
    let windows: IShellWindows = unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER) }.map_err(|_| Refused::NoShell)?;
    // The desktop's own window (CSIDL_DESKTOP, 0).
    let desktop = VARIANT::from(0i32);
    let empty = VARIANT::default();
    let mut window = 0i32;
    let found: IDispatch = unsafe { windows.FindWindowSW(&desktop, &empty, SWC_DESKTOP, &mut window, SWFO_NEEDDISPATCH) }.map_err(|_| Refused::NoShell)?;
    check_shell(HWND(window as isize as *mut _))?;
    let provider: IServiceProvider = found.cast().map_err(|e| failed("its service provider", e))?;
    let browser: IShellBrowser = unsafe { provider.QueryService(&SID_STopLevelBrowser) }.map_err(|e| failed("its browser", e))?;
    let view = unsafe { browser.QueryActiveShellView() }.map_err(|e| failed("its view", e))?;
    let background: IDispatch = unsafe { view.GetItemObject(SVGIO_BACKGROUND) }.map_err(|e| failed("its background", e))?;
    let folder: IShellFolderViewDual = background.cast().map_err(|e| failed("its folder view", e))?;
    let application = unsafe { folder.Application() }.map_err(|e| failed("its application", e))?;
    let shell: IShellDispatch2 = application.cast().map_err(|e| failed("its shell", e))?;
    let arguments = VARIANT::from(BSTR::from(arguments));
    let folder = VARIANT::from(BSTR::from(std::path::Path::new(program).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()));
    let verb = VARIANT::from(BSTR::from("open"));
    // SW_SHOWNORMAL.
    let show = VARIANT::from(1i32);
    unsafe { shell.ShellExecute(&BSTR::from(program), &arguments, &folder, &verb, &show) }.map_err(|e| failed("starting it", e))
}

/// Whether the desktop's shell runs elevated, as this user in this session (UAC off, or the built-in Administrator's
/// account): every program the user starts is then elevated too, a double click on Glance among them.
pub fn shell_elevated() -> bool {
    check_shell(unsafe { GetShellWindow() }) == Err(Refused::ShellElevated)
}

/// Whether the shell whose desktop window is `window` runs as this user, in this session, unelevated.
fn check_shell(window: HWND) -> Result<(), Refused> {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    if pid == 0 {
        return Err(Refused::NoShell);
    }
    let session = |pid: u32| {
        let mut session = u32::MAX;
        unsafe { ProcessIdToSessionId(pid, &mut session) }.ok().map(|_| session)
    };
    if session(pid).is_none() || session(pid) != session(unsafe { GetCurrentProcessId() }) {
        return Err(Refused::ShellNotOurs);
    }
    let theirs = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.map_err(|_| Refused::ShellNotOurs)?;
    let facts = token_facts(theirs);
    let _ = unsafe { CloseHandle(theirs) };
    let (their_user, their_elevation) = facts.ok_or(Refused::ShellNotOurs)?;
    let (our_user, _) = token_facts(unsafe { GetCurrentProcess() }).ok_or(Refused::ShellNotOurs)?;
    if their_user != our_user {
        return Err(Refused::ShellNotOurs);
    }
    if their_elevation {
        return Err(Refused::ShellElevated);
    }
    Ok(())
}

/// A process's user (its SID's bytes) and whether it is elevated.
fn token_facts(process: HANDLE) -> Option<(Vec<u8>, bool)> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let mut user = vec![0u8; 256];
        let mut size = 0u32;
        let user_read = GetTokenInformation(token, TokenUser, Some(user.as_mut_ptr().cast()), user.len() as u32, &mut size).is_ok();
        let mut elevation = TOKEN_ELEVATION::default();
        let elevation_read = GetTokenInformation(token, TokenElevation, Some((&mut elevation as *mut TOKEN_ELEVATION).cast()), size_of::<TOKEN_ELEVATION>() as u32, &mut size).is_ok();
        let _ = CloseHandle(token);
        if !user_read || !elevation_read {
            return None;
        }
        let sid = (*(user.as_ptr() as *const TOKEN_USER)).User.Sid;
        let length = windows::Win32::Security::GetLengthSid(sid) as usize;
        Some((std::slice::from_raw_parts(sid.0 as *const u8, length).to_vec(), elevation.TokenIsElevated != 0))
    }
}

