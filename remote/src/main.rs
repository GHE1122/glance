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
mod sign;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use config::Config;
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
    /// How each device was lit before this helper last put the lights out.
    before: lights::Before,
    night: Option<Night>,
}

struct Helper {
    config: Config,
    key: [u8; 32],
    state: Mutex<State>,
}

impl Helper {
    fn lights(&self, on: bool) -> Result<(), String> {
        // (Not held while OpenRGB is asked: that may take a while.)
        let mut before = self.state.lock().unwrap().before.clone();
        lights::set(&self.config, on, &mut before).map_err(|error| error.to_string())?;
        let mut state = self.state.lock().unwrap();
        state.before = before;
        state.lights = Some(on);
        Ok(())
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
            let woken = display::wake().map_err(|error| error.to_string());
            self.lights(true).and(woken)
        }
    }

    fn state(&self) -> Value {
        // As OpenRGB has them; while it is not running, as this helper last
        // set them.
        let lights = lights::lit().or(self.state.lock().unwrap().lights);
        let state = self.state.lock().unwrap();
        let sound = actions::volume();
        json!({
            "name": self.config.name,
            "version": env!("CARGO_PKG_VERSION"),
            "lights": lights,
            "night": state.night.is_some(),
            "screen": display::on(),
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
            ("POST", "/v1/screen") => if on()? { display::wake() } else { display::sleep() }.map_err(|error| failed(error.to_string()))?,
            ("POST", "/v1/lights") => self.lights(on()?).map_err(failed)?,
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
        Some("monitors") => {
            for monitor in display::monitors() {
                println!("{}: brightness {:?}, input {:?}, power {:?}", monitor.name, monitor.brightness, monitor.input, monitor.power);
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
    let helper = Arc::new(Helper { config, key, state: Mutex::new(State::default()) });
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
