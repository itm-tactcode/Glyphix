//! Render glyphs to exact bitmaps / SVG and parse **clean** rasters back.
//!
//! # Cell scale
//!
//! Each logical cell is drawn as an \(S \times S\) block of device pixels
//! (`RenderOptions::cell_scale`). No anti-aliasing — canonical files are
//! piecewise-constant so lossless re-parse is possible.
//!
//! # Layout (Phase 4)
//!
//! Horizontal **strip** or **grid** of glyphs, quiet **margin**, gaps, and
//! optional **separator** bars. Geometry lives in [`crate::layout`].
//!
//! # PNG
//!
//! PNG encode/decode requires the **`render`** feature (`image` crate).
//! SVG and in-memory RGBA work without it.

use crate::color::{index_to_rgb, rgb_to_index, Rgb};
use crate::error::{GlyphixError, Result};
use crate::grid::Grid;
use crate::layout::{
    glyph_origin, glyph_pixel_size as layout_glyph_pixel_size, image_size, infer_glyph_count,
    GlyphLayout, LayoutOptions, Separator,
};
use crate::profile::GlyphProfile;

pub use crate::layout::glyph_origins;

/// Options for raster / SVG rendering (wraps [`LayoutOptions`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    /// Device pixels per logical cell edge (\(S \ge 1\)). Font-size analogue.
    pub cell_scale: u32,
    /// Quiet margin around the whole composition (device pixels).
    pub margin: u32,
    /// Horizontal gap between glyphs (device pixels, not including separator).
    pub gap: u32,
    /// Vertical gap between rows; if `None`, uses `gap`.
    pub gap_y: Option<u32>,
    /// Arrangement of glyphs.
    pub layout: GlyphLayout,
    /// Optional separator bars between adjacent glyphs.
    pub separator: Option<Separator>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            cell_scale: 1,
            margin: 0,
            gap: 0,
            gap_y: None,
            layout: GlyphLayout::HorizontalStrip,
            separator: None,
        }
    }
}

impl RenderOptions {
    /// Build options with cell scale \(S\) (other fields default).
    pub fn scale(cell_scale: u32) -> Result<Self> {
        let mut o = Self::default();
        o.cell_scale = cell_scale;
        o.validate()?;
        Ok(o)
    }

    /// Horizontal strip convenience.
    pub fn strip(cell_scale: u32, margin: u32, gap: u32) -> Result<Self> {
        let mut o = Self::default();
        o.cell_scale = cell_scale;
        o.margin = margin;
        o.gap = gap;
        o.validate()?;
        Ok(o)
    }

    /// Grid convenience (`columns` per row).
    pub fn grid(cell_scale: u32, columns: u32, margin: u32, gap: u32) -> Result<Self> {
        let mut o = Self::default();
        o.cell_scale = cell_scale;
        o.margin = margin;
        o.gap = gap;
        o.layout = GlyphLayout::grid(columns)?;
        o.validate()?;
        Ok(o)
    }

    /// Convert to pure layout geometry.
    pub fn to_layout(&self) -> LayoutOptions {
        LayoutOptions {
            cell_scale: self.cell_scale,
            margin: self.margin,
            gap_x: self.gap,
            gap_y: self.gap_y.unwrap_or(self.gap),
            layout: self.layout,
            separator: self.separator,
        }
    }

    /// Validate scale and layout.
    pub fn validate(&self) -> Result<()> {
        self.to_layout().validate()
    }
}

/// In-memory RGBA8 image (row-major, 4 bytes per pixel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    /// Length = width * height * 4, order RGBA.
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    /// Pixel at \((x,y)\) as RGBA.
    pub fn get_rgba(&self, x: u32, y: u32) -> Result<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return Err(GlyphixError::OutOfBounds {
                x,
                y,
                width: self.width,
                height: self.height,
            });
        }
        let i = ((y * self.width + x) * 4) as usize;
        Ok([
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ])
    }

    /// RGB only (alpha ignored).
    pub fn get_rgb(&self, x: u32, y: u32) -> Result<Rgb> {
        let [r, g, b, _] = self.get_rgba(x, y)?;
        Ok([r, g, b])
    }
}

/// Device size of one glyph tile (without margin/gap).
pub fn glyph_pixel_size(profile: &GlyphProfile, scale: u32) -> Result<(u32, u32)> {
    layout_glyph_pixel_size(profile, scale)
}

