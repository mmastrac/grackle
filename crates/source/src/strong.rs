//! Strong addresses: the content store made public (IO.md §4a, I11).
//!
//! An output has two address slots. Its `url` is the **canonical** one — where
//! a rule said it lands, what an authored link resolves to and what
//! `rel=canonical` names. Its `strong_url` is the **hash** address, present
//! when the embed policy published it: `/static/{hash}.{ext}`, immutable by
//! construction, and shared by every input with the same bytes.
//!
//! # The hashing law
//!
//! > A content-hashed URL hashes the **inputs plus the transform parameters,
//! > never the output bytes.**
//!
//! It is not a preference; it is what keeps IO.md §1's split honest. An
//! output's *facts* — its url among them — exist the moment routes are
//! planned, before anything renders, so an address computed from what a
//! transform PRODUCED could not exist until after the transform ran, and
//! "facts at planning" would be false for exactly the outputs this module
//! addresses. Hashing the inputs and the recipe gives the same immutability
//! (different bytes in ⇒ different address out, because a transform is a
//! function) at planning time.
//!
//! [`address`] is the one place the law is spent, and it takes the two things
//! the law names and nothing else: the input bytes, and a `variant` string
//! standing for the transform's parameters. The thumbnail cache
//! (`thumbs.rs`, `blake3(image bytes + variant)`) obeys the same law and is
//! deliberately NOT routed through here yet — its variant, its extension
//! choice and its cache layout are I12's to unify, and the addresses it mints
//! today must not move.
//!
//! # Untransformed twins
//!
//! [`IDENTITY`] is the parameter set of the transform that does nothing, so
//! an untransformed embed's address is a pure function of the input bytes.
//! Two inputs holding the same bytes therefore land at one address and one
//! store entry, with no dedupe pass — and when one of those inputs is also a
//! routed output, that address *is* that output's strong URL. The twin rule
//! is arithmetic, not machinery.

/// The transform that does nothing: the parameter set an untransformed embed
/// spends, and what the `{hash}` route token means.
///
/// Distinct from any rendition's variant so that an original and a derivative
/// of it can never collide at one address — `thumbs::VARIANT` is the first
/// other member of this namespace.
pub const IDENTITY: &str = "identity-v1";

/// The published prefix for hash-addressed outputs. Not configurable: it is
/// one directory the engine owns, and the thumbnail cache has published under
/// it since §6b.
pub const PREFIX: &str = "/static";

/// The strong address of `bytes` under `variant`, with `ext` travelling on the
/// URL (§6b: self-describing, no sniffing, no `.htaccess`).
///
/// Inputs plus parameters — see the module doc. The digest is truncated to the
/// same 32 hex characters `thumbs.rs` uses, so the two mints produce one URL
/// shape and a reader meets one convention.
pub fn address(bytes: &[u8], variant: &str, ext: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(bytes);
    hasher.update(variant.as_bytes());
    let hex = hasher.finalize().to_hex();
    let hash = &hex.as_str()[..32];
    match ext.is_empty() {
        true => format!("{PREFIX}/{hash}"),
        false => format!("{PREFIX}/{hash}.{}", ext.to_ascii_lowercase()),
    }
}

/// The `ext` [`address`] wants, read off a path: lowercased, no dot, empty for
/// an extensionless file.
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
    /// address (IO.md I7a's case-insensitive globs, one layer down).
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
