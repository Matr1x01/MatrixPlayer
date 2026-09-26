//! Tag and stream-property extraction.
//!
//! Metadata comes from the file itself via `lofty`. Where a field is missing
//! and MatrixPlayer fills it in (for example a title taken from the file name) the
//! field is listed in [`TrackMetadata::derived_fields`] so the UI can show the
//! difference between what the file says and what the application inferred.

use std::fs::File;
use std::path::Path;

use lofty::config::ParseOptions;
use lofty::file::{AudioFile, FileType, TaggedFile};
use lofty::picture::{MimeType, PictureType};
use lofty::prelude::*;
use lofty::tag::{Tag, TagType};
use serde::Serialize;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrackMetadata {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub year: Option<u16>,
    pub track_number: Option<u32>,
    pub track_total: Option<u32>,
    pub disc_number: Option<u32>,
    pub disc_total: Option<u32>,
    pub composer: Option<String>,
    pub comment: Option<String>,
    pub copyright: Option<String>,
    pub replay_gain: ReplayGain,
    /// Human-readable name of the tag format the values were read from.
    pub tag_format: Option<String>,
    /// Number of tag items in that tag, for the "File" panel.
    pub tag_item_count: usize,
    /// Fields whose value was derived by MatrixPlayer rather than read from a tag.
    pub derived_fields: Vec<&'static str>,
}

/// ReplayGain values in dB (gain) and linear amplitude (peak).
#[derive(Debug, Clone, Copy, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayGain {
    pub track_gain: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain: Option<f32>,
    pub album_peak: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioProperties {
    /// Short codec name as shown on format badges: "FLAC", "MP3", "ALAC"…
    pub codec: String,
    /// Container or file format: "FLAC", "MP4", "Ogg", "WAV"…
    pub container: String,
    pub lossless: bool,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u8>,
    pub channels: Option<u8>,
    pub bitrate_kbps: Option<u32>,
    pub duration_ms: u64,
    pub file_size: u64,
}

#[derive(Debug, Clone)]
pub struct EmbeddedPicture {
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ProbedFile {
    pub metadata: TrackMetadata,
    pub properties: AudioProperties,
    pub picture: Option<EmbeddedPicture>,
}

/// Reads tags, stream properties and the front cover of an audio file.
///
/// Files whose tags are unreadable still yield a result: the title falls back
/// to the file name and every other tag field stays empty. Only files lofty
/// cannot identify at all are reported as errors.
pub fn probe(path: &Path) -> AppResult<ProbedFile> {
    let file_size = std::fs::metadata(path)?.len();
    let fail = |e: &dyn std::fmt::Display| AppError::Metadata(format!("{}: {e}", path.display()));
    let options = ParseOptions::new().read_cover_art(true);
    let tagged = lofty::probe::Probe::open(path)
        .map_err(|e| fail(&e))?
        .options(options)
        .guess_file_type()
        .map_err(|e| fail(&e))?
        .read()
        .map_err(|e| fail(&e))?;

    let properties = audio_properties(path, &tagged, file_size);
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let metadata = track_metadata(path, tag);
    let picture = tag.and_then(front_cover);

    Ok(ProbedFile { metadata, properties, picture })
}

fn track_metadata(path: &Path, tag: Option<&Tag>) -> TrackMetadata {
    let mut md = TrackMetadata::default();
    if let Some(tag) = tag {
        md.title = clean(tag.title()).unwrap_or_default();
        md.artist = clean(tag.artist());
        md.album = clean(tag.album());
        md.album_artist = tag_string(tag, ItemKey::AlbumArtist);
        md.genre = clean(tag.genre());
        md.year = tag.date().map(|d| d.year).filter(|y| *y > 0);
        md.track_number = tag.track();
        md.track_total = tag.track_total();
        md.disc_number = tag.disk();
        md.disc_total = tag.disk_total();
        md.composer = tag_string(tag, ItemKey::Composer);
        md.comment = clean(tag.comment());
        md.copyright = tag_string(tag, ItemKey::CopyrightMessage);
        md.replay_gain = ReplayGain {
            track_gain: tag_number(tag, ItemKey::ReplayGainTrackGain),
            track_peak: tag_number(tag, ItemKey::ReplayGainTrackPeak),
            album_gain: tag_number(tag, ItemKey::ReplayGainAlbumGain),
            album_peak: tag_number(tag, ItemKey::ReplayGainAlbumPeak),
        };
        md.tag_format = Some(tag_format_name(tag.tag_type()).to_owned());
        md.tag_item_count = tag.item_count() as usize;
    }
    if md.title.is_empty() {
        md.title =
            path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
        md.derived_fields.push("title");
    }
    md
}

fn audio_properties(path: &Path, tagged: &TaggedFile, file_size: u64) -> AudioProperties {
    let props = tagged.properties();
    let file_type = tagged.file_type();
    let (codec, container, lossless) = codec_info(path, file_type);
    AudioProperties {
        codec,
        container: container.to_owned(),
        lossless,
        sample_rate: props.sample_rate(),
        bit_depth: props.bit_depth().filter(|_| lossless),
        channels: props.channels(),
        bitrate_kbps: props.audio_bitrate().or(props.overall_bitrate()),
        duration_ms: u64::try_from(props.duration().as_millis()).unwrap_or(u64::MAX),
        file_size,
    }
}

/// Maps a container to the codec inside it. MP4 is the only container whose
/// codec varies (AAC or ALAC in practice), so it gets a second, cheap read.
fn codec_info(path: &Path, file_type: FileType) -> (String, &'static str, bool) {
    let (codec, container, lossless) = match file_type {
        FileType::Flac => ("FLAC", "FLAC", true),
        FileType::Mpeg => ("MP3", "MPEG", false),
        FileType::Mp4 => return mp4_codec(path),
        FileType::Opus => ("Opus", "Ogg", false),
        FileType::Vorbis => ("Vorbis", "Ogg", false),
        FileType::Speex => ("Speex", "Ogg", false),
        FileType::Wav => ("PCM", "WAV", true),
        FileType::Aiff => ("PCM", "AIFF", true),
        FileType::Ape => ("APE", "Monkey's Audio", true),
        FileType::WavPack => ("WavPack", "WavPack", true),
        FileType::Mpc => ("Musepack", "Musepack", false),
        FileType::Aac => ("AAC", "ADTS", false),
        _ => ("Unknown", "Unknown", false),
    };
    (codec.to_owned(), container, lossless)
}

fn mp4_codec(path: &Path) -> (String, &'static str, bool) {
    use lofty::mp4::{Mp4Codec, Mp4File};
    let codec = File::open(path).ok().and_then(|mut f| {
        let options = ParseOptions::new().read_tags(false).read_cover_art(false);
        Mp4File::read_from(&mut f, options).ok().and_then(|m| m.properties().codec())
    });
    match codec {
        Some(Mp4Codec::ALAC) => ("ALAC".into(), "MP4", true),
        Some(Mp4Codec::FLAC) => ("FLAC".into(), "MP4", true),
        Some(Mp4Codec::MP3) => ("MP3".into(), "MP4", false),
        _ => ("AAC".into(), "MP4", false),
    }
}

fn front_cover(tag: &Tag) -> Option<EmbeddedPicture> {
    let pictures = tag.pictures();
    let chosen = pictures
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.iter().find(|p| !matches!(p.mime_type(), Some(MimeType::Unknown(_)))))
        .or_else(|| pictures.first())?;
    (!chosen.data().is_empty()).then(|| EmbeddedPicture { data: chosen.data().to_vec() })
}