/// Full image dimensions for a glyph composition.
pub fn image_dimensions(
    profile: &GlyphProfile,
    glyph_count: usize,
    opts: &RenderOptions,
) -> Result<(u32, u32)> {
    image_size(profile, glyph_count, &opts.to_layout())
}

fn put_rgba(buf: &mut [u8], width: u32, x: u32, y: u32, rgba: [u8; 4]) {
    let i = ((y * width + x) * 4) as usize;
    buf[i] = rgba[0];
    buf[i + 1] = rgba[1];
    buf[i + 2] = rgba[2];
    buf[i + 3] = rgba[3];
}

fn fill_rect(
    buf: &mut [u8],
    width: u32,
    height: u32,
    x0: u32,
    y0: u32,
    rw: u32,
    rh: u32,
    rgba: [u8; 4],
) {
    let x1 = (x0 + rw).min(width);
    let y1 = (y0 + rh).min(height);
    for y in y0..y1 {
        for x in x0..x1 {
            put_rgba(buf, width, x, y, rgba);
        }
    }
}

fn paint_glyph_at(
    buf: &mut [u8],
    img_w: u32,
    img_h: u32,
    profile: &GlyphProfile,
    grid: &Grid,
    ox: u32,
    oy: u32,
    scale: u32,
) -> Result<()> {
    for cy in 0..profile.height {
        for cx in 0..profile.width {
            let idx = grid.get(cx, cy)?;
            let [r, gch, b] = index_to_rgb(profile.palette_size, idx)?;
            let px = ox + cx * scale;
            let py = oy + cy * scale;
            fill_rect(buf, img_w, img_h, px, py, scale, scale, [r, gch, b, 255]);
        }
    }
    Ok(())
}

fn paint_separators(
    buf: &mut [u8],
    img_w: u32,
    img_h: u32,
    profile: &GlyphProfile,
    glyph_count: usize,
    layout: &LayoutOptions,
) -> Result<()> {
    let Some(sep) = layout.separator else {
        return Ok(());
    };
    let (gw, gh) = layout_glyph_pixel_size(profile, layout.cell_scale)?;
    let [r, g, b] = sep.color;
    let rgba = [r, g, b, 255];
    let cols = layout.layout.columns_for(glyph_count);
    let rows = layout.layout.rows_for(glyph_count);

    for i in 0..glyph_count {
        let (ox, oy) = glyph_origin(profile, i, glyph_count, layout)?;
        let (col, row) = match layout.layout {
            GlyphLayout::HorizontalStrip => (i as u32, 0u32),
            GlyphLayout::Grid { columns } => ((i as u32) % columns, (i as u32) / columns),
        };
        // Vertical separator to the right of glyph (if not last in row)
        let last_in_row = match layout.layout {
            GlyphLayout::HorizontalStrip => i + 1 >= glyph_count,
            GlyphLayout::Grid { columns } => {
                col + 1 >= columns || i + 1 >= glyph_count
            }
        };
        if !last_in_row && sep.thickness > 0 {
            // Separator immediately after glyph tile (before next origin's gap+sep budget).
            let sx = ox + gw;
            fill_rect(buf, img_w, img_h, sx, oy, sep.thickness, gh, rgba);
        }
        // Horizontal separator below glyph (if not last row and glyph has row-mate below)
        let last_row = row + 1 >= rows;
        if !last_row {
            // Only draw if there is a glyph in the cell below or we always draw full row bars
            let sy = oy + gh;
            fill_rect(buf, img_w, img_h, ox, sy, gw, sep.thickness, rgba);
        }
        let _ = cols; // used via layout
    }
    Ok(())
}

fn prepare_glyphs(
    profile: &GlyphProfile,
    glyphs: &[Grid],
    opts: &RenderOptions,
) -> Result<Vec<Grid>> {
    if glyphs.is_empty() {
        return Err(GlyphixError::EmptyGlyphSequence);
    }
    for g in glyphs {
        g.validate_profile(profile)?;
    }
    match opts.layout {
        GlyphLayout::Grid { columns } => {
            crate::layout::pad_glyphs_for_grid(profile, glyphs, columns)
        }
        GlyphLayout::HorizontalStrip => Ok(glyphs.to_vec()),
    }
}

