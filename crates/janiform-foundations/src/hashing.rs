const FNV1A_64_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A_64_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Incremental FNV-1a 64-bit hasher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fnv1a64Hasher {
    hash: u64,
}

impl Fnv1a64Hasher {
    /// Create a hasher initialized with the FNV-1a 64-bit offset basis.
    pub const fn new() -> Self {
        Self {
            hash: FNV1A_64_OFFSET_BASIS,
        }
    }

    /// Add bytes to the hash state.
    pub fn update(&mut self, payload: &[u8]) {
        for byte in payload {
            self.hash ^= u64::from(*byte);
            self.hash = self.hash.wrapping_mul(FNV1A_64_PRIME);
        }
    }

    /// Return the current 64-bit hash value.
    pub const fn finish(self) -> u64 {
        self.hash
    }

    /// Return the current hash value encoded as fixed-width lowercase hex.
    pub fn finish_hex_lowercase(self) -> String {
        format!("{:016x}", self.finish())
    }
}

impl Default for Fnv1a64Hasher {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute a FNV-1a 64-bit hash for a byte payload.
pub fn fnv1a_64(payload: &[u8]) -> u64 {
    let mut hasher = Fnv1a64Hasher::new();
    hasher.update(payload);
    hasher.finish()
}

/// Compute a FNV-1a 64-bit hash encoded as fixed-width lowercase hex.
pub fn fnv1a_64_hex_lowercase(payload: &[u8]) -> String {
    let mut hasher = Fnv1a64Hasher::new();
    hasher.update(payload);
    hasher.finish_hex_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{Fnv1a64Hasher, fnv1a_64, fnv1a_64_hex_lowercase};

    #[test]
    fn fnv1a_64_matches_known_empty_string_vector() {
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64_hex_lowercase(b""), "cbf29ce484222325");
    }

    #[test]
    fn fnv1a_64_matches_known_ascii_vector() {
        assert_eq!(fnv1a_64(b"hello"), 0xa430_d846_80aa_bd0b);
        assert_eq!(fnv1a_64_hex_lowercase(b"hello"), "a430d84680aabd0b");
    }

    #[test]
    fn fnv1a_64_streaming_matches_one_shot_hash() {
        let mut hasher = Fnv1a64Hasher::new();
        hasher.update(b"he");
        hasher.update(b"llo");

        assert_eq!(hasher.finish(), fnv1a_64(b"hello"));
    }
}
