//! The lyrics of what plays, each line with the time it is sung at (word by
//! word, where that is known), its translation where there is one, for what
//! shows them as the track plays. Asked for on the network, only while
//! lyrics are to be shown, by the track's number on NetEase Cloud Music
//! (where its player says it, as it does with the InfLink-rs plugin; else as
//! NetEase finds it by its title and length): first of the AMLL TTML
//! collection (lyrics timed word by word, public domain, kept by that
//! number), then of NetEase itself; else of LRCLIB (an open collection of
//! lyrics timed by line), by the track's title, artist, album and length.
//! Each track's are asked for once while Glance runs, on a thread of their
//! own.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// One line: when it is sung (seconds into the track) and till when, where
/// that is said; what is sung, and word by word where that is known; what is
/// sung behind it (backing vocals); and its translation where one is given.
#[derive(Clone, Debug, PartialEq)]
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
#[derive(Clone, Debug, PartialEq)]
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

/// Asks each source in turn: by the track's NetEase number, the word-timed collection and then NetEase; by its
/// name, LRCLIB, and then the same by the number NetEase finds it by.
fn find(track: &Track) -> Option<Vec<Line>> {
    let by_number = |id: u64| amll(id).or_else(|| netease(id));
    match track.netease {
        Some(id) => by_number(id),
        None => lrclib(track).or_else(|| netease_search(track).and_then(by_number)),
    }
}

/// The AMLL TTML collection's lyrics of NetEase's track `id`, timed word by word (through a CDN that is reached
/// from mainland China too); where it gives no translation, NetEase's own, to the line nearest in time.
pub(crate) fn amll(id: u64) -> Option<Vec<Line>> {
    let file = crate::update::get_json(&format!("https://cdn.jsdelivr.net/gh/Steve-xmh/amll-ttml-db@main/ncm-lyrics/{id}.ttml"))?;
    let mut lines = ttml(&String::from_utf8(file).ok()?);
    if !lines.iter().any(|line| !line.words.is_empty()) {
        return None;
    }
    if lines.iter().all(|line| line.translation.is_none()) {
        if let Some(theirs) = netease(id) {
            for line in &mut lines {
                let nearest = theirs.iter().filter(|their| their.translation.is_some()).min_by(|a, b| (a.at - line.at).abs().total_cmp(&(b.at - line.at).abs()));
                if let Some(their) = nearest.filter(|their| (their.at - line.at).abs() <= 1.0) {
                    line.translation = their.translation.clone();
                }
            }
        }
    }
    Some(lines)
}

/// The lines of a TTML lyrics file as Apple Music and the AMLL collection write them: a `<p>` each (`begin`,
/// `end`), a `<span>` for each word in it (`begin`, `end`); spans with a role of their own hold its backing vocals
/// (`x-bg`, themselves word by word), its translation (`x-translation`) or its romanisation (`x-roman`, left out).
pub fn ttml(xml: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<p ") {
        let after = &rest[start..];
        let Some(head_end) = after.find('>') else { break };
        let Some(close) = after.find("</p>") else { break };
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
fn netease(id: u64) -> Option<Vec<Line>> {
    let said: serde_json::Value = serde_json::from_slice(&crate::update::get_json(&format!("https://music.163.com/api/song/lyric/v1?id={id}&lv=1&tv=-1&yv=1"))?).ok()?;
    let translation = said["tlyric"]["lyric"].as_str();
    if let Some(lines) = said["yrc"]["lyric"].as_str().map(|yrc| word_timed(yrc, translation)).filter(|lines| lines.iter().any(|line| !line.words.is_empty())) {
        return Some(lines);
    }
    let lines = merged(said["lrc"]["lyric"].as_str()?, translation);
    lines.iter().any(|line| !line.text.is_empty()).then_some(lines)
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

    #[test]
    fn escapes_a_query() {
        assert_eq!(escaped("我爱你 A&B"), "%E6%88%91%E7%88%B1%E4%BD%A0%20A%26B");
        assert!(same("Yellow ", "yel low"));
    }

    /// Asks the sources for two tracks (network): `cargo test -- --ignored finds_lyrics --nocapture`.
    #[test]
    #[ignore]
    fn finds_lyrics() {
        let worded = amll(1901371647).expect("the collection's, word by word");
        println!("AMLL: {} lines; {:?}", worded.len(), worded.iter().find(|line| line.words.len() > 2).map(|line| (&line.text, line.words.len(), &line.translation)));
        let by_id = find(&Track { netease: Some(2124385847), title: String::new(), artist: String::new(), album: String::new(), seconds: 330 }).expect("NetEase's, by number");
        println!("NetEase by number: {} lines, {} timed word by word, the second {:?}", by_id.len(), by_id.iter().filter(|line| !line.words.is_empty()).count(), by_id.get(1));
        let worded = netease(1974443814).expect("NetEase's, word by word");
        println!("NetEase word by word: {} lines; {:?}", worded.len(), worded.iter().find(|line| line.words.len() > 2).map(|line| (&line.text, &line.words, &line.translation)));
        assert!(worded.iter().any(|line| line.words.len() > 2));
        let by_name = find(&Track { netease: None, title: "Yellow".into(), artist: "Coldplay".into(), album: "Parachutes".into(), seconds: 267 }).expect("LRCLIB's");
        println!("by name: {} lines, the first sung {:?}", by_name.len(), by_name.iter().find(|line| !line.text.is_empty()));
        let searched = netease_search(&Track { netease: None, title: "我爱你中国 (live)".into(), artist: "孙楠".into(), album: String::new(), seconds: 330 });
        println!("NetEase by its title: {searched:?}");
        assert!(searched.is_some());
    }
}