/// Render glyphs to an exact RGBA image (no anti-alias).
///
/// Grid layouts pad the last row with blank (all-zero) glyphs so the image is a
/// full rectangle and clean parse can recover glyph count uniquely.
pub fn render_rgba(
    profile: &GlyphProfile,
    glyphs: &[Grid],
    opts: &RenderOptions,
) -> Result<RgbaImage> {
    opts.validate()?;
    let glyphs = prepare_glyphs(profile, glyphs, opts)?;
    let layout = opts.to_layout();
    let (width, height) = image_size(profile, glyphs.len(), &layout)?;
    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    fill_rect(
        &mut pixels,
        width,
        height,
        0,
        0,
        width,
        height,
        [0, 0, 0, 255],
    );

    let s = opts.cell_scale;
    for (i, g) in glyphs.iter().enumerate() {
        let (ox, oy) = glyph_origin(profile, i, glyphs.len(), &layout)?;
        paint_glyph_at(&mut pixels, width, height, profile, g, ox, oy, s)?;
    }
    paint_separators(&mut pixels, width, height, profile, glyphs.len(), &layout)?;

    Ok(RgbaImage {
        width,
        height,
        pixels,
    })
}

/// Parse a **clean** RGBA raster back into glyphs.
pub fn parse_rgba(
    profile: &GlyphProfile,
    image: &RgbaImage,
    opts: &RenderOptions,
) -> Result<Vec<Grid>> {
    opts.validate()?;
    let layout = opts.to_layout();
    let n = infer_glyph_count(profile, image.width, image.height, &layout)?;
    if n > profile.max_glyphs {
        return Err(GlyphixError::GlyphCountExceeded {
            count: n,
            cap: profile.max_glyphs,
        });
    }
    let expected = image_size(profile, n, &layout)?;
    if image.width != expected.0 || image.height != expected.1 {
        return Err(GlyphixError::InvalidRaster(format!(
            "image size {}x{} != expected {}x{} for {n} glyphs",
            image.width, image.height, expected.0, expected.1
        )));
    }

    let s = opts.cell_scale;
    let mut glyphs = Vec::with_capacity(n);
    for i in 0..n {
        let mut grid = Grid::from_profile(profile);
        let (ox, oy) = glyph_origin(profile, i, n, &layout)?;
        for cy in 0..profile.height {
            for cx in 0..profile.width {
                let px = ox + cx * s;
                let py = oy + cy * s;
                let rgb = sample_solid_block(image, px, py, s)?;
                let idx = rgb_to_index(profile.palette_size, rgb)?;
                grid.set(cx, cy, idx)?;
            }
        }
        glyphs.push(grid);
    }
    Ok(glyphs)
}

/// Require all pixels in the \(S\times S\) block equal; return that RGB.
fn sample_solid_block(image: &RgbaImage, x0: u32, y0: u32, s: u32) -> Result<Rgb> {
    let first = image.get_rgb(x0, y0)?;
    for dy in 0..s {
        for dx in 0..s {
            let rgb = image.get_rgb(x0 + dx, y0 + dy)?;
            if rgb != first {
                return Err(GlyphixError::InvalidRaster(format!(
                    "non-uniform cell block at ({},{}) scale {s}: {:?} vs {:?}",
                    x0, y0, first, rgb
                )));
            }
        }
    }
    Ok(first)
}

/// Render glyphs to an SVG document (exact axis-aligned rects, no AA).
pub fn render_svg(
    profile: &GlyphProfile,
    glyphs: &[Grid],
    opts: &RenderOptions,
) -> Result<String> {
    opts.validate()?;
    let glyphs = prepare_glyphs(profile, glyphs, opts)?;
    let layout = opts.to_layout();
    let (width, height) = image_size(profile, glyphs.len(), &layout)?;
    let s = opts.cell_scale;
    let (gw, gh) = layout_glyph_pixel_size(profile, s)?;

    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    out.push('\n');
    out.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" shape-rendering="crispEdges">"#
    ));
    out.push('\n');
    out.push_str(&format!(
        r##"<rect width="{width}" height="{height}" fill="#000000"/>"##
    ));
    out.push('\n');

    if let Some(sep) = layout.separator {
        let [r, g, b] = sep.color;
        for i in 0..glyphs.len() {
            let (ox, oy) = glyph_origin(profile, i, glyphs.len(), &layout)?;
            let (col, row) = match layout.layout {
                GlyphLayout::HorizontalStrip => (i as u32, 0u32),
                GlyphLayout::Grid { columns } => ((i as u32) % columns, (i as u32) / columns),
            };
            let rows = layout.layout.rows_for(glyphs.len());
            let last_in_row = match layout.layout {
                GlyphLayout::HorizontalStrip => i + 1 >= glyphs.len(),
                GlyphLayout::Grid { columns } => col + 1 >= columns || i + 1 >= glyphs.len(),
            };
            if !last_in_row {
                let sx = ox + gw;
                out.push_str(&format!(
                    r##"<rect x="{sx}" y="{oy}" width="{}" height="{gh}" fill="#{r:02X}{g:02X}{b:02X}"/>"##,
                    sep.thickness
                ));
                out.push('\n');
            }
            if row + 1 < rows {
                let sy = oy + gh;
                out.push_str(&format!(
                    r##"<rect x="{ox}" y="{sy}" width="{gw}" height="{}" fill="#{r:02X}{g:02X}{b:02X}"/>"##,
                    sep.thickness
                ));
                out.push('\n');
            }
        }
    }

    for (i, g) in glyphs.iter().enumerate() {
        let (ox, oy) = glyph_origin(profile, i, glyphs.len(), &layout)?;
        for cy in 0..profile.height {
            for cx in 0..profile.width {
                let idx = g.get(cx, cy)?;
                let [r, gc, b] = index_to_rgb(profile.palette_size, idx)?;
                if r == 0 && gc == 0 && b == 0 {
                    continue;
                }
                let px = ox + cx * s;
                let py = oy + cy * s;
                out.push_str(&format!(
                    r##"<rect x="{px}" y="{py}" width="{s}" height="{s}" fill="#{r:02X}{gc:02X}{b:02X}"/>"##
                ));
                out.push('\n');
            }
        }
    }
    out.push_str("</svg>\n");
    Ok(out)
}

