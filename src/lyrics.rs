//! The lyrics of what plays, each line with the time it is sung at (and its
//! translation, where there is one), for the module that shows them as the
//! track plays. Asked for on the network, only while lyrics are to be
//! shown: of NetEase Cloud Music by the track's own number where its player
//! says it (as it does with the InfLink-rs plugin), else of LRCLIB (an open
//! collection of lyrics) by the track's title, artist, album and length, and
//! last of NetEase Cloud Music by its title and length. Each track's are
//! asked for once while Glance runs, on a thread of their own.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// One line: when it is sung (seconds into the track), what is sung, and its
/// translation where one is given.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub at: f64,
    pub text: String,
    pub translation: Option<String>,
}

/// A track's lyrics, in the order they are sung.
pub type Lyrics = Arc<Vec<Line>>;

/// The track whose lyrics are asked for: its number on NetEase Cloud Music
/// where its player says it, what it is called, by whom, on which album, and
/// how long it is (seconds).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Track {
    pub netease: Option<u64>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub seconds: u32,
}

/// What is known of a track's lyrics: still being asked for, or found (none, where no source has them).
#[derive(Clone, Debug, PartialEq)]
pub enum Known {
    Asking,
    Found(Option<Lyrics>),
}

/// The tracks asked about, and what was found; the most kept.
static KNOWN: Mutex<Option<HashMap<Track, Known>>> = Mutex::new(None);
const KEPT_MOST: usize = 200;

/// What is known of `track`'s lyrics; asked for (on a thread of its own) the first time.
pub fn of(track: &Track) -> Known {
    let mut known = KNOWN.lock().unwrap();
    let known = known.get_or_insert_with(HashMap::new);
    if let Some(found) = known.get(track) {
        return found.clone();
    }
    if known.len() >= KEPT_MOST {
        known.retain(|_, found| *found == Known::Asking);
    }
    known.insert(track.clone(), Known::Asking);
    let track = track.clone();
    std::thread::spawn(move || {
        let found = find(&track).map(Arc::new);
        KNOWN.lock().unwrap().get_or_insert_with(HashMap::new).insert(track, Known::Found(found));
    });
    Known::Asking
}

/// Asks each source in turn.
fn find(track: &Track) -> Option<Vec<Line>> {
    track.netease.and_then(netease).or_else(|| lrclib(track)).or_else(|| netease_search(track).and_then(netease))
}

/// NetEase Cloud Music's lyrics of its track `id`, with their translation.
fn netease(id: u64) -> Option<Vec<Line>> {
    let said: serde_json::Value = serde_json::from_slice(&crate::update::get_json(&format!("https://music.163.com/api/song/lyric?id={id}&lv=1&tv=-1"))?).ok()?;
    let lines = merged(said["lrc"]["lyric"].as_str()?, said["tlyric"]["lyric"].as_str());
    lines.iter().any(|line| !line.text.is_empty()).then_some(lines)
}

/// The track on NetEase Cloud Music that is `track`: of those found by its title and artist, the first of its
/// title and of its length (to two seconds).
fn netease_search(track: &Track) -> Option<u64> {
    let asked = format!("{} {}", track.title, track.artist);
    let said: serde_json::Value = serde_json::from_slice(&crate::update::get_json(&format!("https://music.163.com/api/search/get?s={}&type=1&limit=10", escaped(&asked)))?).ok()?;
    said["result"]["songs"].as_array()?.iter().find_map(|song| {
        let length = song["duration"].as_f64()? / 1000.0;
        (same(song["name"].as_str()?, &track.title) && (length - track.seconds as f64).abs() <= 2.0).then(|| song["id"].as_u64()).flatten()
    })
}

/// LRCLIB's timed lyrics of `track`: the one it has of that title, artist, album and length; else, of those
/// it finds by title and artist, the first of that length (to two seconds) with timed lyrics.
fn lrclib(track: &Track) -> Option<Vec<Line>> {
    let timed = |said: &serde_json::Value| said["syncedLyrics"].as_str().map(|lrc| merged(lrc, None)).filter(|lines| lines.iter().any(|line| !line.text.is_empty()));
    let exact = format!(
        "https://lrclib.net/api/get?track_name={}&artist_name={}&album_name={}&duration={}",
        escaped(&track.title),
        escaped(&track.artist),
        escaped(&track.album),
        track.seconds
    );
    if let Some(found) = crate::update::get_json(&exact).and_then(|body| serde_json::from_slice::<serde_json::Value>(&body).ok()).and_then(|said| timed(&said)) {
        return Some(found);
    }
    let search = format!("https://lrclib.net/api/search?track_name={}&artist_name={}", escaped(&track.title), escaped(&track.artist));
    let said: serde_json::Value = serde_json::from_slice(&crate::update::get_json(&search)?).ok()?;
    said.as_array()?.iter().filter(|one| one["duration"].as_f64().is_some_and(|length| (length - track.seconds as f64).abs() <= 2.0)).find_map(timed)
}

