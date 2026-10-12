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
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::UI::Shell::{IShellItem, SHCreateItemFromParsingName, SIGDN_NORMALDISPLAY};
use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK};

pub struct Playing {
    /// What it is called; empty where the program does not say.
    pub title: String,
    pub artist: String,
    pub album: String,
    /// What kind of music the program says it is (a NetEase Cloud Music
    /// with the InfLink-rs plugin puts the track's number there, `NCM-…`).
    pub genres: Vec<String>,
    /// The program playing it, by a short name, and as Windows identifies
    /// it (empty where it does not tell Windows what it plays).
    pub app: String,
    pub app_id: String,
    pub playing: bool,
    /// How far into the track it is now and how long the track is
    /// (seconds), where the program says.
    pub timeline: Option<(f64, f64)>,
}

/// How far into its track `session` is now, and how long the track is
/// (seconds): where it was when the program last said, and, while it
/// plays, on by the time since. None where the program does not say.
fn timeline(session: &Session, playing: bool) -> Option<(f64, f64)> {
    let told = session.GetTimelineProperties().ok()?;
    let ticks = |span: windows::Foundation::TimeSpan| span.Duration as f64 / 1e7;
    let (position, end) = (ticks(told.Position().ok()?), ticks(told.EndTime().ok()?));
    if end <= 0.0 {
        return None;
    }
    // (Windows' times count from 1601, in tenths of a microsecond.)
    let said = told.LastUpdatedTime().ok()?.UniversalTime as f64 / 1e7;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs_f64() + 11_644_473_600.0;
    let since = if playing { (now - said).max(0.0) } else { 0.0 };
    Some(((position + since).min(end), end))
}

/// What Windows would act on were a media key pressed now. (Asked of the
/// one manager, got the first time: it is looked at every second where
/// what plays is shown.)
fn session() -> Result<Session> {
    static MANAGER: Mutex<Option<Manager>> = Mutex::new(None);
    crate::actions::com();
    let mut kept = MANAGER.lock().unwrap();
    let manager = match kept.as_ref() {
        Some(manager) => manager.clone(),
        None => kept.insert(Manager::RequestAsync()?.join()?).clone(),
    };
    manager.GetCurrentSession()
}

/// What `find` finds for `key`, found once: a program's name does not
/// change while it runs. (What is not found is looked for again.)
fn once(kept: &Mutex<Vec<(String, String)>>, key: &str, find: impl FnOnce() -> Option<String>) -> Option<String> {
    if let Some((_, found)) = kept.lock().unwrap().iter().find(|(of, _)| of == key) {
        return Some(found.clone());
    }
    let found = find()?;
    let mut kept = kept.lock().unwrap();
    // (No more than a session's worth of programs.)
    if kept.len() >= 64 {
        kept.clear();
    }
    kept.push((key.to_string(), found.clone()));
    Some(found)
}

/// The name Windows shows a program by (as in the Start menu), for one it
/// identifies as `id`; none for a program Windows has no such name for.
fn shown_as(id: &str) -> Option<String> {
    let path: Vec<u16> = format!("shell:AppsFolder\\{id}").encode_utf16().chain([0]).collect();
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None).ok()?;
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
    let playing = session.GetPlaybackInfo().and_then(|info| info.PlaybackStatus()).is_ok_and(|status| status == Status::Playing);
    let app_id = session.SourceAppUserModelId().map(|id| id.to_string()).unwrap_or_default();
    Some(Playing {
        title,
        artist: about.Artist().map(|artist| artist.to_string()).unwrap_or_default(),
        album: about.AlbumTitle().map(|album| album.to_string()).unwrap_or_default(),
        genres: about.Genres().map(|genres| genres.into_iter().map(|genre| genre.to_string()).collect()).unwrap_or_default(),
        // (One with no name to show is not named: what plays is said
        // already, and a file's name adds nothing to it.)
        app: shown(&app_id).unwrap_or_default(),
        app_id,
        playing,
        timeline: timeline(&session, playing),
    })
}

/// The programs with sound of their own on the device the sound plays
/// from, each by its file's name and whether it is sounding now. (Its name
/// out of what the sound's session is called, which has the program's path
/// in it: the program itself is not opened.)
pub fn sounding() -> Result<Vec<(String, bool)>> {
    Ok(sounds()?.into_iter().map(|(name, _, active)| (name, active)).collect())
}

/// The same, each with the path of its program's file as the session has it.
fn sounds() -> Result<Vec<(String, String, bool)>> {
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
            if let Some((program, path)) = named(&name) {
                found.push((program, path, session.GetState()? == AudioSessionStateActive));
            }
        }
    }
    Ok(found)
}

