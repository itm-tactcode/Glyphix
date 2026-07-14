//! Bits ↔ grid cells using base-\(C\) place values.
//!
//! # Stream convention
//!
//! - Bytes expand to bits **MSB-first** within each byte.
//! - For each glyph, **low places fill first**: the first `bits_per_cell` bits
//!   form the digit at place 0 (bottom-right), matching “value 1 = BR on.”
//! - Within a multi-bit digit, bits are **MSB-first**.

use crate::error::{GlyphixError, Result};
use crate::grid::Grid;
use crate::profile::GlyphProfile;

/// Expand bytes to a bit vector (MSB-first per byte).
pub fn bytes_to_bits(bytes: &[u8]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(bytes.len() * 8);
    for &b in bytes {
        for i in (0..8).rev() {
            bits.push((b >> i) & 1 == 1);
        }
    }
    bits
}

/// Pack bits into bytes (MSB-first per byte). `bits.len()` need not be a
/// multiple of 8; missing low bits of the last byte are zero.
pub fn bits_to_bytes(bits: &[bool]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bits.len().div_ceil(8));
    let mut acc = 0u8;
    let mut n = 0u8;
    for &bit in bits {
        acc = (acc << 1) | u8::from(bit);
        n += 1;
        if n == 8 {
            out.push(acc);
            acc = 0;
            n = 0;
        }
    }
    if n > 0 {
        acc <<= 8 - n;
        out.push(acc);
    }
    out
}

/// Paint one glyph from `bits[offset..offset+bits_per_glyph]`.
///
/// Low places first; returns the number of bits consumed (= bits_per_glyph).
pub fn pack_glyph(profile: &GlyphProfile, bits: &[bool], offset: usize) -> Result<Grid> {
    let need = profile.bits_per_glyph();
    let available = bits.len().saturating_sub(offset);
    if available < need {
        return Err(GlyphixError::TruncatedStream {
            needed: need,
            available,
        });
    }

    let bpc = profile.bits_per_cell() as usize;
    let mut grid = Grid::from_profile(profile);
    let mut pos = offset;

    for place in 0..profile.cell_count() {
        let mut digit = 0u32;
        for _ in 0..bpc {
            digit = (digit << 1) | u32::from(bits[pos]);
            pos += 1;
        }
        if digit >= profile.palette_size {
            // Unreachable for power-of-two C when we only take log2(C) bits.
            return Err(GlyphixError::ColorOutOfRange {
                index: digit,
                palette_size: profile.palette_size,
            });
        }
        grid.set_place(place, digit)?;
    }
    Ok(grid)
}

/// Read bits from a glyph (low places first). Appends `bits_per_glyph` bits to `out`.
pub fn unpack_glyph(profile: &GlyphProfile, grid: &Grid, out: &mut Vec<bool>) -> Result<()> {
    grid.validate_profile(profile)?;
    let bpc = profile.bits_per_cell() as usize;

    for place in 0..profile.cell_count() {
        let digit = grid.get_place(place)?;
        if digit >= profile.palette_size {
            return Err(GlyphixError::ColorOutOfRange {
                index: digit,
                palette_size: profile.palette_size,
            });
        }
        // Emit bits_per_cell bits, MSB-first.
        for i in (0..bpc).rev() {
            out.push((digit >> i) & 1 == 1);
        }
    }
    Ok(())
}

