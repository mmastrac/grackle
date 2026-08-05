//! Strong addresses: hash URLs from input bytes + transform params, not output.

/// Variant for untransformed embeds; distinct from rendition variants.
pub const IDENTITY: &str = "identity-v1";

/// Fixed `/static` prefix for hash-addressed outputs.
pub const PREFIX: &str = "/static";

/// Blake3(bytes + variant), truncated to 32 hex chars. Computable at planning time.
pub fn digest(bytes: &[u8], variant: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(bytes);
    hasher.update(variant.as_bytes());
    hasher.finalize().to_hex().as_str()[..32].to_string()
}

/// Format digest as `/static/{digest}[.{ext}]`. Self-describing ext (§6b).
pub fn at(digest: &str, ext: &str) -> String {
    match ext.is_empty() {
        true => format!("{PREFIX}/{digest}"),
        false => format!("{PREFIX}/{digest}.{}", ext.to_ascii_lowercase()),
    }
}

pub fn address(bytes: &[u8], variant: &str, ext: &str) -> String {
    at(&digest(bytes, variant), ext)
}

pub fn ext_of(path: &std::path::Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The law, as an assertion rather than as prose: the address is a
    /// function of (bytes, variant) and of nothing else — not of a path, not
    /// of any output. Two different files holding one byte string are ONE
    /// address, which is the untransformed-twin rule and the dedupe claim at
    /// once.
    #[test]
    fn the_address_is_the_bytes_and_the_parameters() {
        let a = address(b"same bytes", IDENTITY, "png");
        let b = address(b"same bytes", IDENTITY, "png");
        assert_eq!(a, b);
        assert!(a.starts_with("/static/"), "{a}");
        // The parameters are part of the key, so a rendition of these bytes
        // can never land on top of the original.
        assert_ne!(
            a,
            address(b"same bytes", "fit640x600-jpg85-pngbest-v1", "png")
        );
        // …and the bytes are, so a changed input is simply a different key.
        assert_ne!(a, address(b"other bytes", IDENTITY, "png"));
    }

    /// The extension travels with the URL and is normalized, so
    /// `after-theme-hack.PNG` and `a.png` do not mint two spellings of one
    /// address.
    #[test]
    fn the_extension_travels_and_is_normalized() {
        assert!(address(b"x", IDENTITY, "PNG").ends_with(".png"));
        assert_eq!(
            address(b"x", IDENTITY, "png"),
            address(b"x", IDENTITY, "PNG")
        );
        assert_eq!(
            address(b"x", IDENTITY, "").len(),
            "/static/".len() + 32,
            "an extensionless input gets a bare hash"
        );
    }
}
