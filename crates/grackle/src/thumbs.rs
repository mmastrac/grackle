//! **Renditions** (IO.md §4a, I12): the engine's one transform, its
//! content-addressed cache, and the `/static/` addresses it publishes at
//! (DESIGN.md §6b). Replaces `thumbnail.rb`.
//!
//! A rendition is another output of the same input, made to a **demand**: the
//! citing edge (`{% image cover.png width=256 %}`) carries the ask, this pass
//! runs the transform for each distinct ask, and an image's rendition set is
//! the union of its consumers' asks. There is no rendition config surface,
//! which is §9's q7 answered by what got built: no parameterized shell, no
//! transform stage upstream of `raw` — the demand is edge-carried and the
//! output carries what it was made with ([`Rendition`], on `Route.rendition`).
//!
//! §6b splits the two jobs `thumbnail.rb` conflated:
//!
//!   * a **build cache** — `_cache/thumbs/{hash}.{ext}`, gitignored, never
//!     shipped, keyed by content so it is self-invalidating and safe to delete;
//!   * a **published location** — `/static/{hash}.{ext}`, where the extension
//!     travels with the URL (no sniffing, no `.htaccess`) and the content hash
//!     makes `Cache-Control: immutable` correct by construction.
//!
//! **The hashing law, and where it is spent** (IO.md §4a). The address hashes
//! the *input bytes plus the transform parameters, never the output bytes*, so
//! it can be named at planning — before the transform runs, which is what lets
//! a page embed a rendition's URL without waiting for its content. This module
//! no longer spells that out for itself: it calls `strong::digest`/`strong::at`,
//! the same mint I11's strong addresses go through. The arithmetic is
//! unchanged, which is why every published thumbnail address is unchanged.
//!
//! Derived assets are exempt from URL parity (§11.12), so this scheme is free
//! to diverge from `_thumbs/{md5}`. The *transform* still matches the plugin:
//! strip metadata, fit within the ask's box (shrink only), ship the smallest of
//! {original, PNG, JPEG}.

use anyhow::{Context, Result};
use grackle_model::Rendition;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const JPEG_QUALITY: u8 = 85;

/// One demand: a source path as written in a citation, and the rendition it
/// asked for. The map's key, so two asks for one image are two entries and one
/// ask from two pages is one.
pub type Ask = (String, Rendition);

/// Every rendition one build materialized, by the demand that asked for it.
pub type Renditions = HashMap<Ask, Thumb>;

/// The engine's default rendition of one source, when this build made it.
///
/// The lookup every affordance that places a picture *without writing an ask*
/// goes through — heroes, cards, gallery members. `{% image %}` does not use
/// it: a tag carries its own ask, which is the whole point.
pub fn default_of<'a>(r: &'a Renditions, src: &str) -> Option<&'a Thumb> {
    r.get(&(src.to_string(), Rendition::THUMB))
}

/// A generated rendition: where it is cached, how it is published, and what it
/// was made with.
pub struct Thumb {
    /// Absolute path in the build cache (`_cache/thumbs/{hash}.{ext}`).
    pub cache_path: PathBuf,
    /// The published address — `/static/{hash}.{ext}`, no baseurl. This is the
    /// output's URL and the key it lands at, the same way `Row.url` carries no
    /// baseurl either (I11's recorded decision).
    pub address: String,
    /// Published URL, baseurl applied — what a citation actually writes.
    pub url: String,
    /// Pixel dimensions of the published variant (q26's dimension facts) —
    /// a header read, not a decode, so warm builds stay warm. None when the
    /// bytes passed through undecodable.
    pub dims: Option<(u32, u32)>,
    /// The parameters this output was made with — the value that lands on
    /// `Route.rendition` and lets a pull reproduce it.
    pub rendition: Rendition,
}

