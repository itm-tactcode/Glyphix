//! Render glyphs to exact bitmaps / SVG and parse **clean** rasters back.
//!
//! # Cell scale
//!
//! Each logical cell is drawn as an \(S \times S\) block of device pixels
//! (`RenderOptions::cell_scale`). No anti-aliasing — canonical files are
//! piecewise-constant so lossless re-parse is possible.
//!
//! # Layout (Phase 3)
//!
//! Horizontal strip of glyphs, left → right, optional quiet **margin** (device
//! pixels) and **gap** between glyphs. Full multi-row layout is Phase 4.
//!
//! # PNG
//!
//! PNG encode/decode requires the **`render`** feature (`image` crate).
//! SVG and in-memory RGBA work without it.

use crate::color::{index_to_rgb, rgb_to_index, Rgb};
use crate::error::{GlyphixError, Result};
use crate::grid::Grid;
use crate::profile::GlyphProfile;

/// How to arrange multiple glyphs in the raster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum GlyphLayout {
    /// Left-to-right strip (Phase 3 default).
    #[default]
    HorizontalStrip,
}

/// Options for raster / SVG rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    /// Device pixels per logical cell edge (\(S \ge 1\)). Font-size analogue.
    pub cell_scale: u32,
    /// Quiet margin around the whole strip (device pixels).
    pub margin: u32,
    /// Gap between adjacent glyphs (device pixels).
    pub gap: u32,
    /// Arrangement of glyphs.
    pub layout: GlyphLayout,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            cell_scale: 1,
            margin: 0,
            gap: 0,
            layout: GlyphLayout::HorizontalStrip,
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

    /// Validate scale ≥ 1.
    pub fn validate(&self) -> Result<()> {
        if self.cell_scale == 0 {
            return Err(GlyphixError::InvalidRender(
                "cell_scale (S) must be ≥ 1".into(),
            ));
        }
        Ok(())
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

/// Full image dimensions for a glyph strip.
pub fn image_dimensions(
    profile: &GlyphProfile,
    glyph_count: usize,
    opts: &RenderOptions,
) -> Result<(u32, u32)> {
    opts.validate()?;
    if glyph_count == 0 {
        return Err(GlyphixError::EmptyGlyphSequence);
    }
    let (gw, gh) = glyph_pixel_size(profile, opts.cell_scale)?;
    let n = glyph_count as u32;
    let gaps = n
        .saturating_sub(1)
        .checked_mul(opts.gap)
        .ok_or_else(|| GlyphixError::InvalidRender("gap overflow".into()))?;
    let content_w = gw
        .checked_mul(n)
        .and_then(|w| w.checked_add(gaps))
        .ok_or_else(|| GlyphixError::InvalidRender("content width overflow".into()))?;
    let width = content_w
        .checked_add(opts.margin.saturating_mul(2))
        .ok_or_else(|| GlyphixError::InvalidRender("image width overflow".into()))?;
    let height = gh
        .checked_add(opts.margin.saturating_mul(2))
        .ok_or_else(|| GlyphixError::InvalidRender("image height overflow".into()))?;
    Ok((width, height))
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

/// Render glyphs to an exact RGBA image (no anti-alias).
pub fn render_rgba(
    profile: &GlyphProfile,
    glyphs: &[Grid],
    opts: &RenderOptions,
) -> Result<RgbaImage> {
    opts.validate()?;
    if glyphs.is_empty() {
        return Err(GlyphixError::EmptyGlyphSequence);
    }
    for g in glyphs {
        g.validate_profile(profile)?;
    }

    let (width, height) = image_dimensions(profile, glyphs.len(), opts)?;
    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    // Quiet margin: transparent black by default; fill margin with opaque black for PNG friendliness.
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
    let (gw, _gh) = glyph_pixel_size(profile, s)?;
    let mut ox = opts.margin;

    for g in glyphs {
        for cy in 0..profile.height {
            for cx in 0..profile.width {
                let idx = g.get(cx, cy)?;
                let [r, gch, b] = index_to_rgb(profile.palette_size, idx)?;
                let px = ox + cx * s;
                let py = opts.margin + cy * s;
                fill_rect(&mut pixels, width, height, px, py, s, s, [r, gch, b, 255]);
            }
        }
        ox = ox
            .checked_add(gw)
            .and_then(|x| x.checked_add(opts.gap))
            .ok_or_else(|| GlyphixError::InvalidRender("glyph x overflow".into()))?;
    }

    Ok(RgbaImage {
        width,
        height,
        pixels,
    })
}

/// Parse a **clean** RGBA raster back into glyphs.
///
/// Requirements:
/// - Dimensions match [`image_dimensions`] for some glyph count ≤ `max_glyphs`
/// - Each \(S\times S\) cell block is a solid color (all pixels identical RGB)
/// - Margin pixels are ignored for data (not validated strictly)
/// - Colors must map exactly via [`rgb_to_index`]
pub fn parse_rgba(
    profile: &GlyphProfile,
    image: &RgbaImage,
    opts: &RenderOptions,
) -> Result<Vec<Grid>> {
    opts.validate()?;
    let s = opts.cell_scale;
    let (gw, gh) = glyph_pixel_size(profile, s)?;

    if image.height != gh + opts.margin.saturating_mul(2) {
        return Err(GlyphixError::InvalidRaster(format!(
            "image height {} does not match expected {} (profile {}x{}, scale {}, margin {})",
            image.height,
            gh + opts.margin * 2,
            profile.width,
            profile.height,
            s,
            opts.margin
        )));
    }

    let inner_w = image
        .width
        .checked_sub(opts.margin.saturating_mul(2))
        .ok_or_else(|| GlyphixError::InvalidRaster("margin larger than width".into()))?;

    // Solve: n * gw + (n-1) * gap = inner_w  =>  n * (gw+gap) - gap = inner_w
    let step = gw
        .checked_add(opts.gap)
        .ok_or_else(|| GlyphixError::InvalidRender("step overflow".into()))?;
    if step == 0 {
        return Err(GlyphixError::InvalidRender("glyph width is 0".into()));
    }
    let n = if opts.gap == 0 {
        if inner_w % gw != 0 {
            return Err(GlyphixError::InvalidRaster(format!(
                "inner width {inner_w} not divisible by glyph width {gw}"
            )));
        }
        (inner_w / gw) as usize
    } else {
        // inner_w + gap = n * step
        let numer = inner_w
            .checked_add(opts.gap)
            .ok_or_else(|| GlyphixError::InvalidRaster("width+gap overflow".into()))?;
        if numer % step != 0 {
            return Err(GlyphixError::InvalidRaster(format!(
                "inner width {inner_w} does not match glyph strip with gap {}",
                opts.gap
            )));
        }
        (numer / step) as usize
    };

    if n == 0 {
        return Err(GlyphixError::EmptyGlyphSequence);
    }
    if n > profile.max_glyphs {
        return Err(GlyphixError::GlyphCountExceeded {
            count: n,
            cap: profile.max_glyphs,
        });
    }

    let expected = image_dimensions(profile, n, opts)?;
    if image.width != expected.0 || image.height != expected.1 {
        return Err(GlyphixError::InvalidRaster(format!(
            "image size {}x{} != expected {}x{} for {n} glyphs",
            image.width, image.height, expected.0, expected.1
        )));
    }

    let mut glyphs = Vec::with_capacity(n);
    for gi in 0..n {
        let mut grid = Grid::from_profile(profile);
        let ox = opts.margin + gi as u32 * step;
        let oy = opts.margin;
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
    if glyphs.is_empty() {
        return Err(GlyphixError::EmptyGlyphSequence);
    }
    for g in glyphs {
        g.validate_profile(profile)?;
    }

    let (width, height) = image_dimensions(profile, glyphs.len(), opts)?;
    let s = opts.cell_scale;
    let (gw, _) = glyph_pixel_size(profile, s)?;
    let step = gw + opts.gap;

    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    out.push('\n');
    out.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" shape-rendering="crispEdges">"#
    ));
    out.push('\n');
    // Background
    out.push_str(&format!(
        r##"<rect width="{width}" height="{height}" fill="#000000"/>"##
    ));
    out.push('\n');

    for (gi, g) in glyphs.iter().enumerate() {
        let ox = opts.margin + gi as u32 * step;
        let oy = opts.margin;
        for cy in 0..profile.height {
            for cx in 0..profile.width {
                let idx = g.get(cx, cy)?;
                let [r, gc, b] = index_to_rgb(profile.palette_size, idx)?;
                // Skip pure black cells (background already black) for smaller SVG;
                // still exact when composited on black.
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
        // BR cell is (7,7) → device rect [28..32) x [28..32)
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
        // BR white rect at scale 2: x=14 y=14
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
            layout: GlyphLayout::HorizontalStrip,
        };
        let img = render_rgba(&p, &glyphs, &opts).unwrap();
        let back = parse_rgba(&p, &img, &opts).unwrap();
        assert_eq!(crate::decode(&p, &back).unwrap(), payload);
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
        // Corrupt one device pixel inside a cell
        let i = 0;
        img.pixels[i] = 255;
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
