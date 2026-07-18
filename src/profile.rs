//! Glyph profiles: grid size and palette size \(C\).
//!
//! Bits per glyph ≈ `width * height * log2(palette_size)` for power-of-two \(C\).

use crate::error::{GlyphixError, Result};

/// Codec version when ECC is enabled (v3: integrity + ecc tag + length + coded body).
pub const CODEC_VERSION: u8 = 3;

/// Phase 2 framing without ECC (still written when [`crate::Ecc::None`]).
pub const CODEC_VERSION_V2: u8 = 2;

/// Legacy Phase 1 framing (decode-only): version + u32 length + payload, no integrity field.
pub const CODEC_VERSION_V1: u8 = 1;

/// Default hard cap on number of glyphs produced/consumed (anti-RAM-bomb).
pub const DEFAULT_MAX_GLYPHS: usize = 4096;

/// Parameters for one symbol of the algorithmic alphabet.
///
/// Place-value order is **not** stored here; it is fixed project-wide:
/// bottom-right = place 0, then left, then up (see `grid` / `pack` modules).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphProfile {
    /// Logical width in cells (≥ 1).
    pub width: u32,
    /// Logical height in cells (≥ 1).
    pub height: u32,
    /// Number of color indices \(C\) (≥ 2). MVP requires power-of-two.
    pub palette_size: u32,
    /// Max glyphs for encode/decode with this profile context (soft policy).
    pub max_glyphs: usize,
}

impl GlyphProfile {
    /// Build a profile; validates dimensions and palette.
    pub fn new(width: u32, height: u32, palette_size: u32) -> Result<Self> {
        Self::with_max_glyphs(width, height, palette_size, DEFAULT_MAX_GLYPHS)
    }

    /// Build a profile with a custom glyph-count hard cap.
    pub fn with_max_glyphs(
        width: u32,
        height: u32,
        palette_size: u32,
        max_glyphs: usize,
    ) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(GlyphixError::InvalidProfile(
                "width and height must be ≥ 1".into(),
            ));
        }
        if palette_size < 2 {
            return Err(GlyphixError::InvalidProfile(
                "palette_size (C) must be ≥ 2".into(),
            ));
        }
        if !palette_size.is_power_of_two() {
            return Err(GlyphixError::InvalidProfile(format!(
                "palette_size {palette_size} must be a power of two in MVP bit packing"
            )));
        }
        if max_glyphs == 0 {
            return Err(GlyphixError::InvalidProfile(
                "max_glyphs must be ≥ 1".into(),
            ));
        }
        // Reject absurd dimensions that would overflow usize cell counts.
        let cells = (width as u64)
            .checked_mul(height as u64)
            .ok_or_else(|| GlyphixError::InvalidProfile("width*height overflow".into()))?;
        if cells > (usize::MAX as u64) / 4 {
            return Err(GlyphixError::InvalidProfile(
                "grid too large for this process".into(),
            ));
        }
        Ok(Self {
            width,
            height,
            palette_size,
            max_glyphs,
        })
    }

    /// Number of logical cells per glyph.
    pub fn cell_count(&self) -> usize {
        (self.width as usize)
            .checked_mul(self.height as usize)
            .expect("validated in constructor")
    }

    /// Bits needed to store one color index (`log2(C)`).
    pub fn bits_per_cell(&self) -> u32 {
        self.palette_size.trailing_zeros()
    }

    /// Total payload bits carried by one glyph (no framing overhead).
    pub fn bits_per_glyph(&self) -> usize {
        self.cell_count()
            .checked_mul(self.bits_per_cell() as usize)
            .expect("bits_per_glyph overflow")
    }

    /// Approximate capacity in whole payload bytes for `glyph_count` glyphs
    /// under **v2 framing with no integrity trailer** (version + integrity tag + u32 length).
    ///
    /// Prefer [`crate::capacity_payload_bytes_with`] when using CRC/BLAKE3.
    pub fn max_payload_bytes(&self, glyph_count: usize) -> usize {
        crate::codec::capacity_payload_bytes(self, glyph_count)
    }

    /// Minimum glyphs needed for framed `payload_len` bytes with **no** integrity trailer.
    ///
    /// Prefer [`crate::glyphs_needed`] when specifying integrity.
    pub fn glyphs_needed_for_payload(&self, payload_len: usize) -> Result<usize> {
        crate::codec::glyphs_needed(self, payload_len, crate::check::Integrity::None)
    }

    /// Human-readable preset id when this matches a known preset; else `None`.
    pub fn preset_id(&self) -> Option<&'static str> {
        for id in preset_ids() {
            if let Ok(p) = preset(id) {
                if p.width == self.width
                    && p.height == self.height
                    && p.palette_size == self.palette_size
                {
                    return Some(id);
                }
            }
        }
        None
    }
}

