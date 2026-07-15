//! Multi-glyph composition: strip, grid, margins, gaps, separators.
//!
//! Geometry is pure (no pixels). [`crate::render`] paints using
//! [`glyph_origins`] / [`image_size`].

use crate::error::{GlyphixError, Result};
use crate::profile::GlyphProfile;

/// How to arrange multiple glyphs in the raster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum GlyphLayout {
    /// Left-to-right single row (default).
    HorizontalStrip,
    /// Left-to-right, wrap to next row after `columns` glyphs.
    /// Rows fill top → bottom. Partial last row is left-aligned.
    Grid {
        /// Glyphs per row (≥ 1).
        columns: u32,
    },
}

impl Default for GlyphLayout {
    fn default() -> Self {
        Self::HorizontalStrip
    }
}

impl GlyphLayout {
    /// Grid with the given column count.
    pub fn grid(columns: u32) -> Result<Self> {
        if columns == 0 {
            return Err(GlyphixError::InvalidRender(
                "grid columns must be ≥ 1".into(),
            ));
        }
        Ok(Self::Grid { columns })
    }

    /// Columns used for placement (`n` for a strip of `n` glyphs).
    pub fn columns_for(&self, glyph_count: usize) -> u32 {
        match *self {
            Self::HorizontalStrip => (glyph_count as u32).max(1),
            Self::Grid { columns } => columns,
        }
    }

    /// Number of rows for `glyph_count` glyphs.
    pub fn rows_for(&self, glyph_count: usize) -> u32 {
        if glyph_count == 0 {
            return 0;
        }
        match *self {
            Self::HorizontalStrip => 1,
            Self::Grid { columns } => (glyph_count as u32).div_ceil(columns),
        }
    }
}

/// Optional bar drawn between glyphs (in addition to gap).
///
/// Layout order along an axis: `[glyph][gap/2?][separator][gap/2?][glyph]…`
/// implemented as: step = glyph_size + gap + separator.thickness, with the
/// separator rect immediately after each glyph (except after the last in a row/col).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Separator {
    /// Device pixels (≥ 1 when used).
    pub thickness: u32,
    /// sRGB fill for the separator bar.
    pub color: [u8; 3],
}

impl Separator {
    /// Mid-gray bar of the given thickness (visible on black/white binary art).
    pub fn gray(thickness: u32) -> Result<Self> {
        if thickness == 0 {
            return Err(GlyphixError::InvalidRender(
                "separator thickness must be ≥ 1".into(),
            ));
        }
        Ok(Self {
            thickness,
            color: [128, 128, 128],
        })
    }

    /// Custom color bar.
    pub fn new(thickness: u32, color: [u8; 3]) -> Result<Self> {
        if thickness == 0 {
            return Err(GlyphixError::InvalidRender(
                "separator thickness must be ≥ 1".into(),
            ));
        }
        Ok(Self { thickness, color })
    }
}

/// Full layout parameters for a glyph composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutOptions {
    /// Device pixels per logical cell edge (\(S \ge 1\)).
    pub cell_scale: u32,
    /// Quiet margin around the whole composition (device pixels).
    pub margin: u32,
    /// Horizontal gap between glyphs (device pixels, not including separator).
    pub gap_x: u32,
    /// Vertical gap between rows (device pixels, not including separator).
    pub gap_y: u32,
    /// Arrangement.
    pub layout: GlyphLayout,
    /// Optional separator bars between adjacent glyphs.
    pub separator: Option<Separator>,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            cell_scale: 1,
            margin: 0,
            gap_x: 0,
            gap_y: 0,
            layout: GlyphLayout::HorizontalStrip,
            separator: None,
        }
    }
}

impl LayoutOptions {
    /// Scale-only defaults.
    pub fn scale(cell_scale: u32) -> Result<Self> {
        let mut o = Self::default();
        o.cell_scale = cell_scale;
        o.validate()?;
        Ok(o)
    }

    /// Horizontal strip with uniform gap.
    pub fn strip(cell_scale: u32, margin: u32, gap: u32) -> Result<Self> {
        let mut o = Self::default();
        o.cell_scale = cell_scale;
        o.margin = margin;
        o.gap_x = gap;
        o.gap_y = gap;
        o.validate()?;
        Ok(o)
    }

