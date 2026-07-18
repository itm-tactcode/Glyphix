//! Framed encode / decode: bytes ↔ sequence of [`Grid`]s.
//!
//! # Framing
//!
//! ## Version 3 (when ECC enabled)
//!
//! 1. `version: u8` = `3`
//! 2. `integrity: u8` — [`Integrity`](crate::check::Integrity) tag
//! 3. `ecc: u8` — [`Ecc`](crate::ecc::Ecc) tag (`0` none, `1..=50` RS parity %)
//! 4. `payload_len: u32` BE — **original** payload length
//! 5. `coded_body` — raw payload, or RS `message‖parity` blocks ([`Ecc::encode_body`])
//! 6. `trailer` — integrity over **original** payload
//! 7. zero padding to glyph boundary
//!
//! ## Version 2 (default encode when ECC is off)
//!
//! 1. `version: u8` = `2`
//! 2. `integrity: u8`
//! 3. `payload_len: u32` BE
//! 4. `payload`
//! 5. `trailer`
//! 6. zero padding
//!
//! ## Version 1 (decode only)
//!
//! Phase 1 layout without integrity/ECC.
//!
//! Glyphs are filled with **low places first** (see `pack` module).
//!
//! Integrity = **error detection**. ECC = **error correction** (Phase 5).

use crate::check::Integrity;
use crate::ecc::Ecc;
use crate::error::{GlyphixError, Result};
use crate::grid::Grid;
use crate::pack::{bits_to_bytes, bytes_to_bits, pack_glyph, unpack_glyph};
use crate::profile::{
    GlyphProfile, CODEC_VERSION_V1, CODEC_VERSION_V2, CODEC_VERSION as CODEC_VERSION_V3,
};

/// Options for [`encode_with`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EncodeOptions {
    /// Payload integrity trailer (default: none).
    pub integrity: Integrity,
    /// Optional Reed–Solomon over the payload body (default: none).
    pub ecc: Ecc,
}

impl EncodeOptions {
    /// Encode with a specific integrity algorithm.
    pub fn with_integrity(integrity: Integrity) -> Self {
        Self {
            integrity,
            ecc: Ecc::None,
        }
    }

    /// Encode with integrity and ECC.
    pub fn with_integrity_and_ecc(integrity: Integrity, ecc: Ecc) -> Self {
        Self { integrity, ecc }
    }

    /// Encode with ECC only.
    pub fn with_ecc(ecc: Ecc) -> Self {
        Self {
            integrity: Integrity::None,
            ecc,
        }
    }
}

/// Fixed header bits for version-2 frames: version + integrity + u32 length.
pub const V2_HEADER_BITS: usize = 8 + 8 + 32;

/// Fixed header bits for version-3 frames: version + integrity + ecc + u32 length.
pub const V3_HEADER_BITS: usize = 8 + 8 + 8 + 32;

/// Fixed header bits for version-1 frames: version + u32 length.
pub const V1_HEADER_BITS: usize = 8 + 32;

/// Overhead bits (header + integrity trailer) for a v2 frame.
pub fn v2_overhead_bits(integrity: Integrity) -> usize {
    V2_HEADER_BITS + integrity.trailer_len() * 8
}

/// Overhead bits (header + integrity trailer) for a v3 frame (ECC expands body separately).
pub fn v3_overhead_bits(integrity: Integrity) -> usize {
    V3_HEADER_BITS + integrity.trailer_len() * 8
}

/// Header + trailer bits for the frame that would be written with `opts`.
pub fn frame_overhead_bits(opts: EncodeOptions) -> usize {
    match opts.ecc {
        Ecc::None => v2_overhead_bits(opts.integrity),
        _ => v3_overhead_bits(opts.integrity),
    }
}

/// Encode `payload` with default options ([`Integrity::None`], [`Ecc::None`]).
pub fn encode(profile: &GlyphProfile, payload: &[u8]) -> Result<Vec<Grid>> {
    encode_with(profile, payload, EncodeOptions::default())
}

