//! What the helper keeps: the port it listens on, the key shared with the
//! paired phone, and how the lights are when they are on. In the user's own
//! folder; the helper runs as the user.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The port the helper listens on unless another is kept.
const PORT: u16 = 47631;
/// How many LEDs a header for addressable strips is taken to have while
/// OpenRGB has none for it: enough for a few fans in a chain.
const STRIP_LEDS: u32 = 120;

/// What the lights do while they are on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    /// One colour, steady.
    #[default]
    Steady,
    /// That colour, swelling and fading.
    Breathing,
    /// Every colour in turn (the colour chosen is not used).
    Rainbow,
    /// A colour for how warm the PC is, from blue through to red (the
    /// colour chosen, while Glance is not there to say how warm).
    Heat,
}

/// How the lights are when they are on: their colour (RRGGBB), what they
/// do, and how bright they are (percent of the colour's own brightness).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Light {
    pub color: String,
    pub effect: Effect,
    pub brightness: u8,
}

impl Default for Light {
    fn default() -> Self {
        Light { color: "FFFFFF".into(), effect: Effect::Steady, brightness: 100 }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub port: u16,
    /// This PC, as the phone names it.
    pub name: String,
    /// The one key every phone shared, before each had its own: read once,
    /// to go on as a phone of its own, and not written again.
    #[serde(skip_serializing)]
    key: String,
    /// The phones paired with, and the pairings offered and not yet taken.
    pub phones: Vec<Phone>,
    /// Where OpenRGB is, if it is not where its installer puts it.
    pub openrgb: Option<PathBuf>,
    pub strip_leds: u32,
    pub light: Light,
    /// The OpenRGB this helper started, by its program's number: its own
    /// to stop and start afresh (one the user started is not).
    pub openrgb_started: Option<u32>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            port: PORT,
            name: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into()),
            key: String::new(),
            phones: Vec::new(),
            openrgb: None,
            strip_leds: STRIP_LEDS,
            light: Light::default(),
            openrgb_started: None,
        }
    }
}

fn path() -> PathBuf {
    let roaming = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
    roaming.join("dev.weiyi.glance.remote").join("config.json")
}

/// How long a pairing offered may wait to be taken (seconds).
const OFFERED_FOR: u64 = 600;

/// A phone paired with, by the key that is its alone.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Phone {
    /// The key shared with it (hex, 32 bytes).
    pub key: String,
    /// What it calls itself.
    pub name: String,
    /// The number it gave itself, once it has used the key: the key is
    /// that phone's from then on (a second phone that scanned the same
    /// code is told to ask for its own).
    pub device: Option<String>,
    /// Offered and not yet taken: when the offer ends (seconds since 1970).
    pub offered_till: Option<u64>,
}

impl Config {
    /// The configuration kept (or a new one), with the pairings offered
    /// and never taken gone from it, and the one key of before as a
    /// phone of its own.
    pub fn load() -> Config {
        let mut config: Config = std::fs::read_to_string(path()).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        if crate::sign::key(&config.key).is_some() && !config.phones.iter().any(|phone| phone.key == config.key) {
            config.phones.push(Phone { key: std::mem::take(&mut config.key), ..Phone::default() });
        }
        let now = crate::sign::now();
        config.phones.retain(|phone| crate::sign::key(&phone.key).is_some() && phone.offered_till.is_none_or(|till| till > now));
        config
    }

    /// Changes the configuration as it is kept now (another of this
    /// program may have changed it since it was read) and keeps it.
    pub fn change(with: impl FnOnce(&mut Config)) -> std::io::Result<Config> {
        let mut config = Config::load();
        with(&mut config);
        config.save()?;
        Ok(config)
    }

    /// Offers a pairing: a key of its own for the phone that takes it,
    /// which is returned.
    pub fn offer() -> std::io::Result<String> {
        let mut key = [0u8; 32];
        getrandom::getrandom(&mut key).map_err(|error| std::io::Error::other(error.to_string()))?;
        let key = crate::sign::hex(&key);
        Config::change(|config| config.phones.push(Phone { key: key.clone(), offered_till: Some(crate::sign::now() + OFFERED_FOR), ..Phone::default() }))?;
        Ok(key)
    }

    /// When the configuration was last kept.
    pub fn kept_at() -> Option<std::time::SystemTime> {
        std::fs::metadata(path()).and_then(|file| file.modified()).ok()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, serde_json::to_string_pretty(self).unwrap())
    }

}
