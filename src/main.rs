//! Glyphix CLI (feature `cli`, pulls in `render` for PNG).

#[cfg(feature = "cli")]
fn main() {
    use std::path::PathBuf;

    use clap::{Parser, Subcommand, ValueEnum};
    use glyphix::capacity::{format_report_line, report_opts, table_all_presets_opts};
    use glyphix::profile::{preset, preset_ids};
    use glyphix::render::{
        decode_png, encode_png, encode_svg, parse_rgba, render_rgba, render_svg, write_png,
        RenderOptions,
    };
    use glyphix::stream::{decode_chunked, encode_chunked};
    use glyphix::{
        decode, encode_with, glyph_count_for_opts, grid_to_bit_string, paint_bit_string,
        paint_bit_string_sequence, Ecc, EncodeOptions, GlyphLayout, Integrity, Separator,
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

    #[derive(Clone, ValueEnum)]
    enum LayoutCli {
        Strip,
        Grid,
    }

    #[derive(Subcommand)]
    enum Cmd {
        /// List built-in profiles.
        Profiles,
        /// Show payload capacity (single profile or all presets).
        Capacity {
            #[arg(short = 'p', long, default_value = "bin10")]
            profile: String,
            /// Glyph counts (comma-separated), e.g. 1,4,10
            #[arg(short = 'n', long, default_value = "1")]
            glyphs: String,
            #[arg(long, default_value = "none")]
            check: String,
            /// ECC: none | rs10 | rs20 | rs:N
            #[arg(long, default_value = "none")]
            ecc: String,
            /// Cell scale for image size column.
            #[arg(short = 's', long, default_value_t = 1)]
            scale: u32,
            /// Print a table for every preset at the given -n values.
            #[arg(long, default_value_t = false)]
            all: bool,
            /// Grid columns (image size uses grid layout when set).
            #[arg(long)]
            columns: Option<u32>,
        },
        /// Encode bytes/text to PNG, SVG, or a debug dump.
        Encode {
            #[arg(short = 'p', long, default_value = "bin10")]
            profile: String,
            #[arg(long, default_value = "none")]
            check: String,
            /// ECC: none | rs10 | rs20 | rs:N (Reed–Solomon before paint)
            #[arg(long, default_value = "none")]
            ecc: String,
            #[arg(short = 's', long, default_value_t = 4)]
            scale: u32,
            #[arg(long, default_value_t = 0)]
            margin: u32,
            #[arg(long, default_value_t = 0)]
            gap: u32,
            #[arg(long)]
            gap_y: Option<u32>,
            /// Layout: strip (default) or grid.
            #[arg(long, value_enum, default_value_t = LayoutCli::Strip)]
            layout: LayoutCli,
            /// Grid columns (required for --layout grid; default 4).
            #[arg(long)]
            columns: Option<u32>,
            /// Separator bar thickness in device pixels (0 = none).
            #[arg(long, default_value_t = 0)]
            separator: u32,
            /// Max glyphs per independent chunk (0 = single frame).
            #[arg(long, default_value_t = 0)]
            chunk_glyphs: usize,
            #[arg(short = 'o', long)]
            output: Option<PathBuf>,
            #[arg(long, value_enum)]
            format: Option<OutFormat>,
            #[arg(short = 'i', long)]
            input: Option<PathBuf>,
            text: Option<String>,
        },
        /// Decode a clean PNG to payload.
        Decode {
            #[arg(short = 'p', long, default_value = "bin10")]
            profile: String,
            #[arg(short = 's', long, default_value_t = 4)]
            scale: u32,
            #[arg(long, default_value_t = 0)]
            margin: u32,
            #[arg(long, default_value_t = 0)]
            gap: u32,
            #[arg(long)]
            gap_y: Option<u32>,
            #[arg(long, value_enum, default_value_t = LayoutCli::Strip)]
            layout: LayoutCli,
            #[arg(long)]
            columns: Option<u32>,
            #[arg(long, default_value_t = 0)]
            separator: u32,
            /// Multiple chunk PNGs (order preserved); alternative to single `input`.
            #[arg(long)]
            chunks: Vec<PathBuf>,
            #[arg(short = 'o', long)]
            output: Option<PathBuf>,
            input: Option<PathBuf>,
        },
        /// Round-trip encode then decode.
        Roundtrip {
            #[arg(short = 'p', long, default_value = "bin10")]
            profile: String,
            #[arg(long, default_value = "none")]
            check: String,
            #[arg(long, default_value = "none")]
            ecc: String,
            #[arg(short = 's', long, default_value_t = 4)]
            scale: u32,
            #[arg(long)]
            png: Option<PathBuf>,
            #[arg(long, value_enum, default_value_t = LayoutCli::Strip)]
            layout: LayoutCli,
            #[arg(long)]
            columns: Option<u32>,
            text: String,
        },
        /// Paint glyph(s) from a human bit string (not framed text encode).
        Paint {
            #[arg(short = 'p', long, default_value = "bin8")]
            profile: String,
            #[arg(short = 'b', long)]
            bits: String,
            #[arg(short = 'n', long)]
            glyphs: Option<usize>,
            #[arg(long, default_value_t = false)]
            strip: bool,
            #[arg(short = 's', long, default_value_t = 8)]
            scale: u32,
            #[arg(long, default_value_t = 0)]
            margin: u32,
            #[arg(long, default_value_t = 0)]
            gap: u32,
            #[arg(long)]
            gap_y: Option<u32>,
            #[arg(long, value_enum, default_value_t = LayoutCli::Strip)]
            layout: LayoutCli,
            #[arg(long)]
            columns: Option<u32>,
            #[arg(long, default_value_t = 0)]
            separator: u32,
            #[arg(short = 'o', long)]
            output: Option<PathBuf>,
            #[arg(long, value_enum)]
            format: Option<OutFormat>,
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

    fn parse_ns(s: &str) -> Vec<usize> {
        s.split(',')
            .map(|p| p.trim().parse::<usize>().expect("glyph count integer"))
            .collect()
    }

    fn make_render_opts(
        scale: u32,
        margin: u32,
        gap: u32,
        gap_y: Option<u32>,
        layout: LayoutCli,
        columns: Option<u32>,
        separator: u32,
    ) -> RenderOptions {
        let lay = match layout {
            LayoutCli::Strip => GlyphLayout::HorizontalStrip,
            LayoutCli::Grid => GlyphLayout::grid(columns.unwrap_or(4)).expect("columns"),
        };
        let sep = if separator > 0 {
            Some(Separator::gray(separator).expect("separator"))
        } else {
            None
        };
        let opts = RenderOptions {
            cell_scale: scale,
            margin,
            gap,
            gap_y,
            layout: lay,
            separator: sep,
        };
        opts.validate().expect("render options");
        opts
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
            ecc,
            scale,
            all,
            columns,
        } => {
            let integ = Integrity::parse(&check).expect("check");
            let ecc = Ecc::parse(&ecc).expect("ecc");
            let opts = EncodeOptions {
                integrity: integ,
                ecc,
            };
            let ns = parse_ns(&glyphs);
            if all {
                let rows = table_all_presets_opts(&ns, opts, scale).expect("table");
                for r in rows {
                    println!("{}", format_report_line(&r));
                }
            } else {
                let p = preset(&profile).expect("profile");
                for n in ns {
                    let mut r = report_opts(&profile, &p, n, opts, scale).expect("report");
                    if let Some(cols) = columns {
                        let layout = glyphix::LayoutOptions::grid(scale, cols, 0, 0).expect("grid");
                        let (w, h) =
                            glyphix::layout::image_size(&p, n.max(1), &layout).expect("size");
                        r.image_w = w;
                        r.image_h = h;
                    }
                    println!("{}", format_report_line(&r));
                    println!(
                        "  payload_bytes_max={}  total_bits={}  image={}x{} @ scale={}",
                        r.payload_bytes, r.total_bits, r.image_w, r.image_h, r.cell_scale
                    );
                }
            }
        }
        Cmd::Encode {
            profile,
            check,
            ecc,
            scale,
            margin,
            gap,
            gap_y,
            layout,
            columns,
            separator,
            chunk_glyphs,
            output,
            format,
            input,
            text,
        } => {
            let p = preset(&profile).expect("profile");
            let integ = Integrity::parse(&check).expect("check");
            let ecc = Ecc::parse(&ecc).expect("ecc");
            let payload = load_payload(&input, &text);
            let enc = EncodeOptions::with_integrity_and_ecc(integ, ecc);
            let opts = make_render_opts(scale, margin, gap, gap_y, layout, columns, separator);
            let fmt = guess_format(&output, &format);

            if chunk_glyphs > 0 {
                let frames = encode_chunked(&p, &payload, enc, chunk_glyphs).expect("chunked");
                let base = output.expect("-o base path required for chunked encode (e.g. out.png)");
                let stem = base
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("chunk");
                let ext = base
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("png");
                let parent = base.parent().unwrap_or_else(|| std::path::Path::new("."));
                for (i, glyphs) in frames.iter().enumerate() {
                    let path = parent.join(format!("{stem}_{i:04}.{ext}"));
                    match fmt {
                        OutFormat::Png | OutFormat::Debug => {
                            let img = render_rgba(&p, glyphs, &opts).expect("render");
                            write_png(&path, &img).expect("png");
                        }
                        OutFormat::Svg => {
                            let svg = render_svg(&p, glyphs, &opts).expect("svg");
                            std::fs::write(&path, svg).expect("write svg");
                        }
                    }
                    println!(
                        "wrote {} (chunk {i}/{}, {} glyphs)",
                        path.display(),
                        frames.len(),
                        glyphs.len()
                    );
                }
                println!(
                    "chunked {} bytes → {} frame(s), max {} glyphs/frame, check={}, ecc={}",
                    payload.len(),
                    frames.len(),
                    chunk_glyphs,
                    integ.as_str(),
                    ecc.as_str()
                );
                return;
            }

            match fmt {
                OutFormat::Png => {
                    let path = output.expect("-o path required for PNG");
                    encode_png(&p, &payload, &path, &opts, enc).expect("encode png");
                    println!(
                        "wrote {} ({} bytes payload, scale={scale}, check={}, ecc={})",
                        path.display(),
                        payload.len(),
                        integ.as_str(),
                        ecc.as_str()
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
            gap_y,
            layout,
            columns,
            separator,
            chunks,
            output,
            input,
        } => {
            let p = preset(&profile).expect("profile");
            let opts = make_render_opts(scale, margin, gap, gap_y, layout, columns, separator);

            let payload = if !chunks.is_empty() {
                let mut frames = Vec::new();
                for path in &chunks {
                    let img = glyphix::read_png(path).expect("read png");
                    frames.push(parse_rgba(&p, &img, &opts).expect("parse"));
                }
                decode_chunked(&p, &frames).expect("decode chunked")
            } else {
                let path = input.expect("input PNG or --chunks");
                decode_png(&p, &path, &opts).expect("decode png")
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
            ecc,
            scale,
            png,
            layout,
            columns,
            text,
        } => {
            let p = preset(&profile).expect("profile");
            let integ = Integrity::parse(&check).expect("check");
            let ecc = Ecc::parse(&ecc).expect("ecc");
            let enc = EncodeOptions::with_integrity_and_ecc(integ, ecc);
            let opts = make_render_opts(scale, 0, 0, None, layout, columns, 0);
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
            let n = glyph_count_for_opts(&p, payload, enc).unwrap();
            println!(
                "ok profile={profile} check={} ecc={} glyphs={n} scale={scale} bytes={}",
                integ.as_str(),
                ecc.as_str(),
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
            gap_y,
            layout,
            columns,
            separator,
            output,
            format,
            show_bits,
        } => {
            let p = preset(&profile).expect("profile");
            let opts = make_render_opts(scale, margin, gap, gap_y, layout, columns, separator);

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
         Library: glyphix::{{encode, decode, render_rgba, render_svg, encode_chunked, bin10}}\n\
         PNG helpers need feature `render`."
    );
    std::process::exit(2);
}
