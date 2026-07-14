//! Color index ↔ sRGB mapping for profiles.
//!
//! | Palette \(C\) | Mapping |
//! |---------------|---------|
//! | 2 | \(0\) = black `#000000`, \(1\) = white `#FFFFFF` |
//! | 8 | 3-bit RGB \(0bRGB\): each bit expands to 0 or 255 |
//! | 256 | grayscale \(i \mapsto (i,i,i)\) |
//! | \(2^{24}\) | packed `#RRGGBB` as 24-bit integer |
//! | other power-of-two | low bits interpreted as truncated RGB (see [`index_to_rgb`]) |

use crate::error::{GlyphixError, Result};
use crate::profile::GlyphProfile;

/// sRGB triple (no alpha).
pub type Rgb = [u8; 3];

/// Map a color index to sRGB for the given palette size.
pub fn index_to_rgb(palette_size: u32, index: u32) -> Result<Rgb> {
    if index >= palette_size {
        return Err(GlyphixError::ColorOutOfRange {
            index,
            palette_size,
        });
    }
    Ok(match palette_size {
        2 => {
            if index == 0 {
                [0, 0, 0]
            } else {
                [255, 255, 255]
            }
        }
        8 => {
            // 0bRGB → full 0/255 channels
            let r = if index & 0b100 != 0 { 255 } else { 0 };
            let g = if index & 0b010 != 0 { 255 } else { 0 };
            let b = if index & 0b001 != 0 { 255 } else { 0 };
            [r, g, b]
        }
        256 => {
            let v = index as u8;
            [v, v, v]
        }
        0x01_00_00_00 => {
            // 24-bit RGB
            let r = ((index >> 16) & 0xFF) as u8;
            let g = ((index >> 8) & 0xFF) as u8;
            let b = (index & 0xFF) as u8;
            [r, g, b]
        }
        other => {
            // Generic: treat index as little-endian-ish packed into log2(C) bits,
            // expand evenly across R,G,B for display (still invertible if bits ≤ 24).
            let bits = other.trailing_zeros();
            if bits <= 24 {
                let mask_bits = bits;
                let val = index & ((1u32 << mask_bits) - 1);
                // Split bits as evenly as possible R, G, B (MSB → R).
                let br = mask_bits / 3 + if mask_bits % 3 > 0 { 1 } else { 0 };
                let bg = mask_bits / 3 + if mask_bits % 3 > 1 { 1 } else { 0 };
                let bb = mask_bits / 3;
                let r_bits = if br > 0 {
                    (val >> (bg + bb)) & ((1 << br) - 1)
                } else {
                    0
                };
                let g_bits = if bg > 0 {
                    (val >> bb) & ((1 << bg) - 1)
                } else {
                    0
                };
                let b_bits = if bb > 0 { val & ((1 << bb) - 1) } else { 0 };
                [
                    expand_channel(r_bits, br),
                    expand_channel(g_bits, bg),
                    expand_channel(b_bits, bb),
                ]
            } else {
                // >24 bits per cell: use low 24 bits as RGB for display (lossy inverse).
                let v = index & 0xFF_FF_FF;
                [
                    ((v >> 16) & 0xFF) as u8,
                    ((v >> 8) & 0xFF) as u8,
                    (v & 0xFF) as u8,
                ]
            }
        }
    })
}

fn expand_channel(bits_val: u32, nbits: u32) -> u8 {
    if nbits == 0 {
        return 0;
    }
    if nbits >= 8 {
        return (bits_val & 0xFF) as u8;
    }
    // Replicate bits into 8-bit channel (exact for 1-bit → 0/255).
    let max = (1u32 << nbits) - 1;
    if max == 0 {
        return 0;
    }
    ((bits_val * 255) / max) as u8
}