// --- PNG I/O (feature = "render") ------------------------------------------------

/// Write an RGBA image as a PNG file (exact pixels).
#[cfg(feature = "render")]
pub fn write_png(path: impl AsRef<std::path::Path>, image: &RgbaImage) -> Result<()> {
    use image::{ImageBuffer, Rgba};
    let img: ImageBuffer<Rgba<u8>, _> =
        ImageBuffer::from_raw(image.width, image.height, image.pixels.clone()).ok_or_else(
            || GlyphixError::InvalidRender("RGBA buffer size mismatch for PNG".into()),
        )?;
    img.save(path)
        .map_err(|e| GlyphixError::Io(format!("write PNG: {e}")))?;
    Ok(())
}

/// Read a PNG into an RGBA image.
#[cfg(feature = "render")]
pub fn read_png(path: impl AsRef<std::path::Path>) -> Result<RgbaImage> {
    let img = image::open(path)
        .map_err(|e| GlyphixError::Io(format!("read PNG: {e}")))?
        .to_rgba8();
    let (width, height) = img.dimensions();
    Ok(RgbaImage {
        width,
        height,
        pixels: img.into_raw(),
    })
}

/// Encode payload → glyphs → PNG file.
#[cfg(feature = "render")]
pub fn encode_png(
    profile: &GlyphProfile,
    payload: &[u8],
    path: impl AsRef<std::path::Path>,
    opts: &RenderOptions,
    encode_opts: crate::codec::EncodeOptions,
) -> Result<()> {
    let glyphs = crate::encode_with(profile, payload, encode_opts)?;
    let img = render_rgba(profile, &glyphs, opts)?;
    write_png(path, &img)
}

/// Read clean PNG → glyphs → payload.
#[cfg(feature = "render")]
pub fn decode_png(
    profile: &GlyphProfile,
    path: impl AsRef<std::path::Path>,
    opts: &RenderOptions,
) -> Result<Vec<u8>> {
    let img = read_png(path)?;
    let glyphs = parse_rgba(profile, &img, opts)?;
    crate::decode(profile, &glyphs)
}

