//! How a request proves it comes from the paired phone: signed with the key
//! shared at pairing, over what it asks, when, and a number used once. The
//! key itself never travels; a request overheard cannot be sent again.

use std::collections::VecDeque;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use sha2::Sha256;

/// How far a request's time may be from this PC's (the phone's clock and
/// the PC's are not one), and so how long its number is remembered.
const SKEW: Duration = Duration::from_secs(90);

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 || !text.is_ascii() {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok()).collect()
}

/// The key kept as `text`, if it is one (32 bytes in hex).
pub fn key(text: &str) -> Option<[u8; 32]> {
    unhex(text)?.try_into().ok()
}

/// What a request signs: its method, its path, its time, its number and
/// its body.
fn signing(key: &[u8; 32], method: &str, path: &str, time: &str, nonce: &str, body: &[u8]) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("a key of any length");
    for part in [method, path, time, nonce] {
        mac.update(part.as_bytes());
        mac.update(b"\n");
    }
    mac.update(body);
    mac
}

/// The numbers of the requests taken lately, each with its time: one seen
/// again is a request sent again.
#[derive(Default)]
pub struct Seen(VecDeque<(u64, String)>);

/// Why a request is not taken.
#[derive(Debug, PartialEq)]
pub enum Refused {
    Unsigned,
    Stale,
    Replayed,
    Forged,
    /// Its key is another phone's: this one is to be paired for itself.
    Taken,
}

impl Seen {
    /// Whether the request is the paired phone's, now, and not one sent
    /// before: `time` in seconds since 1970, `nonce` any text of its own,
    /// `sign` its signature in hex. `now`, this PC's time.
    #[allow(clippy::too_many_arguments)]
    pub fn check(&mut self, key: &[u8; 32], method: &str, path: &str, time: &str, nonce: &str, sign: &str, body: &[u8], now: u64) -> Result<(), Refused> {
        let (Ok(at), Some(given)) = (time.parse::<u64>(), unhex(sign)) else { return Err(Refused::Unsigned) };
        if nonce.is_empty() || nonce.len() > 64 {
            return Err(Refused::Unsigned);
        }
        // Compared whole, in the same time whatever differs.
        if signing(key, method, path, time, nonce, body).verify_slice(&given).is_err() {
            return Err(Refused::Forged);
        }
        if at.abs_diff(now) > SKEW.as_secs() {
            return Err(Refused::Stale);
        }
        self.0.retain(|(seen, _)| seen.abs_diff(now) <= 2 * SKEW.as_secs());
        if self.0.iter().any(|(_, seen)| seen == nonce) {
            return Err(Refused::Replayed);
        }
        self.0.push_back((at, nonce.to_string()));
        Ok(())
    }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_paired_phones_requests_are_taken_and_once() {
        let key = [7u8; 32];
        let sign = |path: &str, time: &str, nonce: &str, body: &[u8]| hex(&signing(&key, "POST", path, time, nonce, body).finalize().into_bytes());
        let mut seen = Seen::default();
        let good = sign("/v1/lights", "1000", "a1", b"{\"on\":false}");
        assert_eq!(seen.check(&key, "POST", "/v1/lights", "1000", "a1", &good, b"{\"on\":false}", 1010), Ok(()));
        // Sent again, as it was.
        assert_eq!(seen.check(&key, "POST", "/v1/lights", "1000", "a1", &good, b"{\"on\":false}", 1011), Err(Refused::Replayed));
        // Another body, path or key under the same signature.
        assert_eq!(seen.check(&key, "POST", "/v1/lights", "1000", "a2", &good, b"{\"on\":true}", 1010), Err(Refused::Forged));
        assert_eq!(seen.check(&key, "POST", "/v1/sleep", "1000", "a1", &good, b"{\"on\":false}", 1010), Err(Refused::Forged));
        assert_eq!(seen.check(&[8u8; 32], "POST", "/v1/lights", "1000", "a3", &sign("/v1/lights", "1000", "a3", b""), b"", 1010), Err(Refused::Forged));
        // Kept and sent much later.
        let late = sign("/v1/sleep", "1000", "b1", b"");
        assert_eq!(seen.check(&key, "POST", "/v1/sleep", "1000", "b1", &late, b"", 5000), Err(Refused::Stale));
        // Not signed at all.
        assert_eq!(seen.check(&key, "POST", "/v1/sleep", "", "", "", b"", 1010), Err(Refused::Unsigned));
        assert!(super::key(&hex(&key)).is_some() && super::key("12").is_none());
    }
}
