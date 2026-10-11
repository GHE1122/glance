//! Glance's remote control helper: a paired phone, on the same network, may
//! ask this PC to put its screens and lights out, to sleep, to turn the
//! sound up or down. It runs as the user, never elevated, only while remote
//! control is on; the monitor itself opens no port.
//!
//!     glance-remote            listens
//!     glance-remote pair       shows the pairing code (a QR code) for the phone
//!     glance-remote lights     lists the lighting devices OpenRGB reaches
//!     glance-remote monitors   lists the monitors and what they answer over DDC/CI

mod actions;
mod config;
mod cooler;
mod display;
mod keys;
mod lights;
mod media;
mod power;
mod reads;
mod sign;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use config::{Config, Effect, Light, Phone};
use serde_json::{json, Value};

/// The most a request may be: none of the phone's is near it.
const MOST: usize = 16 << 10;
/// How long a request may take to arrive whole.
const ARRIVING: Duration = Duration::from_secs(5);
/// How long after night mode begins the mouse and the keyboard are not yet
/// minded (whoever asked for it may still be letting go of them), and how
/// often they are looked at after.
const SETTLING_MS: u32 = 3000;
const WATCH: Duration = Duration::from_millis(500);
/// How often lights that follow the heat are looked at again.
const FOLLOW: Duration = Duration::from_secs(2);
/// The heat the coolest colour is for, and the warmest (°C).
const COOL: i64 = 40;
const HOT: i64 = 85;

/// The colour (RRGGBB) for how warm the PC is: blue while it is cool,
/// through green and amber, to red when it is hot.
fn heat(celsius: i64) -> String {
    let along = (celsius.clamp(COOL, HOT) - COOL) as f32 / (HOT - COOL) as f32;
    // Round the wheel of hues from blue (210 degrees) back to red (0).
    let hue = 210.0 * (1.0 - along) / 60.0;
    let channel = |n: f32| {
        let k = (n + hue) % 6.0;
        ((1.0 - k.min(4.0 - k).clamp(0.0, 1.0)) * 255.0).round() as u8
    };
    format!("{:02X}{:02X}{:02X}", channel(5.0), channel(3.0), channel(1.0))
}

/// Night mode: the power plan to go back to, and when it began (see
/// `actions::tick`).
struct Night {
    power: Option<power::Before>,
    since: u32,
}

#[derive(Default)]
struct State {
    seen: sign::Seen,
    /// Whether the lights are on, once this helper has set them.
    lights: Option<bool>,
    night: Option<Night>,
}

struct Helper {
    config: Config,
    /// How the lights are when they are on: as the phone last chose.
    light: Mutex<Light>,
    /// How bright the light under the keys is with them, once chosen.
    keys: Mutex<Option<u8>>,
    /// The phones paired with, as the configuration had them when it was
    /// last kept (and when that was): read again once it is kept anew, by
    /// this or by another of this program offering a pairing.
    phones: Mutex<(Option<std::time::SystemTime>, Vec<Phone>)>,
    state: Mutex<State>,
    /// Held while the lights are being set: one at a time.
    lighting: Mutex<()>,
    /// A maker's own lighting program has started since OpenRGB was last
    /// started: OpenRGB is to be started afresh before it is asked again.
    stale: std::sync::atomic::AtomicBool,
}

impl Helper {
    fn lights(&self, on: bool) -> Result<(), String> {
        // (The state is not held while OpenRGB is asked: that may take a while.)
        let _one = self.lighting.lock().unwrap();
        self.refresh();
        let mut light = self.light.lock().unwrap().clone();
        if light.effect == Effect::Heat {
            if let Some(celsius) = reads::read().as_ref().and_then(reads::hottest) {
                light.color = heat(celsius);
            }
        }
        // A cooler's little display goes out and comes on with them
        // (whether or not there are other lights to set), and meanwhile:
        // each takes its own while.
        let cooler = std::thread::spawn(move || if on { cooler::lit() } else { cooler::dark() });
        // And the light under a laptop's keys, where its brightness has been chosen.
        if let Some(level) = *self.keys.lock().unwrap() {
            keys::light(if on { level } else { 0 });
        }
        let set = lights::set(&self.config, on, &light).map_err(|error| error.to_string());
        let _ = cooler.join();
        set?;
        self.state.lock().unwrap().lights = Some(on);
        Ok(())
    }

