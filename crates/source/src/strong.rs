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
//! standing for the transform's parameters. **I12 unified the second mint into
//! it**: `thumbs.rs` had been computing `blake3(image bytes + variant)[..32]`
//! and formatting `/static/…` independently — the same law, obeyed twice, with
//! `/static/` hardcoded in two places — and now calls [`digest`] and [`at`].
//! One prefix, one digest, one address shape, and the thumbnail addresses do
//! not move because the arithmetic was already identical.
//!
//! **The one part of a rendition address that is NOT computable at planning is
//! its extension**, and that is measured rather than glossed: the thumbnail
//! transform picks the smallest of {original, PNG, JPEG}, so which extension
//! travels with the URL is a fact about the *output*. The hash — the part the
//! law is about, the part that makes the address immutable — is a pure
//! function of the inputs and the parameters, which is why [`at`] takes a
//! digest and an extension separately: the digest is knowable before the
//! transform runs, and `at(digest, "")` is the address a planner can name.
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
/// of it can never collide at one address — `Rendition::variant` (I12) mints
/// the rest of this namespace.
pub const IDENTITY: &str = "identity-v1";

/// The published prefix for hash-addressed outputs. Not configurable: it is
/// one directory the engine owns, and the thumbnail cache has published under
/// it since §6b.
pub const PREFIX: &str = "/static";

/// **The law, as a function**: the digest of `bytes` under `variant`, and of
/// nothing else.
///
/// Truncated to 32 hex characters, which is the length every address in the
/// store has had since §6b. This is the whole of what a content address knows,
/// and it is computable the moment the inputs and the parameters are — before
/// any transform runs, which is what IO.md §1's "facts at planning" needs.
pub fn digest(bytes: &[u8], variant: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(bytes);
    hasher.update(variant.as_bytes());
    hasher.finalize().to_hex().as_str()[..32].to_string()
}

/// Where a digest publishes, with `ext` travelling on the URL (§6b:
/// self-describing, no sniffing, no `.htaccess`).
///
/// The one place [`PREFIX`] is spent, so `/static/` is one string in the
/// engine rather than one per mint.
pub fn at(digest: &str, ext: &str) -> String {
    match ext.is_empty() {
        true => format!("{PREFIX}/{digest}"),
        false => format!("{PREFIX}/{digest}.{}", ext.to_ascii_lowercase()),
    }
}

/// The strong address of `bytes` under `variant` — [`digest`] and [`at`],
/// which is the shape every mint in the engine goes through.
pub fn address(bytes: &[u8], variant: &str, ext: &str) -> String {
    at(&digest(bytes, variant), ext)
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