/// A program's name, and the path of its file, out of what its sound's
/// session is called (`{…}|\Device\…\cloudmusic.exe%b{…}|…`); none for a
/// session with no program's path in it.
fn named(session: &str) -> Option<(String, String)> {
    let path = session.split('|').nth(1)?.split("%b").next()?;
    let file = path.strip_suffix(".exe").or_else(|| path.strip_suffix(".EXE"))?;
    let name = file.rsplit('\\').next()?;
    (!name.is_empty()).then(|| (name.to_string(), path.to_string()))
}

/// What a program's file says the program is ("NetEase Cloud Music"), in
/// the first language it says it in. `path` as a sound's session has it:
/// from the device up.
fn described(path: &str) -> Option<String> {
    static DESCRIBED: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
    once(&DESCRIBED, path, || read_description(path))
}

fn read_description(path: &str) -> Option<String> {
    let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let file = wide(&format!(r"\\?\GLOBALROOT{path}"));
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(file.as_ptr()), None);
        let mut about = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(file.as_ptr()), None, size, about.as_mut_ptr().cast()).ok()?;
        let (mut found, mut length) = (std::ptr::null_mut(), 0u32);
        // The languages it is described in (each a language and a code page), then the description in the first.
        VerQueryValueW(about.as_ptr().cast(), PCWSTR(wide(r"\VarFileInfo\Translation").as_ptr()), &mut found, &mut length).ok().ok()?;
        if length < 4 {
            return None;
        }
        let said_in = std::slice::from_raw_parts(found as *const u16, 2);
        let asked = wide(&format!(r"\StringFileInfo\{:04x}{:04x}\FileDescription", said_in[0], said_in[1]));
        VerQueryValueW(about.as_ptr().cast(), PCWSTR(asked.as_ptr()), &mut found, &mut length).ok().ok()?;
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(found as *const u16, length as usize));
        let text = text.trim_end_matches('\0').trim();
        (!text.is_empty()).then(|| text.to_string())
    }
}

/// What a program is shown as: by the name Windows has for it (as in the
/// Start menu), else as its file describes it, where its file is known by
/// the sound it has open; `id` as Windows identifies it.
fn shown(id: &str) -> Option<String> {
    static SHOWN: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
    once(&SHOWN, id, || {
        shown_as(id).or_else(|| {
            let file = id.strip_suffix(".exe").or_else(|| id.strip_suffix(".EXE")).unwrap_or(id);
            let sounds = sounds().ok()?;
            sounds.iter().find(|(name, _, _)| name.eq_ignore_ascii_case(file)).and_then(|(_, path, _)| described(path))
        })
    })
}

/// The program last heard sounding, of those that do not tell Windows what
/// they play: paused, it is still the one to play again.
static HEARD: Mutex<Option<String>> = Mutex::new(None);

/// The program that is sounding (the one heard before, if it still is);
/// or, none sounding, the one heard before, paused, for as long as it has
/// its sound open.
fn heard() -> Option<Playing> {
    let programs = sounds().ok()?;
    let mut heard = HEARD.lock().unwrap();
    let is = |name: &String, sounding: bool| programs.iter().any(|(program, _, active)| program == name && *active == sounding);
    let now = heard.clone().filter(|name| is(name, true)).or_else(|| programs.iter().find(|(_, _, active)| *active).map(|(name, _, _)| name.clone()));
    let (name, playing) = match now {
        Some(name) => (name, true),
        None => (heard.clone().filter(|name| is(name, false))?, false),
    };
    *heard = Some(name.clone());
    // As its file describes it; one that does not say, by its file's name.
    let app = programs.iter().find(|(program, _, _)| *program == name).and_then(|(_, path, _)| described(path)).unwrap_or(name);
    Some(Playing { title: String::new(), artist: String::new(), album: String::new(), genres: Vec::new(), app, app_id: String::new(), playing, timeline: None })
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
        let named = super::named(r"{0.0.0.00000000}.{6f2a}|\Device\HarddiskVolume3\App\CloudMusic\cloudmusic.exe%b{00000000-0000-0000-0000-000000000000}|1%b9044");
        assert_eq!(named, Some(("cloudmusic".to_string(), r"\Device\HarddiskVolume3\App\CloudMusic\cloudmusic.exe".to_string())));
        // A session with no program's path in it.
        assert_eq!(super::named("{0.0.0.00000000}.{6f2a}|#%b{6f2a}|1%b#"), None);
        assert_eq!(super::named("nothing of the kind"), None);
    }
}