    /// OpenRGB started afresh, if a maker's program has taken the lights
    /// from the one running (see `lights::RIVALS`).
    fn refresh(&self) {
        if self.stale.swap(false, std::sync::atomic::Ordering::SeqCst) {
            lights::afresh();
        }
    }

    /// Lights that follow the heat, put in the colour of how warm the PC
    /// is now, where that is another than `last` (the one they were last
    /// put in by this).
    fn follow(&self, last: &mut Option<String>) {
        let _one = self.lighting.lock().unwrap();
        let lit = {
            let state = self.state.lock().unwrap();
            state.lights == Some(true) && state.night.is_none()
        };
        if !lit || self.light.lock().unwrap().effect != Effect::Heat {
            *last = None;
            return;
        }
        let Some(color) = reads::read().as_ref().and_then(reads::hottest).map(heat) else { return };
        self.refresh();
        if last.as_ref() != Some(&color) && lights::set(&self.config, true, &Light { color: color.clone(), effect: Effect::Steady, brightness: self.light.lock().unwrap().brightness }).is_ok() {
            *last = Some(color);
        }
    }

    /// The lights' colour or what they do, changed from the phone: kept,
    /// for the lights on from then on.
    fn choose(&self, color: Option<&str>, effect: Option<Effect>, brightness: Option<u64>) {
        let mut light = self.light.lock().unwrap();
        if let Some(color) = color.filter(|color| color.len() == 6 && color.bytes().all(|b| b.is_ascii_hexdigit())) {
            light.color = color.to_ascii_uppercase();
        }
        if let Some(effect) = effect {
            light.effect = effect;
        }
        // (Never none: that is the lights off, which is asked for as such.)
        if let Some(brightness) = brightness {
            light.brightness = brightness.clamp(1, 100) as u8;
        }
        let chosen = light.clone();
        let _ = Config::change(|config| config.light = chosen);
    }

    /// Whether a request is a paired phone's (see `sign::Seen::check`), and
    /// the phone it calls itself is the one its key belongs to. A key not
    /// used before becomes that phone's.
    #[allow(clippy::too_many_arguments)]
    fn admit(&self, method: &str, path: &str, time: &str, nonce: &str, sign: &str, body: &[u8], device: &str, name: &str) -> Result<String, sign::Refused> {
        let mut phones = self.phones.lock().unwrap();
        let kept = Config::kept_at();
        if phones.0 != kept {
            *phones = (kept, Config::load().phones);
        }
        let now = sign::now();
        let mut state = self.state.lock().unwrap();
        for phone in phones.1.iter().filter(|phone| phone.offered_till.is_none_or(|till| till > now)) {
            let Some(key) = sign::key(&phone.key) else { continue };
            match state.seen.check(&key, method, path, time, nonce, sign, body, now) {
                Err(sign::Refused::Forged) => continue,
                Err(refused) => return Err(refused),
                Ok(()) => {}
            }
            return match &phone.device {
                Some(owner) if owner != device => Err(sign::Refused::Taken),
                Some(_) => Ok(phone.key.clone()),
                // (A request that names no phone leaves the key as it is.)
                None if device.is_empty() => Ok(phone.key.clone()),
                None => {
                    let (key, device, name) = (phone.key.clone(), device.to_string(), name.to_string());
                    let changed = Config::change(|config| {
                        // A phone paired anew is that phone still: what it was paired by before is gone.
                        config.phones.retain(|phone| phone.device.as_deref() != Some(device.as_str()));
                        if let Some(phone) = config.phones.iter_mut().find(|phone| phone.key == key) {
                            *phone = Phone { key: key.clone(), name, device: Some(device), offered_till: None };
                        }
                    });
                    if let Ok(config) = changed {
                        *phones = (Config::kept_at(), config.phones);
                    }
                    Ok(key)
                }
            };
        }
        Err(sign::Refused::Forged)
    }

