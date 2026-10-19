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

/// What `target` (a thing kept at hand, or a program the Start menu lists) opens, as one thing however it is named:
/// a desktop program the Start menu names by a known folder and its path in it (`shell:AppsFolder\{folder}\a.exe`)
/// by the file's path, a shortcut by what it points at, a path the same in any case; a Store app, a folder, a web
/// address as it is. Two that open the same are one thing kept (a program dropped on quick launch is the Start
/// menu's). Shortcuts are read through the shell (a thread in a COM apartment); found once each.
pub fn opens(target: &str) -> String {
    static FOUND: Mutex<Option<std::collections::HashMap<String, String>>> = Mutex::new(None);
    if let Some(found) = FOUND.lock().unwrap().as_ref().and_then(|found| found.get(target).cloned()) {
        return found;
    }
    let found = resolved(target);
    let mut held = FOUND.lock().unwrap();
    let held = held.get_or_insert_with(Default::default);
    if held.len() > 2000 {
        held.clear();
    }
    held.insert(target.to_string(), found.clone());
    found
}

/// Whether two things kept at hand (or listed) open the same (see `opens`).
pub fn same(a: &str, b: &str) -> bool {
    a == b || opens(a) == opens(b)
}

fn resolved(target: &str) -> String {
    if target.starts_with("https://") || target.starts_with("http://") {
        return target.to_string();
    }
    let path = match target.strip_prefix("shell:AppsFolder\\") {
        // (A known folder's number, then the path in it; or a path; else a Store app's name.)
        Some(named) => match named.strip_prefix('{').and_then(|rest| rest.split_once("}\\")) {
            Some((folder, rest)) => match known_folder(folder) {
                Some(folder) => folder.join(rest),
                None => return target.to_string(),
            },
            None if Path::new(named).is_absolute() => PathBuf::from(named),
            None => return target.to_string(),
        },
        None => PathBuf::from(target),
    };
    let path = if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("lnk")) { link_target(&path).unwrap_or(path) } else { path };
    path.to_string_lossy().to_lowercase()
}

/// The path of the known folder numbered `id` (its GUID's digits, without braces).
#[cfg(windows)]
fn known_folder(id: &str) -> Option<PathBuf> {
    use windows::Win32::UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    let guid = windows::core::GUID::try_from(id).ok()?;
    unsafe {
        let found = SHGetKnownFolderPath(&guid, KF_FLAG_DEFAULT, None).ok()?;
        let path = found.to_string().ok().map(PathBuf::from);
        windows::Win32::System::Com::CoTaskMemFree(Some(found.0 as *const _));
        path
    }
}

/// What the shortcut at `path` points at, where it says (on a thread in a COM apartment).
#[cfg(windows)]
fn link_target(path: &Path) -> Option<PathBuf> {
    use windows::core::Interface;
    use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        link.cast::<IPersistFile>().ok()?.Load(&windows::core::HSTRING::from(path.as_os_str()), STGM_READ).ok()?;
        let mut found = [0u16; 1024];
        link.GetPath(&mut found, std::ptr::null_mut(), 0).ok()?;
        let length = found.iter().position(|unit| *unit == 0).unwrap_or(found.len());
        (length > 0).then(|| PathBuf::from(String::from_utf16_lossy(&found[..length])))
    }
}

#[cfg(not(windows))]
fn known_folder(_: &str) -> Option<PathBuf> {
    None
}

#[cfg(not(windows))]
fn link_target(_: &Path) -> Option<PathBuf> {
    None
}

/// What is kept at hand changed as `change` changes it (off the calling thread, which the settings then tell).
pub fn change(change: impl FnOnce(&mut Vec<App>) + Send + 'static) {
    std::thread::spawn(move || crate::app().change(|settings| change(&mut settings.apps)));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program the Start menu names by a known folder and the path in it is the program dropped from that folder,
    /// in any case; a Store app and a web address are only themselves.
    #[test]
    #[cfg(windows)]
    fn knows_one_program_however_it_is_named() {
        let system = known_folder("1AC14E77-02E7-4E5D-B744-2EB1AE5198B7").expect("the System folder");
        let dropped = system.join("charmap.exe").to_string_lossy().to_uppercase();
        assert!(same(r"shell:AppsFolder\{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\charmap.exe", &dropped));
        assert!(!same(r"shell:AppsFolder\{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\charmap.exe", &system.join("notepad.exe").to_string_lossy()));
        assert!(!same(r"shell:AppsFolder\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App", r"shell:AppsFolder\Microsoft.Paint_8wekyb3d8bbwe!App"));
        assert!(same("https://github.com", "https://github.com"));
        assert!(!same("https://github.com", "https://GITHUB.com/a"));
    }

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