/// Encode an integer `value` onto a fresh grid using base-\(C\) place expansion.
///
/// Only for values that fit in the glyph (\(V < C^{WH}\)). Used for goldens and
/// “paint this codepoint” demos. For large profiles, pass a little-endian digit
/// iterator via [`paint_digits`] instead of a `u128`.
pub fn paint_value_u128(profile: &GlyphProfile, mut value: u128) -> Result<Grid> {
    let c = profile.palette_size as u128;
    let n = profile.cell_count();
    // Capacity check for small grids only; for huge C^n use digit path.
    if profile.bits_per_glyph() <= 128 {
        let max = if profile.bits_per_glyph() == 128 {
            u128::MAX
        } else {
            (1u128 << profile.bits_per_glyph()) - 1
        };
        if value > max {
            return Err(GlyphixError::InvalidProfile(
                "value does not fit in this glyph".into(),
            ));
        }
    }

    let mut grid = Grid::from_profile(profile);
    for place in 0..n {
        let digit = (value % c) as u32;
        value /= c;
        grid.set_place(place, digit)?;
    }
    if value != 0 {
        return Err(GlyphixError::InvalidProfile(
            "value does not fit in this glyph".into(),
        ));
    }
    Ok(grid)
}

/// Paint precomputed digits \(d_0, d_1, \ldots\) (place order) onto a grid.
pub fn paint_digits(profile: &GlyphProfile, digits: &[u32]) -> Result<Grid> {
    if digits.len() != profile.cell_count() {
        return Err(GlyphixError::InvalidProfile(format!(
            "expected {} digits, got {}",
            profile.cell_count(),
            digits.len()
        )));
    }
    let mut grid = Grid::from_profile(profile);
    for (place, &d) in digits.iter().enumerate() {
        if d >= profile.palette_size {
            return Err(GlyphixError::ColorOutOfRange {
                index: d,
                palette_size: profile.palette_size,
            });
        }
        grid.set_place(place, d)?;
    }
    Ok(grid)
}

// --- Human bit-string notation (place 0 = rightmost bit) -------------------------

/// Parse a bit string into a `Vec<bool>`.
///
/// Allowed characters: `0`, `1`. Whitespace, `_`, and `-` are ignored (for
/// grouping). Any other character is an error.
pub fn parse_bit_string(s: &str) -> Result<Vec<bool>> {
    let mut bits = Vec::with_capacity(s.len());
    for (i, ch) in s.chars().enumerate() {
        match ch {
            '0' => bits.push(false),
            '1' => bits.push(true),
            ' ' | '\t' | '\n' | '\r' | '_' | '-' => {}
            other => {
                return Err(GlyphixError::InvalidBitString(format!(
                    "invalid character {other:?} at index {i}; only 0/1 (and _ - whitespace) allowed"
                )));
            }
        }
    }
    Ok(bits)
}

/// Normalize bits to exactly `n_bits` length.
///
/// - **Shorter:** pad with `0` on the **left** (high side).
/// - **Longer:** discard bits on the **right** (keep the left `n_bits`).
///
/// Empty input becomes `n_bits` zeros.
pub fn normalize_bit_string(bits: &[bool], n_bits: usize) -> Vec<bool> {
    if n_bits == 0 {
        return Vec::new();
    }
    if bits.len() == n_bits {
        return bits.to_vec();
    }
    if bits.len() < n_bits {
        let mut out = vec![false; n_bits - bits.len()];
        out.extend_from_slice(bits);
        out
    } else {
        // Discard right: keep left n_bits
        bits[..n_bits].to_vec()
    }
}

/// Parse + normalize in one step.
pub fn parse_and_normalize_bit_string(s: &str, n_bits: usize) -> Result<Vec<bool>> {
    let raw = parse_bit_string(s)?;
    Ok(normalize_bit_string(&raw, n_bits))
}

/// Paint one glyph from a human bit string.
///
/// Notation matches ordinary binary writing:
/// - **Rightmost** bit → place 0 (bottom-right cell)
/// - **Leftmost** bit → highest place (toward top-left)
/// - Shorter than `bits_per_glyph`: pad **left** with zeros  
///   e.g. `1` or `00000000001` → only bottom-right on
/// - Longer: **right** bits discarded  
///   e.g. 64 ones followed by zeros → all white on `bin8`
///
/// Multi-bit color profiles use `log2(C)` bits per place (MSB-first within each digit),
/// still with place 0 at the right end of the string.
///
/// This is **not** framed `encode` (no version/length header) — direct ink pattern.
pub fn paint_bit_string(profile: &GlyphProfile, bit_str: &str) -> Result<Grid> {
    let n = profile.bits_per_glyph();
    let bits = parse_and_normalize_bit_string(bit_str, n)?;
    paint_normalized_bits(profile, &bits)
}

