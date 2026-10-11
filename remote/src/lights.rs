//! The PC's lights, through OpenRGB: its SDK server on this machine is asked
//! to put each lighting device out, or to light it as the configuration has
//! it. OpenRGB is started for that if it is not running, and needs no
//! administrator rights for the devices it reaches over USB and the
//! graphics card's own bus.
//!
//! The protocol is OpenRGB's network protocol, spoken at version 3: a header
//! ("ORGB", device, packet, size) and a body, all little-endian.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::config::{Config, Lit};

const SERVER: ([u8; 4], u16) = ([127, 0, 0, 1], 6742);
/// The protocol version asked for: modes have a brightness; zones have no
/// segments yet.
const VERSION: u32 = 3;
/// How long OpenRGB is given to find the devices after it is started.
const STARTING: Duration = Duration::from_secs(40);
/// How long any one answer may take.
const ANSWER: Duration = Duration::from_secs(8);

const CONTROLLER_COUNT: u32 = 0;
const CONTROLLER_DATA: u32 = 1;
const PROTOCOL_VERSION: u32 = 40;
const CLIENT_NAME: u32 = 50;
const RESIZE_ZONE: u32 = 1000;
const UPDATE_LEDS: u32 = 1050;
const UPDATE_MODE: u32 = 1101;

/// A mode of a device, as the server describes it: its name, and all of it
/// as it was sent (handed back as it is to choose it), with where its
/// colours are in that.
#[derive(Clone, Debug, PartialEq)]
pub struct Mode {
    pub name: String,
    raw: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub name: String,
    pub leds_max: u32,
    pub leds: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub name: String,
    pub modes: Vec<Mode>,
    pub zones: Vec<Zone>,
    pub leds: usize,
}

/// A reader over a packet's body.
struct Body<'a>(&'a [u8]);

impl<'a> Body<'a> {
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let (taken, rest) = self.0.split_at_checked(count)?;
        self.0 = rest;
        Some(taken)
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    /// A string: its length (with the zero that ends it), then its bytes.
    fn string(&mut self) -> Option<String> {
        let length = self.u16()? as usize;
        let bytes = self.take(length)?;
        Some(String::from_utf8_lossy(bytes.strip_suffix(&[0]).unwrap_or(bytes)).into_owned())
    }
}

/// A device from the body of a "controller data" answer (protocol 3).
pub fn device(body: &[u8]) -> Option<Device> {
    let mut b = Body(body);
    b.u32()?; // the body's size
    b.u32()?; // the device's type
    let name = b.string()?;
    // Its vendor, description, version, serial and location.
    for _ in 0..5 {
        b.string()?;
    }
    let count = b.u16()?;
    b.u32()?; // the mode it is in
    let mut modes = Vec::new();
    for _ in 0..count {
        let whole = b.0;
        let name = b.string()?;
        // Its value, flags, speeds, brightnesses, colour counts, speed,
        // brightness, direction and colour mode.
        b.take(12 * 4)?;
        let colors = b.u16()? as usize;
        b.take(colors * 4)?;
        modes.push(Mode { name, raw: whole[..whole.len() - b.0.len()].to_vec() });
    }
    let mut zones = Vec::new();
    for _ in 0..b.u16()? {
        let name = b.string()?;
        b.u32()?; // its type
        b.u32()?; // the fewest LEDs it may have
        let (leds_max, leds) = (b.u32()?, b.u32()?);
        let matrix = b.u16()? as usize;
        b.take(matrix)?;
        zones.push(Zone { name, leds_max, leds });
    }
    let leds = b.u16()? as usize;
    Some(Device { name, modes, zones, leds })
}

pub struct Client(TcpStream);

impl Client {
    /// The server on this machine, if it answers.
    fn connect() -> std::io::Result<Client> {
        let stream = TcpStream::connect_timeout(&SocketAddr::from(SERVER), Duration::from_secs(1))?;
        stream.set_read_timeout(Some(ANSWER))?;
        stream.set_write_timeout(Some(ANSWER))?;
        let mut client = Client(stream);
        client.send(0, CLIENT_NAME, b"Glance\0")?;
        client.send(0, PROTOCOL_VERSION, &VERSION.to_le_bytes())?;
        client.answer(PROTOCOL_VERSION)?;
        Ok(client)
    }