fn tag_format_name(tag_type: TagType) -> &'static str {
    match tag_type {
        TagType::VorbisComments => "Vorbis comments",
        TagType::Id3v2 => "ID3v2",
        TagType::Id3v1 => "ID3v1",
        TagType::Mp4Ilst => "MP4 metadata",
        TagType::Ape => "APE tag",
        TagType::RiffInfo => "RIFF INFO",
        TagType::AiffText => "AIFF text chunks",
        _ => "Tag",
    }
}

fn clean(value: Option<std::borrow::Cow<'_, str>>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

fn tag_string(tag: &Tag, key: ItemKey) -> Option<String> {
    tag.get_string(key).map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned)
}

/// Numeric tag values such as ReplayGain are stored as text, e.g. "-6.20 dB".
fn tag_number(tag: &Tag, key: ItemKey) -> Option<f32> {
    tag.get_string(key).and_then(parse_leading_number)
}

fn parse_leading_number(value: &str) -> Option<f32> {
    let trimmed = value.trim();
    let end = trimmed
        .char_indices()
        .find(|(i, c)| !(c.is_ascii_digit() || *c == '.' || (*i == 0 && (*c == '-' || *c == '+'))))
        .map_or(trimmed.len(), |(i, _)| i);
    trimmed[..end].parse::<f32>().ok().filter(|v| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_replaygain_strings() {
        assert_eq!(parse_leading_number("-6.20 dB"), Some(-6.2));
        assert_eq!(parse_leading_number("+1.5dB"), Some(1.5));
        assert_eq!(parse_leading_number("0.991"), Some(0.991));
        assert_eq!(parse_leading_number("dB"), None);
        assert_eq!(parse_leading_number(""), None);
    }

    #[test]
    fn missing_title_falls_back_to_file_name() {
        let md = track_metadata(Path::new("/music/03 - Lazarus.flac"), None);
        assert_eq!(md.title, "03 - Lazarus");
        assert_eq!(md.derived_fields, ["title"]);
    }
}
