//! The phone remote, as Glance's settings turn it on and off and pair
//! phones with it.
//!
//! The remote itself is another program, `glance-remote.exe` beside this
//! one: it runs as the user (never elevated), answers a paired phone on the
//! local network, and is the only one of the two to open a port. Glance
//! starts it (through a task of the user's own, which also starts it at
//! sign-in), lets it through the firewall, and asks it for what the
//! settings show: a pairing offered as a QR code, and the phones paired.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use crate::elevation;

/// The port the remote answers on (its own default: see its configuration).
const PORT: u16 = 47631;
/// The firewall rule that lets phones on the local network reach it.
const RULE: &str = "Glance Remote";
/// A child program started without a window of its own.
const NO_WINDOW: u32 = 0x0800_0000;
/// Where the app is to be had: the page a phone is sent to by a QR code.
pub const APP_PAGE: &str = "https://glancepc.com/app";

/// The remote's program, if this Glance came with it.
pub fn helper() -> Option<PathBuf> {
    let beside = std::env::current_exe().ok()?.parent()?.join("glance-remote.exe");
    beside.is_file().then_some(beside)
}

/// What the remote says, asked with `arguments`.
fn ask(arguments: &[&str]) -> Option<String> {
    let said = Command::new(helper()?).args(arguments).creation_flags(NO_WINDOW).output().ok()?;
    said.status.success().then(|| String::from_utf8_lossy(&said.stdout).into_owned())
}

/// A QR code: its cells, row by row (true, dark), with no margin.
pub struct Code(pub Vec<Vec<bool>>);

fn code(rows: &mut dyn Iterator<Item = &str>) -> Option<Code> {
    let cells: Vec<Vec<bool>> = rows.filter(|row| !row.is_empty()).map(|row| row.bytes().map(|cell| cell == b'1').collect()).collect();
    (!cells.is_empty() && cells.iter().all(|row| row.len() == cells.len())).then_some(Code(cells))
}

/// A pairing offered to one phone, as the QR code it scans: with the app,
/// or (`web`) with its camera, which opens the remote's page in its browser.
pub fn offer(web: bool) -> Option<Code> {
    let said = ask(if web { &["pair", "web", "cells"] } else { &["pair", "cells"] })?;
    // (The link itself, then the code's rows.)
    code(&mut said.lines().skip(1))
}

/// The QR code that sends a phone to where the app is to be had.
pub fn app_code() -> Option<Code> {
    code(&mut ask(&["qr", APP_PAGE])?.lines())
}

/// A phone paired with: its place in the remote's own list, and what it calls itself.
pub struct Phone {
    pub at: usize,
    pub name: String,
}

/// The phones paired with, as the remote keeps them (pairings offered and not yet taken are not among them).
pub fn phones() -> Vec<Phone> {
    let kept = std::env::var_os("APPDATA").map(|roaming| PathBuf::from(roaming).join("dev.weiyi.glance.remote").join("config.json"));
    let config: Option<serde_json::Value> = kept.and_then(|path| std::fs::read_to_string(path).ok()).and_then(|text| serde_json::from_str(&text).ok());
    let Some(list) = config.as_ref().and_then(|config| config.get("phones")).and_then(|phones| phones.as_array()) else { return Vec::new() };
    list.iter()
        .enumerate()
        .filter(|(_, phone)| phone.get("offered_till").is_none_or(|till| till.is_null()))
        .map(|(at, phone)| Phone { at, name: phone.get("name").and_then(|name| name.as_str()).unwrap_or_default().to_string() })
        .collect()
}

/// Unpairs the phone at that place in the remote's list.
pub fn unpair(at: usize) {
    let _ = ask(&["unpair", &at.to_string()]);
}

fn firewall(arguments: &[&str]) -> bool {
    Command::new("netsh").args(["advfirewall", "firewall"]).args(arguments).creation_flags(NO_WINDOW).output().is_ok_and(|done| done.status.success())
}

/// Whether the remote is on: started at sign-in, by this account's task.
pub fn on() -> bool {
    helper().is_some_and(|helper| elevation::remote_enabled(&helper))
}

/// Turns the remote on (its task made and run, the firewall opened to the
/// local network for it alone) or off (stopped, its task and its rule
/// gone): whether it is on then.
pub fn turn(on: bool) -> bool {
    let Some(helper) = helper() else { return false };
    let name = format!("name={RULE}");
    // (Whatever rule of that name there was goes first: one, and current.)
    firewall(&["delete", "rule", &name]);
    if on {
        // (What phones ask of it, and their looking for it when its address has changed.)
        for protocol in ["protocol=TCP", "protocol=UDP"] {
            firewall(&["add", "rule", &name, "dir=in", "action=allow", protocol, &format!("localport={PORT}"), "remoteip=localsubnet", "profile=any", &format!("program={}", helper.display())]);
        }
    }
    elevation::set_remote(&helper, on)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_is_square_or_none() {
        let read = code(&mut "101\n010\n111\n".lines()).unwrap();
        assert_eq!(read.0, [[true, false, true], [false, true, false], [true, true, true]]);
        assert!(code(&mut "101\n01\n".lines()).is_none());
        assert!(code(&mut "".lines()).is_none());
    }
}
