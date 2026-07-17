//! Derived-image thumbnails: a content-addressed cache + published `/static/`
//! URLs (DESIGN.md §6b). Replaces `thumbnail.rb`.
//!
//! That plugin keyed on MD5 of the source bytes, wrote an extensionless
//! `_thumbs/{md5}-600-600`, and needed a `.htaccess` to give the blob a
//! Content-Type and a cache header. §6b splits the two jobs the plugin
//! conflated:
//!
//!   * a **build cache** — `_cache/thumbs/{hash}.{ext}`, gitignored, never
//!     shipped, keyed by content so it is self-invalidating and safe to delete;
//!   * a **published location** — `/static/{hash}.{ext}`, where the extension
//!     travels with the URL (no sniffing, no `.htaccess`) and the content hash
//!     makes `Cache-Control: immutable` correct by construction.
//!
//! Derived assets are exempt from URL parity (§11.12), so this scheme is free
//! to diverge from `_thumbs/{md5}`. The *transform* still matches the plugin:
//! strip metadata, fit within 640×600 (shrink only), ship the smallest of
//! {original, PNG, JPEG}.

use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const MAX_W: u32 = 640;
const MAX_H: u32 = 600;
const JPEG_QUALITY: u8 = 85;
/// Part of the content key: bump to invalidate every cached thumbnail at once.
const VARIANT: &str = "fit640x600-jpg85-pngbest-v1";

/// A generated thumbnail: where it is cached, and how it is published.
pub struct Thumb {
    /// Absolute path in the build cache (`_cache/thumbs/{hash}.{ext}`).
    pub cache_path: PathBuf,
    /// Published URL, baseurl applied (`/static/{hash}.{ext}`).
    pub url: String,
    /// Output-relative path to write (`static/{hash}.{ext}`).
    pub rel: String,
}

/// Generate (or reuse cached) thumbnails for every source, in parallel.
///
/// `sources` are as written in `{% image %}` — root-relative paths. The map is
/// keyed by that same string so the renderer can look each one up. Sources with
/// identical bytes collapse to one cache entry, because the hash is over
/// content, not path.
pub fn generate(
    root: &Path,
    cache_dir: &Path,
    baseurl: &str,
    sources: &[String],
) -> Result<HashMap<String, Thumb>> {
    std::fs::create_dir_all(cache_dir)
        .with_context(|| format!("creating thumb cache {}", cache_dir.display()))?;

    // Pre-list the cache once: `{hash}` -> `{hash}.{ext}`. A warm build then
    // needs only to read + hash each source, never to decode or re-encode it.
    let mut existing: HashMap<String, String> = HashMap::new();
    if let Ok(rd) = std::fs::read_dir(cache_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some((h, _)) = name.split_once('.') {
                existing.insert(h.to_string(), name);
            }
        }
    }

    let mut uniq: Vec<&String> = sources.iter().collect();
    uniq.sort();
    uniq.dedup();

    let pairs: Vec<(String, Thumb)> = uniq
        .par_iter()
        .map(|src| -> Result<(String, Thumb)> {
            Ok(((*src).clone(), one(root, cache_dir, baseurl, src, &existing)?))
        })
        .collect::<Result<_>>()?;
    Ok(pairs.into_iter().collect())
}

fn one(
    root: &Path,
    cache_dir: &Path,
    baseurl: &str,
    src: &str,
    existing: &HashMap<String, String>,
) -> Result<Thumb> {
    let source_path = root.join(src);
    let bytes = std::fs::read(&source_path)
        .with_context(|| format!("{{% image %}} source not found: {}", source_path.display()))?;

    // Content key: source bytes + the variant recipe. A changed image or recipe
    // is simply a different key, so entries are never stale (§6b).
    let mut hasher = blake3::Hasher::new();
    hasher.update(&bytes);
    hasher.update(VARIANT.as_bytes());
    let hex = hasher.finalize().to_hex();
    let hash = &hex.as_str()[..32];

    let make = |rel: String, cache_path: PathBuf| Thumb {
        url: format!("{baseurl}/{rel}"),
        rel,
        cache_path,
    };

    // Warm hit: the recipe already ran for these exact bytes.
    if let Some(fname) = existing.get(hash) {
        return Ok(make(format!("static/{fname}"), cache_dir.join(fname)));
    }

    let src_ext = source_path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let (out_bytes, ext) = best_variant(&bytes, &src_ext)?;

    let fname = format!("{hash}.{ext}");
    let cache_path = cache_dir.join(&fname);
    if !cache_path.exists() {
        // Two threads racing on identical content write identical bytes.
        std::fs::write(&cache_path, &out_bytes)
            .with_context(|| format!("writing thumb {}", cache_path.display()))?;
    }
    Ok(make(format!("static/{fname}"), cache_path))
}

