//! The lyrics of what plays, each line with the time it is sung at (word by
//! word, where that is known), its translation where there is one, for what
//! shows them as the track plays. Asked for on the network, only while
//! lyrics are to be shown, by the track's number on NetEase Cloud Music
//! (where its player says it, as it does with the InfLink-rs plugin; else as
//! NetEase finds it by its title and length): of the AMLL TTML collection
//! (lyrics timed word by word, public domain, kept by that number, reached
//! through a CDN) and of NetEase itself; and of LRCLIB (an open collection of
//! lyrics timed by line), by the track's title, artist, album and length.
//! The sources are asked all at once (see `find`): where one far away cannot
//! be reached, as may be in mainland China, NetEase's still come at once.
//! Each track's are asked for on a thread of their own, and kept on disk
//! (see `kept`): asked for again only once they may have changed. Before
//! all of these, a file of the track on this PC (see `library`): its own.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

/// One line: when it is sung (seconds into the track) and till when, where
/// that is said; what is sung, and word by word where that is known; what is
/// sung behind it (backing vocals); and its translation where one is given.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub at: f64,
    pub end: Option<f64>,
    pub text: String,
    pub words: Vec<Word>,
    pub backing: Option<String>,
    pub translation: Option<String>,
}

/// A word (or a syllable, or a character), as it is sung: from when, till when, and what it is (with the space
/// after it, where there is one).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Word {
    pub at: f64,
    pub end: f64,
    pub text: String,
}

/// A track's lyrics, in the order they are sung.
pub type Lyrics = Arc<Vec<Line>>;

/// The track whose lyrics are asked for: its number on NetEase Cloud Music
/// where its player says it, what it is called, by whom, on which album, and
/// how long it is (seconds).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
static KNOWN: Mutex<Option<HashMap<Track, Held>>> = Mutex::new(None);
const KEPT_MOST: usize = 200;

/// What is held of a track's lyrics while Glance runs: as `Known`, or, where a source could not be reached and
/// none of the others had them, none for now (since then), to be asked for again a while later.
#[derive(Clone)]
enum Held {
    Asking,
    Found(Option<Lyrics>),
    Unreached(Instant),
}

/// How long lyrics a source could not be reached for are taken to be none before they are asked for again.
const UNREACHED_FOR: Duration = Duration::from_secs(60);

/// Lyrics found, as they are held: none, where they have no line (a file kept on this PC may say so; shown as
/// lyrics, they were turned through from nothing, and Glance stopped).
fn found(lines: Option<Vec<Line>>) -> Held {
    Held::Found(lines.filter(|lines| !lines.is_empty()).map(Arc::new))
}

/// What is known of `track`'s lyrics; asked for (on a thread of its own) the first time, and again a while after
/// a source could not be reached.
pub fn of(track: &Track) -> Known {
    let mut known = KNOWN.lock().unwrap();
    let known = known.get_or_insert_with(HashMap::new);
    match known.get(track) {
        Some(Held::Asking) => return Known::Asking,
        Some(Held::Found(found)) => return Known::Found(found.clone()),
        Some(Held::Unreached(since)) if since.elapsed() < UNREACHED_FOR => return Known::Found(None),
        _ => {}
    }
    if known.len() >= KEPT_MOST {
        known.retain(|_, held| matches!(held, Held::Asking));
    }
    known.insert(track.clone(), Held::Asking);
    let track = track.clone();
    std::thread::spawn(move || {
        // A file of it on this PC with lyrics: those (as they are now; not kept, as the file can change).
        if let Some(lines) = crate::library::lyrics(&track) {
            KNOWN.lock().unwrap().get_or_insert_with(HashMap::new).insert(track, found(Some(lines)));
            return;
        }
        let held = match kept::read(&track) {
            Some(kept) if kept.fresh() => found(kept.lines),
            kept => {
                let said = find(&track);
                // (That none has them, only where each source said so.)
                if said.sure || said.found.is_some() {
                    kept::write(&track, said.found.as_deref(), said.sure);
                }
                match (said.found, kept) {
                    (Some(lines), _) => found(Some(lines)),
                    // Not found now, a source unreached: what was found before, where there was.
                    (None, Some(kept)) if !said.sure => found(kept.lines),
                    (None, _) if !said.sure => Held::Unreached(Instant::now()),
                    (None, _) => Held::Found(None),
                }
            }
        };
        KNOWN.lock().unwrap().get_or_insert_with(HashMap::new).insert(track, held);
    });
    Known::Asking
}

