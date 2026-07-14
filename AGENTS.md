# Glyphix — Agent Instructions

**Project name:** **Glyphix** (glyph + pixel)  
**CLI binary (planned):** `glyphix`  
**Primary language:** Rust (preferred; Python OK for experiments if human asks)  
**Workspace path:** `~/Projects/Crypto/glyphix`  
**Relation to Effector:** **None in-protocol.** Effector uses **bech32m** for addresses. Glyphix is a **separate** visual / combinatorial encoding experiment — a font/character system whose alphabet is algorithmic, not enumerated.

## Project goal

Build a library (and small CLI) that treats **pixel grids as symbols in a huge algorithmic alphabet** — a “Library of Babel for glyphs.” Every possible ink pattern expressible on a \(W \times H\) grid with \(C\) colors is a valid character. Characters are **not** stored exhaustively; each glyph is an **index** (base-\(C\) place-value pattern) rendered on demand.

Core insight:

\[
|\text{alphabet}| = C^{W \times H}
\quad\text{(e.g. } 2^{100} \text{ for } 10\times 10 \text{ binary)}
\]

Byte strings / numeric data map to **one glyph** or a **sequence of glyphs** (like writing), so capacity scales as:

\[
\text{bits} \approx L \times W \times H \times \log_2 C
\quad\text{(minus version / length / CRC / ECC overhead)}
\]

**Vision (human brief):**

1. **Grid sizes** start at \(8\times 8\), \(10\times 10\); later \(16\times 16\) and beyond (font size / resolution).
2. **Display scale:** a logical grid cell may render as \(S\times S\) device pixels (e.g. each of 64 cells of an \(8\times 8\) becomes a \(4\times 4\) block when “font size” demands it). Codec works on **logical** cells; scale is a render concern.
3. **Binary (2-color) base:** black = 0, white = 1. Counting uses **place values from the lower-right**:
   - Value **0** → all black.
   - Value **1** → only the **bottom-right** cell white.
   - Value **2** → bottom-right black again; the cell **one left** white.
   - Value **3** → bottom-right two cells white.
   - … up to \(2^{W H}-1\) → all white.
4. **Multi-color** = higher-radix digits on the same places: each cell holds \(0..C-1\); when a digit rolls over, the next place “flips” (same as base-\(C\) counting). Color 0 is black; higher indices follow standard computer color encodings where applicable:
   - \(C=2\): black / white  
   - \(C=8\): 3-bit RGB (\(0bRGB\))  
   - \(C=256\): 8-bit index (grayscale \(0..255\) by default)  
   - \(C=16\,777\,216\): 24-bit RGB (`#000000` … `#FFFFFF`)
5. **Sequences** of glyphs act like multi-digit writing in an astronomical base — capacity multiplies by sequence length \(L\).
6. **Later stretch:** discover which indices’ “inked” patterns match ASCII/Unicode shapes (glyph → familiar character lookup demos). Not required for the lossless codec.

**Not goals (unless human reopens):**

- Replacing Effector / blockchain addresses  
- Competing with QR as a phone-scanning standard on day one  
- Claiming human eyeball uniqueness among \(2^{100}\) neighbors  
- Exhaustive on-disk font files for every codepoint  

## Progress log

**Always read and update [`PROGRESS.md`](PROGRESS.md)** when completing work:

- Append a dated entry (**newest first**) describing what landed, paths, and how to verify.
- Keep entries factual; do not rewrite history — correct in a new note if needed.
- Sync **Current status** / **Next** in this file after meaningful phase work.

## What agents must remember

1. **Lossless codec first:** `bytes ↔ sequence of grids ↔ bytes` with tests.
2. **Place-value order is fixed and documented:** origin of significance is **bottom-right**, leftward, then upward (see below). Do not silently switch to top-left MSB without a version/profile flag and PROGRESS note.
3. **Parameters are first-class:** width, height, palette size \(C\), sequence length, version, optional cell scale for render.
4. **Channel ≠ alphabet:** huge \(B\) is easy; robust scan/OCR/camera is hard — ECC is a later phase.
5. **Do not couple to Effector consensus or Airship economics.**

## Place-value & color mapping (canonical)

Coordinates: logical cell \((x, y)\) with **origin top-left** for storage arrays (\(x\) right, \(y\) down). Significance uses a separate **place index**:

\[
\text{place}(x, y) = (H - 1 - y)\cdot W + (W - 1 - x)
\]

- Place **0** = bottom-right cell \((W-1,\, H-1)\).  
- Place increases **left** along the bottom row, then the row above, … up to the top-left as the highest place.

