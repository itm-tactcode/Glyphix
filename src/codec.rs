//! Framed encode / decode: bytes ↔ sequence of [`Grid`]s.
//!
//! # Framing
//!
//! ## Version 2 (current encode default)
//!
//! Bit stream layout (MSB-first within each byte):
//!
//! 1. `version: u8` — [`CODEC_VERSION`] (`2`)
//! 2. `integrity: u8` — [`Integrity`](crate::check::Integrity) tag
//! 3. `payload_len: u32` — big-endian byte length of the payload
//! 4. `payload: [u8; payload_len]`
//! 5. `trailer` — 0 / 4 / 16 / 32 bytes per integrity algorithm (over **payload only**)
//! 6. zero **padding** bits to fill the last glyph
//!
//! ## Version 1 (decode only, Phase 1 compat)
//!
//! 1. `version: u8` = `1`
//! 2. `payload_len: u32` BE
//! 3. `payload`
//! 4. zero padding  
//! (no integrity field or trailer)
//!
//! Glyphs are filled with **low places first** (see `pack` module).
//!
//! Integrity checks are **error detection**, not authentication.

use crate::check::Integrity;
use crate::error::{GlyphixError, Result};
use crate::grid::Grid;
use crate::pack::{bits_to_bytes, bytes_to_bits, pack_glyph, unpack_glyph};
use crate::profile::{GlyphProfile, CODEC_VERSION, CODEC_VERSION_V1};

/// Options for [`encode_with`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EncodeOptions {
    /// Payload integrity trailer (default: none).
    pub integrity: Integrity,
}

impl EncodeOptions {
    /// Encode with a specific integrity algorithm.
    pub fn with_integrity(integrity: Integrity) -> Self {
        Self { integrity }
    }
}

/// Fixed header bits for version-2 frames: version + integrity + u32 length.
pub const V2_HEADER_BITS: usize = 8 + 8 + 32;

/// Fixed header bits for version-1 frames: version + u32 length.
pub const V1_HEADER_BITS: usize = 8 + 32;

/// Total overhead bits (header + trailer) for a v2 frame with the given integrity.
pub fn v2_overhead_bits(integrity: Integrity) -> usize {
    V2_HEADER_BITS + integrity.trailer_len() * 8
}

/// Encode `payload` with default options ([`Integrity::None`]).
pub fn encode(profile: &GlyphProfile, payload: &[u8]) -> Result<Vec<Grid>> {
    encode_with(profile, payload, EncodeOptions::default())
}

/// Encode `payload` with integrity / framing options.
pub fn encode_with(
    profile: &GlyphProfile,
    payload: &[u8],
    opts: EncodeOptions,
) -> Result<Vec<Grid>> {
    if payload.len() > u32::MAX as usize {
        return Err(GlyphixError::InvalidProfile(
            "payload longer than u32::MAX".into(),
        ));
    }

    let n_glyphs = glyphs_needed(profile, payload.len(), opts.integrity)?;
    let bpg = profile.bits_per_glyph();
    let total_bits = n_glyphs * bpg;

    let trailer = opts.integrity.compute(payload);

    let mut frame = Vec::with_capacity(6 + payload.len() + trailer.len());
    frame.push(CODEC_VERSION);
    frame.push(opts.integrity.tag());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    frame.extend_from_slice(&trailer);

    let mut bits = bytes_to_bits(&frame);
    if bits.len() > total_bits {
        return Err(GlyphixError::TruncatedStream {
            needed: bits.len(),
            available: total_bits,
        });
    }
    bits.resize(total_bits, false);

    let mut glyphs = Vec::with_capacity(n_glyphs);
    for i in 0..n_glyphs {
        glyphs.push(pack_glyph(profile, &bits, i * bpg)?);
    }
    Ok(glyphs)
}

