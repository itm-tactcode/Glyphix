//! Reed–Solomon error correction over payload bytes (Phase 5).
//!
//! # Placement in the pipeline
//!
//! ```text
//! payload → [optional RS expand] → framed body → integrity trailer → paint glyphs
//! ```
//!
//! ECC is applied **before** glyphs are painted. It trades payload capacity for
//! the ability to **correct** symbol errors (damaged cells), not only detect them
//! (Phase 2 CRC/BLAKE3).
//!
//! # Tradeoff
//!
//! | | Without ECC | With e.g. 10% RS |
//! |--|-------------|-------------|
//! | Capacity | higher | lower (parity eats glyphs) |
//! | Bit/cell flips | may yield wrong bytes or integrity error | often **recoverable** |
//! | Camera / perspective | still not solved | still need finder patterns (Phase 6) |
//!
//! # Block format (GF(2⁸))
//!
//! The `reed-solomon` crate uses classical RS: each block is
//! `message ‖ parity` with `message.len() + parity_len ≤ 255`.
//! We split the payload into messages of size `255 - ecc_len` and append
//! `ecc_len` parity bytes per block. `ecc_len` comes from a **parity percent**
//! of a full 255-byte codeword (minimum 2).

use reed_solomon::{Decoder, Encoder};

use crate::error::{GlyphixError, Result};

/// Error-correction scheme for the framed body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Ecc {
    /// No ECC (default). Framed body is the raw payload.
    #[default]
    None,
    /// Reed–Solomon over GF(2⁸).
    ///
    /// `parity_percent` is the approximate fraction of each 255-byte codeword
    /// spent on parity (1–50). Example: `10` ≈ 10% ECC rate from the useful prompt.
    ReedSolomon {
        /// Percent of codeword that is parity (1..=50).
        parity_percent: u8,
    },
}

impl Ecc {
    /// RS with the given parity percent (clamped to 1..=50).
    pub fn reed_solomon(parity_percent: u8) -> Self {
        let p = parity_percent.clamp(1, 50);
        Self::ReedSolomon { parity_percent: p }
    }

    /// Common 10% rate preset.
    pub fn rs10() -> Self {
        Self::reed_solomon(10)
    }

    /// Common 20% rate preset.
    pub fn rs20() -> Self {
        Self::reed_solomon(20)
    }

    /// Wire tag for v3 frames: `0` = none, `1..=50` = RS parity percent.
    pub fn tag(self) -> u8 {
        match self {
            Self::None => 0,
            Self::ReedSolomon { parity_percent } => parity_percent.clamp(1, 50),
        }
    }

    /// Parse a v3 ECC tag.
    pub fn from_tag(tag: u8) -> Result<Self> {
        match tag {
            0 => Ok(Self::None),
            1..=50 => Ok(Self::ReedSolomon {
                parity_percent: tag,
            }),
            other => Err(GlyphixError::UnknownEcc { tag: other }),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ReedSolomon { parity_percent } => match parity_percent {
                10 => "rs10",
                20 => "rs20",
                _ => "rs",
            },
        }
    }

    /// Parse CLI names: `none`, `rs10`, `rs20`, `rs:15`, `10%`, etc.
    pub fn parse(s: &str) -> Result<Self> {
        let t = s.trim().to_ascii_lowercase();
        match t.as_str() {
            "none" | "off" | "0" => Ok(Self::None),
            "rs10" | "rs-10" | "10%" | "10" => Ok(Self::rs10()),
            "rs20" | "rs-20" | "20%" | "20" => Ok(Self::rs20()),
            other if other.starts_with("rs:") || other.starts_with("rs=") => {
                let n: u8 = other[3..]
                    .parse()
                    .map_err(|_| GlyphixError::InvalidProfile(format!("bad ecc `{s}`")))?;
                Ok(Self::reed_solomon(n))
            }
            other if other.ends_with('%') => {
                let n: u8 = other[..other.len() - 1]
                    .parse()
                    .map_err(|_| GlyphixError::InvalidProfile(format!("bad ecc `{s}`")))?;
                Ok(Self::reed_solomon(n))
            }
            other => Err(GlyphixError::InvalidProfile(format!(
                "unknown ecc `{other}`; try none, rs10, rs20, rs:15"
            ))),
        }
    }

    /// Parity symbols per RS block (`0` if none).
    pub fn ecc_len(self) -> usize {
        match self {
            Self::None => 0,
            Self::ReedSolomon { parity_percent } => {
                let e = (255usize * parity_percent as usize).div_ceil(100);
                e.clamp(2, 128)
            }
        }
    }

    /// Max message bytes per block.
    pub fn max_message_len(self) -> usize {
        match self {
            Self::None => usize::MAX,
            Self::ReedSolomon { .. } => 255 - self.ecc_len(),
        }
    }

    /// Length of the coded body for a given original payload length.
    pub fn coded_len(self, payload_len: usize) -> usize {
        match self {
            Self::None => payload_len,
            Self::ReedSolomon { .. } => {
                if payload_len == 0 {
                    return 0;
                }
                let max_msg = self.max_message_len();
                let blocks = payload_len.div_ceil(max_msg);
                payload_len + blocks * self.ecc_len()
            }
        }
    }