    /// Night: the lights out, the power-saving plan, the screens off. Day
    /// again: the plan there was and the lights on (the screens come on at
    /// the touch that ends it, or stay off till one).
    fn night(&self, on: bool) -> Result<(), String> {
        if on {
            if self.state.lock().unwrap().night.is_some() {
                return Ok(());
            }
            // The lights first: they may take OpenRGB a while to reach.
            let lit = self.lights(false);
            let power = power::save();
            let dark = display::sleep().map_err(|error| error.to_string());
            self.state.lock().unwrap().night = Some(Night { power, since: actions::tick() });
            lit.and(dark)
        } else {
            let Some(night) = self.state.lock().unwrap().night.take() else { return Ok(()) };
            if let Some(power) = night.power {
                power::restore(power);
            }
            display::wake();
            self.lights(true)
        }
    }

    fn state(&self) -> Value {
        // As OpenRGB has them; while it is not running, as this helper last
        // set them.
        let lights = lights::lit().or(self.state.lock().unwrap().lights);
        let light = self.light.lock().unwrap().clone();
        let playing = media::playing();
        let state = self.state.lock().unwrap();
        let sound = actions::volume();
        json!({
            "name": self.config.name,
            "version": env!("CARGO_PKG_VERSION"),
            "lights": lights,
            "light": light,
            "reads": reads::read(),
            // A laptop's keys: how bright their light is set to be with the lights on (not known till chosen), of how bright it goes.
            "keys": keys::there().then(|| json!({ "level": *self.keys.lock().unwrap(), "most": keys::MOST })),
            "media": playing.map(|media| json!({ "title": media.title, "artist": media.artist, "app": media.app, "playing": media.playing })),
            "night": state.night.is_some(),
            "power": power::choices().iter().map(|choice| json!({ "id": choice.id, "name": choice.name, "on": choice.on })).collect::<Vec<_>>(),
            "screen": display::on(),
            "monitors": display::monitors().iter().map(|monitor| json!({ "name": monitor.name, "x": monitor.place.0, "y": monitor.place.1, "width": monitor.place.2, "height": monitor.place.3, "on": monitor.on, "brightness": monitor.brightness })).collect::<Vec<_>>(),
            "volume": sound.map(|(level, _)| level),
            "muted": sound.map(|(_, muted)| muted),
            "idle_seconds": actions::tick().wrapping_sub(actions::last_input()) / 1000,
        })
    }