/// Decode a glyph sequence back to the original payload bytes.
///
/// Accepts version 1 (no integrity) and version 2 (integrity tag + optional trailer).
/// Verifies the trailer when present; fails on non-zero padding.
pub fn decode(profile: &GlyphProfile, glyphs: &[Grid]) -> Result<Vec<u8>> {
    if glyphs.is_empty() {
        return Err(GlyphixError::EmptyGlyphSequence);
    }
    if glyphs.len() > profile.max_glyphs {
        return Err(GlyphixError::GlyphCountExceeded {
            count: glyphs.len(),
            cap: profile.max_glyphs,
        });
    }

    let bpg = profile.bits_per_glyph();
    let mut bits = Vec::with_capacity(glyphs.len() * bpg);
    for g in glyphs {
        unpack_glyph(profile, g, &mut bits)?;
    }

    if bits.len() < 8 {
        return Err(GlyphixError::TruncatedStream {
            needed: 8,
            available: bits.len(),
        });
    }

    let version = bits_to_bytes(&bits[..8])[0];
    match version {
        CODEC_VERSION_V1 => decode_v1(&bits),
        CODEC_VERSION => decode_v2(&bits),
        got => Err(GlyphixError::UnsupportedVersion {
            got,
            supported: "1, 2",
        }),
    }
}

fn decode_v1(bits: &[bool]) -> Result<Vec<u8>> {
    if bits.len() < V1_HEADER_BITS {
        return Err(GlyphixError::TruncatedStream {
            needed: V1_HEADER_BITS,
            available: bits.len(),
        });
    }
    let header = bits_to_bytes(&bits[..V1_HEADER_BITS]);
    let payload_len = u32::from_be_bytes(header[1..5].try_into().unwrap()) as usize;
    let payload_bits_end = V1_HEADER_BITS + payload_len * 8;
    if payload_bits_end > bits.len() {
        return Err(GlyphixError::PayloadTooLong {
            payload_len: payload_len as u32,
            capacity_bytes: bits.len().saturating_sub(V1_HEADER_BITS) / 8,
        });
    }
    if bits[payload_bits_end..].iter().any(|&b| b) {
        return Err(GlyphixError::DirtyPadding);
    }
    Ok(bits_to_bytes(&bits[V1_HEADER_BITS..payload_bits_end]))
}

fn decode_v2(bits: &[bool]) -> Result<Vec<u8>> {
    if bits.len() < V2_HEADER_BITS {
        return Err(GlyphixError::TruncatedStream {
            needed: V2_HEADER_BITS,
            available: bits.len(),
        });
    }
    let header = bits_to_bytes(&bits[..V2_HEADER_BITS]);
    let integrity = Integrity::from_tag(header[1])?;
    let payload_len = u32::from_be_bytes(header[2..6].try_into().unwrap()) as usize;
    let trailer_len = integrity.trailer_len();

    let payload_bits_end = V2_HEADER_BITS + payload_len * 8;
    let trailer_bits_end = payload_bits_end + trailer_len * 8;
    if trailer_bits_end > bits.len() {
        return Err(GlyphixError::PayloadTooLong {
            payload_len: payload_len as u32,
            capacity_bytes: bits
                .len()
                .saturating_sub(V2_HEADER_BITS + trailer_len * 8)
                / 8,
        });
    }

    if bits[trailer_bits_end..].iter().any(|&b| b) {
        return Err(GlyphixError::DirtyPadding);
    }

    let payload = bits_to_bytes(&bits[V2_HEADER_BITS..payload_bits_end]);
    let trailer = bits_to_bytes(&bits[payload_bits_end..trailer_bits_end]);
    // bits_to_bytes may pad the last trailer byte if trailer_len*8 is exact — it is exact.
    let trailer = trailer[..trailer_len].to_vec();
    integrity.verify(&payload, &trailer)?;
    Ok(payload)
}

