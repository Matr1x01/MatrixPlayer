//! Content-addressed artwork cache.
//!
//! Artwork is stored once per unique image under the app cache directory and
//! referenced by the first 32 hex digits of its BLAKE3 hash. The frontend
//! never receives file paths: it loads `art://localhost/<id>`, which the
//! protocol handler in [`protocol`] resolves against this store only.
//!
//! Each image is decoded exactly once, when first stored, to read its size and
//! sample a small palette for the Now Playing ambient backdrop.

pub mod protocol;

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use image::{DynamicImage, GenericImageView};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::metadata::EmbeddedPicture;

/// Folder images larger than this are ignored rather than read into memory.
const MAX_FOLDER_ART_BYTES: u64 = 25 * 1024 * 1024;
const ID_LEN: usize = 32;
const FOLDER_ART_STEMS: &[&str] = &["cover", "folder", "front", "album", "albumart"];
const FOLDER_ART_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ArtworkSource {
    /// A picture embedded in the audio file's tags.
    Embedded,
    /// An image file next to the audio file (cover.jpg, folder.png…).
    Folder,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtworkInfo {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub source: ArtworkSource,
    /// Colours sampled around the points the ambient backdrop is anchored to:
    /// left-middle, upper-right and bottom-centre of the image.
    pub palette: [[u8; 3]; 3],
}

pub struct ArtworkStore {
    dir: PathBuf,
    known: Mutex<HashMap<String, ArtworkInfo>>,
}

impl ArtworkStore {
    pub fn new(dir: PathBuf) -> AppResult<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir, known: Mutex::new(HashMap::new()) })
    }

    /// Finds artwork for a track: the embedded cover first, then a cover
    /// image in the track's folder. Failures are logged, never fatal.
    pub fn resolve(&self, audio_path: &Path, embedded: Option<&EmbeddedPicture>) -> Option<ArtworkInfo> {
        if let Some(picture) = embedded {
            match self.store(&picture.data, ArtworkSource::Embedded) {
                Ok(info) => return Some(info),
                Err(e) => log::warn!("embedded artwork in {}: {e}", audio_path.display()),
            }
        }
        let image_path = find_folder_art(audio_path.parent()?)?;
        let bytes = read_limited(&image_path).ok()?;
        self.store(&bytes, ArtworkSource::Folder)
            .inspect_err(|e| log::warn!("folder artwork {}: {e}", image_path.display()))
            .ok()
    }

    /// Stores `bytes` if they are a decodable image and returns its info.
    pub fn store(&self, bytes: &[u8], source: ArtworkSource) -> AppResult<ArtworkInfo> {
        let id = blake3::hash(bytes).to_hex()[..ID_LEN].to_owned();
        if let Some(info) = self.known.lock().unwrap().get(&id) {
            return Ok(info.clone());
        }

        let image = image::load_from_memory(bytes).map_err(|e| AppError::Artwork(e.to_string()))?;
        let (width, height) = image.dimensions();
        let info = ArtworkInfo { id: id.clone(), width, height, source, palette: sample_palette(&image) };

        let path = self.dir.join(&id);
        if !path.exists() {
            // Write-then-rename so a crash never leaves a truncated image behind.
            let tmp = self.dir.join(format!("{id}.tmp"));
            std::fs::File::create(&tmp)?.write_all(bytes)?;
            std::fs::rename(&tmp, &path)?;
        }
        self.known.lock().unwrap().insert(id, info.clone());
        Ok(info)
    }

    /// Returns the cached file for `id`, rejecting anything that is not a
    /// well-formed id so request paths can never escape the cache directory.
    pub fn file_for(&self, id: &str) -> Option<PathBuf> {
        let valid = id.len() == ID_LEN && id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
        valid.then(|| self.dir.join(id)).filter(|p| p.is_file())
    }
}

fn find_folder_art(dir: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<(usize, PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let stem = path.file_stem()?.to_str()?.to_ascii_lowercase();
            let ext = path.extension()?.to_str()?.to_ascii_lowercase();
            let rank = FOLDER_ART_STEMS.iter().position(|s| *s == stem)?;
            FOLDER_ART_EXTENSIONS.contains(&ext.as_str()).then_some((rank, path))
        })
        .collect();
    candidates.sort();
    candidates.into_iter().next().map(|(_, p)| p)
}

fn read_limited(path: &Path) -> std::io::Result<Vec<u8>> {
    let len = std::fs::metadata(path)?.len();
    if len > MAX_FOLDER_ART_BYTES {
        return Err(std::io::Error::other("image too large"));
    }
    std::fs::read(path)
}

/// Averages small windows of a 32×32 thumbnail around three anchor points.
fn sample_palette(image: &DynamicImage) -> [[u8; 3]; 3] {
    const SIZE: u32 = 32;
    let thumb = image.thumbnail_exact(SIZE, SIZE).to_rgb8();
    let anchors = [(0.22, 0.45), (0.70, 0.18), (0.45, 0.88)];
    anchors.map(|(fx, fy)| {
        let cx = (fx * SIZE as f32) as i32;
        let cy = (fy * SIZE as f32) as i32;
        let mut sum = [0u32; 3];
        let mut n = 0u32;
        for y in (cy - 4)..=(cy + 4) {
            for x in (cx - 4)..=(cx + 4) {
                if (0..SIZE as i32).contains(&x) && (0..SIZE as i32).contains(&y) {
                    let px = thumb.get_pixel(x as u32, y as u32);
                    for c in 0..3 {
                        sum[c] += u32::from(px[c]);
                    }
                    n += 1;
                }
            }
        }
        sum.map(|s| (s / n.max(1)) as u8)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(color: [u8; 3]) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(8, 8, image::Rgb(color));
        let mut out = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn stores_images_once_under_their_hash() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtworkStore::new(dir.path().to_path_buf()).unwrap();
        let bytes = png([200, 40, 10]);

        let a = store.store(&bytes, ArtworkSource::Embedded).unwrap();
        let b = store.store(&bytes, ArtworkSource::Embedded).unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!((a.width, a.height), (8, 8));
        assert_eq!(a.palette, [[200, 40, 10]; 3]);
        assert_eq!(std::fs::read(store.file_for(&a.id).unwrap()).unwrap(), bytes);
    }

    #[test]
    fn rejects_non_images_and_malformed_ids() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtworkStore::new(dir.path().to_path_buf()).unwrap();
        assert!(store.store(b"not an image", ArtworkSource::Folder).is_err());
        assert!(store.file_for("../../etc/passwd").is_none());
        assert!(store.file_for(&"A".repeat(ID_LEN)).is_none());
    }

    #[test]
    fn prefers_cover_over_folder_images() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["folder.jpg", "Cover.PNG", "scan.jpg"] {
            std::fs::write(dir.path().join(name), "x").unwrap();
        }
        let found = find_folder_art(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap(), "Cover.PNG");
    }
}