    /// Grid layout helper.
    pub fn grid(cell_scale: u32, columns: u32, margin: u32, gap: u32) -> Result<Self> {
        let mut o = Self::default();
        o.cell_scale = cell_scale;
        o.margin = margin;
        o.gap_x = gap;
        o.gap_y = gap;
        o.layout = GlyphLayout::grid(columns)?;
        o.validate()?;
        Ok(o)
    }

    pub fn validate(&self) -> Result<()> {
        if self.cell_scale == 0 {
            return Err(GlyphixError::InvalidRender(
                "cell_scale (S) must be ≥ 1".into(),
            ));
        }
        if let GlyphLayout::Grid { columns } = self.layout {
            if columns == 0 {
                return Err(GlyphixError::InvalidRender(
                    "grid columns must be ≥ 1".into(),
                ));
            }
        }
        if let Some(sep) = self.separator {
            if sep.thickness == 0 {
                return Err(GlyphixError::InvalidRender(
                    "separator thickness must be ≥ 1".into(),
                ));
            }
        }
        Ok(())
    }

    /// Separator thickness or 0.
    pub fn sep_thickness(&self) -> u32 {
        self.separator.map(|s| s.thickness).unwrap_or(0)
    }
}

/// Device size of one glyph tile (without margin/gap/separator).
pub fn glyph_pixel_size(profile: &GlyphProfile, scale: u32) -> Result<(u32, u32)> {
    if scale == 0 {
        return Err(GlyphixError::InvalidRender(
            "cell_scale (S) must be ≥ 1".into(),
        ));
    }
    Ok((
        profile
            .width
            .checked_mul(scale)
            .ok_or_else(|| GlyphixError::InvalidRender("width*scale overflow".into()))?,
        profile
            .height
            .checked_mul(scale)
            .ok_or_else(|| GlyphixError::InvalidRender("height*scale overflow".into()))?,
    ))
}

/// Step between glyph origins on X (glyph width + gap_x + separator).
pub fn step_x(profile: &GlyphProfile, opts: &LayoutOptions) -> Result<u32> {
    let (gw, _) = glyph_pixel_size(profile, opts.cell_scale)?;
    gw.checked_add(opts.gap_x)
        .and_then(|v| v.checked_add(opts.sep_thickness()))
        .ok_or_else(|| GlyphixError::InvalidRender("step_x overflow".into()))
}

/// Step between glyph origins on Y (glyph height + gap_y + separator).
pub fn step_y(profile: &GlyphProfile, opts: &LayoutOptions) -> Result<u32> {
    let (_, gh) = glyph_pixel_size(profile, opts.cell_scale)?;
    gh.checked_add(opts.gap_y)
        .and_then(|v| v.checked_add(opts.sep_thickness()))
        .ok_or_else(|| GlyphixError::InvalidRender("step_y overflow".into()))
}

/// Full image dimensions for `glyph_count` glyphs under `opts`.
pub fn image_size(
    profile: &GlyphProfile,
    glyph_count: usize,
    opts: &LayoutOptions,
) -> Result<(u32, u32)> {
    opts.validate()?;
    if glyph_count == 0 {
        return Err(GlyphixError::EmptyGlyphSequence);
    }
    let (gw, gh) = glyph_pixel_size(profile, opts.cell_scale)?;
    let cols = opts.layout.columns_for(glyph_count).min(glyph_count as u32);
    let rows = opts.layout.rows_for(glyph_count);
    let sep = opts.sep_thickness();

    // content width: cols * gw + (cols-1) * (gap_x + sep)
    let h_gaps = cols.saturating_sub(1);
    let content_w = gw
        .checked_mul(cols)
        .and_then(|w| w.checked_add(h_gaps.checked_mul(opts.gap_x.checked_add(sep)?)?))
        .ok_or_else(|| GlyphixError::InvalidRender("content width overflow".into()))?;

    let v_gaps = rows.saturating_sub(1);
    let content_h = gh
        .checked_mul(rows)
        .and_then(|h| h.checked_add(v_gaps.checked_mul(opts.gap_y.checked_add(sep)?)?))
        .ok_or_else(|| GlyphixError::InvalidRender("content height overflow".into()))?;

    let width = content_w
        .checked_add(opts.margin.saturating_mul(2))
        .ok_or_else(|| GlyphixError::InvalidRender("image width overflow".into()))?;
    let height = content_h
        .checked_add(opts.margin.saturating_mul(2))
        .ok_or_else(|| GlyphixError::InvalidRender("image height overflow".into()))?;
    Ok((width, height))
}

