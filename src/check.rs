//! Optional payload integrity trailers (Phase 2).
//!
//! These provide **error detection**, not authentication. A matching trailer
//! means the payload bytes survived the glyph channel unchanged (with high
//! probability). It is **not** a substitute for digital signatures.
//!
//! | Variant | Trailer size | Notes |
//! |---------|-------------:|-------|
//! | [`Integrity::None`] | 0 | No trailer |
//! | [`Integrity::Crc32`] | 4 | IEEE CRC-32, big-endian |
//! | [`Integrity::Blake3_128`] | 16 | First 16 bytes of BLAKE3-256 |
//! | [`Integrity::Blake3_256`] | 32 | Full BLAKE3 digest |

use crate::error::{GlyphixError, Result};

/// Integrity algorithm tagged in the version-2 frame and appended after the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Integrity {
    /// No trailer (still writes integrity tag `0` in v2 frames).
    #[default]
    None,
    /// IEEE CRC-32 over payload bytes (4-byte BE trailer).
    Crc32,
    /// BLAKE3 truncated to 128 bits (16-byte trailer).
    Blake3_128,
    /// Full BLAKE3-256 (32-byte trailer).
    Blake3_256,
}

impl Integrity {
    /// Wire tag stored in the frame (`0..=3`).
    pub fn tag(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Crc32 => 1,
            Self::Blake3_128 => 2,
            Self::Blake3_256 => 3,
        }
    }

    /// Parse a wire tag from the frame.
    pub fn from_tag(tag: u8) -> Result<Self> {
        match tag {
            0 => Ok(Self::None),
            1 => Ok(Self::Crc32),
            2 => Ok(Self::Blake3_128),
            3 => Ok(Self::Blake3_256),
            other => Err(GlyphixError::UnknownIntegrity { tag: other }),
        }
    }

    /// Trailer length in bytes.
    pub fn trailer_len(self) -> usize {
        match self {
            Self::None => 0,
            Self::Crc32 => 4,
            Self::Blake3_128 => 16,
            Self::Blake3_256 => 32,
        }
    }

    /// Short name for CLI / logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Crc32 => "crc32",
            Self::Blake3_128 => "blake3-128",
            Self::Blake3_256 => "blake3-256",
        }
    }

    /// Parse from a CLI-style name.
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "none" | "off" | "0" => Ok(Self::None),
            "crc32" | "crc" => Ok(Self::Crc32),
            "blake3-128" | "blake3_128" | "b3-128" => Ok(Self::Blake3_128),
            "blake3-256" | "blake3" | "blake3_256" | "b3-256" | "b3" => Ok(Self::Blake3_256),
            other => Err(GlyphixError::InvalidProfile(format!(
                "unknown integrity `{other}`; try none, crc32, blake3-128, blake3-256"
            ))),
        }
    }

    /// Compute the trailer for `payload` (empty for [`Integrity::None`]).
    pub fn compute(self, payload: &[u8]) -> Vec<u8> {
        match self {
            Self::None => Vec::new(),
            Self::Crc32 => {
                let c = crc32fast::hash(payload);
                c.to_be_bytes().to_vec()
            }
            Self::Blake3_128 => {
                let hash = blake3::hash(payload);
                hash.as_bytes()[..16].to_vec()
            }
            Self::Blake3_256 => blake3::hash(payload).as_bytes().to_vec(),
        }
    }

    /// Verify `trailer` against `payload`.
    pub fn verify(self, payload: &[u8], trailer: &[u8]) -> Result<()> {
        let expected = self.compute(payload);
        if trailer.len() != expected.len() {
            return Err(GlyphixError::IntegrityMismatch {
                kind: self.as_str(),
            });
        }
        if trailer != expected.as_slice() {
            return Err(GlyphixError::IntegrityMismatch {
                kind: self.as_str(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_stable_empty() {
        // Known IEEE CRC-32 of empty message is 0.
        assert_eq!(Integrity::Crc32.compute(b""), [0, 0, 0, 0]);
    }

    #[test]
    fn crc32_known_vector() {
        // "123456789" → 0xCBF43926 (common CRC-32 check value)
        let t = Integrity::Crc32.compute(b"123456789");
        assert_eq!(t, [0xCB, 0xF4, 0x39, 0x26]);
    }

    #[test]
    fn blake3_lengths() {
        assert_eq!(Integrity::Blake3_128.compute(b"x").len(), 16);
        assert_eq!(Integrity::Blake3_256.compute(b"x").len(), 32);
        assert_eq!(Integrity::None.compute(b"x").len(), 0);
    }

    #[test]
    fn verify_rejects_flip() {
        let payload = b"hello";
        let mut t = Integrity::Crc32.compute(payload);
        t[0] ^= 1;
        assert!(matches!(
            Integrity::Crc32.verify(payload, &t),
            Err(GlyphixError::IntegrityMismatch { .. })
        ));
    }

    #[test]
    fn parse_names() {
        assert_eq!(Integrity::parse("crc32").unwrap(), Integrity::Crc32);
        assert_eq!(Integrity::parse("blake3").unwrap(), Integrity::Blake3_256);
        assert_eq!(Integrity::parse("none").unwrap(), Integrity::None);
    }
}
