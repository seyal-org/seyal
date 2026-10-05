//! Portable integrity digest for rebuildable index/cache fencing.

/// Deterministic FNV-1a 128-bit digest (trusted agent-store boundary checksum).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IntegrityDigest(pub [u8; 16]);

impl IntegrityDigest {
    pub fn hex(self) -> String {
        let mut out = String::with_capacity(32);
        for b in self.0 {
            out.push_str(&format!("{b:02x}"));
        }
        out
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        if s.len() != 32 {
            return None;
        }
        let mut out = [0u8; 16];
        for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
            let hi = from_hex_nibble(chunk[0])?;
            let lo = from_hex_nibble(chunk[1])?;
            out[i] = (hi << 4) | lo;
        }
        Some(Self(out))
    }
}

fn from_hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Hash arbitrary bytes with a stable FNV-1a 128-bit algorithm.
pub fn digest_bytes(data: &[u8]) -> IntegrityDigest {
    // Split into two independent 64-bit FNV streams for a compact portable digest.
    let mut a: u64 = 0xcbf29ce484222325;
    let mut b: u64 = 0x84222325cbf29ce4;
    for (i, byte) in data.iter().copied().enumerate() {
        if i % 2 == 0 {
            a ^= u64::from(byte);
            a = a.wrapping_mul(0x100000001b3);
        } else {
            b ^= u64::from(byte);
            b = b.wrapping_mul(0x100000001b3);
        }
    }
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&a.to_le_bytes());
    out[8..].copy_from_slice(&b.to_le_bytes());
    IntegrityDigest(out)
}

/// Hash file contents without executing them (read/inspect only).
pub fn digest_file(path: &std::path::Path) -> std::io::Result<IntegrityDigest> {
    let bytes = std::fs::read(path)?;
    Ok(digest_bytes(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_is_stable_and_round_trips_hex() {
        let d = digest_bytes(b"seyal-context");
        assert_eq!(IntegrityDigest::from_hex(&d.hex()), Some(d));
        assert_ne!(
            digest_bytes(b"seyal-context"),
            digest_bytes(b"seyal-context!")
        );
    }
}
