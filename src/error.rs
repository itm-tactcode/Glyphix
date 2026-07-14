//! Explicit error types for Glyphix encode/decode.

use thiserror::Error;

/// Errors produced by profile validation, packing, or the framed codec.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum GlyphixError {
    #[error("invalid profile: {0}")]
    InvalidProfile(String),

    #[error("color index {index} out of range for palette size {palette_size}")]
    ColorOutOfRange { index: u32, palette_size: u32 },

    #[error("grid dimensions {got_w}x{got_h} do not match profile {want_w}x{want_h}")]
    GridSizeMismatch {
        got_w: u32,
        got_h: u32,
        want_w: u32,
        want_h: u32,
    },

    #[error("glyph sequence empty; need at least one glyph for framed header")]
    EmptyGlyphSequence,

    #[error("truncated bit stream: needed {needed} more bits, had {available}")]
    TruncatedStream { needed: usize, available: usize },

    #[error("payload length {payload_len} exceeds remaining stream capacity ({capacity_bytes} bytes)")]
    PayloadTooLong {
        payload_len: u32,
        capacity_bytes: usize,
    },

    #[error("glyph count {count} exceeds hard cap {cap}")]
    GlyphCountExceeded { count: usize, cap: usize },

    #[error("unsupported codec version {got}; this library supports versions {supported}")]
    UnsupportedVersion { got: u8, supported: &'static str },

    #[error("unknown integrity tag {tag} (expected 0=none, 1=crc32, 2=blake3-128, 3=blake3-256)")]
    UnknownIntegrity { tag: u8 },

    #[error("payload integrity check failed ({kind}); data was corrupted or altered")]
    IntegrityMismatch { kind: &'static str },

    #[error("non-zero padding after payload/trailer (stream not cleanly framed)")]
    DirtyPadding,

    #[error("coordinate ({x}, {y}) out of bounds for {width}x{height} grid")]
    OutOfBounds {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },

    #[error("invalid render options: {0}")]
    InvalidRender(String),

    #[error("invalid raster: {0}")]
    InvalidRaster(String),

    #[error("color RGB({r},{g},{b}) is not in palette C={palette_size}")]
    UnmappedColor { r: u8, g: u8, b: u8, palette_size: u32 },

    #[error("I/O error: {0}")]
    Io(String),

    #[error("invalid bit string: {0}")]
    InvalidBitString(String),
}

/// Convenient result alias.
pub type Result<T> = std::result::Result<T, GlyphixError>;