A glyph’s integer value \(V\) (for \(C\) colors) satisfies:

\[
V = \sum_{i=0}^{WH-1} d_i \, C^{i}, \quad d_i \in \{0,\ldots,C-1\}
\]

where \(d_i\) is the color index at place \(i\).

**Bitstream packing (byte payloads):** framed bits are consumed into successive glyphs. Within each glyph, **low places fill first** (place 0 gets the earliest bits of that glyph’s chunk), matching “value 1 = bottom-right on.” Within a multi-bit digit, bits are MSB-first.

**Binary colors:** \(0\) = black, \(1\) = white.  
**Indexed / RGB colors:** index \(0\) = black; max index = full white or full `#FFFFFF` depending on profile.

## Suggested profiles (multi-version)

Ship named **profiles** rather than one hard-coded size. Data-driven:

`GlyphProfile { width, height, palette_size (C), … }`

| Profile id | Grid | Colors \(C\) | Bits / glyph | Notes |
|------------|------|--------------|--------------|--------|
| `bin8` | \(8\times 8\) | 2 | 64 | Compact binary tiles |
| `bin10` | \(10\times 10\) | 2 | 100 | \(\lvert\Sigma\rvert = 2^{100}\) demo |
| `bin16` | \(16\times 16\) | 2 | 256 | Larger font / more ink |
| `c8_8` | \(8\times 8\) | 8 | 192 | 3-bit RGB digits |
| `c256_8` | \(8\times 8\) | 256 | 512 | 1 byte / cell (grayscale) |
| `rgb24_8` | \(8\times 8\) | \(2^{24}\) | 1536 | Full 24-bit hex RGB per cell |

Human may rename profiles; keep them data-driven. **Default for demos:** `bin10`. Tests cover at least `bin8` + `bin10`.

## Phases

### Phase 0 — Spec + agents — **done / living**

1. Name, goals, non-goals, place-value rule, profile table.  
2. `PROGRESS.md` when code starts (Artificer-style).  
3. Keep this file’s status table honest.

### Phase 1 — Core codec (MVP) — **done**

1. ~~Cargo crate `glyphix` (lib; optional CLI behind feature `cli`).~~  
2. ~~`GlyphProfile` + validation (dims ≥ 1, \(C \ge 2\); power-of-two \(C\) for bit packing).~~  
3. ~~`Grid`: logical color indices; place ↔ \((x,y)\) helpers.~~  
4. ~~**Pack / unpack:** bits ↔ cells with **bottom-right place 0**.~~  
5. ~~`encode` / `decode` with explicit errors.~~  
6. ~~**Framing:** `version: u8` + `payload_len: u32` BE + payload + zero pad.~~  
7. ~~Unit/integration tests + goldens (value 0 all black; value 1 only BR white).~~  
8. ~~Hard cap on glyph count (default 4096).~~

### Phase 2 — Integrity (lightweight) — **done**

1. ~~Optional **CRC32** or **BLAKE3-128/256** trailer over payload (`Integrity` + `encode_with`).~~  
2. ~~Tests: flip one cell / trailer bit → decode error when check enabled.~~  
3. ~~Docs: **error detection**, not authentication; not a substitute for signatures.~~  
4. ~~v2 framing: `version | integrity tag | len | payload | trailer | pad`; v1 still decodable.~~

### Phase 3 — Render / parse clean bitmaps — **done**

1. ~~Render glyph or strip to **PNG/SVG** (exact pixels, no anti-alias / `crispEdges`).~~  
2. ~~**Cell scale \(S\):** each logical cell → \(S\times S\) device pixels.~~  
3. ~~Color map: binary B/W; 3-bit RGB; grayscale 256; 24-bit RGB (`src/color.rs`).~~  
4. ~~Decode from **clean** RGBA/PNG (uniform cell blocks, exact color map inverse).~~  
5. ~~CLI: `glyphix encode -p bin10 -o out.png -s 4`, `glyphix decode -p bin10 -s 4 in.png`.~~  
6. ~~PNG behind feature `render` (`image` crate, png-only); SVG always available.~~

### Phase 4 — Sequences & layout

1. Layout modes: horizontal strip, grid of glyphs, **quiet margins**, optional **separators**.  
2. Capacity calculator: `glyphix capacity -p bin10 -n 10`.  
3. Streaming API for large payloads (chunk into fixed glyph counts).

### Phase 5 — Error correction (optional, hard)

1. Reed–Solomon (or similar) over payload bytes *before* painting glyphs.  
2. Document tradeoff: fewer information bits per row, better damage tolerance.  
3. Still not a camera QR killer without finder patterns + perspective.