/// How multi-glyph bit strings are split.
///
/// See [`paint_bit_string_sequence`] for the recommended rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BitSequenceMode {
    /// Auto: comma-separated if `,` present, else continuous stream.
    #[default]
    Auto,
    /// Continuous bit stream: fill glyph 0, then glyph 1, … (no mid-stream discard).
    /// Partial final glyph is **left-padded** with zeros.
    Stream,
    /// Comma-separated glyphs; each field uses single-glyph pad-left / trunc-right.
    CommaSeparated,
}

/// Paint a horizontal sequence of glyphs from a bit string.
///
/// # Two notations (both supported)
///
/// ## 1. Comma-separated (hand-authored tiles)
///
/// ```text
/// 1, 11, 101
/// ```
///
/// Each field is **one glyph** with the same rules as [`paint_bit_string`]:
/// pad left if short, truncate right if long. Empty field → all black.
/// Good when you design each character on purpose.
///
/// ## 2. Continuous stream (like text encode spilling into the next char)
///
/// ```text
/// <64 bits for glyph 0><next bits for glyph 1>…
/// ```
///
/// - Consume bits **left → right** in chunks of `bits_per_glyph`.
/// - Overflow goes to the **next** glyph (nothing discarded mid-stream).
/// - If the final chunk is short, **left-pad with zeros** (so a trailing `1`
///   still means only bottom-right on that glyph).
/// - Optional `glyph_count`: force exactly N glyphs (pad final with zeros, or
///   error if more bits than N·bpg would require — extra bits on the right of
///   the *stream* are discarded only when `glyph_count` caps the length).
///
/// # Why not whole-string left-pad then split?
///
/// That pulled low bits into the wrong glyph. Stream chunking matches “keep
/// writing” the way framed text encode fills successive glyphs.
pub fn paint_bit_string_sequence(
    profile: &GlyphProfile,
    bit_str: &str,
    glyph_count: Option<usize>,
) -> Result<Vec<Grid>> {
    paint_bit_string_sequence_with(profile, bit_str, glyph_count, BitSequenceMode::Auto)
}

/// [`paint_bit_string_sequence`] with an explicit [`BitSequenceMode`].
pub fn paint_bit_string_sequence_with(
    profile: &GlyphProfile,
    bit_str: &str,
    glyph_count: Option<usize>,
    mode: BitSequenceMode,
) -> Result<Vec<Grid>> {
    let bpg = profile.bits_per_glyph();
    if bpg == 0 {
        return Err(GlyphixError::InvalidProfile("bits_per_glyph is 0".into()));
    }

    let mode = match mode {
        BitSequenceMode::Auto => {
            if bit_str.contains(',') {
                BitSequenceMode::CommaSeparated
            } else {
                BitSequenceMode::Stream
            }
        }
        other => other,
    };

    match mode {
        BitSequenceMode::CommaSeparated => paint_bit_string_comma(profile, bit_str, glyph_count),
        BitSequenceMode::Stream => paint_bit_string_stream(profile, bit_str, glyph_count),
        BitSequenceMode::Auto => unreachable!("resolved above"),
    }
}

