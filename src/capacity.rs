//! Capacity reports for CLI and library users (Phase 4).

use crate::check::Integrity;
use crate::codec::{
    capacity_bits, capacity_payload_bytes_opts, glyphs_needed, EncodeOptions,
};
use crate::ecc::Ecc;
use crate::error::Result;
use crate::layout::{image_size, GlyphLayout, LayoutOptions};
use crate::profile::{preset_ids, GlyphProfile};
use crate::stream::chunks_needed;

/// One row of a capacity table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapacityReport {
    pub profile_id: String,
    pub width: u32,
    pub height: u32,
    pub palette_size: u32,
    pub bits_per_glyph: usize,
    pub glyph_count: usize,
    pub integrity: Integrity,
    pub ecc: Ecc,
    pub payload_bytes: usize,
    pub total_bits: usize,
    /// Image size at the given scale (strip layout, no margin/gap).
    pub image_w: u32,
    pub image_h: u32,
    pub cell_scale: u32,
    /// Chunks needed if each chunk is at most `glyph_count` glyphs (for this payload size = full capacity).
    pub chunks_if_budget_is_this: usize,
}

/// Build a capacity report for one profile / glyph count / integrity / scale (no ECC).
pub fn report(
    profile_id: &str,
    profile: &GlyphProfile,
    glyph_count: usize,
    integrity: Integrity,
    cell_scale: u32,
) -> Result<CapacityReport> {
    report_opts(
        profile_id,
        profile,
        glyph_count,
        EncodeOptions {
            integrity,
            ecc: Ecc::None,
        },
        cell_scale,
    )
}

/// Capacity report with full encode options (integrity + ECC).
pub fn report_opts(
    profile_id: &str,
    profile: &GlyphProfile,
    glyph_count: usize,
    opts: EncodeOptions,
    cell_scale: u32,
) -> Result<CapacityReport> {
    let payload_bytes = capacity_payload_bytes_opts(profile, glyph_count, opts);
    let total_bits = capacity_bits(profile, glyph_count);
    let layout = LayoutOptions {
        cell_scale,
        margin: 0,
        gap_x: 0,
        gap_y: 0,
        layout: GlyphLayout::HorizontalStrip,
        separator: None,
    };
    let (image_w, image_h) = if glyph_count == 0 {
        (0, 0)
    } else {
        image_size(profile, glyph_count, &layout)?
    };
    // chunks_needed ignores ECC; approximate with integrity-only for table
    let chunks = chunks_needed(profile, payload_bytes, glyph_count.max(1), opts.integrity)?;
    Ok(CapacityReport {
        profile_id: profile_id.to_string(),
        width: profile.width,
        height: profile.height,
        palette_size: profile.palette_size,
        bits_per_glyph: profile.bits_per_glyph(),
        glyph_count,
        integrity: opts.integrity,
        ecc: opts.ecc,
        payload_bytes,
        total_bits,
        image_w,
        image_h,
        cell_scale,
        chunks_if_budget_is_this: chunks,
    })
}

/// Capacity for a grid layout (extra image dimensions).
pub fn report_grid(
    profile_id: &str,
    profile: &GlyphProfile,
    glyph_count: usize,
    integrity: Integrity,
    cell_scale: u32,
    columns: u32,
) -> Result<(CapacityReport, u32, u32)> {
    let mut r = report(profile_id, profile, glyph_count, integrity, cell_scale)?;
    let layout = LayoutOptions::grid(cell_scale, columns, 0, 0)?;
    let (w, h) = image_size(profile, glyph_count.max(1), &layout)?;
    r.image_w = w;
    r.image_h = h;
    Ok((r, w, h))
}

/// Glyphs needed for an exact payload length.
pub fn glyphs_for_bytes(
    profile: &GlyphProfile,
    payload_len: usize,
    integrity: Integrity,
) -> Result<usize> {
    glyphs_needed(profile, payload_len, integrity)
}

/// Format a single report line for CLI.
pub fn format_report_line(r: &CapacityReport) -> String {
    format!(
        "{:10}  n={:<4}  check={:<11}  ecc={:<6}  payload_bytes={:<6}  bits={:<6}  {}x{} @s={}  ({}x{} C={})",
        r.profile_id,
        r.glyph_count,
        r.integrity.as_str(),
        r.ecc.as_str(),
        r.payload_bytes,
        r.total_bits,
        r.image_w,
        r.image_h,
        r.cell_scale,
        r.width,
        r.height,
        r.palette_size,
    )
}

/// Table for all presets at the given glyph counts.
pub fn table_all_presets(
    glyph_counts: &[usize],
    integrity: Integrity,
    cell_scale: u32,
) -> Result<Vec<CapacityReport>> {
    table_all_presets_opts(
        glyph_counts,
        EncodeOptions {
            integrity,
            ecc: Ecc::None,
        },
        cell_scale,
    )
}

/// Table with ECC included.
pub fn table_all_presets_opts(
    glyph_counts: &[usize],
    opts: EncodeOptions,
    cell_scale: u32,
) -> Result<Vec<CapacityReport>> {
    let mut rows = Vec::new();
    for id in preset_ids() {
        let p = crate::profile::preset(id)?;
        for &n in glyph_counts {
            rows.push(report_opts(id, &p, n, opts, cell_scale)?);
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::bin10;

    #[test]
    fn bin10_ten_glyphs() {
        let p = bin10();
        let r = report("bin10", &p, 10, Integrity::None, 1).unwrap();
        assert_eq!(r.bits_per_glyph, 100);
        assert_eq!(r.total_bits, 1000);
        assert_eq!(r.payload_bytes, 119); // 1000 - 48 header bits
        assert_eq!((r.image_w, r.image_h), (100, 10));
    }
}
