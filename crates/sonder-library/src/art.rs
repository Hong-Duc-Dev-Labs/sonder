//! Cover art: source selection and the on-disk thumbnail cache.
//!
//! A cover is identified by the SHA-256 of its original bytes. For each cover the cache holds
//! one JPEG per size in [`THUMB_SIZES`] at `<cache>/<hash[..2]>/<hash>-<size>.jpg`; images
//! smaller than a size are stored at their own size, never upscaled.

use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{ImageReader, Limits};
use sha2::{Digest, Sha256};

/// Longest edge, in pixels, of each cached thumbnail.
pub const THUMB_SIZES: [u32; 2] = [300, 1000];

/// Folder image names, in priority order (matched case-insensitively).
const FOLDER_IMAGE_STEMS: [&str; 3] = ["cover", "folder", "front"];
const FOLDER_IMAGE_EXTS: [&str; 3] = ["jpg", "jpeg", "png"];

const JPEG_QUALITY: u8 = 88;

/// Decoder limits for untrusted image data.
const MAX_IMAGE_EDGE: u32 = 12_000;
const MAX_IMAGE_ALLOC: u64 = 512 * 1024 * 1024;

/// A cover stored in the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtImage {
    pub hash: String,
    pub width: u32,
    pub height: u32,
    /// Up to four dominant vivid colours as comma-separated `#rrggbb`, most dominant first;
    /// empty for art with none (greyscale, near-black). `None` when not computed yet.
    pub colors: Option<String>,
}

/// Pixels whose chroma (max − min channel) is below this read as grey.
const MIN_CHROMA: u8 = 28;
/// A colour must cover at least this share of the sampled pixels to count as the cover's.
const MIN_SHARE: f32 = 0.04;
const HUE_BUCKETS: usize = 12;
/// Most colours kept per cover.
pub const MAX_COVER_COLORS: usize = 4;

/// The dominant vivid colours of `image`, most dominant first, as comma-separated
/// `#rrggbb` (empty when it has none). Samples a 32×32 downscale, groups vivid pixels by hue
/// and ranks the groups by population × average chroma, so a small bright accent can outrank
/// a large dull area but scattered noise cannot.
pub fn cover_colors(image: &image::DynamicImage) -> String {
    let small = image.resize_exact(32, 32, FilterType::Triangle).to_rgb8();
    let total = (small.width() * small.height()) as f32;
    // Per hue bucket: pixel count, summed chroma, summed r, g, b.
    let mut buckets = [(0u32, 0u32, 0u32, 0u32, 0u32); HUE_BUCKETS];
    for p in small.pixels() {
        let [r, g, b] = p.0;
        let max = r.max(g).max(b);
        let chroma = max - r.min(g).min(b);
        if chroma < MIN_CHROMA || max < 40 {
            continue;
        }
        let e = &mut buckets[hue_bucket(r, g, b)];
        e.0 += 1;
        e.1 += u32::from(chroma);
        e.2 += u32::from(r);
        e.3 += u32::from(g);
        e.4 += u32::from(b);
    }
    let mut ranked: Vec<_> = buckets
        .iter()
        .filter(|e| e.0 as f32 / total >= MIN_SHARE)
        .collect();
    // Summed chroma is population × average chroma.
    ranked.sort_by_key(|e| std::cmp::Reverse(e.1));
    ranked
        .iter()
        .take(MAX_COVER_COLORS)
        .map(|&&(n, _, r, g, b)| format!("#{:02x}{:02x}{:02x}", r / n, g / n, b / n))
        .collect::<Vec<_>>()
        .join(",")
}

/// Which of [`HUE_BUCKETS`] equal hue sectors a vivid pixel falls in.
fn hue_bucket(r: u8, g: u8, b: u8) -> usize {
    let (r, g, b) = (f32::from(r), f32::from(g), f32::from(b));
    let max = r.max(g).max(b);
    let c = max - r.min(g).min(b);
    let hue = if max == r {
        ((g - b) / c).rem_euclid(6.0)
    } else if max == g {
        (b - r) / c + 2.0
    } else {
        (r - g) / c + 4.0
    };
    ((hue / 6.0 * HUE_BUCKETS as f32) as usize).min(HUE_BUCKETS - 1)
}