/// Minimum glyphs needed for a framed payload with the given integrity.
pub fn glyphs_needed(
    profile: &GlyphProfile,
    payload_len: usize,
    integrity: Integrity,
) -> Result<usize> {
    let need_bits = v2_overhead_bits(integrity) + payload_len.saturating_mul(8);
    let bpg = profile.bits_per_glyph();
    if bpg == 0 {
        return Err(GlyphixError::InvalidProfile("bits_per_glyph is 0".into()));
    }
    let n = need_bits.div_ceil(bpg);
    if n > profile.max_glyphs {
        return Err(GlyphixError::GlyphCountExceeded {
            count: n,
            cap: profile.max_glyphs,
        });
    }
    Ok(n)
}

/// Number of glyphs [`encode`] / [`encode_with`] would produce.
pub fn glyph_count_for(profile: &GlyphProfile, payload: &[u8]) -> Result<usize> {
    glyph_count_for_with(profile, payload, Integrity::None)
}

/// Number of glyphs for a payload with explicit integrity.
pub fn glyph_count_for_with(
    profile: &GlyphProfile,
    payload: &[u8],
    integrity: Integrity,
) -> Result<usize> {
    glyphs_needed(profile, payload.len(), integrity)
}

/// Total information bits available in `n` glyphs (including header/pad).
pub fn capacity_bits(profile: &GlyphProfile, glyph_count: usize) -> usize {
    profile.bits_per_glyph().saturating_mul(glyph_count)
}

/// Max payload bytes for `glyph_count` glyphs under v2 framing with no integrity trailer.
pub fn capacity_payload_bytes(profile: &GlyphProfile, glyph_count: usize) -> usize {
    capacity_payload_bytes_with(profile, glyph_count, Integrity::None)
}