    fn send(&mut self, device: u32, packet: u32, body: &[u8]) -> std::io::Result<()> {
        let mut out = Vec::with_capacity(16 + body.len());
        out.extend_from_slice(b"ORGB");
        for word in [device, packet, body.len() as u32] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(body);
        self.0.write_all(&out)
    }

    /// The body of the next answer to `packet` (what else the server says
    /// meanwhile, that its devices changed, is passed over).
    fn answer(&mut self, packet: u32) -> std::io::Result<Vec<u8>> {
        loop {
            let mut header = [0u8; 16];
            self.0.read_exact(&mut header)?;
            if &header[..4] != b"ORGB" {
                return Err(std::io::Error::other("not OpenRGB's answer"));
            }
            let word = |at: usize| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
            // No answer of the protocol's is near this large.
            let size = word(12) as usize;
            if size > 16 << 20 {
                return Err(std::io::Error::other("an answer too large"));
            }
            let mut body = vec![0u8; size];
            self.0.read_exact(&mut body)?;
            if word(8) == packet {
                return Ok(body);
            }
        }
    }

    fn devices(&mut self) -> std::io::Result<Vec<Device>> {
        self.send(0, CONTROLLER_COUNT, &[])?;
        let count = self.answer(CONTROLLER_COUNT)?;
        let count = u32::from_le_bytes(count.get(..4).and_then(|c| c.try_into().ok()).ok_or_else(|| std::io::Error::other("no count"))?);
        (0..count)
            .map(|index| {
                self.send(index, CONTROLLER_DATA, &VERSION.to_le_bytes())?;
                device(&self.answer(CONTROLLER_DATA)?).ok_or_else(|| std::io::Error::other("a device not understood"))
            })
            .collect()
    }

    /// Puts device `index` in `mode`: the mode handed back as the server
    /// described it.
    fn set_mode(&mut self, index: u32, at: usize, mode: &Mode) -> std::io::Result<()> {
        let mut body = Vec::new();
        body.extend_from_slice(&((8 + mode.raw.len()) as u32).to_le_bytes());
        body.extend_from_slice(&(at as u32).to_le_bytes());
        body.extend_from_slice(&mode.raw);
        self.send(index, UPDATE_MODE, &body)
    }

    /// Every LED of device `index`, `count` of them, in one colour.
    fn set_color(&mut self, index: u32, count: usize, (r, g, b): (u8, u8, u8)) -> std::io::Result<()> {
        let mut body = Vec::new();
        body.extend_from_slice(&((4 + 2 + 4 * count) as u32).to_le_bytes());
        body.extend_from_slice(&(count as u16).to_le_bytes());
        for _ in 0..count {
            body.extend_from_slice(&[r, g, b, 0]);
        }
        self.send(index, UPDATE_LEDS, &body)
    }
}

/// Where OpenRGB is: where the configuration says, or where its installer
/// puts it.
fn program(config: &Config) -> Option<PathBuf> {
    let installed = std::env::var_os("ProgramFiles").map(|files| Path::new(&files).join("OpenRGB").join("OpenRGB.exe"));
    config.openrgb.clone().into_iter().chain(installed).find(|path| path.is_file())
}

/// The server, started if it is not running: with its window out of the
/// way, and without its trying to reach a server itself.
fn server(config: &Config) -> std::io::Result<Client> {
    if let Ok(client) = Client::connect() {
        return Ok(client);
    }
    let program = program(config).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "OpenRGB is not installed"))?;
    std::process::Command::new(&program).args(["--server", "--startminimized", "--noautoconnect"]).current_dir(program.parent().unwrap()).spawn()?;
    // It listens before it has found the devices: asked until it has some.
    let since = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(mut client) = Client::connect() {
            if client.devices().is_ok_and(|devices| !devices.is_empty()) || since.elapsed() > STARTING {
                return Ok(client);
            }
        } else if since.elapsed() > STARTING {
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "OpenRGB did not start"));
        }
    }
}

fn rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let value = u32::from_str_radix(hex, 16).ok().filter(|_| hex.len() == 6)?;
    Some(((value >> 16) as u8, (value >> 8) as u8, value as u8))
}

