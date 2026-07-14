# Glyphix

**Glyph** + **pixel**: an algorithmic font / character encoding whose alphabet is every possible pixel pattern on a grid — a *Library of Babel for glyphs*.

\[
|\text{alphabet}| = C^{W \times H}
\quad\text{(e.g. } 2^{100} \text{ for } 10\times 10 \text{ binary)}
\]

Byte strings map to **one glyph** or a **sequence** of glyphs. Symbols are indices rendered on demand, not an exhaustive font file.

This is **not** an Effector address format. Effector uses **bech32m**. Glyphix is a separate encoding playground.

See **[AGENTS.md](AGENTS.md)** for goals, place-value rules, profiles, and phases. See **[PROGRESS.md](PROGRESS.md)** for what has landed.

## Place values (binary)

Counting starts at the **bottom-right** (least significant place):

| Value | Pattern (binary) |
|------:|------------------|
| 0 | all black |
| 1 | only bottom-right white |
| 2 | one cell left of BR white |
| 3 | bottom-right two cells white |
| \(2^{WH}-1\) | all white |

Multi-color profiles use the same places in base \(C\) (8 colors, 256 grayscale, 24-bit RGB, …).

## Quick start

```bash
cd ~/Projects/Crypto/glyphix
cargo test
cargo test --features render   # includes PNG file round-trip

cargo run --features cli -- profiles
cargo run --features cli -- capacity -p bin10 -n 10

# Encode → PNG (cell scale 8 = each logical pixel is 8×8 device pixels)
cargo run --features cli -- encode -p bin10 -s 8 -o /tmp/hello.png "hello glyphs"
cargo run --features cli -- decode -p bin10 -s 8 /tmp/hello.png

# SVG (no need for separate feature; still via cli/render for the binary)
cargo run --features cli -- encode -p bin10 -s 8 -o /tmp/hello.svg "hello"

# Integrity + PNG round-trip
cargo run --features cli -- roundtrip -p bin10 -s 4 --png /tmp/rt.png --check crc32 "hi"

# Bit-string paint (rightmost bit = bottom-right; pad left / trunc right)
cargo run --features cli -- paint -p bin8 -b 1 -s 16 -o /tmp/br.png --show-bits

# Sequence: comma = one glyph per field
cargo run --features cli -- paint -p bin8 -b '1,11,101' -s 8 -o /tmp/seq.png --show-bits

# Sequence: continuous stream spills into next glyph (65 ones → full white + BR)
cargo run --features cli -- paint -p bin8 -b "$(python3 -c "print('1'*65)")" --strip -s 8 -o /tmp/spill.png --show-bits
```

### Text encode vs bit-string paint

| Mode | Command | Meaning |
|------|---------|---------|
| **Text / bytes** | `encode "hello"` | Framed payload (version, length, …) → pixels |
| **Bit string** | `paint -b 1011` | You choose each place bit; **no** frame header |

**Single glyph:** left-pad with `0` if short; drop right bits if long; rightmost bit = bottom-right.

**Sequence (pick one):**

| Style | Example | Behavior |
|-------|---------|----------|
| **Comma-separated** | `1,11,101` | Each field is its own character (pad/trunc per field). Best for hand-designed tiles. |
| **Stream** (`--strip`) | 65× `1` | Fill glyph 0 (64 bits), leftover `1` → glyph 1 left-padded. Like text spilling to the next char; no mid-stream discard. |

### Library

```rust
use glyphix::{
    decode, encode, encode_with, bin10, paint_value_u128,
    EncodeOptions, Integrity,
};

let profile = bin10();
let glyphs = encode(&profile, b"hello").unwrap();
assert_eq!(decode(&profile, &glyphs).unwrap(), b"hello");

// Optional integrity trailer (error detection, not a signature)
let protected = encode_with(
    &profile,
    b"hello",
    EncodeOptions::with_integrity(Integrity::Crc32),
).unwrap();
assert_eq!(decode(&profile, &protected).unwrap(), b"hello");

// Integer codepoint → grid (value 1 = bottom-right only)
let g = paint_value_u128(&profile, 1).unwrap();
assert_eq!(g.get(9, 9).unwrap(), 1);
```

CLI integrity / render flags:

```bash
cargo run --features cli -- roundtrip -p bin10 --check blake3-256 "hello"
cargo run --features cli -- encode -p bin8 -s 16 -o checker.png --format png "x"
```

Library render (PNG needs feature `render`):

```rust
use glyphix::render::{render_rgba, parse_rgba, RenderOptions};
// let img = render_rgba(&profile, &glyphs, &RenderOptions::scale(4).unwrap()).unwrap();
```

## Profiles (MVP)

| Id | Grid | \(C\) | Bits / glyph |
|----|------|------:|-------------:|
| `bin8` | 8×8 | 2 | 64 |
| `bin10` | 10×10 | 2 | 100 |
| `bin16` | 16×16 | 2 | 256 |
| `c8_8` | 8×8 | 8 | 192 |
| `c256_8` | 8×8 | 256 | 512 |
| `rgb24_8` | 8×8 | \(2^{24}\) | 1536 |

## Status

| Phase | Status |
|-------|--------|
| 0 Spec + agents | Done |
| 1 Core codec | Done (lib + tests + optional CLI) |
| 2 Integrity (CRC/BLAKE3) | Done (`Integrity` + v2 framing) |
| 3 PNG/SVG + cell scale | Done (`render` / SVG / CLI `-s`) |
| 4 Layout / capacity UX | Partial (strip + margin/gap; `capacity` CLI) |
| 5+ ECC / camera / familiar shapes | Not started |

## License

MIT OR Apache-2.0