/// Max payload bytes for `glyph_count` glyphs under v2 framing with `integrity`.
pub fn capacity_payload_bytes_with(
    profile: &GlyphProfile,
    glyph_count: usize,
    integrity: Integrity,
) -> usize {
    let total_bits = profile.bits_per_glyph().saturating_mul(glyph_count);
    total_bits
        .saturating_sub(v2_overhead_bits(integrity))
        / 8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{bin10, bin16, bin8, c8_8};

    #[test]
    fn roundtrip_empty() {
        let p = bin8();
        let g = encode(&p, b"").unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(decode(&p, &g).unwrap(), b"");
    }

    #[test]
    fn roundtrip_one_byte() {
        let p = bin8();
        let g = encode(&p, b"A").unwrap();
        assert_eq!(decode(&p, &g).unwrap(), b"A");
    }

    #[test]
    fn roundtrip_32_and_1k() {
        let p = bin10();
        let a = vec![0x5Au8; 32];
        let b = vec![0x3Cu8; 1024];
        assert_eq!(decode(&p, &encode(&p, &a).unwrap()).unwrap(), a);
        assert_eq!(decode(&p, &encode(&p, &b).unwrap()).unwrap(), b);
    }

    #[test]
    fn multi_color_roundtrip() {
        let p = c8_8();
        let data: Vec<u8> = (0..64).collect();
        assert_eq!(decode(&p, &encode(&p, &data).unwrap()).unwrap(), data);
    }

    #[test]
    fn roundtrip_all_integrity_modes() {
        let p = bin10();
        let payload = b"integrity-roundtrip-payload";
        for integ in [
            Integrity::None,
            Integrity::Crc32,
            Integrity::Blake3_128,
            Integrity::Blake3_256,
        ] {
            let g = encode_with(&p, payload, EncodeOptions::with_integrity(integ)).unwrap();
            assert_eq!(decode(&p, &g).unwrap(), payload, "{integ:?}");
        }
    }

    #[test]
    fn flip_pixel_fails_with_crc32() {
        let p = bin8();
        let payload = b"glyphix";
        let mut glyphs = encode_with(
            &p,
            payload,
            EncodeOptions::with_integrity(Integrity::Crc32),
        )
        .unwrap();
        // Flip bottom-right of first glyph (place 0 → first stream bits / header area
        // or nearby). Any corruption should not yield the original payload silently.
        let v = glyphs[0].get(7, 7).unwrap();
        glyphs[0].set(7, 7, 1 - v).unwrap();
        assert!(
            decode(&p, &glyphs).is_err(),
            "expected decode error after pixel flip with CRC32"
        );
    }

    #[test]
    fn flip_pixel_fails_with_blake3() {
        let p = bin10();
        let payload = b"blake3-protected-payload!!";
        let mut glyphs = encode_with(
            &p,
            payload,
            EncodeOptions::with_integrity(Integrity::Blake3_256),
        )
        .unwrap();
        // Flip a cell near the top-left (high place) — still corrupts the bit stream.
        let v = glyphs[0].get(0, 0).unwrap();
        glyphs[0].set(0, 0, 1 - v).unwrap();
        assert!(decode(&p, &glyphs).is_err());
    }

    #[test]
    fn corrupt_payload_region_integrity_mismatch() {
        // Encode enough data that payload bits exist past the 48-bit header.
        let p = bin16(); // 256 bits/glyph — room for payload + crc in one glyph
        let payload = b"0123456789abcdef"; // 16 bytes
        let mut glyphs = encode_with(
            &p,
            payload,
            EncodeOptions::with_integrity(Integrity::Crc32),
        )
        .unwrap();
        assert_eq!(glyphs.len(), 1);
        // Place indices fill low-first: places 0.. cover header then payload.
        // Flip place 20 (well into the stream after 48 header bits → place 48+ for
        // binary is bit index; each place is 1 bit on bin16).
        // Header = 48 bits → places 0..47. Flip place 50 (payload bit).
        let (x, y) = Grid::coords_for_place(16, 16, 50).unwrap();
        let v = glyphs[0].get(x, y).unwrap();
        glyphs[0].set(x, y, 1 - v).unwrap();
        let err = decode(&p, &glyphs).unwrap_err();
        assert!(
            matches!(err, GlyphixError::IntegrityMismatch { kind: "crc32" })
                || matches!(err, GlyphixError::DirtyPadding)
                || matches!(err, GlyphixError::PayloadTooLong { .. }),
            "unexpected error: {err:?}"
        );
        // Stronger path: mutate only by re-encoding then patching trailer via bit flip
        // in the trailer region for a guaranteed IntegrityMismatch.
        let glyphs2 = encode_with(
            &p,
            payload,
            EncodeOptions::with_integrity(Integrity::Crc32),
        )
        .unwrap();
        // Stream: 48 header + 16*8 payload = 48+128 = 176; trailer 4 bytes at bits 176..208
        // place 176 is first trailer bit.
        let mut glyphs2 = glyphs2;
        let (x, y) = Grid::coords_for_place(16, 16, 176).unwrap();
        let v = glyphs2[0].get(x, y).unwrap();
        glyphs2[0].set(x, y, 1 - v).unwrap();
        assert!(matches!(
            decode(&p, &glyphs2),
            Err(GlyphixError::IntegrityMismatch { kind: "crc32" })
        ));
    }

    #[test]
    fn empty_sequence_errors() {
        let p = bin8();
        assert!(matches!(
            decode(&p, &[]),
            Err(GlyphixError::EmptyGlyphSequence)
        ));
    }

    #[test]
    fn v1_legacy_roundtrip_manual() {
        // Build a v1 stream and ensure decode still accepts it.
        use crate::pack::{bytes_to_bits, pack_glyph};
        let p = bin8();
        let payload = b"v1";
        let mut frame = vec![CODEC_VERSION_V1];
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(payload);
        let mut bits = bytes_to_bits(&frame);
        let bpg = p.bits_per_glyph();
        let n = bits.len().div_ceil(bpg);
        bits.resize(n * bpg, false);
        let mut glyphs = Vec::new();
        for i in 0..n {
            glyphs.push(pack_glyph(&p, &bits, i * bpg).unwrap());
        }
        assert_eq!(decode(&p, &glyphs).unwrap(), payload);
    }
}
