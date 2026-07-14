//! Glyphix CLI (feature `cli`, pulls in `render` for PNG).

#[cfg(feature = "cli")]
fn main() {
    use std::path::PathBuf;

    use clap::{Parser, Subcommand, ValueEnum};
    use glyphix::profile::{preset, preset_ids};
    use glyphix::render::{
        decode_png, encode_png, encode_svg, parse_rgba, render_rgba, render_svg, write_png,
        RenderOptions,
    };
    use glyphix::{
        capacity_payload_bytes_with, decode, encode_with, glyph_count_for_with, grid_to_bit_string,
        paint_bit_string, paint_bit_string_sequence, EncodeOptions, Integrity,
    };

    #[derive(Parser)]
    #[command(
        name = "glyphix",
        about = "Pixel-grid glyph codec (Library of Babel for glyphs)"
    )]
    struct Cli {
        #[command(subcommand)]
        cmd: Cmd,
    }

    #[derive(Clone, ValueEnum)]
    enum OutFormat {
        Png,
        Svg,
        /// Debug dump of color indices to stdout (no image file).
        Debug,
    }

    #[derive(Subcommand)]
    enum Cmd {
        /// List built-in profiles.
        Profiles,
        /// Show payload capacity for N glyphs.
        Capacity {
            #[arg(short = 'p', long, default_value = "bin10")]
            profile: String,
            #[arg(short = 'n', long, default_value_t = 1)]
            glyphs: usize,
            /// Integrity overhead: none | crc32 | blake3-128 | blake3-256
            #[arg(long, default_value = "none")]
            check: String,
        },
        /// Encode bytes/text to PNG, SVG, or a debug dump.
        Encode {
            #[arg(short = 'p', long, default_value = "bin10")]
            profile: String,
            #[arg(long, default_value = "none")]
            check: String,
            /// Cell scale S (device pixels per logical cell).
            #[arg(short = 's', long, default_value_t = 4)]
            scale: u32,
            /// Quiet margin in device pixels.
            #[arg(long, default_value_t = 0)]
            margin: u32,
            /// Gap between glyphs in device pixels.
            #[arg(long, default_value_t = 0)]
            gap: u32,
            /// Output path (`.png` / `.svg`); required unless `--format debug`.
            #[arg(short = 'o', long)]
            output: Option<PathBuf>,
            /// Force format (default: from output extension, else debug).
            #[arg(long, value_enum)]
            format: Option<OutFormat>,
            /// Read payload from file instead of `text`.
            #[arg(short = 'i', long)]
            input: Option<PathBuf>,
            /// Payload text (UTF-8). Ignored if `--input` is set.
            text: Option<String>,
        },
        /// Decode a clean PNG (or re-parse after encode) to payload.
        Decode {
            #[arg(short = 'p', long, default_value = "bin10")]
            profile: String,
            /// Cell scale used when the image was rendered.
            #[arg(short = 's', long, default_value_t = 4)]
            scale: u32,
            #[arg(long, default_value_t = 0)]
            margin: u32,
            #[arg(long, default_value_t = 0)]
            gap: u32,
            /// Write payload bytes to this file (default: stdout if text-like, else hex note).
            #[arg(short = 'o', long)]
            output: Option<PathBuf>,
            /// Input PNG path.
            input: PathBuf,
        },
        /// Round-trip encode then decode (optional PNG file in the middle).
        Roundtrip {
            #[arg(short = 'p', long, default_value = "bin10")]
            profile: String,
            #[arg(long, default_value = "none")]
            check: String,
            #[arg(short = 's', long, default_value_t = 4)]
            scale: u32,
            /// If set, write PNG and decode via raster.
            #[arg(long)]
            png: Option<PathBuf>,
            text: String,
        },
        /// Paint glyph(s) from a human bit string (not framed text encode).
        ///
        /// Single glyph: rightmost bit = bottom-right; pad left if short; trunc right if long.
        ///
        /// Sequence:
        /// - Comma-separated: `1,11,101` → one glyph per field (each pad/trunc alone)
        /// - Continuous + `--strip`: bits spill into the next glyph; final partial left-padded
        Paint {
            #[arg(short = 'p', long, default_value = "bin8")]
            profile: String,
            /// Bit string (0/1; `_` `-` whitespace ignored). Use commas for multiple glyphs.
            #[arg(short = 'b', long)]
            bits: String,
            /// Force this many glyphs (stream or comma list padded/truncated to N).
            #[arg(short = 'n', long)]
            glyphs: Option<usize>,
            /// Continuous multi-glyph stream (overflow → next char). Implied by commas in `-b`.
            #[arg(long, default_value_t = false)]
            strip: bool,
            #[arg(short = 's', long, default_value_t = 8)]
            scale: u32,
            #[arg(long, default_value_t = 0)]
            margin: u32,
            #[arg(long, default_value_t = 0)]
            gap: u32,
            #[arg(short = 'o', long)]
            output: Option<PathBuf>,
            #[arg(long, value_enum)]
            format: Option<OutFormat>,
            /// Also print the normalized bit string(s) to stdout.
            #[arg(long, default_value_t = false)]
            show_bits: bool,
        },
    }

    fn load_payload(input: &Option<PathBuf>, text: &Option<String>) -> Vec<u8> {
        if let Some(path) = input {
            std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        } else if let Some(t) = text {
            t.as_bytes().to_vec()
        } else {
            panic!("provide text argument or --input file");
        }
    }

    fn guess_format(output: &Option<PathBuf>, format: &Option<OutFormat>) -> OutFormat {
        if let Some(f) = format {
            return f.clone();
        }
        if let Some(path) = output {
            match path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .as_deref()
            {
                Some("png") => OutFormat::Png,
                Some("svg") => OutFormat::Svg,
                _ => OutFormat::Debug,
            }
        } else {
            OutFormat::Debug
        }
    }

    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Profiles => {
            for id in preset_ids() {
                let p = preset(id).unwrap();
                println!(
                    "{id:10}  {}x{}  C={}  bits/glyph={}",
                    p.width,
                    p.height,
                    p.palette_size,
                    p.bits_per_glyph()
                );
            }
        }
        Cmd::Capacity {
            profile,
            glyphs,
            check,
        } => {
            let p = preset(&profile).expect("profile");
            let integ = Integrity::parse(&check).expect("check");
            let bytes = capacity_payload_bytes_with(&p, glyphs, integ);
            println!(
                "profile={profile} glyphs={glyphs} check={} payload_bytes_max={bytes} bits={}",
                integ.as_str(),
                p.bits_per_glyph() * glyphs
            );
        }
        Cmd::Encode {
            profile,
            check,
            scale,
            margin,
            gap,
            output,
            format,
            input,
            text,
        } => {
            let p = preset(&profile).expect("profile");
            let integ = Integrity::parse(&check).expect("check");
            let payload = load_payload(&input, &text);
            let enc = EncodeOptions::with_integrity(integ);
            let opts = RenderOptions {
                cell_scale: scale,
                margin,
                gap,
                ..RenderOptions::default()
            };
            opts.validate().expect("render options");
            let fmt = guess_format(&output, &format);

            match fmt {
                OutFormat::Png => {
                    let path = output.expect("-o path required for PNG");
                    encode_png(&p, &payload, &path, &opts, enc).expect("encode png");
                    println!(
                        "wrote {} ({} bytes payload, scale={scale}, check={})",
                        path.display(),
                        payload.len(),
                        integ.as_str()
                    );
                }
                OutFormat::Svg => {
                    let svg = encode_svg(&p, &payload, &opts, enc).expect("encode svg");
                    if let Some(path) = output {
                        std::fs::write(&path, svg.as_bytes())
                            .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
                        println!(
                            "wrote {} ({} bytes payload, scale={scale})",
                            path.display(),
                            payload.len()
                        );
                    } else {
                        print!("{svg}");
                    }
                }
                OutFormat::Debug => {
                    let glyphs = encode_with(&p, &payload, enc).expect("encode");
                    println!(
                        "glyphs={} check={} scale={scale}",
                        glyphs.len(),
                        integ.as_str()
                    );
                    for (i, g) in glyphs.iter().enumerate() {
                        print!("glyph[{i}]:");
                        for y in 0..g.height() {
                            print!(" row{y}=[");
                            for x in 0..g.width() {
                                if x > 0 {
                                    print!(",");
                                }
                                print!("{}", g.get(x, y).unwrap());
                            }
                            print!("]");
                        }
                        println!();
                    }
                }
            }
        }
        Cmd::Decode {
            profile,
            scale,
            margin,
            gap,
            output,
            input,
        } => {
            let p = preset(&profile).expect("profile");
            let opts = RenderOptions {
                cell_scale: scale,
                margin,
                gap,
                ..RenderOptions::default()
            };
            let payload = if input
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("png"))
                .unwrap_or(false)
            {
                decode_png(&p, &input, &opts).expect("decode png")
            } else {
                // Treat as PNG anyway if image crate can open it; else error.
                decode_png(&p, &input, &opts).expect("decode image as png/rgba")
            };

            if let Some(path) = output {
                std::fs::write(&path, &payload)
                    .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
                println!("wrote {} ({} bytes)", path.display(), payload.len());
            } else if let Ok(s) = std::str::from_utf8(&payload) {
                println!("{s}");
            } else {
                println!("{}", hex::encode_fallback(&payload));
            }
        }
        Cmd::Roundtrip {
            profile,
            check,
            scale,
            png,
            text,
        } => {
            let p = preset(&profile).expect("profile");
            let integ = Integrity::parse(&check).expect("check");
            let enc = EncodeOptions::with_integrity(integ);
            let opts = RenderOptions::scale(scale).expect("scale");
            let payload = text.as_bytes();

            let out = if let Some(path) = png {
                encode_png(&p, payload, &path, &opts, enc).expect("png encode");
                let decoded = decode_png(&p, &path, &opts).expect("png decode");
                println!("png {}", path.display());
                decoded
            } else {
                let glyphs = encode_with(&p, payload, enc).expect("encode");
                let img = render_rgba(&p, &glyphs, &opts).expect("render");
                let back = parse_rgba(&p, &img, &opts).expect("parse");
                decode(&p, &back).expect("decode")
            };

            assert_eq!(out, payload);
            let n = glyph_count_for_with(&p, payload, integ).unwrap();
            println!(
                "ok profile={profile} check={} glyphs={n} scale={scale} bytes={}",
                integ.as_str(),
                payload.len()
            );
        }
        Cmd::Paint {
            profile,
            bits,
            glyphs: glyph_n,
            strip,
            scale,
            margin,
            gap,
            output,
            format,
            show_bits,
        } => {
            let p = preset(&profile).expect("profile");
            let opts = RenderOptions {
                cell_scale: scale,
                margin,
                gap,
                ..RenderOptions::default()
            };
            opts.validate().expect("render options");

            // Commas → multi-glyph list. --strip / -n → continuous stream (or forced N).
            // Plain short/long string without those → single glyph (pad left / trunc right).
            let gs = if bits.contains(',') || strip || glyph_n.is_some() {
                paint_bit_string_sequence(&p, &bits, glyph_n).expect("paint bits")
            } else {
                vec![paint_bit_string(&p, &bits).expect("paint bits")]
            };

            if show_bits {
                for (i, g) in gs.iter().enumerate() {
                    let s = grid_to_bit_string(&p, g).expect("bits");
                    println!("glyph[{i}] bits={s}");
                }
            }

            let fmt = guess_format(&output, &format);
            match fmt {
                OutFormat::Png => {
                    let path = output.expect("-o path required for PNG");
                    let img = render_rgba(&p, &gs, &opts).expect("render");
                    write_png(&path, &img).expect("write png");
                    println!(
                        "wrote {} ({} glyph(s), scale={scale}, bit-paint)",
                        path.display(),
                        gs.len()
                    );
                }
                OutFormat::Svg => {
                    let svg = render_svg(&p, &gs, &opts).expect("svg");
                    if let Some(path) = output {
                        std::fs::write(&path, svg.as_bytes())
                            .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
                        println!("wrote {} ({} glyph(s))", path.display(), gs.len());
                    } else {
                        print!("{svg}");
                    }
                }
                OutFormat::Debug => {
                    println!(
                        "glyphs={} profile={profile} bits_per_glyph={}",
                        gs.len(),
                        p.bits_per_glyph()
                    );
                    for (i, g) in gs.iter().enumerate() {
                        print!("glyph[{i}]:");
                        for y in 0..g.height() {
                            print!(" row{y}=[");
                            for x in 0..g.width() {
                                if x > 0 {
                                    print!(",");
                                }
                                print!("{}", g.get(x, y).unwrap());
                            }
                            print!("]");
                        }
                        println!();
                    }
                }
            }
        }
    }

    /// Tiny hex encoder so we do not add a hex crate for rare binary stdout.
    mod hex {
        pub fn encode_fallback(bytes: &[u8]) -> String {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let mut s = String::with_capacity(bytes.len() * 2);
            for &b in bytes {
                s.push(HEX[(b >> 4) as usize] as char);
                s.push(HEX[(b & 0xf) as usize] as char);
            }
            s
        }
    }
}

#[cfg(not(feature = "cli"))]
fn main() {
    eprintln!(
        "glyphix CLI is disabled; rebuild with: cargo run --features cli -- --help\n\
         Library: glyphix::{{encode, decode, render_rgba, render_svg, bin10}}\n\
         PNG helpers need feature `render`."
    );
    std::process::exit(2);
}
