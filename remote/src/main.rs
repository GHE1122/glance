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
mod display;
mod lights;
mod media;
mod reads;
mod sign;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use config::{Config, Effect, Light};
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
    plan: Option<windows::core::GUID>,
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
    key: [u8; 32],
    state: Mutex<State>,
    /// Held while the lights are being set: one at a time.
    lighting: Mutex<()>,
}

impl Helper {
    fn lights(&self, on: bool) -> Result<(), String> {
        // (The state is not held while OpenRGB is asked: that may take a while.)
        let _one = self.lighting.lock().unwrap();
        let mut light = self.light.lock().unwrap().clone();
        if light.effect == Effect::Heat {
            if let Some(celsius) = reads::read().as_ref().and_then(reads::hottest) {
                light.color = heat(celsius);
            }
        }
        lights::set(&self.config, on, &light).map_err(|error| error.to_string())?;
        self.state.lock().unwrap().lights = Some(on);
        Ok(())
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
        let _ = Config { light: light.clone(), ..self.config.clone() }.save();
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
            let plan = actions::power_plan().filter(|plan| *plan != actions::POWER_SAVER);
            actions::set_power_plan(actions::POWER_SAVER);
            let dark = display::sleep().map_err(|error| error.to_string());
            self.state.lock().unwrap().night = Some(Night { plan, since: actions::tick() });
            lit.and(dark)
        } else {
            let Some(night) = self.state.lock().unwrap().night.take() else { return Ok(()) };
            if let Some(plan) = night.plan {
                actions::set_power_plan(plan);
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
        let plan = actions::power_plan();
        let state = self.state.lock().unwrap();
        let sound = actions::volume();
        json!({
            "name": self.config.name,
            "version": env!("CARGO_PKG_VERSION"),
            "lights": lights,
            "light": light,
            "reads": reads::read(),
            "media": playing.map(|media| json!({ "title": media.title, "artist": media.artist, "app": media.app, "playing": media.playing })),
            "night": state.night.is_some(),
            "power": actions::power_plans().iter().map(|(id, name)| json!({ "id": format!("{id:?}"), "name": name, "on": Some(*id) == plan })).collect::<Vec<_>>(),
            "screen": display::on(),
            "monitors": display::monitors().iter().map(|monitor| json!({ "name": monitor.name, "x": monitor.place.0, "y": monitor.place.1, "width": monitor.place.2, "height": monitor.place.3, "on": monitor.on, "brightness": monitor.brightness })).collect::<Vec<_>>(),
            "volume": sound.map(|(level, _)| level),
            "muted": sound.map(|(_, muted)| muted),
            "idle_seconds": actions::tick().wrapping_sub(actions::last_input()) / 1000,
        })
    }

    /// Does what a request asks: its answer, or why not (with the status).
    fn act(self: &Arc<Self>, method: &str, path: &str, body: &[u8]) -> Result<Value, (u16, String)> {
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
            // One of the PC's power plans, by its id as the state gives it.
            // (Night over, the plan gone back to is the one there was before
            // night, not one chosen during it.)
            ("POST", "/v1/power") => {
                let wanted = asked.get("plan").and_then(Value::as_str).unwrap_or_default();
                let plan = actions::power_plans().into_iter().find(|(id, _)| format!("{id:?}") == wanted).ok_or((400, "no such plan".to_string()))?.0;
                if !actions::set_power_plan(plan) {
                    return Err((500, "the plan was not taken".into()));
                }
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
        let checked = self.state.lock().unwrap().seen.check(&self.key, method, path, header("x-glance-time"), header("x-glance-nonce"), header("x-glance-sign"), &body, sign::now());
        let (status, answer) = match checked {
            Err(refused) => (401, json!({ "error": format!("{refused:?}").to_lowercase(), "time": sign::now() })),
            Ok(()) => match self.act(method, path, &body) {
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

/// What the phone needs to pair, as the link its QR code holds.
fn pairing(config: &Config) -> String {
    let name: String = config.name.bytes().map(|b| if b.is_ascii_alphanumeric() || b == b'-' { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    format!("glance://pair?h={}&p={}&k={}&n={name}", address().unwrap_or_default(), config.port, config.key)
}

fn main() -> std::io::Result<()> {
    // The monitors' sizes in their own pixels, whatever each one's scale.
    let _ = unsafe { windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let config = Config::load()?;
    match std::env::args().nth(1).as_deref() {
        Some("pair") => {
            let link = pairing(&config);
            let code = qrcode::QrCode::new(link.as_bytes()).map_err(std::io::Error::other)?;
            let picture = code.render::<qrcode::render::svg::Color>().min_dimensions(480, 480).quiet_zone(true).build();
            let path = std::env::temp_dir().join("glance-pair.svg");
            std::fs::write(&path, picture)?;
            println!("{link}\n{}", path.display());
            return Ok(());
        }
        Some("lights") => {
            for device in lights::devices(&config)? {
                println!("{}\n  modes: {}\n  zones: {}", device.name, device.modes.iter().map(|mode| mode.name.as_str()).collect::<Vec<_>>().join(", "), device.zones.iter().map(|zone| format!("{} ({} LEDs)", zone.name, zone.leds)).collect::<Vec<_>>().join(", "));
            }
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
            for (id, name) in actions::power_plans() {
                println!("{id:?} {name}{}", if Some(id) == actions::power_plan() { " *" } else { "" });
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
    let key = sign::key(&config.key).expect("a key made at first run");
    let listener = TcpListener::bind(("0.0.0.0", config.port))?;
    // (Whether the lights are on, as OpenRGB has them if it is running.)
    let state = State { lights: lights::lit(), ..State::default() };
    let helper = Arc::new(Helper { light: Mutex::new(config.light.clone()), config, key, state: Mutex::new(state), lighting: Mutex::new(()) });
    let following = helper.clone();
    std::thread::spawn(move || {
        let mut last = None;
        loop {
            std::thread::sleep(FOLLOW);
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
    fn the_heat_goes_from_blue_to_red() {
        assert_eq!(super::heat(20), "0080FF");
        assert_eq!(super::heat(40), "0080FF");
        assert_eq!(super::heat(85), "FF0000");
        assert_eq!(super::heat(120), "FF0000");
        // About half way: green.
        assert_eq!(super::heat(62), "36FF00");
    }
}
