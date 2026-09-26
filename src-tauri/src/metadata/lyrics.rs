//! Lyrics: a `.lrc` file next to the track, or lyrics embedded in its tags.
//!
//! Timed lyrics use the LRC format, whether they come from a sidecar file or
//! were embedded as LRC text (as many taggers do). Anything without
//! timestamps is shown as plain text.

use std::path::{Path, PathBuf};

use lofty::config::ParseOptions;
use lofty::file::TaggedFileExt;
use lofty::tag::ItemKey;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LyricsSource {
    File,
    Embedded,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LyricLine {
    /// When the line starts (ms), already corrected by the file's offset.
    /// `None` for plain lyrics.
    pub time_ms: Option<u64>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Lyrics {
    pub source: LyricsSource,
    /// The sidecar's file name, for lyrics from a file.
    pub file_name: Option<String>,
    pub synced: bool,
    /// The `[offset:]` the file declared (ms, positive = earlier).
    pub offset_ms: i64,
    pub lines: Vec<LyricLine>,
}

/// Lyrics for the audio file at `path`: a sidecar `.lrc` wins over tags.
pub fn load(path: &Path) -> Option<Lyrics> {
    if let Some(file) = sidecar(path)
        && let Ok(bytes) = std::fs::read(&file)
    {
        let text = String::from_utf8_lossy(&bytes);
        if let Some(mut lyrics) = parse(&text, LyricsSource::File) {
            lyrics.file_name = file.file_name().map(|n| n.to_string_lossy().into_owned());
            return Some(lyrics);
        }
    }
    parse(&embedded(path)?, LyricsSource::Embedded)
}

/// `<stem>.lrc` in the track's folder, matched without regard to case.
fn sidecar(path: &Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let stem = path.file_stem()?;
    let exact = dir.join(stem).with_extension("lrc");
    if exact.is_file() {
        return Some(exact);
    }
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.file_stem() == Some(stem) && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lrc")) && p.is_file()
    })
}

fn embedded(path: &Path) -> Option<String> {
    let tagged = lofty::probe::Probe::open(path)
        .ok()?
        .options(ParseOptions::new().read_properties(false).read_cover_art(false))
        .guess_file_type()
        .ok()?
        .read()
        .ok()?;
    let primary = tagged.primary_tag().into_iter();
    primary
        .chain(tagged.tags())
        .find_map(|tag| tag.get_string(ItemKey::Lyrics).map(str::to_owned))
        .filter(|s| !s.trim().is_empty())
}

/// Parses LRC or plain text. Returns `None` when there is nothing to show.
pub fn parse(text: &str, source: LyricsSource) -> Option<Lyrics> {
    let mut offset_ms = 0i64;
    let mut timed: Vec<(i64, String)> = Vec::new();
    let mut plain: Vec<String> = Vec::new();

    for raw in text.lines() {
        let mut rest = raw.trim_start_matches('\u{feff}').trim();
        let mut stamps = Vec::new();
        let mut tag_only = false;
        while let Some(inner) = rest.strip_prefix('[').and_then(|r| r.split_once(']')) {
            let (tag, after) = inner;
            if let Some(ms) = timestamp(tag) {
                stamps.push(ms);
            } else if let Some(value) = tag.strip_prefix("offset:") {
                offset_ms = value.trim().parse().unwrap_or(0);
                tag_only = true;
            } else if is_id_tag(tag) {
                tag_only = true;
            } else {
                break;
            }
            rest = after;
        }
        let words = strip_word_stamps(rest);
        if !stamps.is_empty() {
            timed.extend(stamps.into_iter().map(|t| (t, words.clone())));
        } else if !tag_only {
            plain.push(words);
        }
    }

    if !timed.is_empty() {
        timed.sort_by_key(|(t, _)| *t);
        let lines = timed
            .into_iter()
            .map(|(t, text)| LyricLine { time_ms: Some((t - offset_ms).max(0) as u64), text })
            .collect();
        return Some(Lyrics { source, file_name: None, synced: true, offset_ms, lines });
    }

    // Plain text: trim blank lines at the ends, keep blank lines between
    // verses (collapsed to one).
    let mut lines: Vec<LyricLine> = Vec::new();
    for text in plain {
        let blank = text.is_empty();
        if blank && lines.last().is_none_or(|l| l.text.is_empty()) {
            continue;
        }
        lines.push(LyricLine { time_ms: None, text });
    }
    while lines.last().is_some_and(|l| l.text.is_empty()) {
        lines.pop();
    }
    (!lines.is_empty()).then_some(Lyrics { source, file_name: None, synced: false, offset_ms: 0, lines })
}

