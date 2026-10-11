//! What is playing on this PC, and pausing, playing and skipping it.
//!
//! A program that tells Windows what it plays (the same that the
//! keyboard's media keys act on, shown in Windows' own flyout) is known by
//! its title and who it is by, and asked directly. One that does not (some
//! music players) is known only by the sound it makes: its name, and
//! whether it is sounding; it is asked as the keyboard would ask it, by a
//! media key.

use std::sync::Mutex;

use windows::core::{Interface, Result};
use windows::Media::Control::{GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager, GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status};
use windows::Win32::Media::Audio::{eConsole, eRender, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL};
use windows::Win32::UI::Shell::{IShellItem, SHCreateItemFromParsingName, SIGDN_NORMALDISPLAY};
use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK};

pub struct Playing {
    /// What it is called; empty where the program does not say.
    pub title: String,
    pub artist: String,
    /// The program playing it, by a short name.
    pub app: String,
    pub playing: bool,
}

/// What Windows would act on were a media key pressed now.
fn session() -> Result<Session> {
    crate::actions::com();
    Manager::RequestAsync()?.join()?.GetCurrentSession()
}

/// The name Windows shows a program by (as in the Start menu), for one it
/// identifies as `id`; none for a program Windows has no such name for.
fn shown_as(id: &str) -> Option<String> {
    let path: Vec<u16> = format!("shell:AppsFolder\\{id}").encode_utf16().chain([0]).collect();
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(windows::core::PCWSTR(path.as_ptr()), None).ok()?;
        let name = item.GetDisplayName(SIGDN_NORMALDISPLAY).ok()?;
        let shown = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as *const _));
        shown.filter(|shown| !shown.is_empty())
    }
}

/// What a program has told Windows it plays.
fn told() -> Option<Playing> {
    let session = session().ok()?;
    let about = session.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
    let title = about.Title().ok()?.to_string();
    if title.is_empty() {
        return None;
    }
    Some(Playing {
        title,
        artist: about.Artist().map(|artist| artist.to_string()).unwrap_or_default(),
        // By the name Windows shows it by; one it has none for is not named
        // (what plays is said already: a file's name adds nothing to it).
        app: session.SourceAppUserModelId().ok().and_then(|id| shown_as(&id.to_string())).unwrap_or_default(),
        playing: session.GetPlaybackInfo().and_then(|info| info.PlaybackStatus()).is_ok_and(|status| status == Status::Playing),
    })
}

/// The programs with sound of their own on the device the sound plays
/// from, each by its file's name and whether it is sounding now. (Its name
/// out of what the sound's session is called, which has the program's path
/// in it: the program itself is not opened.)
pub fn sounding() -> Result<Vec<(String, bool)>> {
    crate::actions::com();
    let mut found = Vec::new();
    unsafe {
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let manager: IAudioSessionManager2 = devices.GetDefaultAudioEndpoint(eRender, eConsole)?.Activate(CLSCTX_ALL, None)?;
        let sessions = manager.GetSessionEnumerator()?;
        for at in 0..sessions.GetCount()? {
            let session: IAudioSessionControl2 = sessions.GetSession(at)?.cast()?;
            // (Windows' own sounds are no program's.)
            if session.IsSystemSoundsSession().0 == 0 {
                continue;
            }
            let called = session.GetSessionInstanceIdentifier()?;
            let name = called.to_string().unwrap_or_default();
            CoTaskMemFree(Some(called.0 as *const _));
            if let Some(program) = named(&name) {
                found.push((program, session.GetState()? == AudioSessionStateActive));
            }
        }
    }
    Ok(found)
}

/// A program's name out of what its sound's session is called
/// (`{…}|\Device\…\cloudmusic.exe%b{…}|…`); none for a session with no
/// program's path in it.
fn named(session: &str) -> Option<String> {
    let path = session.split('|').nth(1)?.split("%b").next()?;
    let file = path.strip_suffix(".exe").or_else(|| path.strip_suffix(".EXE"))?;
    let name = file.rsplit('\\').next()?;
    (!name.is_empty()).then(|| name.to_string())
}

/// The program last heard sounding, of those that do not tell Windows what
/// they play: paused, it is still the one to play again.
static HEARD: Mutex<Option<String>> = Mutex::new(None);

/// The program that is sounding (the one heard before, if it still is);
/// or, none sounding, the one heard before, paused, for as long as it has
/// its sound open.
fn heard() -> Option<Playing> {
    let programs = sounding().ok()?;
    let mut heard = HEARD.lock().unwrap();
    let is = |name: &String, sounding: bool| programs.iter().any(|(program, active)| program == name && *active == sounding);
    let now = heard.clone().filter(|name| is(name, true)).or_else(|| programs.iter().find(|(_, active)| *active).map(|(name, _)| name.clone()));
    let (app, playing) = match now {
        Some(name) => (name, true),
        None => (heard.clone().filter(|name| is(name, false))?, false),
    };
    *heard = Some(app.clone());
    Some(Playing { title: String::new(), artist: String::new(), app, playing })
}

/// What is playing (or paused); none, when nothing is.
pub fn playing() -> Option<Playing> {
    told().or_else(heard)
}

/// Presses and lets go one of the keyboard's media keys.
fn press(key: VIRTUAL_KEY) -> bool {
    let turn = |up: bool| INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: key, dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() }, ..Default::default() } } };
    unsafe { SendInput(&[turn(false), turn(true)], size_of::<INPUT>() as i32) == 2 }
}

/// How what was asked was done.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Asked {
    /// The program playing took it.
    Taken,
    /// A media key was pressed for it (as far as Windows goes, a key
    /// pressed by hand: the screens wake at it).
    Pressed,
    /// Neither.
    Not,
}

/// Pauses or plays what is playing ("toggle"), or skips on or back
/// ("next", "previous").
pub fn ask(what: &str) -> Asked {
    let key = match what {
        "toggle" => VK_MEDIA_PLAY_PAUSE,
        "next" => VK_MEDIA_NEXT_TRACK,
        "previous" => VK_MEDIA_PREV_TRACK,
        _ => return Asked::Not,
    };
    if let Ok(session) = session() {
        let asked = match what {
            "toggle" => session.TryTogglePlayPauseAsync(),
            "next" => session.TrySkipNextAsync(),
            _ => session.TrySkipPreviousAsync(),
        };
        if asked.and_then(|asking| asking.join()).unwrap_or(false) {
            return Asked::Taken;
        }
    }
    if press(key) {
        Asked::Pressed
    } else {
        Asked::Not
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_a_program_from_its_sound() {
        assert_eq!(super::named(r"{0.0.0.00000000}.{6f2a}|\Device\HarddiskVolume3\App\CloudMusic\cloudmusic.exe%b{00000000-0000-0000-0000-000000000000}|1%b9044").as_deref(), Some("cloudmusic"));
        // A session with no program's path in it.
        assert_eq!(super::named("{0.0.0.00000000}.{6f2a}|#%b{6f2a}|1%b#"), None);
        assert_eq!(super::named("nothing of the kind"), None);
    }
}
