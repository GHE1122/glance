//! Quick launch: the programs, files, folders and web addresses kept at hand on the workbench, in their order (see
//! docs/station-and-notes.md, section 八). Each is kept as Windows names it to be opened: a program from the Start
//! menu by its shell name (`shell:AppsFolder\…`, a Store app's and a desktop program's alike), a shortcut, file or
//! folder by its path (a shortcut kept as the shortcut), a web address as it is. Opened as the user, by the shell, on
//! the station's thread (see `run::Ask::Launch`): Glance's UI runs unelevated, so what it opens does too.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// One thing kept at hand: what opens it, and its name as it is shown.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct App {
    pub target: String,
    pub name: String,
}

impl App {
    /// A file, folder or shortcut dropped to be kept at hand: by its name without its extension (a folder's whole).
    pub fn of_path(path: &Path) -> Option<App> {
        let name = if path.is_dir() { path.file_name()? } else { path.file_stem()? };
        Some(App { target: path.display().to_string(), name: name.to_string_lossy().into_owned() })
    }

    /// A web address kept at hand: by its site.
    pub fn of_url(url: &str) -> Option<App> {
        let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
        let site = rest.split(['/', '?', '#']).next()?.trim_start_matches("www.");
        (!site.is_empty()).then(|| App { target: url.to_string(), name: site.to_string() })
    }

    /// Whether it is a web address.
    pub fn is_url(&self) -> bool {
        self.target.starts_with("https://") || self.target.starts_with("http://")
    }

    /// Where it is on the disk, for it to be shown in Explorer (none for a Start menu program or a web address).
    pub fn location(&self) -> Option<PathBuf> {
        (!self.is_url() && !self.target.starts_with("shell:")).then(|| PathBuf::from(&self.target))
    }
}

static SHOWN: Mutex<Option<Arc<Vec<App>>>> = Mutex::new(None);

/// What is kept at hand, in its order.
pub fn shown() -> Arc<Vec<App>> {
    SHOWN.lock().unwrap().clone().unwrap_or_default()
}

/// The settings' list taken as what is kept at hand (as they are read, and each time they change).
pub fn follow(apps: &[App]) {
    let mut shown = SHOWN.lock().unwrap();
    if shown.as_deref().map(Vec::as_slice) != Some(apps) {
        *shown = Some(Arc::new(apps.to_vec()));
    }
}

/// For pictures: as if these were kept at hand.
#[cfg(all(test, feature = "studio"))]
pub fn pretend(apps: Vec<App>) {
    *SHOWN.lock().unwrap() = Some(Arc::new(apps));
}

/// What is kept at hand changed as `change` changes it (off the calling thread, which the settings then tell).
pub fn change(change: impl FnOnce(&mut Vec<App>) + Send + 'static) {
    std::thread::spawn(move || crate::app().change(|settings| change(&mut settings.apps)));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dropped file by its name without its extension, a web address by its site; where each is on the disk.
    #[test]
    fn keeps_what_is_dropped() {
        let file = App::of_path(Path::new(r"C:\Tools\Notepad++.lnk")).unwrap();
        assert_eq!((file.name.as_str(), file.location()), ("Notepad++", Some(PathBuf::from(r"C:\Tools\Notepad++.lnk"))));
        let site = App::of_url("https://www.github.com/lulu-loopp/glance").unwrap();
        assert_eq!((site.name.as_str(), site.is_url(), site.location()), ("github.com", true, None));
        let store = App { target: r"shell:AppsFolder\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".into(), name: "Calculator".into() };
        assert_eq!(store.location(), None);
        assert!(App::of_url("ftp://x").is_none());
    }
}