#[derive(Debug, thiserror::Error)]
pub enum ArtError {
    #[error("cannot decode image: {0}")]
    Decode(String),
    #[error("cannot write thumbnail {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// On-disk thumbnail cache.
#[derive(Debug, Clone)]
pub struct ArtCache {
    dir: PathBuf,
}

impl ArtCache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        ArtCache { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Path of the thumbnail for `hash` at `size` (one of [`THUMB_SIZES`]).
    pub fn thumbnail_path(&self, hash: &str, size: u32) -> PathBuf {
        let shard = hash.get(..2).unwrap_or("00");
        self.dir.join(shard).join(format!("{hash}-{size}.jpg"))
    }

    fn has_all_thumbnails(&self, hash: &str) -> bool {
        THUMB_SIZES
            .iter()
            .all(|s| self.thumbnail_path(hash, *s).is_file())
    }

    /// Decodes `bytes` and writes every thumbnail size.
    pub(crate) fn store(&self, hash: &str, bytes: &[u8]) -> Result<ArtImage, ArtError> {
        let mut reader = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| ArtError::Decode(e.to_string()))?;
        let mut limits = Limits::default();
        limits.max_image_width = Some(MAX_IMAGE_EDGE);
        limits.max_image_height = Some(MAX_IMAGE_EDGE);
        limits.max_alloc = Some(MAX_IMAGE_ALLOC);
        reader.limits(limits);
        let image = reader
            .decode()
            .map_err(|e| ArtError::Decode(e.to_string()))?;
        let (width, height) = (image.width(), image.height());
        if width == 0 || height == 0 {
            return Err(ArtError::Decode("image has no pixels".into()));
        }

        for size in THUMB_SIZES {
            let path = self.thumbnail_path(hash, size);
            if path.is_file() {
                continue;
            }
            let scaled = if width.max(height) > size {
                image.resize(size, size, FilterType::CatmullRom)
            } else {
                image.clone()
            };
            let mut jpeg = Vec::new();
            scaled
                .to_rgb8()
                .write_with_encoder(JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY))
                .map_err(|e| ArtError::Decode(e.to_string()))?;
            write_atomic(&path, &jpeg).map_err(|source| ArtError::Write {
                path: path.clone(),
                source,
            })?;
        }
        Ok(ArtImage {
            hash: hash.to_string(),
            width,
            height,
            colors: Some(cover_colors(&image)),
        })
    }

    /// Deletes every thumbnail of `hash`; missing files are ignored.
    pub(crate) fn remove(&self, hash: &str) {
        for size in THUMB_SIZES {
            let _ = fs::remove_file(self.thumbnail_path(hash, size));
        }
    }
}

/// Writes through a uniquely named temporary file and renames it into place, so concurrent
/// writers of the same thumbnail never leave a torn file.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("{}-{n}.tmp", std::process::id()));
    fs::write(&tmp, bytes)?;
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        // Another writer won the race on a platform that refuses to replace: same content.
        if !path.is_file() {
            return Err(err);
        }
    }
    Ok(())
}

pub(crate) fn hash_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for b in digest.iter() {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

/// `cover|folder|front.(jpg|jpeg|png)` in `dir`, by name priority.
pub(crate) fn folder_image(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<(usize, PathBuf)> = Vec::new();
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let (Some(stem), Some(ext)) = (
            path.file_stem().and_then(|s| s.to_str()),
            path.extension().and_then(|s| s.to_str()),
        ) else {
            continue;
        };
        if !FOLDER_IMAGE_EXTS
            .iter()
            .any(|e| ext.eq_ignore_ascii_case(e))
        {
            continue;
        }
        if let Some(rank) = FOLDER_IMAGE_STEMS
            .iter()
            .position(|s| stem.eq_ignore_ascii_case(s))
            && entry.file_type().is_ok_and(|t| t.is_file())
        {
            found.push((rank, path));
        }
    }
    found.sort();
    found.into_iter().next().map(|(_, p)| p)
}

