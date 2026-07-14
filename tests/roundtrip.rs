//! Integration: framed codec round-trips, integrity, and place-value goldens.

use glyphix::pack::{paint_value_u128, value_u128};
use glyphix::profile::{bin10, bin16, bin8, c256_8, c8_8, rgb24_8, GlyphProfile};
use glyphix::{
    decode, encode, encode_with, glyph_count_for, glyph_count_for_with, EncodeOptions, Grid,
    Integrity,
};

#[test]
fn roundtrip_empty_bin8_bin10() {
    for p in [bin8(), bin10()] {
        let g = encode(&p, b"").unwrap();
        assert_eq!(decode(&p, &g).unwrap(), b"");
    }
}

#[test]
fn roundtrip_sizes() {
    let payloads: Vec<Vec<u8>> = vec![
        vec![],
        vec![0x00],
        vec![0xFF],
        b"x".to_vec(),
        (0u8..=255).collect(),
        vec![0xA5; 32],
        vec![0x3C; 1024],
    ];
    for p in [bin8(), bin10(), bin16(), c8_8()] {
        for data in &payloads {
            let glyphs = encode(&p, data).unwrap();
            assert!(glyphs.len() <= p.max_glyphs);
            assert_eq!(decode(&p, &glyphs).unwrap(), *data);
        }
    }
}

#[test]
fn roundtrip_with_all_integrity() {
    let p = bin10();
    let data = b"phase-2-integrity-bytes";
    for integ in [
        Integrity::None,
        Integrity::Crc32,
        Integrity::Blake3_128,
        Integrity::Blake3_256,
    ] {
        let g = encode_with(&p, data, EncodeOptions::with_integrity(integ)).unwrap();
        assert_eq!(decode(&p, &g).unwrap(), data);
    }
}

#[test]
fn flip_one_cell_crc32_errors() {
    let p = bin8();
    let payload = b"detect-me";
    let mut glyphs =
        encode_with(&p, payload, EncodeOptions::with_integrity(Integrity::Crc32)).unwrap();
    let v = glyphs[0].get(7, 7).unwrap();
    glyphs[0].set(7, 7, 1 - v).unwrap();
    assert!(decode(&p, &glyphs).is_err());
}

#[test]
fn flip_trailer_bit_blake3_mismatch() {
    let p = bin16();
    let payload = b"0123456789abcdef"; // 16 bytes
    let mut glyphs = encode_with(
        &p,
        payload,
        EncodeOptions::with_integrity(Integrity::Blake3_128),
    )
    .unwrap();
    // v2 header 48 bits + 16*8 payload = 176; trailer starts at bit/place 176
    let (x, y) = Grid::coords_for_place(16, 16, 176).unwrap();
    let v = glyphs[0].get(x, y).unwrap();
    glyphs[0].set(x, y, 1 - v).unwrap();
    let err = decode(&p, &glyphs).unwrap_err();
    assert!(
        matches!(
            err,
            glyphix::GlyphixError::IntegrityMismatch {
                kind: "blake3-128"
            }
        ),
        "{err:?}"
    );
}

#[test]
fn golden_value_0_all_black_bin10() {
    let p = bin10();
    let g = paint_value_u128(&p, 0).unwrap();
    assert!(g.is_all_zero());
    for y in 0..10 {
        for x in 0..10 {
            assert_eq!(g.get(x, y).unwrap(), 0);
        }
    }
}

#[test]
fn golden_value_1_bottom_right_white() {
    let p = bin10();
    let g = paint_value_u128(&p, 1).unwrap();
    assert_eq!(g.get(9, 9).unwrap(), 1);
    for y in 0..10 {
        for x in 0..10 {
            if (x, y) != (9, 9) {
                assert_eq!(g.get(x, y).unwrap(), 0, "({x},{y})");
            }
        }
    }
    assert_eq!(value_u128(&p, &g).unwrap(), 1);
}

#[test]
fn golden_value_2_and_3_binary_count() {
    let p = bin8();
    let g2 = paint_value_u128(&p, 2).unwrap();
    assert_eq!(g2.get(7, 7).unwrap(), 0);
    assert_eq!(g2.get(6, 7).unwrap(), 1);

    let g3 = paint_value_u128(&p, 3).unwrap();
    assert_eq!(g3.get(7, 7).unwrap(), 1);
    assert_eq!(g3.get(6, 7).unwrap(), 1);
    assert_eq!(g3.get(5, 7).unwrap(), 0);
}

#[test]
fn place_index_bottom_right_is_zero() {
    assert_eq!(Grid::place_index(10, 10, 9, 9).unwrap(), 0);
    assert_eq!(Grid::place_index(8, 8, 7, 7).unwrap(), 0);
}

#[test]
fn c256_and_rgb24_roundtrip_small() {
    let data = b"rgb-palette-test-bytes!!";
    for p in [c256_8(), rgb24_8()] {
        let g = encode(&p, data).unwrap();
        assert_eq!(decode(&p, &g).unwrap(), data);
    }
}

#[test]
fn glyph_count_grows_with_payload() {
    let p = bin8();
    // v2 header = 48 bits; bin8 = 64 bits/glyph → 16 bits left for payload in 1 glyph = 2 bytes.
    // 3 payload bytes need 48+24=72 bits → 2 glyphs.
    assert_eq!(glyph_count_for(&p, &[]).unwrap(), 1);
    assert_eq!(glyph_count_for(&p, &[0; 2]).unwrap(), 1);
    assert_eq!(glyph_count_for(&p, &[0; 3]).unwrap(), 2);
    // CRC32 adds 4 trailer bytes → empty payload still 48+32=80 bits → 2 glyphs
    assert_eq!(
        glyph_count_for_with(&p, &[], Integrity::Crc32).unwrap(),
        2
    );
}

#[test]
fn custom_profile() {
    let p = GlyphProfile::new(4, 4, 2).unwrap();
    assert_eq!(p.bits_per_glyph(), 16);
    let data = b"hi";
    let g = encode(&p, data).unwrap();
    assert_eq!(decode(&p, &g).unwrap(), data);
}

#[test]
fn deterministic_encode() {
    let p = bin10();
    let a = encode(&p, b"same").unwrap();
    let b = encode(&p, b"same").unwrap();
    assert_eq!(a, b);
    let c = encode_with(
        &p,
        b"same",
        EncodeOptions::with_integrity(Integrity::Blake3_256),
    )
    .unwrap();
    let d = encode_with(
        &p,
        b"same",
        EncodeOptions::with_integrity(Integrity::Blake3_256),
    )
    .unwrap();
    assert_eq!(c, d);
}