/// A source that could not be reached, or would not answer: what it has is not known.
pub(crate) struct Unreached;

/// What a source has of what it was asked for (none, where it said it has none), where it answered.
pub(crate) type Answer<T> = Result<Option<T>, Unreached>;

/// What the sources asked had of a track's lyrics, and whether each answered (else they may yet have them).
struct Said {
    found: Option<Vec<Line>>,
    sure: bool,
}

/// Asks the sources, all at once (one far away, slow or not reached, keeps none waiting for the others): by the
/// track's NetEase number, the word-timed collection and NetEase itself (the collection's, with NetEase's
/// translation where it gives none; else NetEase's); by its name, LRCLIB, and the same by the number NetEase finds
/// it by: of these, lyrics timed word by word first, then LRCLIB's (found by title, artist, album and length, more
/// surely the track's own than a search's), then NetEase's.
fn find(track: &Track) -> Said {
    use std::sync::atomic::{AtomicBool, Ordering};
    let sure = AtomicBool::new(true);
    // What a source has, none where it was not reached (and then what the sources have is not sure).
    let answered = |answer: Answer<Vec<Line>>| {
        answer.unwrap_or_else(|Unreached| {
            sure.store(false, Ordering::Relaxed);
            None
        })
    };
    let by_number = |id: u64| {
        let (worded, theirs) = std::thread::scope(|scope| {
            let theirs = scope.spawn(|| answered(netease(id)));
            (answered(amll(id)), theirs.join().expect("NetEase asked"))
        });
        match (worded, theirs) {
            (Some(mut lines), Some(theirs)) => {
                if lines.iter().all(|line| line.translation.is_none()) {
                    translated(&mut lines, &theirs);
                }
                Some(lines)
            }
            (worded, theirs) => worded.or(theirs),
        }
    };
    let found = match track.netease {
        Some(id) => by_number(id),
        None => std::thread::scope(|scope| {
            let searched = scope.spawn(|| match netease_search(track) {
                Ok(id) => id.and_then(by_number),
                Err(Unreached) => answered(Err(Unreached)),
            });
            let named = answered(lrclib(track));
            let searched = searched.join().expect("NetEase asked");
            let worded = |lines: &Option<Vec<Line>>| lines.as_ref().is_some_and(|lines| lines.iter().any(|line| !line.words.is_empty()));
            if worded(&searched) { searched } else { named.or(searched) }
        }),
    };
    Said { found, sure: sure.into_inner() }
}

/// `lines` with the translation of the line of `theirs` nearest in time to each, within a second.
fn translated(lines: &mut [Line], theirs: &[Line]) {
    for line in lines {
        let nearest = theirs.iter().filter(|their| their.translation.is_some()).min_by(|a, b| (a.at - line.at).abs().total_cmp(&(b.at - line.at).abs()));
        if let Some(their) = nearest.filter(|their| (their.at - line.at).abs() <= 1.0) {
            line.translation = their.translation.clone();
        }
    }
}

/// The body of `url` (fetched as JSON is): none where it is not there (404).
fn fetched(url: &str) -> Answer<Vec<u8>> {
    match crate::update::answer_json(url) {
        Some((200, body)) => Ok(Some(body)),
        Some((404, _)) => Ok(None),
        _ => Err(Unreached),
    }
}

/// The JSON `url` answers with: none where it is not there (404).
fn fetched_json(url: &str) -> Answer<serde_json::Value> {
    match fetched(url)? {
        Some(body) => serde_json::from_slice(&body).map(Some).map_err(|_| Unreached),
        None => Ok(None),
    }
}