/// Whether two titles are the same, but for case and spaces.
fn same(a: &str, b: &str) -> bool {
    let bare = |s: &str| s.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect::<String>();
    bare(a) == bare(b)
}

/// `text` as a URL's query has it (RFC 3986: all but the unreserved, by their UTF-8 bytes).
fn escaped(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (byte as char).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// The lines of an LRC text (`[mm:ss.xx]` before each, one or more), in the order they are sung, each with the
/// line of `translation` (another LRC text) at its time. (NetEase's put some lines' breaks in as `\n`, written
/// out: they are breaks.)
pub fn merged(lrc: &str, translation: Option<&str>) -> Vec<Line> {
    let mut lines: Vec<Line> = timed(lrc).into_iter().map(|(at, text)| Line { at, text, translation: None }).collect();
    for (at, text) in translation.map(timed).unwrap_or_default() {
        // (A translation of nothing is written "//".)
        if text.is_empty() || text == "//" {
            continue;
        }
        if let Some(line) = lines.iter_mut().find(|line| (line.at - at).abs() < 0.05) {
            line.translation = Some(text);
        }
    }
    lines
}

/// Each timed line of an LRC text: when, and what (trimmed; empty where nothing is sung from then).
fn timed(lrc: &str) -> Vec<(f64, String)> {
    let mut found = Vec::new();
    for line in lrc.replace("\\n", "\n").lines() {
        let mut rest = line.trim();
        let mut times = Vec::new();
        // Its times, one after another; a tag that is no time ("[ar:…]") says nothing sung.
        while let Some(tag) = rest.strip_prefix('[').and_then(|after| after.split_once(']')) {
            let Some(at) = time(tag.0) else { break };
            times.push(at);
            rest = tag.1;
        }
        found.extend(times.into_iter().map(|at| (at, rest.trim().to_string())));
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found
}

/// `mm:ss`, `mm:ss.xx` or `mm:ss:xx` as seconds.
fn time(tag: &str) -> Option<f64> {
    let (minutes, seconds) = tag.split_once(':')?;
    let seconds = seconds.replacen(':', ".", 1);
    let (minutes, seconds) = (minutes.trim().parse::<u32>().ok()?, seconds.trim().parse::<f64>().ok()?);
    (0.0..60.0).contains(&seconds).then_some(minutes as f64 * 60.0 + seconds)
}

/// The line sung at `position` (seconds), sung a little early (a line shows as it is about to be sung, not
/// after): its place among `lines`; none before the first.
pub fn sung_at(lines: &[Line], position: f64) -> Option<usize> {
    lines.iter().rposition(|line| line.at <= position + EARLY)
}

/// How early a line is shown (seconds).
pub const EARLY: f64 = 0.2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_lrc_with_its_translation() {
        let lrc = "[ar:Someone]\n[00:00.00] 作词 : 汪峰\\n[00:36.09]每当我感到疼痛 就想让你抱紧我\n[01:02:50][02:10.5]Again\n[03:00.00]\n";
        let lines = merged(lrc, Some("[00:36.09]Whenever I hurt\n[03:00.00]//"));
        let said: Vec<(f64, &str, Option<&str>)> = lines.iter().map(|l| (l.at, l.text.as_str(), l.translation.as_deref())).collect();
        assert_eq!(said, vec![(0.0, "作词 : 汪峰", None), (36.09, "每当我感到疼痛 就想让你抱紧我", Some("Whenever I hurt")), (62.5, "Again", None), (130.5, "Again", None), (180.0, "", None)]);
        assert_eq!(sung_at(&lines, 35.0), Some(0));
        assert_eq!(sung_at(&lines, 35.95), Some(1));
        assert_eq!(sung_at(&lines, 500.0), Some(4));
        assert_eq!(sung_at(&merged("[00:10.00]a", None), 1.0), None);
    }

    #[test]
    fn escapes_a_query() {
        assert_eq!(escaped("我爱你 A&B"), "%E6%88%91%E7%88%B1%E4%BD%A0%20A%26B");
        assert!(same("Yellow ", "yel low"));
    }

    /// Asks the sources for two tracks (network): `cargo test -- --ignored finds_lyrics --nocapture`.
    #[test]
    #[ignore]
    fn finds_lyrics() {
        let by_id = find(&Track { netease: Some(2124385847), title: String::new(), artist: String::new(), album: String::new(), seconds: 330 }).expect("NetEase's, by number");
        println!("NetEase by number: {} lines, the second {:?}", by_id.len(), by_id.get(1));
        let by_name = find(&Track { netease: None, title: "Yellow".into(), artist: "Coldplay".into(), album: "Parachutes".into(), seconds: 267 }).expect("LRCLIB's");
        println!("by name: {} lines, the first sung {:?}", by_name.len(), by_name.iter().find(|line| !line.text.is_empty()));
        let searched = netease_search(&Track { netease: None, title: "我爱你中国 (live)".into(), artist: "孙楠".into(), album: String::new(), seconds: 330 });
        println!("NetEase by its title: {searched:?}");
        assert!(searched.is_some());
    }
}