/// Run (or reuse cached) every demanded rendition, in parallel.
///
/// `asks` are as written in the citing edge — a root-relative path and the
/// rendition it asked for. The map is keyed by that same pair so the renderer
/// can look its own ask up. Asks that agree on both halves collapse to one
/// entry, and asks whose *bytes* and parameters agree collapse to one cache
/// entry and one address, because the hash is over content and parameters
/// rather than over paths.
pub fn generate(
    root: &Path,
    cache_dir: &Path,
    baseurl: &str,
    asks: &[Ask],
) -> Result<HashMap<Ask, Thumb>> {
    std::fs::create_dir_all(cache_dir)
        .with_context(|| format!("creating thumb cache {}", cache_dir.display()))?;

    // Pre-list the cache once: `{hash}` -> `{hash}.{ext}`. A warm build then
    // needs only to read + hash each source, never to decode or re-encode it.
    let mut existing: HashMap<String, String> = HashMap::new();
    if let Ok(rd) = std::fs::read_dir(cache_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            // An extensionless entry is its own hash: the contest's answer for
            // an input with no extension to normalize. It used to be indexed as
            // nothing at all, so it never warmed.
            let hash = name.split_once('.').map(|(h, _)| h).unwrap_or(&name);
            existing.insert(hash.to_string(), name.clone());
        }
    }

    let mut uniq: Vec<&Ask> = asks.iter().collect();
    uniq.sort();
    uniq.dedup();

    let pairs: Vec<(Ask, Thumb)> = uniq
        .par_iter()
        .map(|ask| -> Result<(Ask, Thumb)> {
            Ok((
                (*ask).clone(),
                one(root, cache_dir, baseurl, &ask.0, ask.1, &existing)?,
            ))
        })
        .collect::<Result<_>>()?;
    Ok(pairs.into_iter().collect())
}

fn one(
    root: &Path,
    cache_dir: &Path,
    baseurl: &str,
    src: &str,
    rendition: Rendition,
    existing: &HashMap<String, String>,
) -> Result<Thumb> {
    let source_path = root.join(src);
    let bytes = std::fs::read(&source_path)
        .with_context(|| format!("{{% image %}} source not found: {}", source_path.display()))?;

    // **The hashing law** (IO.md §4a): the key is the source bytes plus the
    // ask's parameters, and never the bytes this transform is about to
    // produce. A changed image, or a different ask, is simply a different key
    // — so cache entries are never stale (§6b) and the address is nameable
    // before the recipe runs. One mint, shared with I11's strong addresses.
    let variant = rendition.variant();
    let hash = grackle_source::strong::digest(&bytes, &variant);

    let make = |ext: &str, cache_path: PathBuf| {
        let address = grackle_source::strong::at(&hash, ext);
        Thumb {
            url: format!("{baseurl}{address}"),
            address,
            dims: image::image_dimensions(&cache_path).ok(),
            cache_path,
            rendition,
        }
    };

    // Warm hit: the recipe already ran for these exact bytes and this exact
    // ask. The cached filename carries the extension the contest picked, which
    // is the one part of the address the law cannot supply (see the module
    // doc) — so it is read back rather than recomputed.
    if let Some(fname) = existing.get(&hash) {
        let ext = fname.split_once('.').map(|(_, e)| e).unwrap_or_default();
        return Ok(make(ext, cache_dir.join(fname)));
    }

    let src_ext = source_path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let (out_bytes, ext) = render(&bytes, &src_ext, rendition)?;

    let fname = match ext.is_empty() {
        true => hash.clone(),
        false => format!("{hash}.{ext}"),
    };
    let cache_path = cache_dir.join(&fname);
    if !cache_path.exists() {
        // Two threads racing on identical content write identical bytes.
        std::fs::write(&cache_path, &out_bytes)
            .with_context(|| format!("writing thumb {}", cache_path.display()))?;
    }
    Ok(make(&ext, cache_path))
}