    /// Does what a request asks: its answer, or why not (with the status).
    fn act(self: &Arc<Self>, method: &str, path: &str, body: &[u8], key: &str) -> Result<Value, (u16, String)> {
        let failed = |error: String| (500, error);
        let asked: Value = if body.is_empty() { Value::Null } else { serde_json::from_slice(body).map_err(|_| (400, "the body is not JSON".to_string()))? };
        let on = || asked.get("on").and_then(Value::as_bool).ok_or((400, "\"on\" is missing".to_string()));
        match (method, path) {
            ("GET", "/v1/state") => {}
            ("POST", "/v1/screen") => {
                if on()? {
                    display::wake()
                } else {
                    display::sleep().map_err(|error| failed(error.to_string()))?
                }
            }
            // One monitor by itself, by its place among them.
            ("POST", "/v1/monitor") => {
                let index = asked.get("index").and_then(Value::as_u64).ok_or((400, "\"index\" is missing".to_string()))? as usize;
                // On or off, or as bright as asked.
                let took = match asked.get("brightness").and_then(Value::as_u64) {
                    Some(percent) => display::dim(index, percent.min(100) as u8),
                    None => display::turn(index, on()?),
                };
                if !took {
                    return Err((500, "the monitor did not take it".into()));
                }
            }
            // On or off, in another colour, doing something else: any of
            // them. A colour or an effect alone lights them so.
            ("POST", "/v1/lights") => {
                let effect = asked.get("effect").and_then(|effect| serde_json::from_value::<Effect>(effect.clone()).ok());
                self.choose(asked.get("color").and_then(Value::as_str), effect, asked.get("brightness").and_then(Value::as_u64));
                self.lights(asked.get("on").and_then(Value::as_bool).unwrap_or(true)).map_err(failed)?
            }
            // How bright the light under the keys is: set now (unless the
            // lights are out: then for when they are on again), and kept.
            ("POST", "/v1/keys") => {
                let level = asked.get("level").and_then(Value::as_u64).ok_or((400, "\"level\" is missing".to_string()))?.min(keys::MOST as u64) as u8;
                *self.keys.lock().unwrap() = Some(level);
                let _ = Config::change(|config| config.keys = Some(level));
                let out = self.state.lock().unwrap().lights == Some(false);
                if !out && !keys::light(level) {
                    return Err((500, "the keyboard did not take it".into()));
                }
            }
            ("POST", "/v1/media") => {
                match media::ask(asked.get("do").and_then(Value::as_str).unwrap_or_default()) {
                    media::Asked::Taken => {}
                    // A key pressed is a touch of the keyboard, at which night
                    // would end and the screens wake: it is none of the user's,
                    // so night goes on and the screens are put out again.
                    media::Asked::Pressed => {
                        let mut state = self.state.lock().unwrap();
                        if let Some(night) = state.night.as_mut() {
                            night.since = actions::tick();
                            drop(state);
                            let _ = display::sleep();
                        }
                    }
                    media::Asked::Not => return Err((500, "nothing playing took it".into())),
                }
            }
            // One of the PC's power modes (or plans), by its id as the state
            // gives it. (Night over, what is gone back to is what there was before
            // night, not one chosen during it.)
            ("POST", "/v1/power") => {
                if !power::choose(asked.get("plan").and_then(Value::as_str).unwrap_or_default()) {
                    return Err((500, "the plan was not taken".into()));
                }
            }
            // The phone that asks, paired no more (it has unpaired itself).
            ("POST", "/v1/unpair") => {
                let _ = Config::change(|config| config.phones.retain(|phone| phone.key != key));
            }
            ("POST", "/v1/night") => self.night(on()?).map_err(failed)?,
            ("POST", "/v1/volume") => {
                let level = asked.get("level").and_then(Value::as_u64).map(|level| level.min(100) as u8);
                let muted = asked.get("muted").and_then(Value::as_bool);
                actions::set_volume(level, muted).map_err(|error| failed(error.to_string()))?
            }
            // Answered first: asleep, it answers nothing.
            ("POST", "/v1/sleep") => {
                std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_secs(1));
                    actions::sleep();
                });
            }
            _ => return Err((404, "no such thing to ask".into())),
        }
        Ok(self.state())
    }

    /// Reads one request from `stream`, checks it is the paired phone's,
    /// does it and answers.
    fn serve(self: &Arc<Self>, mut stream: TcpStream) -> std::io::Result<()> {
        stream.set_read_timeout(Some(ARRIVING))?;
        stream.set_write_timeout(Some(ARRIVING))?;
        let mut taken = Vec::new();
        let mut chunk = [0u8; 2048];
        // The head, then as much body as it says.
        let (head, mut body) = loop {
            let read = stream.read(&mut chunk)?;
            taken.extend_from_slice(&chunk[..read]);
            if let Some(end) = taken.windows(4).position(|four| four == b"\r\n\r\n") {
                break (String::from_utf8_lossy(&taken[..end]).into_owned(), taken[end + 4..].to_vec());
            }
            if read == 0 || taken.len() > MOST {
                return Ok(());
            }
        };
        let mut lines = head.split("\r\n");
        let mut request = lines.next().unwrap_or_default().split(' ');
        let (method, path) = (request.next().unwrap_or_default(), request.next().unwrap_or_default());
        let header = |name: &str| lines.clone().filter_map(|line| line.split_once(':')).find(|(key, _)| key.eq_ignore_ascii_case(name)).map_or("", |(_, value)| value.trim());
        let length: usize = header("content-length").parse().unwrap_or(0);
        if length > MOST {
            return Ok(());
        }
        while body.len() < length {
            let read = stream.read(&mut chunk)?;
            if read == 0 {
                return Ok(());
            }
            body.extend_from_slice(&chunk[..read]);
        }
        body.truncate(length);
        // The page a phone's browser controls this PC from (a phone with no
        // app for it): anyone's to fetch, for it holds nothing; what it
        // then asks is signed as the app's is, with a key it was paired by.
        if method == "GET" && path == "/" {
            let page = include_str!("web.html");
            return write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len());
        }
        let checked = self.admit(method, path, header("x-glance-time"), header("x-glance-nonce"), header("x-glance-sign"), &body, header("x-glance-phone"), &unescaped(header("x-glance-name")));
        let (status, answer) = match checked {
            Err(refused) => (401, json!({ "error": format!("{refused:?}").to_lowercase(), "time": sign::now() })),
            Ok(key) => match self.act(method, path, &body, &key) {
                Ok(answer) => (200, answer),
                Err((status, error)) => (status, json!({ "error": error })),
            },
        };
        let answer = answer.to_string();
        let reason = if status == 200 { "OK" } else { "Error" };
        write!(stream, "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}", answer.len())
    }
}