// --- Named presets ----------------------------------------------------------------

/// \(8\times 8\) binary — 64 bits / glyph.
pub fn bin8() -> GlyphProfile {
    GlyphProfile::new(8, 8, 2).expect("bin8 valid")
}

/// \(10\times 10\) binary — 100 bits / glyph (\(2^{100}\) alphabet).
pub fn bin10() -> GlyphProfile {
    GlyphProfile::new(10, 10, 2).expect("bin10 valid")
}

/// \(16\times 16\) binary — 256 bits / glyph.
pub fn bin16() -> GlyphProfile {
    GlyphProfile::new(16, 16, 2).expect("bin16 valid")
}

/// \(8\times 8\) with 8 colors (3-bit RGB digits) — 192 bits / glyph.
pub fn c8_8() -> GlyphProfile {
    GlyphProfile::new(8, 8, 8).expect("c8_8 valid")
}

/// \(8\times 8\) with 256 grayscale indices — 512 bits / glyph.
pub fn c256_8() -> GlyphProfile {
    GlyphProfile::new(8, 8, 256).expect("c256_8 valid")
}

/// \(8\times 8\) with full 24-bit RGB (\(C = 2^{24}\)) — 1536 bits / glyph.
pub fn rgb24_8() -> GlyphProfile {
    GlyphProfile::new(8, 8, 1 << 24).expect("rgb24_8 valid")
}

/// Look up a preset by id (`"bin8"`, `"bin10"`, …).
pub fn preset(id: &str) -> Result<GlyphProfile> {
    match id {
        "bin8" => Ok(bin8()),
        "bin10" => Ok(bin10()),
        "bin16" => Ok(bin16()),
        "c8_8" => Ok(c8_8()),
        "c256_8" => Ok(c256_8()),
        "rgb24_8" => Ok(rgb24_8()),
        other => Err(GlyphixError::InvalidProfile(format!(
            "unknown preset `{other}`; try bin8, bin10, bin16, c8_8, c256_8, rgb24_8"
        ))),
    }
}

/// List known preset ids.
pub fn preset_ids() -> &'static [&'static str] {
    &["bin8", "bin10", "bin16", "c8_8", "c256_8", "rgb24_8"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_per_glyph_presets() {
        assert_eq!(bin8().bits_per_glyph(), 64);
        assert_eq!(bin10().bits_per_glyph(), 100);
        assert_eq!(bin16().bits_per_glyph(), 256);
        assert_eq!(c8_8().bits_per_glyph(), 192);
        assert_eq!(c256_8().bits_per_glyph(), 512);
        assert_eq!(rgb24_8().bits_per_glyph(), 1536);
    }

    #[test]
    fn rejects_non_power_of_two_palette() {
        assert!(GlyphProfile::new(8, 8, 3).is_err());
        assert!(GlyphProfile::new(8, 8, 1).is_err());
    }

    #[test]
    fn glyphs_needed_empty_still_one() {
        // v2 header alone needs 48 bits → 1 glyph on bin8 (64 bits)
        assert_eq!(bin8().glyphs_needed_for_payload(0).unwrap(), 1);
    }
}