### Phase 6 — Stretch

1. Finder / alignment patterns; camera pipeline; palette quantization.  
2. Animated frames / color-temporal codes.  
3. **Familiar-shape search:** which indices approximate ASCII/Unicode ink patterns (demo / educational).  
4. Interop demos (URL, pubkey fingerprint as art — **not** as Effector address standard).  
5. Optional `SCROLL.md`-style educational writeup if the project grows (Artificer pattern).

## Architecture sketch

```
glyphix/
  AGENTS.md
  PROGRESS.md
  README.md
  Cargo.toml
  src/
    lib.rs             # public API
    profile.rs         # GlyphProfile, presets
    grid.rs            # Grid, place index, get/set
    pack.rs            # bits ↔ cells (base-C places)
    codec.rs           # encode/decode + framing header
    check.rs           # optional CRC/BLAKE3 (phase 2)
    color.rs           # index ↔ sRGB (phase 3)
    render.rs          # RGBA/SVG/PNG + cell scale (phase 3)
    layout.rs          # multi-row composition (phase 4 polish)
    error.rs           # thiserror types
  tests/
    roundtrip.rs
    golden/
```

Keep **pure encode/decode** free of image crates; put PNG behind a feature flag (`render`) if deps get heavy.

Single crate is fine for MVP (unlike Artificer’s multi-crate workspace). Split crates only if the tree grows enough to justify it.

## Coding standards

- **Deterministic:** same payload + profile ⇒ same glyphs (stable tests / goldens).  
- **Explicit errors** (`thiserror`); no silent truncation.  
- Document place order and color index mapping in module docs.  
- Prefer small deps; justify each in `PROGRESS.md` when added.  
- After meaningful work: append `PROGRESS.md`; update this file’s **Current status**.

## Open decisions (MVP leans)

Agents may pick sensible defaults and note them in `PROGRESS.md`; escalate only if product-facing:

| Topic | MVP lean |
|-------|----------|
| Length framing | v2: `version` + `integrity` + `payload_len: u32` BE + payload + trailer |
| Color model | Indexed \(0..C-1\); binary \(0\) black / \(1\) white |
| Place order | Bottom-right = place 0; left then up; low places fill first from stream |
| Non-power-of-two \(C\) | Reject or defer; power-of-two only for bit packing MVP |
| Max glyph count | Hard cap default **4096** |
| Default profile | `bin10` demos; tests **bin8** + **bin10** |
| Cell scale | Not in codec; render option (phase 3), default \(S=1\) |
| Version byte | Encode **`2`**; decode accepts **`1`** (legacy, no integrity field) |
| Integrity default | `Integrity::None` (tag 0); optional CRC32 / BLAKE3-128 / BLAKE3-256 |

## Agent workflow

1. Read this `AGENTS.md` (+ latest `PROGRESS.md`).  
2. Implement the user’s request or the next open phase item.  
3. `cargo test` (and CLI smoke when it exists).  
4. Append `PROGRESS.md`; refresh **Current status** here.  
5. Do **not** change Effector address specs to use Glyphix.

## Useful prompts

- “Implement Phase 1 round-trip for `bin8` and `bin10`.”  
- “Golden test: value 0 all black, value 1 bottom-right only.”  
- “Add BLAKE3-256 payload trailer behind a flag.”  
- “SVG strip render with cell scale 4 for a 32-byte payload.”  
- “Capacity table for all presets at L=1,4,10.”  
- “RS ECC rate 10% on payload then paint.”  
- “Find bin10 indices whose ink roughly matches ASCII ‘A’.”

## Current status

| Item | Status |
|------|--------|
| Project named Glyphix | **Done** |
| Vision + place-value rule in `AGENTS.md` | **Done** |
| `PROGRESS.md` / README | **Done** (living) |
| Phase 1 core codec | **Done** |
| Phase 2 integrity (CRC/BLAKE3) | **Done** |
| Phase 3 PNG/SVG + cell scale | **Done** |
| Phase 4 layout (grid, margins polish) | Partial (strip + margin/gap in render) |
| Phase 5+ ECC / camera / stretch | Not started |

**Next:** Phase 4 multi-row layout / capacity UX polish, or Phase 5 ECC if needed.

---

**Neighbor project:** Effector (`~/Projects/Crypto/effector`) — custom L1; addresses stay **bech32m**.  
**Doc/structure inspiration:** Artificer (`~/Projects/Artificer`) — phases in `AGENTS.md`, append-only `PROGRESS.md`.  
Glyphix is playground infrastructure for visual bases and sequenced glyphs.