/// The AMLL TTML collection's lyrics of NetEase's track `id`, timed word by word (through a CDN that is reached
/// from mainland China too).
pub(crate) fn amll(id: u64) -> Answer<Vec<Line>> {
    let Some(file) = fetched(&format!("https://cdn.jsdelivr.net/gh/Steve-xmh/amll-ttml-db@main/ncm-lyrics/{id}.ttml"))? else { return Ok(None) };
    let lines = ttml(&String::from_utf8(file).map_err(|_| Unreached)?);
    Ok(lines.iter().any(|line| !line.words.is_empty()).then_some(lines))
}

/// The lines of a TTML lyrics file as Apple Music and the AMLL collection write them: a `<p>` each (`begin`,
/// `end`), a `<span>` for each word in it (`begin`, `end`); spans with a role of their own hold its backing vocals
/// (`x-bg`, themselves word by word), its translation (`x-translation`) or its romanisation (`x-roman`, left out).
pub fn ttml(xml: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<p ") {
        let after = &rest[start..];
        let Some(close) = after.find("</p>") else { break };
        // (Its head closed before its end, or it is not a line: left out, the lines after it read.)
        let Some(head_end) = after[..close].find('>') else {
            rest = &after[close + 4..];
            continue;
        };
        let head = &after[..head_end];
        let body = &after[head_end + 1..close];
        rest = &after[close + 4..];
        let (Some(at), end) = (attribute(head, "begin").and_then(clock), attribute(head, "end").and_then(clock)) else { continue };
        let (mut words, mut backing, mut translation) = (Vec::new(), String::new(), String::new());
        // What each open span is: a word (its times), or of a role.
        let mut open: Vec<(Option<String>, Option<(f64, f64)>)> = Vec::new();
        let mut body = body;
        while !body.is_empty() {
            let Some(tag) = body.find('<') else {
                text_into(&mut words, &mut backing, &mut translation, &open, body);
                break;
            };
            text_into(&mut words, &mut backing, &mut translation, &open, &body[..tag]);
            let Some(tag_end) = body[tag..].find('>') else { break };
            let inside = &body[tag + 1..tag + tag_end];
            body = &body[tag + tag_end + 1..];
            if inside.starts_with('/') {
                open.pop();
            } else if inside.starts_with("span") {
                let role = attribute(inside, "ttm:role").map(str::to_string);
                let times = attribute(inside, "begin").and_then(clock).zip(attribute(inside, "end").and_then(clock));
                if !inside.ends_with('/') {
                    open.push((role, times));
                }
            }
        }
        let text: String = words.iter().map(|word: &Word| word.text.as_str()).collect::<String>().trim().to_string();
        let said = |text: String| Some(text.trim().to_string()).filter(|text| !text.is_empty());
        lines.push(Line { at, end, text, words, backing: said(backing), translation: said(translation) });
    }
    lines.sort_by(|a, b| a.at.total_cmp(&b.at));
    lines
}

/// Text met inside a line: a word's (or the space after one), the backing vocals', or the translation's, as the
/// spans it is in say.
fn text_into(words: &mut Vec<Word>, backing: &mut String, translation: &mut String, open: &[(Option<String>, Option<(f64, f64)>)], text: &str) {
    if text.is_empty() {
        return;
    }
    let text = unescaped(text);
    let role = open.iter().find_map(|(role, _)| role.as_deref());
    match role {
        Some("x-translation") => translation.push_str(&text),
        Some("x-bg") => backing.push_str(&text),
        Some(_) => {}
        None => match open.last().and_then(|(_, times)| *times) {
            Some((at, end)) => words.push(Word { at, end, text }),
            // (Between words: the space after the last.)
            None => {
                if let Some(last) = words.last_mut() {
                    last.text.push_str(&text);
                }
            }
        },
    }
}

/// The value of `name` in a tag's attributes.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    let value = &tag[at..];
    Some(&value[..value.find('"')?])
}