/// The lights put out, or lit as the configuration has each device.
/// Returns how many devices there are.
pub fn set(config: &Config, on: bool) -> std::io::Result<usize> {
    let mut client = server(config)?;
    let mut devices = client.devices()?;
    // A header for addressable strips that OpenRGB has no LEDs for sends
    // nothing: given some, so that what is plugged into it is reached.
    let mut resized = false;
    for (index, device) in devices.iter().enumerate() {
        for (zone, of) in device.zones.iter().enumerate() {
            if of.leds == 0 && of.leds_max > 0 {
                let mut body = (zone as u32).to_le_bytes().to_vec();
                body.extend_from_slice(&config.strip_leds.min(of.leds_max).to_le_bytes());
                client.send(index as u32, RESIZE_ZONE, &body)?;
                resized = true;
            }
        }
    }
    if resized {
        devices = client.devices()?;
    }
    for (index, device) in devices.iter().enumerate() {
        let index = index as u32;
        let named = |name: &str| device.modes.iter().enumerate().find(|(_, mode)| mode.name.eq_ignore_ascii_case(name));
        // In one colour: in the mode that takes each LED's colour as it is
        // sent (or, where a device has none, its static one).
        let paint = |client: &mut Client, colour: (u8, u8, u8)| -> std::io::Result<()> {
            if let Some((at, mode)) = named("Direct").or_else(|| named("Static")) {
                client.set_mode(index, at, mode)?;
            }
            client.set_color(index, device.leds, colour)
        };
        let lit = if on { Some(config.lit(&device.name)) } else { None };
        match lit {
            // Out: the device's own "off", where it has one.
            None => match named("Off") {
                Some((at, mode)) => client.set_mode(index, at, mode)?,
                None => paint(&mut client, (0, 0, 0))?,
            },
            Some(Lit::Mode(name)) => match named(&name) {
                Some((at, mode)) => client.set_mode(index, at, mode)?,
                None => paint(&mut client, (255, 255, 255))?,
            },
            Some(Lit::Color(hex)) => paint(&mut client, rgb(&hex).unwrap_or((255, 255, 255)))?,
        }
    }
    // Sent before the connection closes behind them.
    client.0.flush()?;
    std::thread::sleep(Duration::from_millis(300));
    Ok(devices.len())
}

/// The lighting devices, for choosing how each is lit.
pub fn devices(config: &Config) -> std::io::Result<Vec<Device>> {
    server(config)?.devices()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string(text: &str) -> Vec<u8> {
        let mut out = ((text.len() + 1) as u16).to_le_bytes().to_vec();
        out.extend_from_slice(text.as_bytes());
        out.push(0);
        out
    }

    #[test]
    fn reads_a_device_as_the_server_describes_it() {
        let mode = |name: &str, colors: u16| {
            let mut out = string(name);
            out.extend_from_slice(&[0u8; 48]);
            out.extend_from_slice(&colors.to_le_bytes());
            out.extend(std::iter::repeat_n(0u8, colors as usize * 4));
            out
        };
        let zone = |name: &str, max: u32, leds: u32| {
            let mut out = string(name);
            for word in [1u32, 0, max, leds] {
                out.extend_from_slice(&word.to_le_bytes());
            }
            out.extend_from_slice(&0u16.to_le_bytes());
            out
        };
        let mut body = vec![0u8; 4];
        body.extend_from_slice(&2u32.to_le_bytes());
        for text in ["A card", "Its maker", "", "1.0", "", "I2C"] {
            body.extend(string(text));
        }
        body.extend_from_slice(&2u16.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        let (off, wave) = (mode("Off", 0), mode("Rainbow Wave", 2));
        body.extend(off.iter().chain(&wave));
        body.extend_from_slice(&2u16.to_le_bytes());
        body.extend(zone("GPU", 1, 1));
        body.extend(zone("Strip", 300, 0));
        body.extend_from_slice(&1u16.to_le_bytes());
        let read = device(&body).unwrap();
        assert_eq!(read.name, "A card");
        assert_eq!(read.modes, [Mode { name: "Off".into(), raw: off }, Mode { name: "Rainbow Wave".into(), raw: wave }]);
        assert_eq!(read.zones, [Zone { name: "GPU".into(), leds_max: 1, leds: 1 }, Zone { name: "Strip".into(), leds_max: 300, leds: 0 }]);
        assert_eq!(read.leds, 1);
        // Cut short, it is none.
        assert_eq!(device(&body[..body.len() - 3]), None);
        assert_eq!(rgb("ff8000"), Some((255, 128, 0)));
        assert_eq!(rgb("fff"), None);
    }
}
