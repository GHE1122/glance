//! The station's own folder (`%LOCALAPPDATA%\Glance\Station`): its shelf (`station.json`), and what it keeps itself,
//! each in a file named by Glance (never by what it came with: a name given by a page or a mail could reach out of
//! the folder, or name a link).
//!
//! A kept file being dragged out is held (`hold`) till the program it was dropped on has read it: taken off the shelf
//! meanwhile, it goes as it is let go (`let_go`).

use super::shelf::Shelf;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub struct Folder {
    dir: PathBuf,
    /// The kept files being dragged out, and how many drags hold each.
    held: HashMap<String, usize>,
    /// Those of them taken off the shelf meanwhile: to go as they are let go.
    doomed: HashSet<String>,
}

const SHELF: &str = "station.json";

impl Folder {
    /// The station's own folder in the user's local application data.
    pub fn own() -> Folder {
        Folder::at(crate::settings::local_dir().join("Station"))
    }

    pub fn at(dir: PathBuf) -> Folder {
        Folder { dir, held: HashMap::new(), doomed: HashSet::new() }
    }

    /// The shelf as it was last saved: none saved, an empty one; one that cannot be read, put aside (as
    /// `station.json.unread`, not written over) and an empty one.
    pub fn load(&self) -> Shelf {
        let path = self.dir.join(SHELF);
        let Ok(bytes) = std::fs::read(&path) else { return Shelf::default() };
        match serde_json::from_slice(&bytes) {
            Ok(shelf) => shelf,
            Err(error) => {
                crate::journal::note(format!("station: its shelf could not be read ({error}); put aside"));
                let _ = std::fs::rename(&path, self.dir.join(format!("{SHELF}.unread")));
                Shelf::default()
            }
        }
    }

    /// Saves `shelf`, whole or not at all (see `elevation::write_in_place`).
    pub fn save(&self, shelf: &Shelf) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        crate::elevation::write_in_place(&self.dir, SHELF, &serde_json::to_vec(shelf).expect("a shelf is written"))
    }

    /// Keeps `bytes` in a file of its own, of the kind `extension` says (letters and digits, else none): its name.
    pub fn keep(&self, bytes: &[u8], extension: &str) -> std::io::Result<String> {
        self.written(extension, |file| std::io::Write::write_all(file, bytes))
    }

    /// Keeps a copy of the file at `path` in a file of its own, of its kind: its name.
    pub fn keep_file(&self, path: &Path) -> std::io::Result<String> {
        let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or_default();
        let mut from = std::fs::File::open(path)?;
        self.written(extension, |file| std::io::copy(&mut from, file).map(|_| ()))
    }

    /// A new file of its own of the kind `extension` says (letters and digits, else none), as `write` writes it: its
    /// name. (Not written whole: none.)
    fn written(&self, extension: &str, write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>) -> std::io::Result<String> {
        std::fs::create_dir_all(&self.dir)?;
        let extension: String = extension.chars().filter(char::is_ascii_alphanumeric).take(8).collect::<String>().to_ascii_lowercase();
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        for attempt in 0u32.. {
            let name = if extension.is_empty() { format!("{stamp:x}-{attempt}") } else { format!("{stamp:x}-{attempt}.{extension}") };
            // (A new file: a name taken, the next.)
            match std::fs::OpenOptions::new().write(true).create_new(true).open(self.dir.join(&name)) {
                Ok(mut file) => {
                    if let Err(error) = write(&mut file) {
                        drop(file);
                        let _ = std::fs::remove_file(self.dir.join(&name));
                        return Err(error);
                    }
                    return Ok(name);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        unreachable!("a name is found")
    }

    /// Where a kept file `name` is.
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(Path::new(name).file_name().unwrap_or_default())
    }

    /// Removes the kept files `names` (one being dragged out, as it is let go).
    pub fn remove<'a>(&mut self, names: impl IntoIterator<Item = &'a str>) {
        for name in names {
            if self.held.contains_key(name) {
                self.doomed.insert(name.to_string());
            } else {
                let _ = std::fs::remove_file(self.path(name));
            }
        }
    }

    /// Holds the kept file `name` while a drag takes it out.
    pub fn hold(&mut self, name: &str) {
        *self.held.entry(name.to_string()).or_default() += 1;
    }

    /// Lets go of the kept file `name` (the drag that held it over): removed, if it was taken off the shelf meanwhile
    /// and no other drag holds it.
    pub fn let_go(&mut self, name: &str) {
        let Some(count) = self.held.get_mut(name) else { return };
        *count -= 1;
        if *count == 0 {
            self.held.remove(name);
            if self.doomed.remove(name) {
                let _ = std::fs::remove_file(self.path(name));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::station::shelf::What;

    fn folder(test: &str) -> Folder {
        let dir = crate::settings::local_dir().join(format!("station-{test}"));
        let _ = std::fs::remove_dir_all(&dir);
        Folder::at(dir)
    }

    #[test]
    fn the_shelf_is_saved_and_read_back() {
        let folder = folder("saved");
        assert_eq!(folder.load(), Shelf::default());
        let mut shelf = Shelf::default();
        shelf.put(What::Text { text: "你好".into() }, 5, true, true, 0);
        folder.save(&shelf).unwrap();
        assert_eq!(folder.load(), shelf);
        // One that cannot be read: put aside, not written over.
        std::fs::write(folder.dir.join(SHELF), b"{ not").unwrap();
        assert_eq!(folder.load(), Shelf::default());
        assert!(folder.dir.join("station.json.unread").exists());
    }

    #[test]
    fn kept_files_are_named_by_glance() {
        let folder = folder("named");
        let a = folder.keep(b"one", "PNG").unwrap();
        let b = folder.keep(b"two", "../../evil\\x").unwrap();
        assert!(a.ends_with(".png"));
        assert_ne!(a, b);
        assert!(b.ends_with(".evilx") && !b.contains(['/', '\\']));
        assert_eq!(std::fs::read(folder.path(&b)).unwrap(), b"two");
        // (A name reaching out of the folder stays in it.)
        assert_eq!(folder.path("..\\..\\x"), folder.dir.join("x"));
    }

    #[test]
    fn a_file_dragged_out_goes_as_it_is_let_go() {
        let mut folder = folder("held");
        let name = folder.keep(b"picture", "png").unwrap();
        folder.hold(&name);
        folder.remove([name.as_str()]);
        assert!(folder.path(&name).exists(), "kept while the drag holds it");
        folder.let_go(&name);
        assert!(!folder.path(&name).exists());
        // Not held: removed at once.
        let other = folder.keep(b"other", "png").unwrap();
        folder.remove([other.as_str()]);
        assert!(!folder.path(&other).exists());
    }
}
