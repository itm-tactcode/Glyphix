//! Chunked / streaming encode for large payloads (Phase 4).
//!
//! Framed `encode` needs the full payload length up front. For large data we
//! **split the payload** into independent framed chunks, each fitting in at
//! most `max_glyphs` glyphs. Reassemble with [`decode_chunked`].
//!
//! This is multi-frame messaging, not a single long frame split mid-header.

use crate::check::Integrity;
use crate::codec::{
    capacity_payload_bytes_with, decode, encode_with, glyphs_needed, EncodeOptions,
};
use crate::error::{GlyphixError, Result};
use crate::grid::Grid;
use crate::profile::GlyphProfile;

/// Encode `payload` as a sequence of independent framed glyph frames.
///
/// Each frame holds at most `max_glyphs` glyphs (including its own header/trailer).
/// Empty payload yields a single empty frame.
pub fn encode_chunked(
    profile: &GlyphProfile,
    payload: &[u8],
    opts: EncodeOptions,
    max_glyphs_per_chunk: usize,
) -> Result<Vec<Vec<Grid>>> {
    if max_glyphs_per_chunk == 0 {
        return Err(GlyphixError::InvalidProfile(
            "max_glyphs_per_chunk must be ≥ 1".into(),
        ));
    }
    if max_glyphs_per_chunk > profile.max_glyphs {
        return Err(GlyphixError::GlyphCountExceeded {
            count: max_glyphs_per_chunk,
            cap: profile.max_glyphs,
        });
    }

    let cap = capacity_payload_bytes_with(profile, max_glyphs_per_chunk, opts.integrity);
    if cap == 0 {
        return Err(GlyphixError::InvalidProfile(format!(
            "max_glyphs_per_chunk={max_glyphs_per_chunk} cannot hold any payload bytes with {:?}",
            opts.integrity
        )));
    }

    if payload.is_empty() {
        return Ok(vec![encode_with(profile, b"", opts)?]);
    }

    let mut frames = Vec::new();
    for chunk in payload.chunks(cap) {
        let g = encode_with(profile, chunk, opts)?;
        debug_assert!(g.len() <= max_glyphs_per_chunk);
        frames.push(g);
    }
    Ok(frames)
}

/// Decode independently framed chunks and concatenate payloads in order.
pub fn decode_chunked(profile: &GlyphProfile, frames: &[Vec<Grid>]) -> Result<Vec<u8>> {
    if frames.is_empty() {
        return Err(GlyphixError::EmptyGlyphSequence);
    }
    let mut out = Vec::new();
    for frame in frames {
        out.extend(decode(profile, frame)?);
    }
    Ok(out)
}

/// Split an already-encoded glyph list into batches of at most `batch_size`
/// (for multi-page render). Does **not** re-frame; batches are not independently
/// decodable unless they happen to be full messages.
pub fn glyph_batches(glyphs: &[Grid], batch_size: usize) -> Result<Vec<&[Grid]>> {
    if batch_size == 0 {
        return Err(GlyphixError::InvalidProfile(
            "batch_size must be ≥ 1".into(),
        ));
    }
    if glyphs.is_empty() {
        return Err(GlyphixError::EmptyGlyphSequence);
    }
    Ok(glyphs.chunks(batch_size).collect())
}

/// How many payload bytes fit in one chunk of `max_glyphs` under `integrity`.
pub fn chunk_payload_capacity(
    profile: &GlyphProfile,
    max_glyphs: usize,
    integrity: Integrity,
) -> usize {
    capacity_payload_bytes_with(profile, max_glyphs, integrity)
}

/// Number of chunks needed for `payload_len` with the given chunk glyph budget.
pub fn chunks_needed(
    profile: &GlyphProfile,
    payload_len: usize,
    max_glyphs_per_chunk: usize,
    integrity: Integrity,
) -> Result<usize> {
    let cap = chunk_payload_capacity(profile, max_glyphs_per_chunk, integrity);
    if cap == 0 {
        return Err(GlyphixError::InvalidProfile(
            "chunk capacity is 0 bytes".into(),
        ));
    }
    if payload_len == 0 {
        return Ok(1);
    }
    Ok(payload_len.div_ceil(cap))
}

/// Incremental encoder: push payload bytes, flush full chunks as glyph frames.
///
/// Useful when the full payload is not in memory at once.
#[derive(Debug, Clone)]
pub struct ChunkEncoder {
    profile: GlyphProfile,
    opts: EncodeOptions,
    max_glyphs: usize,
    capacity: usize,
    buf: Vec<u8>,
}