/// The contest `thumbnail.rb` runs: the smallest of the original file, a
/// resized PNG, and a resized JPEG.
///
/// Two deliberate improvements over the plugin: GIFs are passed through
/// verbatim (re-encoding a handful of tiny legacy files risks losing
/// animation), and the JPEG variant is skipped for images with an alpha
/// channel (a smaller-but-flattened JPEG must never win over a transparent
/// PNG — a size-only contest would otherwise silently drop transparency).
/// Anything that fails to decode is also passed through unchanged.
fn best_variant(orig: &[u8], src_ext: &str) -> Result<(Vec<u8>, String)> {
    if src_ext == "gif" {
        return Ok((orig.to_vec(), "gif".into()));
    }
    let Ok(img) = image::load_from_memory(orig) else {
        return Ok((orig.to_vec(), norm_ext(src_ext)));
    };
    // Shrink-only fit (ImageMagick `640x600>`): resize preserves aspect and
    // never upscales because we only call it when a dimension exceeds the box.
    let fitted = if img.width() > MAX_W || img.height() > MAX_H {
        img.resize(MAX_W, MAX_H, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };

    let mut best = (orig.to_vec(), norm_ext(src_ext));
    let png = encode_png(&fitted)?;
    if png.len() < best.0.len() {
        best = (png, "png".into());
    }
    if !fitted.color().has_alpha() {
        let jpg = encode_jpg(&fitted)?;
        if jpg.len() < best.0.len() {
            best = (jpg, "jpg".into());
        }
    }
    Ok(best)
}

/// `jpeg` and `jpg` are the same format; publish one spelling.
fn norm_ext(ext: &str) -> String {
    match ext {
        "jpeg" => "jpg".into(),
        other => other.into(),
    }
}

fn encode_png(img: &image::DynamicImage) -> Result<Vec<u8>> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::ImageEncoder;
    let mut buf = Vec::new();
    let enc = PngEncoder::new_with_quality(&mut buf, CompressionType::Best, FilterType::Adaptive);
    enc.write_image(img.as_bytes(), img.width(), img.height(), img.color().into())?;
    Ok(buf)
}

fn encode_jpg(img: &image::DynamicImage) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY);
    enc.encode_image(&img.to_rgb8())?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let mut img = image::RgbImage::new(w, h);
        for (i, px) in img.pixels_mut().enumerate() {
            *px = image::Rgb([(i % 251) as u8, ((i / 7) % 251) as u8, 0]);
        }
        let dy = image::DynamicImage::ImageRgb8(img);
        let mut buf = Vec::new();
        dy.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    #[test]
    fn norm_ext_folds_jpeg() {
        assert_eq!(norm_ext("jpeg"), "jpg");
        assert_eq!(norm_ext("png"), "png");
    }

    #[test]
    fn gif_is_passed_through_verbatim() {
        let bytes = b"GIF89a untouched".to_vec();
        let (out, ext) = best_variant(&bytes, "gif").unwrap();
        assert_eq!((out, ext.as_str()), (bytes, "gif"));
    }

    #[test]
    fn undecodable_bytes_pass_through() {
        let bytes = b"not an image".to_vec();
        let (out, ext) = best_variant(&bytes, "png").unwrap();
        assert_eq!((out, ext.as_str()), (b"not an image".to_vec(), "png"));
    }

    #[test]
    fn large_image_is_shrunk_to_fit() {
        let (out, ext) = best_variant(&png_bytes(800, 800), "png").unwrap();
        let img = image::load_from_memory(&out).unwrap();
        assert!(img.width() <= MAX_W && img.height() <= MAX_H, "{}x{}", img.width(), img.height());
        assert!(ext == "png" || ext == "jpg", "{ext}");
    }

    #[test]
    fn small_image_is_not_upscaled() {
        let (out, _) = best_variant(&png_bytes(100, 80), "png").unwrap();
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!((img.width(), img.height()), (100, 80));
    }
}