/// `mm:ss`, `mm:ss.xx` or `mm:ss.xxx` (also with `:` before the fraction).
fn timestamp(tag: &str) -> Option<i64> {
    let (min, rest) = tag.split_once(':')?;
    let min: i64 = min.trim().parse().ok()?;
    let (sec, frac) = match rest.split_once(['.', ':']) {
        Some((s, f)) => (s, f),
        None => (rest, ""),
    };
    let sec: i64 = sec.trim().parse().ok()?;
    if !(0..60).contains(&sec) || min < 0 || !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let frac_ms = match frac.len() {
        0 => 0,
        1 => frac.parse::<i64>().ok()? * 100,
        2 => frac.parse::<i64>().ok()? * 10,
        _ => frac[..3].parse::<i64>().ok()?,
    };
    Some(min * 60_000 + sec * 1000 + frac_ms)
}

/// LRC header tags: `[ar:…]`, `[ti:…]`, `[length:…]`, `[#:…]` and so on.
fn is_id_tag(tag: &str) -> bool {
    tag.split_once(':').is_some_and(|(key, _)| {
        !key.is_empty() && key.len() <= 8 && key.chars().all(|c| c.is_ascii_alphabetic() || c == '#')
    })
}

/// Removes enhanced-LRC word timings (`<00:12.34>`).
fn strip_word_stamps(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        match rest[start + 1..].find('>') {
            Some(end) if timestamp(&rest[start + 1..start + 1 + end]).is_some() => {
                out.push_str(&rest[..start]);
                rest = &rest[start + end + 2..];
            }
            _ => {
                out.push_str(&rest[..=start]);
                rest = &rest[start + 1..];
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synced_lyrics_with_offsets_repeats_and_word_timings() {
        let text = "\u{feff}[ti:Isobar]\n[ar:Halden Reach]\n[offset:+120]\n\n\
                    [00:12.34]Pressure lines across the map\n\
                    [00:05.00][00:30.5]and I <00:31.00>follow <00:31.40>where\n\
                    [01:02.123]\n";
        let lyrics = parse(text, LyricsSource::File).unwrap();
        assert!(lyrics.synced);
        assert_eq!(lyrics.offset_ms, 120);
        let got: Vec<(u64, &str)> = lyrics.lines.iter().map(|l| (l.time_ms.unwrap(), l.text.as_str())).collect();
        assert_eq!(
            got,
            [
                (4880, "and I follow where"),
                (12220, "Pressure lines across the map"),
                (30380, "and I follow where"),
                (62003, ""),
            ]
        );
    }

    #[test]
    fn plain_lyrics_keep_verse_breaks() {
        let lyrics = parse("\n\nFirst line\nSecond line\n\n\n\nNext verse\n\n", LyricsSource::Embedded).unwrap();
        assert!(!lyrics.synced);
        let got: Vec<&str> = lyrics.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(got, ["First line", "Second line", "", "Next verse"]);
        assert!(parse("  \n[ar:Someone]\n", LyricsSource::Embedded).is_none());
    }

    #[test]
    fn brackets_in_plain_text_are_kept() {
        let lyrics = parse("[Chorus]\nLa la la", LyricsSource::Embedded).unwrap();
        assert_eq!(lyrics.lines[0].text, "[Chorus]");
    }

    #[test]
    fn finds_a_sidecar_file_whatever_its_case() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("02 - Isobar.flac");
        std::fs::write(&audio, b"").unwrap();
        std::fs::write(dir.path().join("02 - Isobar.LRC"), "[00:01.00]Hello").unwrap();
        let lyrics = load(&audio).unwrap();
        assert_eq!(lyrics.source, LyricsSource::File);
        assert_eq!(lyrics.file_name.as_deref(), Some("02 - Isobar.LRC"));
        assert_eq!(lyrics.lines[0], LyricLine { time_ms: Some(1000), text: "Hello".into() });
    }

    #[test]
    fn reads_lyrics_embedded_in_tags() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("a.flac");
        crate::test_support::write_flac(&audio, 0.2, "A");
        // GStreamer cannot write a lyrics comment; lofty can.
        let mut tagged = lofty::read_from_path(&audio).unwrap();
        let tag = tagged.primary_tag_mut().unwrap();
        tag.insert_text(ItemKey::Lyrics, "[00:02.00]Sung line".into());
        lofty::prelude::TagExt::save_to_path(tag, &audio, lofty::config::WriteOptions::default()).unwrap();
        let lyrics = load(&audio).unwrap();
        assert_eq!(lyrics.source, LyricsSource::Embedded);
        assert!(lyrics.synced);
        assert_eq!(lyrics.lines[0].text, "Sung line");
    }
}