/// Top-left origin of glyph index `i` (0-based reading order).
pub fn glyph_origin(
    profile: &GlyphProfile,
    glyph_index: usize,
    glyph_count: usize,
    opts: &LayoutOptions,
) -> Result<(u32, u32)> {
    if glyph_index >= glyph_count {
        return Err(GlyphixError::InvalidRender(format!(
            "glyph index {glyph_index} out of range for count {glyph_count}"
        )));
    }
    let sx = step_x(profile, opts)?;
    let sy = step_y(profile, opts)?;
    let (col, row) = match opts.layout {
        GlyphLayout::HorizontalStrip => (glyph_index as u32, 0u32),
        GlyphLayout::Grid { columns } => {
            let col = (glyph_index as u32) % columns;
            let row = (glyph_index as u32) / columns;
            (col, row)
        }
    };
    let x = opts
        .margin
        .checked_add(col.checked_mul(sx).ok_or_else(|| {
            GlyphixError::InvalidRender("glyph x overflow".into())
        })?)
        .ok_or_else(|| GlyphixError::InvalidRender("glyph x overflow".into()))?;
    let y = opts
        .margin
        .checked_add(row.checked_mul(sy).ok_or_else(|| {
            GlyphixError::InvalidRender("glyph y overflow".into())
        })?)
        .ok_or_else(|| GlyphixError::InvalidRender("glyph y overflow".into()))?;
    Ok((x, y))
}

/// All glyph origins in order.
pub fn glyph_origins(
    profile: &GlyphProfile,
    glyph_count: usize,
    opts: &LayoutOptions,
) -> Result<Vec<(u32, u32)>> {
    (0..glyph_count)
        .map(|i| glyph_origin(profile, i, glyph_count, opts))
        .collect()
}

/// Infer glyph count from image dimensions (clean layout).
pub fn infer_glyph_count(
    profile: &GlyphProfile,
    image_w: u32,
    image_h: u32,
    opts: &LayoutOptions,
) -> Result<usize> {
    opts.validate()?;
    let (gw, gh) = glyph_pixel_size(profile, opts.cell_scale)?;
    let margin2 = opts.margin.saturating_mul(2);
    if image_w < margin2 || image_h < margin2 {
        return Err(GlyphixError::InvalidRaster(
            "image smaller than margins".into(),
        ));
    }
    let inner_w = image_w - margin2;
    let inner_h = image_h - margin2;
    let sep = opts.sep_thickness();
    let step_x = gw
        .checked_add(opts.gap_x)
        .and_then(|v| v.checked_add(sep))
        .ok_or_else(|| GlyphixError::InvalidRender("step_x overflow".into()))?;
    let step_y = gh
        .checked_add(opts.gap_y)
        .and_then(|v| v.checked_add(sep))
        .ok_or_else(|| GlyphixError::InvalidRender("step_y overflow".into()))?;

    match opts.layout {
        GlyphLayout::HorizontalStrip => {
            if inner_h != gh {
                return Err(GlyphixError::InvalidRaster(format!(
                    "strip height {inner_h} != glyph height {gh}"
                )));
            }
            // n * gw + (n-1)*(gap_x+sep) = inner_w
            let pitch = opts.gap_x.saturating_add(sep);
            let n = if pitch == 0 {
                if inner_w % gw != 0 {
                    return Err(GlyphixError::InvalidRaster(format!(
                        "inner width {inner_w} not divisible by glyph width {gw}"
                    )));
                }
                (inner_w / gw) as usize
            } else {
                let numer = inner_w
                    .checked_add(pitch)
                    .ok_or_else(|| GlyphixError::InvalidRaster("width overflow".into()))?;
                if numer % step_x != 0 {
                    return Err(GlyphixError::InvalidRaster(format!(
                        "inner width {inner_w} does not match strip step {step_x}"
                    )));
                }
                (numer / step_x) as usize
            };
            if n == 0 {
                return Err(GlyphixError::EmptyGlyphSequence);
            }
            Ok(n)
        }
        GlyphLayout::Grid { columns } => {
            // Full rows of `columns`, last row may be shorter — but clean images
            // use exact image_size for a known n. Infer maximal n that fits:
            // rows from height, cols fixed; n cannot be determined uniquely if last
            // row is partial without knowing n. Require exact match for some n.
            let pitch_y = opts.gap_y.saturating_add(sep);
            let rows = if pitch_y == 0 {
                if inner_h % gh != 0 {
                    return Err(GlyphixError::InvalidRaster(format!(
                        "inner height {inner_h} not divisible by glyph height {gh}"
                    )));
                }
                inner_h / gh
            } else {
                let numer = inner_h
                    .checked_add(pitch_y)
                    .ok_or_else(|| GlyphixError::InvalidRaster("height overflow".into()))?;
                if numer % step_y != 0 {
                    return Err(GlyphixError::InvalidRaster(format!(
                        "inner height {inner_h} does not match grid step {step_y}"
                    )));
                }
                numer / step_y
            };
            if rows == 0 {
                return Err(GlyphixError::EmptyGlyphSequence);
            }
            // Width should match full columns (composition always uses full columns width)
            let pitch_x = opts.gap_x.saturating_add(sep);
            let expect_w = gw
                .checked_mul(columns)
                .and_then(|w| {
                    w.checked_add(columns.saturating_sub(1).checked_mul(pitch_x)?)
                })
                .ok_or_else(|| GlyphixError::InvalidRender("grid width overflow".into()))?;
            if inner_w != expect_w {
                return Err(GlyphixError::InvalidRaster(format!(
                    "grid inner width {inner_w} != expected {expect_w} for {columns} columns"
                )));
            }
            // Grid images are full rectangles (render pads the last row with blank glyphs).
            let n = (rows * columns) as usize;
            let (w, h) = image_size(profile, n, opts)?;
            if w == image_w && h == image_h {
                return Ok(n);
            }
            Err(GlyphixError::InvalidRaster(format!(
                "could not infer grid glyph count for image {image_w}x{image_h} (rows={rows}, cols={columns})"
            )))
        }
    }
}

