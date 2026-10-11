//! What the phone may ask of this PC besides its lights: the screens off,
//! sleep, the sound's level, and the power plan. Each as any program of the
//! user's may do it: none needs administrator rights.

use windows::core::{Result, GUID};
use windows::Win32::Foundation::HLOCAL;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::System::Power::{PowerGetActiveScheme, PowerSetActiveScheme, SetSuspendState};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

/// Windows' own "Power saver" plan.
pub const POWER_SAVER: GUID = GUID::from_u128(0xa1841308_3541_4fab_bc81_f71556f20b4a);

/// The PC to sleep (not hibernation), letting what is running hear of it.
pub fn sleep() -> bool {
    unsafe { SetSuspendState(false, false, false) }
}

/// The power plan in use.
pub fn power_plan() -> Option<GUID> {
    let mut active = std::ptr::null_mut();
    unsafe {
        PowerGetActiveScheme(None, &mut active).ok().ok()?;
        let plan = *active;
        let _ = windows::Win32::Foundation::LocalFree(Some(HLOCAL(active as *mut _)));
        Some(plan)
    }
}

pub fn set_power_plan(plan: GUID) -> bool {
    unsafe { PowerSetActiveScheme(None, Some(&plan)) }.is_ok()
}

/// The sound's level on the device the sound plays from now.
fn sound() -> Result<IAudioEndpointVolume> {
    unsafe {
        // Once a thread; again is no error to mind.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        devices.GetDefaultAudioEndpoint(eRender, eConsole)?.Activate(CLSCTX_ALL, None)
    }
}

/// How loud the sound is (0 to 100), and whether it is muted.
pub fn volume() -> Option<(u8, bool)> {
    let sound = sound().ok()?;
    unsafe { Some(((sound.GetMasterVolumeLevelScalar().ok()? * 100.0).round() as u8, sound.GetMute().ok()?.as_bool())) }
}

pub fn set_volume(level: Option<u8>, muted: Option<bool>) -> Result<()> {
    let sound = sound()?;
    unsafe {
        if let Some(level) = level {
            sound.SetMasterVolumeLevelScalar(level.min(100) as f32 / 100.0, std::ptr::null())?;
        }
        if let Some(muted) = muted {
            sound.SetMute(muted, std::ptr::null())?;
        }
    }
    Ok(())
}

/// Windows' clock of milliseconds, as the time of the last input is told in.
pub fn tick() -> u32 {
    unsafe { GetTickCount() }
}

/// When the mouse or the keyboard was last touched (see `tick`).
pub fn last_input() -> u32 {
    let mut info = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    let _ = unsafe { GetLastInputInfo(&mut info) };
    info.dwTime
}