/// Encode payload → SVG string (no `render` feature required).
pub fn encode_svg(
    profile: &GlyphProfile,
    payload: &[u8],
    opts: &RenderOptions,
    encode_opts: crate::codec::EncodeOptions,
) -> Result<String> {
    let glyphs = crate::encode_with(profile, payload, encode_opts)?;
    render_svg(profile, &glyphs, opts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{encode, encode_with, EncodeOptions};
    use crate::pack::paint_value_u128;
    use crate::profile::{bin10, bin8, c8_8};
    use crate::Integrity;

    #[test]
    fn rgba_roundtrip_encode_decode() {
        let p = bin10();
        let payload = b"phase3-rgba";
        let glyphs = encode(&p, payload).unwrap();
        let opts = RenderOptions::scale(3).unwrap();
        let img = render_rgba(&p, &glyphs, &opts).unwrap();
        let back = parse_rgba(&p, &img, &opts).unwrap();
        assert_eq!(back, glyphs);
        assert_eq!(crate::decode(&p, &back).unwrap(), payload);
    }

    #[test]
    fn scale_four_value_one_bottom_right_block() {
        let p = bin8();
        let g = paint_value_u128(&p, 1).unwrap();
        let opts = RenderOptions::scale(4).unwrap();
        let img = render_rgba(&p, &[g], &opts).unwrap();
        for y in 28..32 {
            for x in 28..32 {
                assert_eq!(img.get_rgb(x, y).unwrap(), [255, 255, 255], "{x},{y}");
            }
        }
        assert_eq!(img.get_rgb(0, 0).unwrap(), [0, 0, 0]);
        assert_eq!(img.get_rgb(27, 31).unwrap(), [0, 0, 0]);
    }

    #[test]
    fn svg_contains_crisp_and_dimensions() {
        let p = bin8();
        let g = paint_value_u128(&p, 1).unwrap();
        let opts = RenderOptions::scale(2).unwrap();
        let svg = render_svg(&p, &[g], &opts).unwrap();
        assert!(svg.contains("crispEdges"));
        assert!(svg.contains(r#"width="16""#));
        assert!(svg.contains(r#"height="16""#));
        assert!(svg.contains(r#"x="14" y="14""#));
    }

    #[test]
    fn c8_color_roundtrip() {
        let p = c8_8();
        let payload = b"colors!";
        let glyphs = encode(&p, payload).unwrap();
        let opts = RenderOptions {
            cell_scale: 2,
            margin: 1,
            gap: 1,
            gap_y: None,
            layout: GlyphLayout::HorizontalStrip,
            separator: None,
        };
        let img = render_rgba(&p, &glyphs, &opts).unwrap();
        let back = parse_rgba(&p, &img, &opts).unwrap();
        assert_eq!(crate::decode(&p, &back).unwrap(), payload);
    }

    #[test]
    fn grid_layout_roundtrip() {
        let p = bin8();
        let payload = b"grid-layout-test-bytes!!!!"; // enough for several glyphs
        let glyphs = encode(&p, payload).unwrap();
        assert!(glyphs.len() >= 2);
        let opts = RenderOptions::grid(2, 2, 1, 1).unwrap();
        let img = render_rgba(&p, &glyphs, &opts).unwrap();
        let back = parse_rgba(&p, &img, &opts).unwrap();
        assert_eq!(crate::decode(&p, &back).unwrap(), payload);
    }

    #[test]
    fn separator_strip_roundtrip() {
        let p = bin8();
        let glyphs = encode(&p, b"sep!").unwrap();
        let mut opts = RenderOptions::strip(2, 0, 1).unwrap();
        opts.separator = Some(Separator::gray(2).unwrap());
        let img = render_rgba(&p, &glyphs, &opts).unwrap();
        let back = parse_rgba(&p, &img, &opts).unwrap();
        assert_eq!(crate::decode(&p, &back).unwrap(), b"sep!");
    }

    #[test]
    fn integrity_survives_rgba() {
        let p = bin10();
        let payload = b"crc-in-image";
        let glyphs = encode_with(
            &p,
            payload,
            EncodeOptions::with_integrity(Integrity::Crc32),
        )
        .unwrap();
        let opts = RenderOptions::scale(2).unwrap();
        let img = render_rgba(&p, &glyphs, &opts).unwrap();
        let back = parse_rgba(&p, &img, &opts).unwrap();
        assert_eq!(crate::decode(&p, &back).unwrap(), payload);
    }

    #[test]
    fn non_uniform_block_errors() {
        let p = bin8();
        let g = paint_value_u128(&p, 0).unwrap();
        let opts = RenderOptions::scale(2).unwrap();
        let mut img = render_rgba(&p, &[g], &opts).unwrap();
        img.pixels[0] = 255;
        assert!(parse_rgba(&p, &img, &opts).is_err());
    }
}

#[cfg(all(test, feature = "render"))]
mod png_tests {
    use super::*;
    use crate::profile::bin10;

    #[test]
    fn png_file_roundtrip() {
        let dir = std::env::temp_dir().join("glyphix_png_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("rt.png");
        let p = bin10();
        let payload = b"png-roundtrip";
        let opts = RenderOptions::scale(4).unwrap();
        encode_png(
            &p,
            payload,
            &path,
            &opts,
            crate::EncodeOptions::default(),
        )
        .unwrap();
        let out = decode_png(&p, &path, &opts).unwrap();
        assert_eq!(out, payload);
        let _ = std::fs::remove_file(&path);
    }
}