/// Pad glyph list so a grid layout fills a complete rectangle (last row blank-padded).
pub fn pad_glyphs_for_grid(
    profile: &GlyphProfile,
    glyphs: &[crate::grid::Grid],
    columns: u32,
) -> Result<Vec<crate::grid::Grid>> {
    if columns == 0 {
        return Err(GlyphixError::InvalidRender(
            "grid columns must be ≥ 1".into(),
        ));
    }
    if glyphs.is_empty() {
        return Err(GlyphixError::EmptyGlyphSequence);
    }
    let rows = (glyphs.len() as u32).div_ceil(columns);
    let target = (rows * columns) as usize;
    if target > profile.max_glyphs {
        return Err(GlyphixError::GlyphCountExceeded {
            count: target,
            cap: profile.max_glyphs,
        });
    }
    let mut out = glyphs.to_vec();
    while out.len() < target {
        out.push(crate::grid::Grid::from_profile(profile));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::bin8;

    #[test]
    fn strip_dimensions() {
        let p = bin8();
        let opts = LayoutOptions::strip(2, 1, 1).unwrap();
        // glyph 16x16, 3 glyphs: 16*3 + 1*2 + margin 2 = 48+2+2=52 wide, 16+2=18 tall
        let (w, h) = image_size(&p, 3, &opts).unwrap();
        assert_eq!((w, h), (52, 18));
    }

    #[test]
    fn grid_2x2() {
        let p = bin8();
        let opts = LayoutOptions::grid(1, 2, 0, 0).unwrap();
        let (w, h) = image_size(&p, 4, &opts).unwrap();
        assert_eq!((w, h), (16, 16)); // 8*1 * 2 cols, 2 rows
        assert_eq!(glyph_origin(&p, 0, 4, &opts).unwrap(), (0, 0));
        assert_eq!(glyph_origin(&p, 1, 4, &opts).unwrap(), (8, 0));
        assert_eq!(glyph_origin(&p, 2, 4, &opts).unwrap(), (0, 8));
        assert_eq!(glyph_origin(&p, 3, 4, &opts).unwrap(), (8, 8));
    }

    #[test]
    fn separator_widens_strip() {
        let p = bin8();
        let mut opts = LayoutOptions::strip(1, 0, 0).unwrap();
        opts.separator = Some(Separator::gray(2).unwrap());
        let (w, _) = image_size(&p, 2, &opts).unwrap();
        // 8+8+2 sep
        assert_eq!(w, 18);
    }

    #[test]
    fn infer_strip_count() {
        let p = bin8();
        let opts = LayoutOptions::strip(4, 0, 2).unwrap();
        let (w, h) = image_size(&p, 5, &opts).unwrap();
        assert_eq!(infer_glyph_count(&p, w, h, &opts).unwrap(), 5);
    }
}