fn paint_bit_string_comma(
    profile: &GlyphProfile,
    bit_str: &str,
    glyph_count: Option<usize>,
) -> Result<Vec<Grid>> {
    // split on comma; do not trim away empty trailing if user wants blank glyph —
    // but "1,2," gives three parts with last empty → black glyph. OK.
    let parts: Vec<&str> = bit_str.split(',').collect();
    if parts.is_empty() {
        return Err(GlyphixError::EmptyGlyphSequence);
    }
    let mut glyphs = Vec::with_capacity(parts.len());
    for part in &parts {
        // parse_bit_string ignores whitespace inside; empty → zeros via normalize
        glyphs.push(paint_bit_string(profile, part.trim())?);
    }
    if let Some(n) = glyph_count {
        if n == 0 {
            return Err(GlyphixError::EmptyGlyphSequence);
        }
        if n > profile.max_glyphs {
            return Err(GlyphixError::GlyphCountExceeded {
                count: n,
                cap: profile.max_glyphs,
            });
        }
        if glyphs.len() > n {
            glyphs.truncate(n);
        } else {
            while glyphs.len() < n {
                glyphs.push(Grid::from_profile(profile));
            }
        }
    }
    if glyphs.len() > profile.max_glyphs {
        return Err(GlyphixError::GlyphCountExceeded {
            count: glyphs.len(),
            cap: profile.max_glyphs,
        });
    }
    Ok(glyphs)
}

fn paint_bit_string_stream(
    profile: &GlyphProfile,
    bit_str: &str,
    glyph_count: Option<usize>,
) -> Result<Vec<Grid>> {
    let bpg = profile.bits_per_glyph();
    // Commas are not legal in pure stream parse — strip them? Better error if present
    // when forced Stream mode. Allow only 01_ whitespace.
    let raw = parse_bit_string(bit_str)?;
    let n = match glyph_count {
        Some(n) => {
            if n == 0 {
                return Err(GlyphixError::EmptyGlyphSequence);
            }
            n
        }
        None => raw.len().div_ceil(bpg).max(1),
    };
    if n > profile.max_glyphs {
        return Err(GlyphixError::GlyphCountExceeded {
            count: n,
            cap: profile.max_glyphs,
        });
    }

    // Need exactly n * bpg bits: take left-to-right from raw, then left-pad only
    // the final partial chunk — equivalent to: use first n*bpg bits with right
    // discarded only if raw longer than n*bpg; if shorter, pad zeros on the right
    // of the *stream* then… wait user said left-pad partial final glyph.
    //
    // Stream of 65 ones, n=2, bpg=64:
    //   glyph0 = raw[0..64]
    //   glyph1 = left_pad(raw[64..65])  → 63 zeros + one 1
    //
    // Stream of 10 ones, n=1 (explicit): left_pad to 64.
    // Stream of 10 ones, n=None: n=1, left_pad.
    // Stream of 100 ones, n=1: take left 64 only (cap discards rest) — only when glyph_count set.
    // Stream of 100 ones, n=None: n=2, glyph0=64, glyph1=left_pad(36 ones).

    let mut glyphs = Vec::with_capacity(n);
    for i in 0..n {
        let start = i * bpg;
        let end = start + bpg;
        let chunk = if raw.len() >= end {
            raw[start..end].to_vec()
        } else if raw.len() > start {
            // partial final (or hole if raw shorter than this glyph start — pad all)
            let piece = &raw[start..];
            normalize_bit_string(piece, bpg) // left-pad short piece
        } else {
            // past end of raw: empty glyph (all zero), or if we forced n larger
            vec![false; bpg]
        };
        glyphs.push(paint_normalized_bits(profile, &chunk)?);
    }
    Ok(glyphs)
}

/// Paint from an already-normalized bit slice (`len == bits_per_glyph`).
/// Left = high place, right = place 0.
pub fn paint_normalized_bits(profile: &GlyphProfile, bits: &[bool]) -> Result<Grid> {
    let n = profile.bits_per_glyph();
    if bits.len() != n {
        return Err(GlyphixError::InvalidBitString(format!(
            "expected {n} bits after normalize, got {}",
            bits.len()
        )));
    }
    let bpc = profile.bits_per_cell() as usize;
    let mut grid = Grid::from_profile(profile);
    // place 0 uses the rightmost bpc bits
    for place in 0..profile.cell_count() {
        let start = n - (place + 1) * bpc;
        let mut digit = 0u32;
        for k in 0..bpc {
            digit = (digit << 1) | u32::from(bits[start + k]);
        }
        if digit >= profile.palette_size {
            return Err(GlyphixError::ColorOutOfRange {
                index: digit,
                palette_size: profile.palette_size,
            });
        }
        grid.set_place(place, digit)?;
    }
    Ok(grid)
}

