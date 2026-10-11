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
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3, WINHTTP_OPTION_SECURE_PROTOCOLS,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

/// OpenRGB 1.0 for 64-bit Windows, as its project publishes it: from a
/// copy kept with Glance's own releases where mainland China reaches it,
/// then from the project itself.
const SOURCES: [&str; 2] = [
    "https://gitee.com/lulu-loopp/glance/releases/download/v0.3.0/OpenRGB_1.0_Windows_64_81bbe18.zip",
    "https://codeberg.org/OpenRGB/OpenRGB/releases/download/release_1.0/OpenRGB_1.0_Windows_64_81bbe18.zip",
];
/// The SHA-256 of that archive.
const DIGEST: &str = "182a52a3c97c4c4ae52c80286b4260c9666c51c3447dbc416ee8945f66192e90";
/// The most that is taken of what a place serves (the archive is some
/// twenty megabytes), and how long a place is waited on: to be found, to
/// be reached, and for each piece of its answer (ms).
const MOST: usize = 64 << 20;
const FOUND_MS: i32 = 10_000;
const REACHED_MS: i32 = 10_000;
const ANSWERED_MS: i32 = 30_000;

/// A WinHTTP handle, closed when dropped.
struct Handle(*mut core::ffi::c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let _ = unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

/// What an HTTPS address serves (redirects followed, never from HTTPS to
/// HTTP), by this user's proxy if there is one. Windows' own way to fetch
/// that asks nothing of the user's session: it works the same started by
/// Glance as started by the user.
fn get(url: &str) -> std::io::Result<Vec<u8>> {
    let rest = url.strip_prefix("https://").ok_or_else(|| other("not an https address"))?;
    let (host, path) = rest.split_at(rest.find('/').ok_or_else(|| other("an address with no path"))?);
    let there = |handle: Handle| if handle.0.is_null() { Err(std::io::Error::last_os_error()) } else { Ok(handle) };
    unsafe {
        let session = there(Handle(WinHttpOpen(&HSTRING::from(format!("Glance/{}", env!("CARGO_PKG_VERSION"))), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0)))?;
        let protocols = (WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3).to_ne_bytes();
        // Older systems know no TLS 1.3: then 1.2 alone.
        if WinHttpSetOption(Some(session.0), WINHTTP_OPTION_SECURE_PROTOCOLS, Some(&protocols)).is_err() {
            let _ = WinHttpSetOption(Some(session.0), WINHTTP_OPTION_SECURE_PROTOCOLS, Some(&WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2.to_ne_bytes()));
        }
        let _ = WinHttpSetTimeouts(session.0, FOUND_MS, REACHED_MS, ANSWERED_MS, ANSWERED_MS);
        let connection = there(Handle(WinHttpConnect(session.0, &HSTRING::from(host), INTERNET_DEFAULT_HTTPS_PORT, 0)))?;
        let request = there(Handle(WinHttpOpenRequest(connection.0, w!("GET"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE)))?;
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(other)?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(other)?;
        let mut status = 0u32;
        let mut size = size_of::<u32>() as u32;
        WinHttpQueryHeaders(request.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some((&mut status as *mut u32).cast()), &mut size, std::ptr::null_mut()).map_err(other)?;
        if status != 200 {
            return Err(other(format!("it answered {status}")));
        }
        let mut body = Vec::new();
        let mut chunk = vec![0u8; 64 << 10];
        loop {
            let mut read = 0u32;
            WinHttpReadData(request.0, chunk.as_mut_ptr().cast(), chunk.len() as u32, &mut read).map_err(other)?;
            if read == 0 {
                return Ok(body);
            }
            body.extend_from_slice(&chunk[..read as usize]);
            if body.len() > MOST {
                return Err(other("more than the release is"));
            }
        }
    }
}

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
    let mut taken = Err(other("nowhere to fetch it from"));
    for source in SOURCES {
        // (Kept in memory alone till it is known to be the release meant: what is checked is what is unpacked.)
        taken = get(source).and_then(|fetched| {
            let digest: String = Sha256::digest(&fetched).iter().map(|byte| format!("{byte:02x}")).collect();
            if digest == DIGEST {
                Ok(fetched)
            } else {
                Err(other("what was fetched is not the release meant"))
            }
        });
        if taken.is_ok() {
            break;
        }
    }
    std::fs::create_dir_all(&folder)?;
    unpack(taken?, &folder)?;
    fetched().ok_or_else(|| other("the archive held no OpenRGB"))
}

/// Unpacks an archive into `folder`: every file under it, none anywhere else.
fn unpack(archive: Vec<u8>, folder: &Path) -> std::io::Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(archive)).map_err(other)?;
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
