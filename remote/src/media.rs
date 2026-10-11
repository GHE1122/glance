//! What is playing on this PC, as Windows knows it from the program playing
//! it (the same that the keyboard's media keys act on): its title, who it
//! is by, and whether it is playing; and pausing, playing and skipping it.

use windows::core::Result;
use windows::Media::Control::{GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager, GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status};

pub struct Playing {
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

/// A program's name out of how Windows identifies it: "cloudmusic.exe",
/// or "Vendor.Program_abc123!App".
fn program(id: &str) -> String {
    let id = id.split(['_', '!']).next().unwrap_or(id);
    let id = id.strip_suffix(".exe").or_else(|| id.strip_suffix(".EXE")).unwrap_or(id);
    id.rsplit('.').next().unwrap_or(id).to_string()
}

/// What is playing (or paused); none, when nothing is.
pub fn playing() -> Option<Playing> {
    let session = session().ok()?;
    let about = session.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
    let title = about.Title().ok()?.to_string();
    if title.is_empty() {
        return None;
    }
    Some(Playing {
        title,
        artist: about.Artist().map(|artist| artist.to_string()).unwrap_or_default(),
        app: session.SourceAppUserModelId().map(|id| program(&id.to_string())).unwrap_or_default(),
        playing: session.GetPlaybackInfo().and_then(|info| info.PlaybackStatus()).is_ok_and(|status| status == Status::Playing),
    })
}

/// Pauses or plays what is playing ("toggle"), or skips on or back
/// ("next", "previous"): whether its program took it.
pub fn ask(what: &str) -> bool {
    let Ok(session) = session() else { return false };
    let asked = match what {
        "toggle" => session.TryTogglePlayPauseAsync(),
        "next" => session.TrySkipNextAsync(),
        "previous" => session.TrySkipPreviousAsync(),
        _ => return false,
    };
    asked.and_then(|asking| asking.join()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_a_program_shortly() {
        assert_eq!(super::program("cloudmusic.exe"), "cloudmusic");
        assert_eq!(super::program("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"), "ZuneMusic");
        assert_eq!(super::program("Spotify"), "Spotify");
    }
}