/// **The transform**, as a pure function of its inputs and its parameters —
/// which is the shape the hashing law needs it to have, and the shape that
/// lets a pull reproduce a rendition from `Route.inputs` + `Route.rendition`
/// alone.
///
/// The contest `thumbnail.rb` runs: the smallest of the original file, a
/// resized PNG, and a resized JPEG.
///
/// Two deliberate improvements over the plugin: GIFs are passed through
/// verbatim (re-encoding a handful of tiny legacy files risks losing
/// animation), and the JPEG variant is skipped for images with an alpha
/// channel (a smaller-but-flattened JPEG must never win over a transparent
/// PNG — a size-only contest would otherwise silently drop transparency).
/// Anything that fails to decode is also passed through unchanged.
///
/// The returned extension is the contest's answer, and so the one part of a
/// rendition's address that is a fact about the OUTPUT (module doc).
pub fn render(orig: &[u8], src_ext: &str, r: Rendition) -> Result<(Vec<u8>, String)> {
    if src_ext == "gif" {
        return Ok((orig.to_vec(), "gif".into()));
    }
    let Ok(img) = image::load_from_memory(orig) else {
        return Ok((orig.to_vec(), norm_ext(src_ext)));
    };
    // Shrink-only fit (ImageMagick `640x600>`): resize preserves aspect and
    // never upscales because we only call it when a dimension exceeds the box.
    // A width-only ask leaves height unconstrained, so only the width term can
    // trip.
    let (max_w, max_h) = (r.max_w, r.max_h.unwrap_or(u32::MAX));
    let fitted = if img.width() > max_w || img.height() > max_h {
        img.resize(max_w, max_h, image::imageops::FilterType::Lanczos3)
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
    enc.write_image(
        img.as_bytes(),
        img.width(),
        img.height(),
        img.color().into(),
    )?;
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
        let (out, ext) = render(&bytes, "gif", Rendition::THUMB).unwrap();
        assert_eq!((out, ext.as_str()), (bytes, "gif"));
    }

    #[test]
    fn undecodable_bytes_pass_through() {
        let bytes = b"not an image".to_vec();
        let (out, ext) = render(&bytes, "png", Rendition::THUMB).unwrap();
        assert_eq!((out, ext.as_str()), (b"not an image".to_vec(), "png"));
    }

    /// **The ask reaches the transform** (IO.md §4a: parameters come from
    /// demand). A width-only ask fits the width and leaves the height to the
    /// aspect ratio — which is what makes two asks two different outputs
    /// rather than two names for one.
    ///
    /// Mutation, red and restored: ignore `r` in `render` and use
    /// `Rendition::THUMB`'s box → the 256 ask produces a 640-wide image.
    #[test]
    fn a_width_ask_fits_the_width_and_leaves_the_height() {
        let (out, _) = render(&png_bytes(800, 800), "png", Rendition::width(256)).unwrap();
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!(img.width(), 256, "the ask decided the width");
        assert_eq!(img.height(), 256, "aspect preserved, height unconstrained");

        // A tall image the default box would have shrunk to 600 high is left
        // taller than that by a width-only ask: the height term is genuinely
        // absent rather than defaulted.
        let (out, _) = render(&png_bytes(400, 1600), "png", Rendition::width(256)).unwrap();
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!((img.width(), img.height()), (256, 1024));
    }

    #[test]
    fn large_image_is_shrunk_to_fit() {
        let (out, ext) = render(&png_bytes(800, 800), "png", Rendition::THUMB).unwrap();
        let img = image::load_from_memory(&out).unwrap();
        assert!(
            img.width() <= Rendition::THUMB.max_w
                && img.height() <= Rendition::THUMB.max_h.expect("the default box is closed"),
            "{}x{}",
            img.width(),
            img.height()
        );
        // Accepting either extension cannot catch the bug that matters: an
        // extension that does not describe the bytes it labels.
        let want = match ext.as_str() {
            "jpg" => image::ImageFormat::Jpeg,
            "png" => image::ImageFormat::Png,
            other => panic!("unexpected variant extension {other}"),
        };
        assert_eq!(image::guess_format(&out).unwrap(), want, "ext {ext} lies");
    }

    #[test]
    fn small_image_is_not_upscaled() {
        let (out, _) = render(&png_bytes(100, 80), "png", Rendition::THUMB).unwrap();
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!((img.width(), img.height()), (100, 80));
    }

    /// **The hashing law, pinned at the mint** (IO.md §4a): a rendition's
    /// address hashes the INPUT bytes plus the parameters, never the bytes the
    /// transform produced — so it is nameable at planning, before the recipe
    /// has run.
    ///
    /// Asserted the only way that is a measurement rather than a restatement:
    /// compute the address from the input alone, then run the transform and
    /// show that its output — different bytes, different length — did not
    /// change the digest.
    ///
    /// This states the law where the transform is; the mint that has to OBEY
    /// it is `one`, and that is pinned one level up — mutation, red and
    /// restored: hash the transform's output instead of `bytes` in `one` →
    /// `io_renditions::a_rendition_address_is_computable_before_the_transform_runs`
    /// fails, from a fixture that never runs the transform at all.
    #[test]
    fn the_address_is_the_input_and_the_parameters_not_the_output() {
        use grackle_source::strong;
        let input = png_bytes(800, 800);
        let ask = Rendition::THUMB;

        // Planning: the address, with no transform in sight.
        let planned = strong::digest(&input, &ask.variant());

        // Materialization: the transform runs and produces different bytes.
        let (out_bytes, ext) = render(&input, "png", ask).unwrap();
        assert_ne!(out_bytes, input, "the transform did something");
        assert_ne!(
            strong::digest(&out_bytes, &ask.variant()),
            planned,
            "…and hashing the output would have given a different address"
        );

        // The published address is the planned one, extension apart.
        assert_eq!(
            strong::at(&planned, &ext),
            format!("/static/{planned}.{ext}")
        );
        // And the ask is part of the key, so a second ask cannot land on it.
        assert_ne!(
            strong::digest(&input, &Rendition::width(256).variant()),
            planned
        );
    }
}