/// XML's five named characters and numbered ones, as themselves.
fn unescaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at..];
        let Some(end) = after.find(';') else { break };
        let name = &after[1..end];
        let named = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => name.strip_prefix("#x").and_then(|hex| u32::from_str_radix(hex, 16).ok()).or_else(|| name.strip_prefix('#').and_then(|n| n.parse().ok())).and_then(char::from_u32),
        };
        match named {
            Some(c) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A TTML clock time (`h:mm:ss.fff`, `mm:ss.fff`, `ss.fff`, with or without `s`) as seconds.
fn clock(time: &str) -> Option<f64> {
    let time = time.trim().trim_end_matches('s');
    let mut seconds = 0.0;
    for part in time.split(':') {
        seconds = seconds * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(seconds)
}

/// NetEase Cloud Music's lyrics of its track `id`, with their translation: timed word by word where it has them
/// (its `yrc`), else line by line.
fn netease(id: u64) -> Answer<Vec<Line>> {
    let said = netease_said(&format!("https://music.163.com/api/song/lyric/v1?id={id}&lv=1&tv=-1&yv=1"))?;
    let translation = said["tlyric"]["lyric"].as_str();
    if let Some(lines) = said["yrc"]["lyric"].as_str().map(|yrc| word_timed(yrc, translation)).filter(|lines| lines.iter().any(|line| !line.words.is_empty())) {
        return Ok(Some(lines));
    }
    let lines = said["lrc"]["lyric"].as_str().map(|lrc| merged(lrc, translation));
    Ok(lines.filter(|lines| lines.iter().any(|line| !line.text.is_empty())))
}

/// What NetEase's API says to `url`, where it answers (its own `code` 200; another, such as when it is asked too
/// often, is no answer).
fn netease_said(url: &str) -> Result<serde_json::Value, Unreached> {
    fetched_json(url)?.filter(|said| said["code"].as_i64() == Some(200)).ok_or(Unreached)
}

/// The lines of NetEase's word-timed lyrics (`yrc`): `[start,length]` (milliseconds) before each, then each word as
/// `(start,length,0)` and what is sung; lines of another kind (its credits, written as JSON) are left out. Each
/// with the line of `translation` (an LRC text) nearest in time, within a second.
pub fn word_timed(yrc: &str, translation: Option<&str>) -> Vec<Line> {
    let mut lines = Vec::new();
    for raw in yrc.lines() {
        let Some((head, mut rest)) = raw.trim().strip_prefix('[').and_then(|after| after.split_once(']')) else { continue };
        let Some((start, length)) = head.split_once(',').and_then(|(a, b)| Some((a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?))) else { continue };
        let mut words = Vec::new();
        while let Some((timing, after)) = rest.strip_prefix('(').and_then(|after| after.split_once(')')) {
            let mut parts = timing.split(',').map(|part| part.trim().parse::<f64>());
            let (Some(Ok(at)), Some(Ok(lasts))) = (parts.next(), parts.next()) else { break };
            // What is sung runs to the next word's timing, or the line's end.
            let end = after.find('(').unwrap_or(after.len());
            let text = &after[..end];
            if !text.is_empty() {
                words.push(Word { at: at / 1000.0, end: (at + lasts) / 1000.0, text: text.to_string() });
            }
            rest = &after[end..];
        }
        let text: String = words.iter().map(|word| word.text.as_str()).collect::<String>().trim().to_string();
        lines.push(Line { at: start / 1000.0, end: Some((start + length) / 1000.0), text, words, backing: None, translation: None });
    }
    lines.sort_by(|a, b| a.at.total_cmp(&b.at));
    for (at, text) in translation.map(timed).unwrap_or_default() {
        // (A translation of nothing is written "//".)
        if text.is_empty() || text == "//" {
            continue;
        }
        let nearest = lines.iter_mut().min_by(|a, b| (a.at - at).abs().total_cmp(&(b.at - at).abs()));
        if let Some(line) = nearest.filter(|line| (line.at - at).abs() <= 1.0 && line.translation.is_none()) {
            line.translation = Some(text);
        }
    }
    lines
}

/// The track on NetEase Cloud Music that is `track`: of those found by its title and artist, the first of its
/// title and of its length (to two seconds).
fn netease_search(track: &Track) -> Answer<u64> {
    let asked = format!("{} {}", track.title, track.artist);
    let said = netease_said(&format!("https://music.163.com/api/search/get?s={}&type=1&limit=10", escaped(&asked)))?;
    Ok(said["result"]["songs"].as_array().into_iter().flatten().find_map(|song| {
        let length = song["duration"].as_f64()? / 1000.0;
        (same(song["name"].as_str()?, &track.title) && (length - track.seconds as f64).abs() <= 2.0).then(|| song["id"].as_u64()).flatten()
    }))
}

/// LRCLIB's timed lyrics of `track`: the one it has of that title, artist, album and length; else, of those
/// it finds by title and artist, the first of that length (to two seconds) with timed lyrics.
fn lrclib(track: &Track) -> Answer<Vec<Line>> {
    let timed = |said: &serde_json::Value| said["syncedLyrics"].as_str().map(|lrc| merged(lrc, None)).filter(|lines| lines.iter().any(|line| !line.text.is_empty()));
    let exact = format!(
        "https://lrclib.net/api/get?track_name={}&artist_name={}&album_name={}&duration={}",
        escaped(&track.title),
        escaped(&track.artist),
        escaped(&track.album),
        track.seconds
    );
    if let Some(found) = fetched_json(&exact)?.and_then(|said| timed(&said)) {
        return Ok(Some(found));
    }
    let search = format!("https://lrclib.net/api/search?track_name={}&artist_name={}", escaped(&track.title), escaped(&track.artist));
    let said = fetched_json(&search)?.unwrap_or_default();
    Ok(said.as_array().into_iter().flatten().filter(|one| one["duration"].as_f64().is_some_and(|length| (length - track.seconds as f64).abs() <= 2.0)).find_map(timed))
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
    let mut lines: Vec<Line> = timed(lrc).into_iter().map(|(at, text)| Line { at, end: None, text, words: Vec::new(), backing: None, translation: None }).collect();
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

/// The lines of an LRC text as players keep it beside a track or in its tags: a line at the same time as the one
/// before it is that one's translation; words timed within a line (`<mm:ss.xx>` before each, as the "enhanced"
/// LRC times them, and after the last, where its end is said) are timed word by word.
pub fn lrc(text: &str) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    for (at, said) in timed(text) {
        let (words, closed) = worded(&said);
        let plain = if words.is_empty() { said } else { words.iter().map(|(_, word)| word.as_str()).collect::<String>().trim().to_string() };
        if let Some(line) = lines.last_mut().filter(|line| line.at == at && !line.text.is_empty() && line.translation.is_none()) {
            if !plain.is_empty() {
                line.translation = Some(plain);
            }
            continue;
        }
        // Each word ends as the next begins; the last, where its end is said, else as the next line begins.
        let words = (0..words.len()).map(|i| (words[i].0, words.get(i + 1).map(|next| next.0).or(closed), words[i].1.clone())).collect::<Vec<_>>();
        let mut line = Line { at, end: None, text: plain, words: Vec::new(), backing: None, translation: None };
        line.words = words.into_iter().map(|(at, end, text)| Word { at, end: end.unwrap_or(f64::NAN), text }).collect();
        lines.push(line);
    }
    for i in 0..lines.len() {
        let next = lines.get(i + 1).map(|line| line.at);
        if let Some(last) = lines[i].words.last_mut().filter(|last| last.end.is_nan()) {
            last.end = next.unwrap_or(last.at).max(last.at);
        }
    }
    lines
}

/// The words timed in an LRC line (`<mm:ss.xx>` before each): when each begins and what it is (with the space
/// after it); and when the last ends, where a time follows it. None where no word is timed.
fn worded(said: &str) -> (Vec<(f64, String)>, Option<f64>) {
    let mut words: Vec<(f64, String)> = Vec::new();
    let mut rest = said;
    let mut closed = None;
    while let Some(open) = rest.find('<') {
        let Some((tag, after)) = rest[open + 1..].split_once('>') else { break };
        let Some(at) = time(tag) else { break };
        if let Some(last) = words.last_mut() {
            last.1.push_str(&rest[..open]);
        }
        let end = after.find('<').unwrap_or(after.len());
        if after[..end].is_empty() && end == after.len() {
            closed = Some(at);
            rest = "";
            break;
        }
        words.push((at, after[..end].to_string()));
        rest = &after[end..];
    }
    if let Some(last) = words.last_mut() {
        last.1.push_str(rest);
    }
    words.retain(|(_, word)| !word.is_empty());
    (words, closed)
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

/// The lyrics found of the tracks asked about, kept on disk (a file each, in `folder`), so that a track played again
/// shows them at once without asking: lyrics timed word by word, for good; timed only line by line, for a month (a
/// source may come to have them word by word); that none has them, for a week; and what was found while a source
/// could not be reached (which may have had better), for a day. The most kept, the files written longest ago go
/// first.
pub mod kept {
    use super::*;

    const LINED_FOR: Duration = Duration::from_secs(30 * 24 * 3600);
    const NONE_FOR: Duration = Duration::from_secs(7 * 24 * 3600);
    const UNSURE_FOR: Duration = Duration::from_secs(24 * 3600);
    const KEPT_MOST: usize = 2000;
    /// The largest file read (larger: not one Glance wrote).
    const FILE_MOST: u64 = 4 << 20;

    /// A file: the track (as the name it is kept under could be another's too), when its lyrics were asked for
    /// (seconds since 1970), what was found, and whether each source asked answered.
    #[derive(Serialize, Deserialize)]
    struct File {
        track: Track,
        asked: u64,
        lines: Option<Vec<Line>>,
        sure: bool,
    }

    /// A track's lyrics as kept (none: none had them), when they were asked for, and whether each source answered.
    pub struct Kept {
        pub lines: Option<Vec<Line>>,
        asked: SystemTime,
        sure: bool,
    }

    impl Kept {
        /// Whether they are still as the sources would now give them, so far as can be told.
        pub fn fresh(&self) -> bool {
            let age = SystemTime::now().duration_since(self.asked).unwrap_or_default();
            match &self.lines {
                _ if !self.sure => age < UNSURE_FOR,
                Some(lines) if lines.iter().any(|line| !line.words.is_empty()) => true,
                Some(_) => age < LINED_FOR,
                None => age < NONE_FOR,
            }
        }
    }

    /// Where they are kept: `lyrics` in Glance's local application data.
    pub fn folder() -> PathBuf {
        crate::settings::local_dir().join("lyrics")
    }

    /// The name `track`'s are kept under: a hash (64-bit FNV-1a, the same in every build) of all it is.
    pub(super) fn name(track: &Track) -> String {
        let hash = serde_json::to_string(track).unwrap_or_default().bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| (hash ^ byte as u64).wrapping_mul(0x0100_0000_01b3));
        format!("{hash:016x}.json")
    }

    pub fn read(track: &Track) -> Option<Kept> {
        read_in(&folder(), track)
    }

    /// As `read`, from `folder`.
    pub(super) fn read_in(folder: &std::path::Path, track: &Track) -> Option<Kept> {
        let text = crate::elevation::read_in_place(folder, &name(track), FILE_MOST)?;
        let file: File = serde_json::from_str(&text).ok()?;
        (file.track == *track).then(|| Kept { lines: file.lines, asked: SystemTime::UNIX_EPOCH + Duration::from_secs(file.asked), sure: file.sure })
    }

    /// Keeps what was found of `track`'s lyrics (none: none had them), asked for now, and whether each source
    /// answered; and, kept the most, lets the files written longest ago go. (Not kept where it cannot be written:
    /// they are asked for again.)
    pub fn write(track: &Track, lines: Option<&[Line]>, sure: bool) {
        write_in(&folder(), track, lines, sure);
    }

    /// As `write`, into `folder`.
    pub(super) fn write_in(folder: &std::path::Path, track: &Track, lines: Option<&[Line]>, sure: bool) {
        if folder.parent().is_none_or(|own| crate::elevation::ensure_folder(own).is_err()) || crate::elevation::ensure_folder(folder).is_err() {
            return;
        }
        let asked = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
        let file = File { track: track.clone(), asked, lines: lines.map(<[Line]>::to_vec), sure };
        let Ok(json) = serde_json::to_vec(&file) else { return };
        if crate::elevation::write_in_place(folder, &name(track), &json).is_ok() {
            trimmed(folder);
        }
    }

    fn trimmed(folder: &std::path::Path) {
        let Ok(entries) = std::fs::read_dir(folder) else { return };
        let mut files: Vec<(SystemTime, PathBuf)> = entries
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json"))
            .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
            .collect();
        if files.len() <= KEPT_MOST {
            return;
        }
        files.sort();
        for (_, path) in &files[..files.len() - KEPT_MOST] {
            let _ = std::fs::remove_file(path);
        }
    }

    /// The names of the files kept (for the uninstaller to take away).
    pub fn names() -> Vec<String> {
        std::fs::read_dir(folder()).map(|entries| entries.flatten().filter_map(|entry| entry.file_name().into_string().ok()).collect()).unwrap_or_default()
    }
}

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
    fn reads_a_local_lrc() {
        let lines = lrc("[ti:x]\n[00:01.00]第一行\n[00:01.00]First line\n[00:02.50]<00:02.50>Hel<00:03.00>lo <00:03.40>you<00:04.00>\n[00:05.00]<00:05.00>end\n[00:07.00]\n");
        let said: Vec<(f64, &str, Option<&str>)> = lines.iter().map(|l| (l.at, l.text.as_str(), l.translation.as_deref())).collect();
        assert_eq!(said, vec![(1.0, "第一行", Some("First line")), (2.5, "Hello you", None), (5.0, "end", None), (7.0, "", None)]);
        let words: Vec<(f64, f64, &str)> = lines[1].words.iter().map(|w| (w.at, w.end, w.text.as_str())).collect();
        assert_eq!(words, vec![(2.5, 3.0, "Hel"), (3.0, 3.4, "lo "), (3.4, 4.0, "you")]);
        assert_eq!((lines[2].words[0].at, lines[2].words[0].end), (5.0, 7.0));
    }

    /// A line whose head is not closed before its end is left out, and the lines after it read (it was cut from
    /// past its end, and Glance stopped: what the collection serves is not checked before it is read).
    #[test]
    fn a_line_not_closed_is_left_out() {
        let file = r#"<tt><p begin="00:00.5"</p><p begin="00:01.000" end="00:02.000"><span begin="00:01.000" end="00:02.000">kept</span></p></tt>"#;
        let lines = ttml(file);
        assert_eq!(lines.iter().map(|line| line.text.as_str()).collect::<Vec<_>>(), ["kept"]);
    }

    /// Lyrics with no line are held as none found, wherever they come from.
    #[test]
    fn lyrics_with_no_line_are_none() {
        assert!(matches!(found(Some(Vec::new())), Held::Found(None)));
        assert!(matches!(found(None), Held::Found(None)));
        let line = Line { at: 1.0, end: None, text: "kept".into(), words: Vec::new(), backing: None, translation: None };
        assert!(matches!(found(Some(vec![line])), Held::Found(Some(lines)) if lines.len() == 1));
    }

    #[test]
    fn reads_ttml_word_by_word() {
        let file = r#"<tt><body><div><p begin="00:22.402" end="00:26.962" ttm:agent="v1"><span begin="00:22.402" end="00:23.331">Hello</span> <span begin="00:25.274" end="00:25.619">wor</span><span begin="00:25.619" end="00:26.962">ld &amp; you</span><span ttm:role="x-bg"><span begin="00:26.0" end="00:26.5">(ooh)</span></span><span ttm:role="x-translation" xml:lang="zh-CN">你好世界</span></p><p begin="1:00:01.5" end="1:00:02"><span begin="3601.5" end="3602">late</span></p></div></body></tt>"#;
        let lines = ttml(file);
        assert_eq!(lines.len(), 2);
        let line = &lines[0];
        assert_eq!((line.at, line.end, line.text.as_str()), (22.402, Some(26.962), "Hello world & you"));
        assert_eq!(line.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), ["Hello ", "wor", "ld & you"]);
        assert_eq!((line.words[1].at, line.words[1].end), (25.274, 25.619));
        assert_eq!((line.backing.as_deref(), line.translation.as_deref()), (Some("(ooh)"), Some("你好世界")));
        assert_eq!(lines[1].at, 3601.5);
    }

    #[test]
    fn reads_netease_word_by_word() {
        let yrc = "{\"t\":0,\"c\":[{\"tx\":\"作词: \"},{\"tx\":\"某人\"}]}
[16210,3070](16210,670,0)Hello (16880,410,0)wor(17290,1990,0)ld
[20000,1000](20000,1000,0)Again
";
        let lines = word_timed(yrc, Some("[00:16.20]你好世界
[00:20.00]//"));
        assert_eq!(lines.len(), 2);
        let line = &lines[0];
        assert_eq!((line.at, line.end, line.text.as_str(), line.translation.as_deref()), (16.21, Some(19.28), "Hello world", Some("你好世界")));
        assert_eq!(line.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), ["Hello ", "wor", "ld"]);
        assert_eq!((line.words[1].at, line.words[1].end), (16.88, 17.29));
        assert_eq!((lines[1].text.as_str(), lines[1].translation.as_deref()), ("Again", None));
    }

    /// Kept in a folder of the test's own (the user's kept lyrics are not touched: written there, the oldest of them
    /// could be let go).
    #[test]
    fn keeps_lyrics_while_they_are_fresh() {
        let folder = std::env::temp_dir().join(format!("glance-kept-lyrics-{}", std::process::id())).join("lyrics");
        let track = Track { netease: Some(7), title: "kept test".into(), artist: "nobody".into(), album: String::new(), seconds: 1 };
        let worded = word_timed("[1000,500](1000,500,0)la", None);
        kept::write_in(&folder, &track, Some(&worded), true);
        let read = kept::read_in(&folder, &track).expect("kept");
        assert_eq!(read.lines.as_deref(), Some(&worded[..]));
        assert!(read.fresh());
        kept::write_in(&folder, &track, None, true);
        assert!(kept::read_in(&folder, &track).is_some_and(|kept| kept.lines.is_none() && kept.fresh()));
        let other = Track { seconds: 2, ..track.clone() };
        assert!(kept::read_in(&folder, &other).is_none());
        let _ = std::fs::remove_dir_all(folder.parent().unwrap());
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
        let worded = amll(1901371647).ok().flatten().expect("the collection's, word by word");
        println!("AMLL: {} lines; {:?}", worded.len(), worded.iter().find(|line| line.words.len() > 2).map(|line| (&line.text, line.words.len(), &line.translation)));
        let by_id = find(&Track { netease: Some(2124385847), title: String::new(), artist: String::new(), album: String::new(), seconds: 330 }).found.expect("NetEase's, by number");
        println!("NetEase by number: {} lines, {} timed word by word, the second {:?}", by_id.len(), by_id.iter().filter(|line| !line.words.is_empty()).count(), by_id.get(1));
        let worded = netease(1974443814).ok().flatten().expect("NetEase's, word by word");
        println!("NetEase word by word: {} lines; {:?}", worded.len(), worded.iter().find(|line| line.words.len() > 2).map(|line| (&line.text, &line.words, &line.translation)));
        assert!(worded.iter().any(|line| line.words.len() > 2));
        let by_name = find(&Track { netease: None, title: "Yellow".into(), artist: "Coldplay".into(), album: "Parachutes".into(), seconds: 267 }).found.expect("LRCLIB's");
        println!("by name: {} lines, the first sung {:?}", by_name.len(), by_name.iter().find(|line| !line.text.is_empty()));
        let searched = netease_search(&Track { netease: None, title: "我爱你中国 (live)".into(), artist: "孙楠".into(), album: String::new(), seconds: 330 });
        println!("NetEase by its title: {:?}", searched.as_ref().ok());
        assert!(matches!(searched, Ok(Some(_))));
        let none = find(&Track { netease: None, title: "zzqqxx no such song".into(), artist: "qqqww".into(), album: String::new(), seconds: 100 });
        assert!(none.sure && none.found.is_none(), "that no source has a track's lyrics is known");
        assert!(matches!(netease(1), Ok(None)), "NetEase says it has none");
    }
}
