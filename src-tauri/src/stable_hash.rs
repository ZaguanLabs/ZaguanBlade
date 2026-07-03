//! M6 CL1 — version-stable content hashing.
//!
//! std's `DefaultHasher` (SipHash) is NOT guaranteed stable across Rust toolchains,
//! so persisted fingerprints — the `indexed_files.file_hash` column, the M6.1
//! reconcile checkpoint, and the on-disk project-state filename — can spuriously
//! mismatch after a compiler upgrade and trigger needless reindexes or orphaned
//! state. xxh3 is a fixed algorithm with a well-defined, toolchain-independent
//! output, so these fingerprints stay valid across upgrades.
//!
//! Use these helpers ONLY for version-stable fingerprints / cache keys — never for
//! cryptographic purposes.

/// A version-stable 64-bit hash of `bytes` (xxh3).
pub fn stable_hash(bytes: &[u8]) -> u64 {
    xxhash_rust::xxh3::xxh3_64(bytes)
}

/// Hex form of [`stable_hash`] (`format!("{:x}", ...)`) — used where a `String`
/// fingerprint is persisted (e.g. `indexed_files.file_hash`).
pub fn stable_hash_hex(bytes: &[u8]) -> String {
    format!("{:x}", stable_hash(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_hash_is_deterministic_and_content_sensitive() {
        assert_eq!(stable_hash(b"hello world"), stable_hash(b"hello world"));
        assert_ne!(stable_hash(b"hello world"), stable_hash(b"hello worlds"));
        assert_eq!(stable_hash(b""), stable_hash(b""));
    }

    #[test]
    fn stable_hash_hex_matches_stable_hash() {
        assert_eq!(
            stable_hash_hex(b"some content"),
            format!("{:x}", stable_hash(b"some content"))
        );
    }
}