/// Encode `payload` with integrity / ECC / framing options.
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

    let n_glyphs = glyphs_needed_with(profile, payload.len(), opts)?;
    let bpg = profile.bits_per_glyph();
    let total_bits = n_glyphs * bpg;

    let coded = opts.ecc.encode_body(payload)?;
    let trailer = opts.integrity.compute(payload);

    let mut frame = Vec::with_capacity(8 + coded.len() + trailer.len());
    match opts.ecc {
        Ecc::None => {
            frame.push(CODEC_VERSION_V2);
            frame.push(opts.integrity.tag());
            frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            frame.extend_from_slice(&coded); // raw payload
        }
        _ => {
            frame.push(CODEC_VERSION_V3);
            frame.push(opts.integrity.tag());
            frame.push(opts.ecc.tag());
            frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            frame.extend_from_slice(&coded);
        }
    }
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
/// Accepts versions 1, 2, and 3. Applies RS correction when the frame carries ECC.
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
        CODEC_VERSION_V2 => decode_v2(&bits),
        CODEC_VERSION_V3 => decode_v3(&bits),
        got => Err(GlyphixError::UnsupportedVersion {
            got,
            supported: "1, 2, 3",
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
    let trailer = trailer[..trailer_len].to_vec();
    integrity.verify(&payload, &trailer)?;
    Ok(payload)
}

fn decode_v3(bits: &[bool]) -> Result<Vec<u8>> {
    if bits.len() < V3_HEADER_BITS {
        return Err(GlyphixError::TruncatedStream {
            needed: V3_HEADER_BITS,
            available: bits.len(),
        });
    }
    let header = bits_to_bytes(&bits[..V3_HEADER_BITS]);
    let integrity = Integrity::from_tag(header[1])?;
    let ecc = Ecc::from_tag(header[2])?;
    let payload_len = u32::from_be_bytes(header[3..7].try_into().unwrap()) as usize;
    let trailer_len = integrity.trailer_len();
    let coded_len = ecc.coded_len(payload_len);

    let body_bits_end = V3_HEADER_BITS + coded_len * 8;
    let trailer_bits_end = body_bits_end + trailer_len * 8;
    if trailer_bits_end > bits.len() {
        return Err(GlyphixError::PayloadTooLong {
            payload_len: payload_len as u32,
            capacity_bytes: bits
                .len()
                .saturating_sub(V3_HEADER_BITS + trailer_len * 8)
                / 8,
        });
    }

    if bits[trailer_bits_end..].iter().any(|&b| b) {
        return Err(GlyphixError::DirtyPadding);
    }

    let coded = bits_to_bytes(&bits[V3_HEADER_BITS..body_bits_end]);
    let coded = coded[..coded_len].to_vec();
    let payload = ecc.decode_body(&coded, payload_len)?;

    let trailer = bits_to_bytes(&bits[body_bits_end..trailer_bits_end]);
    let trailer = trailer[..trailer_len].to_vec();
    integrity.verify(&payload, &trailer)?;
    Ok(payload)
}

/// Minimum glyphs needed for a framed payload with integrity only (no ECC).
pub fn glyphs_needed(
    profile: &GlyphProfile,
    payload_len: usize,
    integrity: Integrity,
) -> Result<usize> {
    glyphs_needed_with(
        profile,
        payload_len,
        EncodeOptions {
            integrity,
            ecc: Ecc::None,
        },
    )
}

/// Minimum glyphs needed for a framed payload with full options (integrity + ECC).
pub fn glyphs_needed_with(
    profile: &GlyphProfile,
    payload_len: usize,
    opts: EncodeOptions,
) -> Result<usize> {
    let body = opts.ecc.coded_len(payload_len);
    let need_bits = frame_overhead_bits(opts) + body.saturating_mul(8);
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

/// Number of glyphs [`encode`] would produce.
pub fn glyph_count_for(profile: &GlyphProfile, payload: &[u8]) -> Result<usize> {
    glyph_count_for_with(profile, payload, Integrity::None)
}

/// Number of glyphs for a payload with integrity (no ECC).
pub fn glyph_count_for_with(
    profile: &GlyphProfile,
    payload: &[u8],
    integrity: Integrity,
) -> Result<usize> {
    glyphs_needed(profile, payload.len(), integrity)
}

/// Number of glyphs for a payload with full encode options.
pub fn glyph_count_for_opts(
    profile: &GlyphProfile,
    payload: &[u8],
    opts: EncodeOptions,
) -> Result<usize> {
    glyphs_needed_with(profile, payload.len(), opts)
}

/// Total information bits available in `n` glyphs (including header/pad).
pub fn capacity_bits(profile: &GlyphProfile, glyph_count: usize) -> usize {
    profile.bits_per_glyph().saturating_mul(glyph_count)
}

/// Max payload bytes for `glyph_count` glyphs under v2 framing, no integrity/ECC.
pub fn capacity_payload_bytes(profile: &GlyphProfile, glyph_count: usize) -> usize {
    capacity_payload_bytes_with(profile, glyph_count, Integrity::None)
}

/// Max payload bytes for `glyph_count` glyphs under v2 framing with `integrity` (no ECC).
pub fn capacity_payload_bytes_with(
    profile: &GlyphProfile,
    glyph_count: usize,
    integrity: Integrity,
) -> usize {
    capacity_payload_bytes_opts(
        profile,
        glyph_count,
        EncodeOptions {
            integrity,
            ecc: Ecc::None,
        },
    )
}

/// Max original payload bytes for `glyph_count` glyphs with full options (incl. ECC).
pub fn capacity_payload_bytes_opts(
    profile: &GlyphProfile,
    glyph_count: usize,
    opts: EncodeOptions,
) -> usize {
    let total_bits = profile.bits_per_glyph().saturating_mul(glyph_count);
    let overhead = frame_overhead_bits(opts);
    let body_budget = total_bits.saturating_sub(overhead) / 8;
    opts.ecc.max_payload_for_body(body_budget)
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
    fn roundtrip_rs_ecc() {
        let p = bin10();
        let payload: Vec<u8> = (0..80).map(|i| (i * 3) as u8).collect();
        for ecc in [Ecc::rs10(), Ecc::rs20()] {
            let g = encode_with(&p, &payload, EncodeOptions::with_ecc(ecc)).unwrap();
            assert_eq!(decode(&p, &g).unwrap(), payload, "{ecc:?}");
            // More glyphs than without ECC
            let plain = encode(&p, &payload).unwrap();
            assert!(g.len() >= plain.len(), "ecc={ecc:?}");
        }
    }

    #[test]
    fn rs_recovers_flipped_payload_bits() {
        let p = bin16();
        let payload = b"0123456789abcdef0123456789abcdef"; // 32 bytes
        let opts = EncodeOptions::with_integrity_and_ecc(Integrity::Crc32, Ecc::rs10());
        let mut glyphs = encode_with(&p, payload, opts).unwrap();
        // V3 header 56 bits; flip a few places in the coded body (after header)
        for place in [60usize, 70, 80] {
            let (x, y) = Grid::coords_for_place(16, 16, place).unwrap();
            let v = glyphs[0].get(x, y).unwrap();
            glyphs[0].set(x, y, 1 - v).unwrap();
        }
        assert_eq!(decode(&p, &glyphs).unwrap(), payload);
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
        let v = glyphs[0].get(0, 0).unwrap();
        glyphs[0].set(0, 0, 1 - v).unwrap();
        assert!(decode(&p, &glyphs).is_err());
    }

    #[test]
    fn corrupt_payload_region_integrity_mismatch() {
        let p = bin16();
        let payload = b"0123456789abcdef";
        let mut glyphs = encode_with(
            &p,
            payload,
            EncodeOptions::with_integrity(Integrity::Crc32),
        )
        .unwrap();
        assert_eq!(glyphs.len(), 1);
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
        let glyphs2 = encode_with(
            &p,
            payload,
            EncodeOptions::with_integrity(Integrity::Crc32),
        )
        .unwrap();
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

    #[test]
    fn capacity_shrinks_with_ecc() {
        let p = bin10();
        let plain = capacity_payload_bytes(&p, 10);
        let with_rs = capacity_payload_bytes_opts(&p, 10, EncodeOptions::with_ecc(Ecc::rs10()));
        assert!(with_rs < plain);
        assert!(with_rs > 0);
    }
}