/// Inverse of [`index_to_rgb`] for **exact** sRGB triples produced by that mapping.
pub fn rgb_to_index(palette_size: u32, rgb: Rgb) -> Result<u32> {
    let [r, g, b] = rgb;
    match palette_size {
        2 => {
            if rgb == [0, 0, 0] {
                Ok(0)
            } else if rgb == [255, 255, 255] {
                Ok(1)
            } else {
                Err(GlyphixError::UnmappedColor { r, g, b, palette_size })
            }
        }
        8 => {
            let bit = |c: u8| -> Result<u32> {
                match c {
                    0 => Ok(0),
                    255 => Ok(1),
                    _ => Err(GlyphixError::UnmappedColor { r, g, b, palette_size }),
                }
            };
            Ok((bit(r)? << 2) | (bit(g)? << 1) | bit(b)?)
        }
        256 => {
            if r == g && g == b {
                Ok(r as u32)
            } else {
                Err(GlyphixError::UnmappedColor { r, g, b, palette_size })
            }
        }
        0x01_00_00_00 => Ok(((r as u32) << 16) | ((g as u32) << 8) | (b as u32)),
        other => {
            let bits = other.trailing_zeros();
            if bits > 24 {
                return Err(GlyphixError::InvalidProfile(
                    "cannot invert color map for palette with >24 bits/cell from RGB alone".into(),
                ));
            }
            // Brute force small palettes; for large C walk is impossible — use bit packing inverse.
            if other <= 4096 {
                for i in 0..other {
                    if index_to_rgb(other, i)? == rgb {
                        return Ok(i);
                    }
                }
                return Err(GlyphixError::UnmappedColor { r, g, b, palette_size });
            }
            // Large power-of-two: invert expand_channel by nearest; require exact match from our expand.
            let br = bits / 3 + if bits % 3 > 0 { 1 } else { 0 };
            let bg = bits / 3 + if bits % 3 > 1 { 1 } else { 0 };
            let bb = bits / 3;
            let r_bits = compress_channel(r, br)?;
            let g_bits = compress_channel(g, bg)?;
            let b_bits = compress_channel(b, bb)?;
            let index = (r_bits << (bg + bb)) | (g_bits << bb) | b_bits;
            // Verify round-trip
            if index_to_rgb(other, index)? != rgb {
                return Err(GlyphixError::UnmappedColor { r, g, b, palette_size });
            }
            if index >= other {
                return Err(GlyphixError::ColorOutOfRange {
                    index,
                    palette_size: other,
                });
            }
            Ok(index)
        }
    }
}

fn compress_channel(byte: u8, nbits: u32) -> Result<u32> {
    if nbits == 0 {
        return if byte == 0 {
            Ok(0)
        } else {
            Err(GlyphixError::UnmappedColor {
                r: byte,
                g: 0,
                b: 0,
                palette_size: 0,
            })
        };
    }
    if nbits >= 8 {
        return Ok(byte as u32);
    }
    let max = (1u32 << nbits) - 1;
    // Inverse of (val * 255) / max — find val such that expand matches exactly.
    for v in 0..=max {
        if expand_channel(v, nbits) == byte {
            return Ok(v);
        }
    }
    Err(GlyphixError::UnmappedColor {
        r: byte,
        g: 0,
        b: 0,
        palette_size: max + 1,
    })
}

/// Convenience using a profile's palette.
pub fn profile_index_to_rgb(profile: &GlyphProfile, index: u32) -> Result<Rgb> {
    index_to_rgb(profile.palette_size, index)
}

/// Convenience inverse using a profile's palette.
pub fn profile_rgb_to_index(profile: &GlyphProfile, rgb: Rgb) -> Result<u32> {
    rgb_to_index(profile.palette_size, rgb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_roundtrip() {
        assert_eq!(index_to_rgb(2, 0).unwrap(), [0, 0, 0]);
        assert_eq!(index_to_rgb(2, 1).unwrap(), [255, 255, 255]);
        assert_eq!(rgb_to_index(2, [0, 0, 0]).unwrap(), 0);
        assert_eq!(rgb_to_index(2, [255, 255, 255]).unwrap(), 1);
    }

    #[test]
    fn c8_all_roundtrip() {
        for i in 0..8 {
            let rgb = index_to_rgb(8, i).unwrap();
            assert_eq!(rgb_to_index(8, rgb).unwrap(), i);
        }
    }

    #[test]
    fn gray_and_rgb24() {
        assert_eq!(index_to_rgb(256, 128).unwrap(), [128, 128, 128]);
        assert_eq!(rgb_to_index(256, [128, 128, 128]).unwrap(), 128);
        let c = 0x00AB_12_34;
        assert_eq!(index_to_rgb(1 << 24, c).unwrap(), [0xAB, 0x12, 0x34]);
        assert_eq!(rgb_to_index(1 << 24, [0xAB, 0x12, 0x34]).unwrap(), c);
    }
}