/// This PC's address on the network it reaches others by.
fn address() -> Option<String> {
    // Nothing is sent: a socket "connected" to an address beyond the
    // network takes the address of the way there.
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

/// A text's QR code as rows of 1 (dark) and 0, a line each, with no margin.
fn cells(text: &str) -> std::io::Result<String> {
    let code = qrcode::QrCode::new(text.as_bytes()).map_err(std::io::Error::other)?;
    let width = code.width();
    let dark: Vec<bool> = code.to_colors().into_iter().map(|cell| cell == qrcode::Color::Dark).collect();
    Ok(dark.chunks(width).map(|row| row.iter().map(|cell| if *cell { '1' } else { '0' }).collect::<String>() + "\n").collect())
}

/// Text as a link carries it: all but letters, figures and hyphens by
/// their bytes' numbers.
fn escaped(text: &str) -> String {
    text.bytes().map(|b| if b.is_ascii_alphanumeric() || b == b'-' { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// The text a link carried (see `escaped`).
fn unescaped(text: &str) -> String {
    let mut bytes = Vec::new();
    let mut rest = text.as_bytes();
    while let Some((first, after)) = rest.split_first() {
        let number = (*first == b'%' && after.len() >= 2).then(|| std::str::from_utf8(&after[..2]).ok().and_then(|hex| u8::from_str_radix(hex, 16).ok())).flatten();
        match number {
            Some(byte) => {
                bytes.push(byte);
                rest = &after[2..];
            }
            None => {
                bytes.push(*first);
                rest = after;
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// What a phone needs to pair, as the link its QR code holds: where this
/// PC is, and a key offered to the phone that takes it.
/// The same for a phone's browser: the page's address, the key after the
/// `#` (which a browser keeps to itself: it is never sent).
fn pairing_web(config: &Config, key: &str) -> String {
    format!("http://{}:{}/#k={key}", address().unwrap_or_default(), config.port)
}

fn pairing(config: &Config, key: &str) -> String {
    format!("glance://pair?h={}&p={}&k={key}&n={}", address().unwrap_or_default(), config.port, escaped(&config.name))
}

fn main() -> std::io::Result<()> {
    // The monitors' sizes in their own pixels, whatever each one's scale.
    let _ = unsafe { windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let config = Config::load();
    match std::env::args().nth(1).as_deref() {
        // glance-remote pair [web] [cells]: a pairing offered, as its link
        // (for a phone's browser rather than the app, with "web"). With
        // "cells", the link's QR code follows as rows of 1 and 0 (for
        // Glance to draw); without, it is written as a picture, whose
        // path follows.
        Some("pair") => {
            let asked = |word: &str| std::env::args().skip(2).any(|given| given == word);
            let key = Config::offer()?;
            let link = if asked("web") { pairing_web(&config, &key) } else { pairing(&config, &key) };
            println!("{link}");
            if asked("cells") {
                print!("{}", cells(&link)?);
            } else {
                let code = qrcode::QrCode::new(link.as_bytes()).map_err(std::io::Error::other)?;
                let picture = code.render::<qrcode::render::svg::Color>().min_dimensions(480, 480).quiet_zone(true).build();
                let path = std::env::temp_dir().join("glance-pair.svg");
                std::fs::write(&path, picture)?;
                println!("{}", path.display());
            }
            return Ok(());
        }
        // glance-remote qr TEXT: any text's QR code, as rows of 1 and 0.
        Some("qr") => {
            print!("{}", cells(&std::env::args().nth(2).unwrap_or_default())?);
            return Ok(());
        }
        Some("lights") => {
            for device in lights::devices(&config)? {
                println!("{} now: {} ({}), first colours {:06X?}", device.name, device.modes.get(device.active).map_or("?", |mode| mode.name.as_str()), if device.lit() { "lit" } else { "dark" }, device.colors.iter().take(3).collect::<Vec<_>>());
                println!("{}\n  modes: {}\n  zones: {}", device.name, device.modes.iter().map(|mode| mode.name.as_str()).collect::<Vec<_>>().join(", "), device.zones.iter().map(|zone| format!("{} ({} LEDs)", zone.name, zone.leds)).collect::<Vec<_>>().join(", "));
            }
            return Ok(());
        }
        // The phones paired with (and the pairings offered), in their order.
        Some("phones") => {
            for (at, phone) in config.phones.iter().enumerate() {
                let what = match (&phone.device, phone.offered_till) {
                    (Some(_), _) => "paired",
                    (None, Some(_)) => "offered",
                    (None, None) => "paired before phones were told apart",
                };
                println!("{at}: {} ({what})", if phone.name.is_empty() { "?" } else { &phone.name });
            }
            return Ok(());
        }
        // glance-remote unpair N: the phone at that place, paired no more.
        Some("unpair") => {
            if let Some(at) = std::env::args().nth(2).and_then(|text| text.parse::<usize>().ok()) {
                Config::change(|config| {
                    if at < config.phones.len() {
                        config.phones.remove(at);
                    }
                })?;
            }
            return Ok(());
        }
        // glance-remote cooler off|on: the cooler's display alone.
        Some("cooler") => {
            let off = std::env::args().nth(2).as_deref() == Some("off");
            println!("{}", if off { cooler::dark() } else { cooler::lit() });
            return Ok(());
        }
        Some("media") => {
            println!("{:?}", media::sounding());
            if let Some(playing) = media::playing() {
                println!("{:?} by {:?} in {}: playing {}", playing.title, playing.artist, playing.app, playing.playing);
            }
            return Ok(());
        }
        Some("power") => {
            for choice in power::choices() {
                println!("{} {}{}", choice.id, choice.name, if choice.on { " *" } else { "" });
            }
            return Ok(());
        }
        Some("monitors") => {
            for monitor in display::monitors() {
                println!("{} ({}x{} at {},{}): on {:?}", monitor.name, monitor.place.2, monitor.place.3, monitor.place.0, monitor.place.1, monitor.on);
                println!("  brightness {:?}", monitor.brightness);
            }
            return Ok(());
        }
        // glance-remote keys LEVEL: the light under a laptop's keys.
        Some("keys") => {
            match std::env::args().nth(2).and_then(|text| text.parse::<u8>().ok()) {
                Some(level) => println!("{}", if keys::light(level) { "taken" } else { "not taken" }),
                None => println!("{}", if keys::there() { "a keyboard whose light can be set" } else { "none" }),
            }
            return Ok(());
        }
        // glance-remote dim MONITOR PERCENT: one monitor as bright as that.
        Some("dim") => {
            let number = |at: usize| std::env::args().nth(at).and_then(|text| text.parse::<usize>().ok());
            if let (Some(monitor), Some(percent)) = (number(2), number(3)) {
                println!("{}", if display::dim(monitor, percent.min(100) as u8) { "taken" } else { "not taken" });
            }
            return Ok(());
        }
        // glance-remote screen off|on: every screen.
        Some("screen") => {
            if std::env::args().nth(2).as_deref() == Some("off") {
                println!("{:?}", display::sleep());
            } else {
                display::wake();
            }
            return Ok(());
        }
        // glance-remote vcp MONITOR CODE VALUE: one feature of one monitor, set.
        Some("vcp") => {
            let number = |at: usize, radix: u32| std::env::args().nth(at).and_then(|text| u32::from_str_radix(&text, radix).ok());
            if let (Some(monitor), Some(code), Some(value)) = (number(2, 10), number(3, 16), number(4, 10)) {
                println!("{}", if display::tell(monitor as usize, code as u8, value) { "told" } else { "not taken" });
            }
            return Ok(());
        }
        _ => {}
    }
    display::watch();
    let listener = TcpListener::bind(("0.0.0.0", config.port))?;
    // A phone that has lost this PC (its address changed) asks the whole
    // network for it by its key's name: answered with proof of the key,
    // from the address this PC has now.
    if let Ok(asked) = UdpSocket::bind(("0.0.0.0", config.port)) {
        std::thread::spawn(move || {
            let mut packet = [0u8; 256];
            loop {
                let Ok((length, from)) = asked.recv_from(&mut packet) else { continue };
                let text = String::from_utf8_lossy(&packet[..length]);
                let mut words = text.split(' ');
                let (Some("glance-find"), Some(id), Some(nonce)) = (words.next(), words.next(), words.next()) else { continue };
                if nonce.is_empty() || nonce.len() > 64 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
                    continue;
                }
                let now = sign::now();
                let phones = Config::load().phones;
                let key = phones.iter().filter(|phone| phone.offered_till.is_none_or(|till| till > now)).filter_map(|phone| sign::key(&phone.key)).find(|key| sign::key_id(key) == id);
                if let Some(key) = key {
                    let _ = asked.send_to(format!("glance-here {nonce} {}", sign::here(&key, nonce)).as_bytes(), from);
                }
            }
        });
    }
    // (Whether the lights are on, as OpenRGB has them if it is running.)
    let state = State { lights: lights::lit(), ..State::default() };
    let helper = Arc::new(Helper { light: Mutex::new(config.light.clone()), keys: Mutex::new(config.keys), phones: Mutex::new((None, Vec::new())), config, state: Mutex::new(state), lighting: Mutex::new(()), stale: std::sync::atomic::AtomicBool::new(false) });
    let following = helper.clone();
    std::thread::spawn(move || {
        let mut last = None;
        // The makers' lighting programs seen running, by their numbers: others than last time, one has started or gone.
        let mut rivals: Vec<u32> = Vec::new();
        loop {
            std::thread::sleep(FOLLOW);
            let now: Vec<u32> = actions::programs().into_iter().filter(|(_, name)| lights::RIVALS.iter().any(|rival| name.eq_ignore_ascii_case(rival))).map(|(number, _)| number).collect();
            // (One gone counts too: it does not give the lights back as it goes.)
            if now != rivals {
                following.stale.store(true, std::sync::atomic::Ordering::SeqCst);
                // (Lights that follow the heat are put in their colour again by the OpenRGB to come.)
                last = None;
            }
            rivals = now;
            following.follow(&mut last);
        }
    });
    // Night ends at the first touch of the mouse or the keyboard.
    let watching = helper.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(WATCH);
        let touched = watching.state.lock().unwrap().night.as_ref().is_some_and(|night| actions::last_input().wrapping_sub(night.since) as i32 > SETTLING_MS as i32);
        if touched {
            let _ = watching.night(false);
        }
    });
    for stream in listener.incoming().flatten() {
        let helper = helper.clone();
        std::thread::spawn(move || {
            let _ = helper.serve(stream);
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn text_goes_through_a_link_and_back() {
        for text in ["DESKTOP-9950X", "书房 的电脑", "SM-F971U1", "100%"] {
            assert_eq!(super::unescaped(&super::escaped(text)), text);
        }
        assert_eq!(super::escaped("a b"), "a%20b");
        // What is no number stays as it is.
        assert_eq!(super::unescaped("%zz%4"), "%zz%4");
    }

    #[test]
    fn the_heat_goes_from_blue_to_red() {
        assert_eq!(super::heat(20), "0080FF");
        assert_eq!(super::heat(40), "0080FF");
        assert_eq!(super::heat(85), "FF0000");
        assert_eq!(super::heat(120), "FF0000");
        // About half way: green.
        assert_eq!(super::heat(62), "36FF00");
    }
}
