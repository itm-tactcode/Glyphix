//! **Glyphix** — pixel grids as symbols in a huge algorithmic alphabet.
//!
//! Each glyph is a \(W \times H\) grid of color indices. The alphabet size is
//! \(C^{W H}\) (e.g. \(2^{100}\) for a \(10\times 10\) binary profile). Symbols
//! are **not** stored exhaustively; they are indices rendered on demand
//! (MIDI-like instructions, not a giant font file).
//!
//! ## Place values
//!
//! Counting starts at the **bottom-right** (place 0), moves **left**, then **up**.
//! Color 0 is black; in binary, 1 is white. Multi-color profiles use base-\(C\)
//! digits on the same places (see [`grid`] and [`pack`]).
//!
//! ## Codec
//!
//! [`encode`] / [`decode`] map byte strings to glyph sequences with a framed
//! header (version + integrity tag + length). Optional CRC32 / BLAKE3 trailers
//! detect corruption (see [`check::Integrity`]).
//!
//! ## Render & layout (Phase 3–4)
//!
//! [`render::render_rgba`] / [`render::render_svg`] paint glyphs with cell scale
//! \(S\). Layout: horizontal strip or grid, margins, gaps, separators
//! ([`layout`]). PNG I/O is behind the **`render`** feature.
//!
//! ## Streaming (Phase 4)
//!
//! [`stream::encode_chunked`] splits large payloads into independent framed
//! frames of at most N glyphs each.
//!
//! ```
//! use glyphix::{decode, encode, encode_with, EncodeOptions, Integrity, bin10};
//! use glyphix::render::{render_rgba, parse_rgba, RenderOptions};
//!
//! let profile = bin10();
//! let glyphs = encode(&profile, b"hello").unwrap();
//! assert_eq!(decode(&profile, &glyphs).unwrap(), b"hello");
//!
//! let img = render_rgba(&profile, &glyphs, &RenderOptions::scale(4).unwrap()).unwrap();
//! let back = parse_rgba(&profile, &img, &RenderOptions::scale(4).unwrap()).unwrap();
//! assert_eq!(decode(&profile, &back).unwrap(), b"hello");
//! ```
//!
//! ## Bit-string paint (direct pixels)
//!
//! Human binary notation: **rightmost bit = bottom-right**. Shorter strings pad
//! left with zeros; longer strings drop right bits. Not the same as framed
//! [`encode`] (no header).
//!
//! ```
//! use glyphix::{paint_bit_string, bin8};
//!
//! let g = paint_bit_string(&bin8(), "1").unwrap(); // only BR white
//! assert_eq!(g.get(7, 7).unwrap(), 1);
//! ```
//!
//! Integrity is **error detection**, not authentication.

#![forbid(unsafe_code)]

pub mod capacity;
pub mod check;
pub mod codec;
pub mod color;
pub mod error;
pub mod grid;
pub mod layout;
pub mod pack;
pub mod profile;
pub mod render;
pub mod stream;

pub use capacity::{format_report_line, report as capacity_report, table_all_presets, CapacityReport};
pub use check::Integrity;
pub use codec::{
    capacity_bits, capacity_payload_bytes, capacity_payload_bytes_with, decode, encode,
    encode_with, glyph_count_for, glyph_count_for_with, glyphs_needed, EncodeOptions,
    V1_HEADER_BITS, V2_HEADER_BITS,
};
pub use color::{index_to_rgb, rgb_to_index, Rgb};
pub use error::{GlyphixError, Result};
pub use grid::Grid;
pub use layout::{GlyphLayout, LayoutOptions, Separator};
pub use pack::{
    grid_to_bit_string, normalize_bit_string, paint_bit_string, paint_bit_string_sequence,
    paint_bit_string_sequence_with, paint_digits, paint_normalized_bits, paint_value_u128,
    parse_and_normalize_bit_string, parse_bit_string, value_u128, BitSequenceMode,
};
pub use profile::{
    bin10, bin16, bin8, c256_8, c8_8, preset, preset_ids, rgb24_8, GlyphProfile, CODEC_VERSION,
    CODEC_VERSION_V1, DEFAULT_MAX_GLYPHS,
};
pub use render::{
    encode_svg, glyph_pixel_size, image_dimensions, parse_rgba, render_rgba, render_svg,
    RenderOptions, RgbaImage,
};
pub use stream::{
    chunk_payload_capacity, chunks_needed, decode_chunked, encode_chunked, glyph_batches,
    ChunkEncoder,
};

#[cfg(feature = "render")]
pub use render::{decode_png, encode_png, read_png, write_png};