impl ChunkEncoder {
    /// Start a streaming encoder.
    pub fn new(
        profile: GlyphProfile,
        opts: EncodeOptions,
        max_glyphs_per_chunk: usize,
    ) -> Result<Self> {
        let capacity = chunk_payload_capacity(&profile, max_glyphs_per_chunk, opts.integrity);
        if capacity == 0 || max_glyphs_per_chunk == 0 {
            return Err(GlyphixError::InvalidProfile(
                "invalid chunk encoder capacity".into(),
            ));
        }
        Ok(Self {
            profile,
            opts,
            max_glyphs: max_glyphs_per_chunk,
            capacity,
            buf: Vec::new(),
        })
    }

    /// Bytes still buffered (not yet a full chunk).
    pub fn buffered_len(&self) -> usize {
        self.buf.len()
    }

    /// Payload bytes per full chunk.
    pub fn chunk_capacity(&self) -> usize {
        self.capacity
    }

    /// Append payload bytes; returns any completed frames.
    pub fn push(&mut self, data: &[u8]) -> Result<Vec<Vec<Grid>>> {
        self.buf.extend_from_slice(data);
        let mut frames = Vec::new();
        while self.buf.len() >= self.capacity {
            let chunk: Vec<u8> = self.buf.drain(..self.capacity).collect();
            let g = encode_with(&self.profile, &chunk, self.opts)?;
            debug_assert!(g.len() <= self.max_glyphs);
            frames.push(g);
        }
        Ok(frames)
    }

    /// Finish: encode remaining buffered bytes (possibly empty) as a final frame
    /// if anything was ever pushed or `force_empty` is true.
    pub fn finish(&mut self) -> Result<Option<Vec<Grid>>> {
        if self.buf.is_empty() {
            return Ok(None);
        }
        let chunk = std::mem::take(&mut self.buf);
        Ok(Some(encode_with(&self.profile, &chunk, self.opts)?))
    }

    /// Finish, always emitting at least one frame (empty payload if buffer empty).
    pub fn finish_with_empty(&mut self) -> Result<Vec<Grid>> {
        let chunk = std::mem::take(&mut self.buf);
        encode_with(&self.profile, &chunk, self.opts)
    }
}

/// Max payload bytes for a given glyph budget (re-export style helper).
pub fn max_payload_for_glyphs(
    profile: &GlyphProfile,
    glyph_count: usize,
    integrity: Integrity,
) -> usize {
    capacity_payload_bytes_with(profile, glyph_count, integrity)
}

/// Glyphs required for a payload (single frame).
pub fn glyphs_for_payload(
    profile: &GlyphProfile,
    payload_len: usize,
    integrity: Integrity,
) -> Result<usize> {
    glyphs_needed(profile, payload_len, integrity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{bin10, bin8};

    #[test]
    fn chunked_roundtrip_large() {
        let p = bin8();
        let data: Vec<u8> = (0..500).map(|i| i as u8).collect();
        let frames = encode_chunked(
            &p,
            &data,
            EncodeOptions::with_integrity(Integrity::Crc32),
            4,
        )
        .unwrap();
        assert!(frames.len() > 1);
        for f in &frames {
            assert!(f.len() <= 4);
        }
        assert_eq!(decode_chunked(&p, &frames).unwrap(), data);
    }

    #[test]
    fn chunked_empty() {
        let p = bin10();
        let frames = encode_chunked(&p, b"", EncodeOptions::default(), 2).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(decode_chunked(&p, &frames).unwrap(), b"");
    }

    #[test]
    fn streaming_encoder() {
        let p = bin8();
        let mut enc = ChunkEncoder::new(p, EncodeOptions::default(), 3).unwrap();
        let cap = enc.chunk_capacity();
        let mut all_frames = Vec::new();
        all_frames.extend(enc.push(&vec![0xABu8; cap + 10]).unwrap());
        if let Some(f) = enc.finish().unwrap() {
            all_frames.push(f);
        }
        let out = decode_chunked(&p, &all_frames).unwrap();
        assert_eq!(out, vec![0xABu8; cap + 10]);
    }

    #[test]
    fn glyph_batches_split() {
        let p = bin8();
        let g = encode_with(&p, b"hello world!!!!", EncodeOptions::default()).unwrap();
        let batches = glyph_batches(&g, 2).unwrap();
        assert!(!batches.is_empty());
        assert!(batches.iter().all(|b| b.len() <= 2));
    }
}
