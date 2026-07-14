# Progress Log

Ongoing record of completed work on **Glyphix**. **Append new entries at the top** (newest first). Keep entries factual: what landed, paths, and how to verify.

Agents: update this file when finishing meaningful work. Sync **Current status** / **Next** in [`AGENTS.md`](AGENTS.md).

---

## 2026-07-14 — Bit-string paint (direct pixel patterns)

### Done

1. **`paint_bit_string` / `paint_bit_string_sequence`** (`src/pack.rs`)
   - Human notation: **rightmost bit = place 0 (bottom-right)**
   - Single glyph: **pad left** / **trunc right**
   - **Comma-separated** sequence: each field is one glyph (pad/trunc per field)
   - **Stream** sequence (`--strip`): overflow → next glyph; partial final **left-padded** (no whole-string left-pad)
   - `BitSequenceMode::{Auto, Stream, CommaSeparated}`
2. **CLI** `paint -b '1,11,101'` or `paint -b … --strip`
3. Distinct from framed `encode` (no version/length header)

### Verify

```bash
cargo test
cargo run --features cli -- paint -p bin8 -b 1 -s 16 -o /tmp/br.png --show-bits
cargo run --features cli -- paint -p bin8 -b 00000000001 -s 16 -o /tmp/br2.png
# 64 ones + trailing zeros → all white
cargo run --features cli -- paint -p bin8 -b "$(printf '1%.0s' {1..64})0000" -s 4 -o /tmp/white.png
```

---

## 2026-07-14 — Phase 3 render / parse (PNG + SVG)

### Done

1. **`src/color.rs`** — index ↔ sRGB
   - \(C=2\) B/W, \(C=8\) 3-bit RGB, \(C=256\) gray, \(C=2^{24}\) packed RGB
2. **`src/render.rs`**
   - `RenderOptions { cell_scale S, margin, gap, HorizontalStrip }`
   - `render_rgba` / `parse_rgba` (clean uniform blocks only)
   - `render_svg` / `encode_svg` (crispEdges, no image dep)
   - PNG: `write_png` / `read_png` / `encode_png` / `decode_png` behind feature **`render`**
3. **Feature flags:** `render` → `image` (png only); `cli` enables `render` + clap
4. **CLI:** `encode -o out.png|out.svg -s N`, `decode -s N in.png`, `roundtrip --png path`
5. **Tests:** RGBA round-trip, scale-4 BR white block, SVG smoke, PNG file round-trip, integrity through raster

### Deps

- `image` 0.25 optional, `default-features = false`, `features = ["png"]` only

### Verify

```bash
cargo test
cargo test --features render
cargo run --features cli -- encode -p bin10 -s 8 -o /tmp/g.png "hello glyphs"
cargo run --features cli -- decode -p bin10 -s 8 /tmp/g.png
cargo run --features cli -- encode -p bin10 -s 8 -o /tmp/g.svg "hello"
cargo run --features cli -- roundtrip -p bin10 -s 4 --png /tmp/rt.png --check crc32 "hi"
```

### Next

- Phase 4: multi-row glyph grid layout; richer capacity UX  
- Optional: terminal block renderer (no PNG)

---

## 2026-07-14 — Phase 2 integrity trailers

### Done

1. **`src/check.rs`** — `Integrity::{None, Crc32, Blake3_128, Blake3_256}`
   - Wire tags 0–3; `compute` / `verify` over **payload only**
   - CRC-32 IEEE BE (known vector `"123456789"` → `0xCBF43926`)
   - BLAKE3-128 = first 16 bytes of BLAKE3-256; BLAKE3-256 full digest
   - Docs: **error detection**, not authentication

2. **v2 framing** (`src/codec.rs`)
   - Encode writes `version=2 | integrity | u32 BE len | payload | trailer | zero pad`
   - `encode_with` + `EncodeOptions`; `encode` defaults to `Integrity::None`
   - Decode accepts **v1** (legacy, no integrity field) and **v2**
   - Errors: `IntegrityMismatch`, `UnknownIntegrity`

3. **Capacity helpers** account for trailer overhead (`capacity_payload_bytes_with`, `glyphs_needed`)

4. **Deps:** `crc32fast` 1.x, `blake3` 1.x (justified: standard integrity algorithms)

5. **Tests:** unit + `tests/integrity.rs` + flip-cell / flip-trailer failure cases

6. **CLI** `--check none|crc32|blake3-128|blake3-256` on encode / roundtrip / capacity

### Verify

```bash
cargo test
cargo run --features cli -- roundtrip -p bin10 --check crc32 "hello"
cargo run --features cli -- capacity -p bin10 -n 10 --check blake3-256
```

### Next

- Phase 3: PNG/SVG render + cell scale \(S\)

---

## 2026-07-14 — Phase 1 core codec + vision lock-in

### Done

1. **`AGENTS.md` rewrite** (Artificer-style phases + living status)
   - Library-of-Babel-for-glyphs vision from human brief
   - **Canonical place order:** bottom-right = place 0, left then up
   - Binary 0=black / 1=white; multi-color base-\(C\) (8 / 256 / \(2^{24}\))
   - Cell scale deferred to render (phase 3); sequences and stretch goals documented
   - `PROGRESS.md` workflow mirrored from Artificer

2. **Crate `glyphix` 0.1.0** (single package)
   - `src/profile.rs` — `GlyphProfile`, presets `bin8` / `bin10` / `bin16` / `c8_8` / `c256_8` / `rgb24_8`
   - `src/grid.rs` — `Grid`, place ↔ \((x,y)\)
   - `src/pack.rs` — bits ↔ cells; `paint_value_u128` / `value_u128` for integer codepoints
   - `src/codec.rs` — framed `encode` / `decode` (`version:u8` + `len:u32 BE` + payload + zero pad)
   - `src/error.rs` — `thiserror` errors (no silent truncation)
   - `src/lib.rs` — public API; `src/main.rs` — optional CLI (`--features cli`)
   - `tests/roundtrip.rs` — empty/1B/32B/1KiB, multi-profile, goldens for values 0–3

3. **Deps:** `thiserror` only for lib; optional `clap` for CLI.

### MVP leans recorded

| Topic | Choice |
|-------|--------|
| Place order | BR place 0; stream fills low places first |
| Framing | version `1` + u32 BE length |
| Palette | power-of-two \(C\) only |
| Max glyphs | 4096 default |

### Verify

```bash
cd ~/Projects/Crypto/glyphix
cargo test
cargo run --features cli -- profiles
cargo run --features cli -- roundtrip -p bin10 "hello"
cargo run --features cli -- capacity -p bin10 -n 10
```

### Next

- Phase 2: optional CRC32 / BLAKE3 trailer  
- Phase 3: PNG/SVG render with **cell scale \(S\)** (font-size analogue)  
- Phase 4: strip/grid layout + quiet margins  

---