/// Per-scan art resolution shared by the worker threads.
///
/// Knows which hashes are already cached (from the database and from earlier files in this
/// scan) so a cover embedded in every track of an album is decoded once, and resolves each
/// folder's image once.
pub(crate) struct ArtResolver<'a> {
    cache: &'a ArtCache,
    known: Mutex<HashMap<String, ArtImage>>,
    folders: Mutex<HashMap<PathBuf, Option<ArtImage>>>,
}

impl<'a> ArtResolver<'a> {
    pub(crate) fn new(cache: &'a ArtCache, known: HashMap<String, ArtImage>) -> Self {
        ArtResolver {
            cache,
            known: Mutex::new(known),
            folders: Mutex::new(HashMap::new()),
        }
    }

    /// Embedded cover first, then a folder image next to the track or in its album folder.
    pub(crate) fn resolve(&self, embedded: Option<&[u8]>, track: &Path) -> Option<ArtImage> {
        if let Some(bytes) = embedded
            && let Some(art) = self.image(bytes)
        {
            return Some(art);
        }
        let parent = track.parent()?;
        if let Some(art) = self.folder(parent) {
            return Some(art);
        }
        match crate::group::album_dir(track) {
            Some(album) if album != parent => self.folder(album),
            _ => None,
        }
    }

    fn image(&self, bytes: &[u8]) -> Option<ArtImage> {
        let hash = hash_bytes(bytes);
        if let Some(art) = self.known.lock().unwrap().get(&hash)
            && self.cache.has_all_thumbnails(&hash)
        {
            return Some(art.clone());
        }
        let art = self.cache.store(&hash, bytes).ok()?;
        self.known.lock().unwrap().insert(hash, art.clone());
        Some(art)
    }

    fn folder(&self, dir: &Path) -> Option<ArtImage> {
        if let Some(cached) = self.folders.lock().unwrap().get(dir) {
            return cached.clone();
        }
        // Two threads may resolve the same folder at once; both get the same answer.
        let art = folder_image(dir)
            .and_then(|p| fs::read(p).ok())
            .and_then(|bytes| self.image(&bytes));
        self.folders
            .lock()
            .unwrap()
            .insert(dir.to_path_buf(), art.clone());
        art
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, RgbImage};

    pub(crate) fn png(width: u32, height: u32) -> Vec<u8> {
        let img = RgbImage::from_fn(width, height, |x, y| image::Rgb([x as u8, y as u8, 128]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn thumbnails_are_downscaled_not_upscaled() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ArtCache::new(dir.path());
        let bytes = png(1200, 600);
        let hash = hash_bytes(&bytes);
        let art = cache.store(&hash, &bytes).unwrap();
        assert_eq!((art.width, art.height), (1200, 600));
        let small = image::open(cache.thumbnail_path(&hash, 300)).unwrap();
        assert_eq!((small.width(), small.height()), (300, 150));
        let large = image::open(cache.thumbnail_path(&hash, 1000)).unwrap();
        assert_eq!((large.width(), large.height()), (1000, 500));

        let tiny = png(200, 200);
        let tiny_hash = hash_bytes(&tiny);
        cache.store(&tiny_hash, &tiny).unwrap();
        let t = image::open(cache.thumbnail_path(&tiny_hash, 1000)).unwrap();
        assert_eq!((t.width(), t.height()), (200, 200));

        cache.remove(&hash);
        assert!(!cache.thumbnail_path(&hash, 300).exists());
    }

    #[test]
    fn garbage_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ArtCache::new(dir.path());
        assert!(cache.store("ab", b"not an image").is_err());
    }

    #[test]
    fn folder_image_priority() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(folder_image(dir.path()), None);
        fs::write(dir.path().join("Front.PNG"), b"x").unwrap();
        fs::write(dir.path().join("back.jpg"), b"x").unwrap();
        assert_eq!(folder_image(dir.path()), Some(dir.path().join("Front.PNG")));
        fs::write(dir.path().join("Folder.jpg"), b"x").unwrap();
        assert_eq!(
            folder_image(dir.path()),
            Some(dir.path().join("Folder.jpg"))
        );
        fs::write(dir.path().join("cover.jpeg"), b"x").unwrap();
        assert_eq!(
            folder_image(dir.path()),
            Some(dir.path().join("cover.jpeg"))
        );
    }
}