    /// Max original payload bytes that fit in `body_budget` coded body bytes.
    pub fn max_payload_for_body(self, body_budget: usize) -> usize {
        match self {
            Self::None => body_budget,
            Self::ReedSolomon { .. } => {
                let e = self.ecc_len();
                let m = self.max_message_len();
                if e == 0 || m == 0 {
                    return 0;
                }
                // Binary search payload length
                let mut lo = 0usize;
                let mut hi = body_budget; // coded >= payload
                while lo < hi {
                    let mid = (lo + hi + 1) / 2;
                    if self.coded_len(mid) <= body_budget {
                        lo = mid;
                    } else {
                        hi = mid - 1;
                    }
                }
                lo
            }
        }
    }

    /// Expand payload to coded body (message‖parity blocks).
    pub fn encode_body(self, payload: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::None => Ok(payload.to_vec()),
            Self::ReedSolomon { .. } => {
                if payload.is_empty() {
                    return Ok(Vec::new());
                }
                let ecc_len = self.ecc_len();
                let max_msg = self.max_message_len();
                let enc = Encoder::new(ecc_len);
                let mut out = Vec::with_capacity(self.coded_len(payload.len()));
                for chunk in payload.chunks(max_msg) {
                    let encoded = enc.encode(chunk);
                    // encoded is Buffer: data + ecc, Deref to [u8]
                    out.extend_from_slice(&encoded[..]);
                }
                Ok(out)
            }
        }
    }

    /// Recover original payload of length `payload_len` from coded body.
    pub fn decode_body(self, coded: &[u8], payload_len: usize) -> Result<Vec<u8>> {
        match self {
            Self::None => {
                if coded.len() < payload_len {
                    return Err(GlyphixError::TruncatedStream {
                        needed: payload_len * 8,
                        available: coded.len() * 8,
                    });
                }
                Ok(coded[..payload_len].to_vec())
            }
            Self::ReedSolomon { .. } => {
                if payload_len == 0 {
                    if !coded.is_empty() {
                        return Err(GlyphixError::EccDecodeFailed(
                            "empty payload but non-empty coded body".into(),
                        ));
                    }
                    return Ok(Vec::new());
                }
                let expected = self.coded_len(payload_len);
                if coded.len() != expected {
                    return Err(GlyphixError::EccDecodeFailed(format!(
                        "coded body length {} != expected {expected} for payload_len={payload_len}"
                        ,
                        coded.len()
                    )));
                }
                let ecc_len = self.ecc_len();
                let max_msg = self.max_message_len();
                let dec = Decoder::new(ecc_len);
                let mut out = Vec::with_capacity(payload_len);
                let mut offset = 0;
                let mut remaining = payload_len;
                while remaining > 0 {
                    let msg_len = remaining.min(max_msg);
                    let block_len = msg_len + ecc_len;
                    if offset + block_len > coded.len() {
                        return Err(GlyphixError::EccDecodeFailed(
                            "truncated RS block".into(),
                        ));
                    }
                    let mut block = coded[offset..offset + block_len].to_vec();
                    let recovered = dec.correct(&mut block, None).map_err(|e| {
                        GlyphixError::EccDecodeFailed(format!("RS correct failed: {e:?}"))
                    })?;
                    let data = recovered.data();
                    if data.len() != msg_len {
                        return Err(GlyphixError::EccDecodeFailed(format!(
                            "RS data len {} != {msg_len}",
                            data.len()
                        )));
                    }
                    out.extend_from_slice(data);
                    offset += block_len;
                    remaining -= msg_len;
                }
                Ok(out)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rs_roundtrip_and_correct() {
        let ecc = Ecc::rs10();
        let payload: Vec<u8> = (0..200).map(|i| i as u8).collect();
        let mut coded = ecc.encode_body(&payload).unwrap();
        assert!(coded.len() > payload.len());
        // Corrupt a few symbols in the first block
        coded[0] ^= 0xFF;
        coded[3] ^= 0x55;
        coded[7] ^= 0x0F;
        let recovered = ecc.decode_body(&coded, payload.len()).unwrap();
        assert_eq!(recovered, payload);
    }

    #[test]
    fn none_is_identity() {
        let p = b"hello";
        assert_eq!(Ecc::None.encode_body(p).unwrap(), p);
        assert_eq!(Ecc::None.decode_body(p, 5).unwrap(), p);
    }

    #[test]
    fn coded_len_matches_encode() {
        let ecc = Ecc::rs20();
        for n in [0usize, 1, 10, 229, 230, 500] {
            let data = vec![0xABu8; n];
            let coded = ecc.encode_body(&data).unwrap();
            assert_eq!(coded.len(), ecc.coded_len(n), "n={n}");
        }
    }

    #[test]
    fn max_payload_for_body_fits() {
        let ecc = Ecc::rs10();
        let budget = 500;
        let n = ecc.max_payload_for_body(budget);
        assert!(ecc.coded_len(n) <= budget);
        if n + 1 > 0 {
            assert!(ecc.coded_len(n + 1) > budget || n == budget);
        }
    }

    #[test]
    fn parse_names() {
        assert_eq!(Ecc::parse("rs10").unwrap(), Ecc::rs10());
        assert_eq!(Ecc::parse("none").unwrap(), Ecc::None);
        assert_eq!(Ecc::parse("rs:12").unwrap().tag(), 12);
    }
}
