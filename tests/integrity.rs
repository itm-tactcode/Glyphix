//! Phase 2: integrity trailers detect corruption.

use glyphix::{
    decode, encode_with, EncodeOptions, GlyphixError, Integrity, bin10, bin8,
};

#[test]
fn crc32_roundtrip_and_mismatch() {
    let p = bin10();
    let payload = b"crc-payload";
    let glyphs = encode_with(
        &p,
        payload,
        EncodeOptions::with_integrity(Integrity::Crc32),
    )
    .unwrap();
    assert_eq!(decode(&p, &glyphs).unwrap(), payload);

    let mut bad = glyphs;
    let v = bad[0].get(5, 5).unwrap();
    bad[0].set(5, 5, 1 - v).unwrap();
    assert!(decode(&p, &bad).is_err());
}

#[test]
fn blake3_256_roundtrip() {
    let p = bin10();
    let payload = vec![0xABu8; 200];
    let glyphs = encode_with(
        &p,
        &payload,
        EncodeOptions::with_integrity(Integrity::Blake3_256),
    )
    .unwrap();
    assert_eq!(decode(&p, &glyphs).unwrap(), payload);
}

#[test]
fn none_still_roundtrips() {
    let p = bin8();
    let g = encode_with(
        &p,
        b"x",
        EncodeOptions::with_integrity(Integrity::None),
    )
    .unwrap();
    assert_eq!(decode(&p, &g).unwrap(), b"x");
}

#[test]
fn unknown_integrity_tag_rejected() {
    // Manually not needed if from_tag is unit-tested; smoke via from_tag.
    assert!(matches!(
        Integrity::from_tag(99),
        Err(GlyphixError::UnknownIntegrity { tag: 99 })
    ));
}
