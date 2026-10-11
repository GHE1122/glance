//! OpenRGB, fetched when the lights are first wanted and kept in this
//! user's own folder: the program that knows each maker's lights. It does
//! not come with Glance (most of whom have no lights to set), and it is
//! another project's, under its own licence (GPL-2.0-or-later): taken from
//! where that project publishes it, as that project built it.
//!
//! The project publishes no digest of its releases: the one here is of the
//! release as it was first taken, so that what is run is that, whoever
//! serves it.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use windows::core::HSTRING;
use windows::Win32::System::Com::Urlmon::URLDownloadToFileW;

/// OpenRGB 1.0 for 64-bit Windows, as its project publishes it.
const SOURCES: [&str; 1] = ["https://codeberg.org/OpenRGB/OpenRGB/releases/download/release_1.0/OpenRGB_1.0_Windows_64_81bbe18.zip"];
/// The SHA-256 of that archive.
const DIGEST: &str = "182a52a3c97c4c4ae52c80286b4260c9666c51c3447dbc416ee8945f66192e90";

/// The folder it is kept in: this user's, with what else the remote keeps for itself.
fn folder() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("dev.weiyi.glance.remote").join("OpenRGB"))
}

fn within(folder: &Path) -> Option<PathBuf> {
    let mut left = vec![folder.to_path_buf()];
    while let Some(here) = left.pop() {
        for entry in std::fs::read_dir(&here).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                left.push(path);
            } else if path.file_name().is_some_and(|name| name.eq_ignore_ascii_case("OpenRGB.exe")) {
                return Some(path);
            }
        }
    }
    None
}

/// The OpenRGB fetched before, if there is one.
pub fn fetched() -> Option<PathBuf> {
    within(&folder()?)
}

fn other(error: impl ToString) -> std::io::Error {
    std::io::Error::other(error.to_string())
}

/// Fetches OpenRGB (from each place it is to be had, till one gives what
/// it should), checks it is the release meant, and unpacks it: where its
/// program then is.
pub fn fetch() -> std::io::Result<PathBuf> {
    if let Some(there) = fetched() {
        return Ok(there);
    }
    let folder = folder().ok_or_else(|| other("no folder of this user's to keep it in"))?;
    std::fs::create_dir_all(&folder)?;
    let archive = folder.join("OpenRGB.zip");
    let mut taken = Err(other("nowhere to fetch it from"));
    for source in SOURCES {
        let _ = std::fs::remove_file(&archive);
        // (Windows' own way to fetch a file: by this user's proxy and certificates.)
        taken = unsafe { URLDownloadToFileW(None, &HSTRING::from(source), &HSTRING::from(archive.as_os_str()), 0, None) }.map_err(other).and_then(|()| {
            let digest = Sha256::digest(std::fs::read(&archive)?);
            let digest: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
            if digest == DIGEST {
                Ok(())
            } else {
                Err(other("what was fetched is not the release meant"))
            }
        });
        if taken.is_ok() {
            break;
        }
    }
    let unpacked = taken.and_then(|()| unpack(&archive, &folder));
    let _ = std::fs::remove_file(&archive);
    unpacked?;
    fetched().ok_or_else(|| other("the archive held no OpenRGB"))
}

/// Unpacks an archive into `folder`: every file under it, none anywhere else.
fn unpack(archive: &Path, folder: &Path) -> std::io::Result<()> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(archive)?).map_err(other)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(other)?;
        // (A name that would lead out of the folder is no file of this archive's.)
        let Some(name) = entry.enclosed_name() else { return Err(other("the archive names a file outside itself")) };
        let path = folder.join(name);
        if entry.is_dir() {
            std::fs::create_dir_all(&path)?;
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::io::copy(&mut entry, &mut std::fs::File::create(&path)?)?;
        }
    }
    Ok(())
}

/// Takes the OpenRGB fetched away again.
pub fn forget() -> std::io::Result<()> {
    match folder() {
        Some(folder) if folder.is_dir() => std::fs::remove_dir_all(folder),
        _ => Ok(()),
    }
}
