//! Logical pixel grids and **place-value** coordinates.
//!
//! # Place order (canonical)
//!
//! Storage uses top-left origin, row-major `cells[y * width + x]`.
//! Significance for the algorithmic alphabet uses **place index**:
//!
//! ```text
//! place(x, y) = (H - 1 - y) * W + (W - 1 - x)
//! ```
//!
//! - Place **0** = bottom-right cell.
//! - Places increase **left** along a row, then **up** to the next row.
//!
//! A glyph value \(V = \sum d_i C^i\) puts digit \(d_i\) at place \(i\).
//! Value 0 → all zeros (black). Value 1 → only bottom-right = 1 (white in binary).

use crate::error::{GlyphixError, Result};
use crate::profile::GlyphProfile;

/// One glyph: a \(W \times H\) array of color indices in \(0..C\).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Grid {
    width: u32,
    height: u32,
    /// Row-major, top-left origin. Length = width * height.
    cells: Vec<u32>,
}

impl Grid {
    /// Create a grid filled with zeros (all black / value 0).
    pub fn zeros(width: u32, height: u32) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(GlyphixError::InvalidProfile(
                "width and height must be ≥ 1".into(),
            ));
        }
        let n = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| GlyphixError::InvalidProfile("grid too large".into()))?;
        Ok(Self {
            width,
            height,
            cells: vec![0; n],
        })
    }

    /// Create a grid matching a profile, all zeros.
    pub fn from_profile(profile: &GlyphProfile) -> Self {
        Self::zeros(profile.width, profile.height).expect("profile already validated")
    }

    /// Width in cells.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in cells.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Borrow raw row-major cells (top-left origin).
    pub fn cells(&self) -> &[u32] {
        &self.cells
    }

    /// Mutable raw cells.
    pub fn cells_mut(&mut self) -> &mut [u32] {
        &mut self.cells
    }

    /// Place index for \((x, y)\). Place 0 = bottom-right.
    pub fn place_index(width: u32, height: u32, x: u32, y: u32) -> Result<usize> {
        if x >= width || y >= height {
            return Err(GlyphixError::OutOfBounds {
                x,
                y,
                width,
                height,
            });
        }
        let place = (height - 1 - y) as usize * width as usize + (width - 1 - x) as usize;
        Ok(place)
    }

    /// \((x, y)\) for a place index (inverse of [`place_index`]).
    pub fn coords_for_place(width: u32, height: u32, place: usize) -> Result<(u32, u32)> {
        let n = width as usize * height as usize;
        if place >= n {
            return Err(GlyphixError::OutOfBounds {
                x: place as u32,
                y: 0,
                width,
                height,
            });
        }
        let row_from_bottom = place / width as usize;
        let col_from_right = place % width as usize;
        let y = height - 1 - row_from_bottom as u32;
        let x = width - 1 - col_from_right as u32;
        Ok((x, y))
    }

    /// Color index at \((x, y)\).
    pub fn get(&self, x: u32, y: u32) -> Result<u32> {
        if x >= self.width || y >= self.height {
            return Err(GlyphixError::OutOfBounds {
                x,
                y,
                width: self.width,
                height: self.height,
            });
        }
        Ok(self.cells[y as usize * self.width as usize + x as usize])
    }

    /// Set color index at \((x, y)\) (does not check palette bounds).
    pub fn set(&mut self, x: u32, y: u32, color: u32) -> Result<()> {
        if x >= self.width || y >= self.height {
            return Err(GlyphixError::OutOfBounds {
                x,
                y,
                width: self.width,
                height: self.height,
            });
        }
        self.cells[y as usize * self.width as usize + x as usize] = color;
        Ok(())
    }

    /// Color at place index \(i\).
    pub fn get_place(&self, place: usize) -> Result<u32> {
        let (x, y) = Self::coords_for_place(self.width, self.height, place)?;
        self.get(x, y)
    }

    /// Set color at place index \(i\).
    pub fn set_place(&mut self, place: usize, color: u32) -> Result<()> {
        let (x, y) = Self::coords_for_place(self.width, self.height, place)?;
        self.set(x, y, color)
    }

    /// Ensure every cell is in \(0..palette_size\).
    pub fn validate_palette(&self, palette_size: u32) -> Result<()> {
        for &c in &self.cells {
            if c >= palette_size {
                return Err(GlyphixError::ColorOutOfRange {
                    index: c,
                    palette_size,
                });
            }
        }
        Ok(())
    }

    /// Ensure dimensions match profile.
    pub fn validate_profile(&self, profile: &GlyphProfile) -> Result<()> {
        if self.width != profile.width || self.height != profile.height {
            return Err(GlyphixError::GridSizeMismatch {
                got_w: self.width,
                got_h: self.height,
                want_w: profile.width,
                want_h: profile.height,
            });
        }
        self.validate_palette(profile.palette_size)
    }

    /// True if every cell is zero (value 0 / all black).
    pub fn is_all_zero(&self) -> bool {
        self.cells.iter().all(|&c| c == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_zero_is_bottom_right() {
        let w = 8u32;
        let h = 8u32;
        assert_eq!(Grid::place_index(w, h, 7, 7).unwrap(), 0);
        assert_eq!(Grid::coords_for_place(w, h, 0).unwrap(), (7, 7));
    }

    #[test]
    fn place_one_is_left_of_bottom_right() {
        let w = 8u32;
        let h = 8u32;
        assert_eq!(Grid::place_index(w, h, 6, 7).unwrap(), 1);
        assert_eq!(Grid::coords_for_place(w, h, 1).unwrap(), (6, 7));
    }

    #[test]
    fn place_width_is_row_above_right() {
        // After finishing bottom row (places 0..W-1), place W is row above, rightmost.
        let w = 10u32;
        let h = 10u32;
        assert_eq!(Grid::place_index(w, h, 9, 8).unwrap(), 10);
        assert_eq!(Grid::coords_for_place(w, h, 10).unwrap(), (9, 8));
    }

    #[test]
    fn place_roundtrip_all_cells() {
        let w = 5u32;
        let h = 4u32;
        for y in 0..h {
            for x in 0..w {
                let p = Grid::place_index(w, h, x, y).unwrap();
                assert_eq!(Grid::coords_for_place(w, h, p).unwrap(), (x, y));
            }
        }
    }
}