/// Format a glyph as a bit string (left = high place, right = place 0).
///
/// Inverse of [`paint_bit_string`] for a single glyph (no separators).
pub fn grid_to_bit_string(profile: &GlyphProfile, grid: &Grid) -> Result<String> {
    grid.validate_profile(profile)?;
    let bpc = profile.bits_per_cell() as usize;
    let n = profile.bits_per_glyph();
    let mut bits = vec![false; n];
    for place in 0..profile.cell_count() {
        let digit = grid.get_place(place)?;
        let start = n - (place + 1) * bpc;
        for k in 0..bpc {
            let shift = bpc - 1 - k;
            bits[start + k] = (digit >> shift) & 1 == 1;
        }
    }
    Ok(bits
        .iter()
        .map(|&b| if b { '1' } else { '0' })
        .collect())
}

/// Recover a `u128` value from a grid (only when `bits_per_glyph ≤ 128`).
pub fn value_u128(profile: &GlyphProfile, grid: &Grid) -> Result<u128> {
    if profile.bits_per_glyph() > 128 {
        return Err(GlyphixError::InvalidProfile(
            "value_u128 only for glyphs ≤ 128 bits".into(),
        ));
    }
    grid.validate_profile(profile)?;
    let c = profile.palette_size as u128;
    let mut v = 0u128;
    let mut place_pow = 1u128;
    for place in 0..profile.cell_count() {
        let d = grid.get_place(place)? as u128;
        v = v
            .checked_add(d.checked_mul(place_pow).expect("digit*pow"))
            .expect("value overflow");
        place_pow = place_pow.checked_mul(c).unwrap_or(0);
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{bin8, bin10};

    #[test]
    fn value_zero_all_black() {
        let g = paint_value_u128(&bin8(), 0).unwrap();
        assert!(g.is_all_zero());
    }

    #[test]
    fn value_one_bottom_right_only() {
        let p = bin8();
        let g = paint_value_u128(&p, 1).unwrap();
        assert_eq!(g.get(7, 7).unwrap(), 1);
        // Every other cell black.
        for y in 0..8 {
            for x in 0..8 {
                if (x, y) != (7, 7) {
                    assert_eq!(g.get(x, y).unwrap(), 0, "cell ({x},{y})");
                }
            }
        }
        assert_eq!(value_u128(&p, &g).unwrap(), 1);
    }

    #[test]
    fn value_two_left_of_br() {
        let p = bin8();
        let g = paint_value_u128(&p, 2).unwrap();
        assert_eq!(g.get(7, 7).unwrap(), 0); // BR off
        assert_eq!(g.get(6, 7).unwrap(), 1); // one left white
        assert_eq!(value_u128(&p, &g).unwrap(), 2);
    }

    #[test]
    fn value_three_bottom_two() {
        let p = bin10();
        let g = paint_value_u128(&p, 3).unwrap();
        assert_eq!(g.get(9, 9).unwrap(), 1);
        assert_eq!(g.get(8, 9).unwrap(), 1);
        assert_eq!(g.get(7, 9).unwrap(), 0);
        assert_eq!(value_u128(&p, &g).unwrap(), 3);
    }

    #[test]
    fn pack_unpack_roundtrip_bits() {
        let p = bin8();
        let mut bits = vec![false; 64];
        // Set place 0 digit = 1 → first bit of stream for this glyph is 1
        bits[0] = true;
        let g = pack_glyph(&p, &bits, 0).unwrap();
        assert_eq!(g.get(7, 7).unwrap(), 1);
        let mut out = Vec::new();
        unpack_glyph(&p, &g, &mut out).unwrap();
        assert_eq!(out, bits);
    }

    #[test]
    fn bytes_bits_roundtrip() {
        let data = [0x00u8, 0xA5, 0xFF, 0x12];
        let bits = bytes_to_bits(&data);
        assert_eq!(bits.len(), 32);
        assert_eq!(bits_to_bytes(&bits), data);
    }

    #[test]
    fn bit_string_one_is_bottom_right() {
        let p = bin8();
        for s in ["1", "00000000001", "0_0_0_1"] {
            let g = paint_bit_string(&p, s).unwrap();
            assert_eq!(g.get(7, 7).unwrap(), 1, "{s}");
            for y in 0..8 {
                for x in 0..8 {
                    if (x, y) != (7, 7) {
                        assert_eq!(g.get(x, y).unwrap(), 0, "{s} ({x},{y})");
                    }
                }
            }
            assert_eq!(value_u128(&p, &g).unwrap(), 1);
        }
    }

    #[test]
    fn bit_string_all_ones_left_truncates_to_full_white() {
        let p = bin8();
        // 64 ones + trailing zeros on the right → discard right → all white
        let mut s = "1".repeat(64);
        s.push_str("000000000000000");
        let g = paint_bit_string(&p, &s).unwrap();
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(g.get(x, y).unwrap(), 1, "({x},{y})");
            }
        }
        assert_eq!(grid_to_bit_string(&p, &g).unwrap(), "1".repeat(64));
    }

    #[test]
    fn bit_string_roundtrip_grid() {
        let p = bin10();
        let s = "10110001";
        let g = paint_bit_string(&p, s).unwrap();
        let out = grid_to_bit_string(&p, &g).unwrap();
        assert!(out.ends_with("10110001"));
        assert_eq!(out.len(), 100);
        assert!(out.starts_with(&"0".repeat(100 - 8)));
    }

    #[test]
    fn bit_string_matches_paint_value() {
        let p = bin8();
        assert_eq!(
            paint_bit_string(&p, "1").unwrap(),
            paint_value_u128(&p, 1).unwrap()
        );
        assert_eq!(
            paint_bit_string(&p, "10").unwrap(),
            paint_value_u128(&p, 2).unwrap()
        );
        assert_eq!(
            paint_bit_string(&p, "11").unwrap(),
            paint_value_u128(&p, 3).unwrap()
        );
    }

    #[test]
    fn bit_string_stream_spills_to_next_glyph() {
        let p = bin8();
        // 65 ones → glyph0 all white, glyph1 only BR (left-padded partial)
        let s = "1".repeat(65);
        let glyphs = paint_bit_string_sequence(&p, &s, None).unwrap();
        assert_eq!(glyphs.len(), 2);
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(glyphs[0].get(x, y).unwrap(), 1, "g0 ({x},{y})");
            }
        }
        assert_eq!(glyphs[1].get(7, 7).unwrap(), 1);
        assert_eq!(value_u128(&p, &glyphs[1]).unwrap(), 1);
    }

    #[test]
    fn bit_string_comma_separated() {
        let p = bin8();
        let glyphs = paint_bit_string_sequence(&p, "1, 11, 0", None).unwrap();
        assert_eq!(glyphs.len(), 3);
        assert_eq!(value_u128(&p, &glyphs[0]).unwrap(), 1);
        assert_eq!(value_u128(&p, &glyphs[1]).unwrap(), 3);
        assert!(glyphs[2].is_all_zero());
    }

    #[test]
    fn bit_string_comma_each_field_truncates() {
        let p = bin8();
        let long = "1".repeat(70);
        let glyphs =
            paint_bit_string_sequence_with(&p, &format!("{long},1"), None, BitSequenceMode::CommaSeparated)
                .unwrap();
        assert_eq!(glyphs.len(), 2);
        // first field truncated to 64 ones → all white (value 2^64 - 1)
        assert_eq!(value_u128(&p, &glyphs[0]).unwrap(), (1u128 << 64) - 1);
        assert_eq!(value_u128(&p, &glyphs[1]).unwrap(), 1);
    }
}
