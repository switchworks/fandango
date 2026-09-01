//! GUI 素材 (静止画・動画) の生成ツール。開発用であり配布物には入らない。
//!
//! 計画: `plans/shots_v01.md`

mod anim;
mod chain;
mod gui_context;
mod offscreen;
mod promo;
mod targets;
mod tile;

use std::collections::BTreeMap;
use std::path::PathBuf;

use targets::Target;

fn usage() -> ! {
    eprintln!("usage:");
    eprintln!("  fandango-shots still --crate <name> [--set <id>=<0.0-1.0>]… [--scale <f>] --out <path.png>");
    eprintln!("  fandango-shots anim --crate <name> [--fps <n>] [--scale <f>] [--text <id>=<s>] --key <t>:<id>=<v>[,…]… --out <dir>");
    eprintln!("  fandango-shots tile [--set <crate>:<id>=<0.0-1.0>]… [--bg <#rrggbb>] [--border <#rrggbb>]");
    eprintln!("                      [--header <text>] [--header-size <f>] --out <path.png>");
    eprintln!("  fandango-shots tile [--fps <n>] --key <t>:<crate>:<id>=<v>[,…]… --out <dir>   (動画)");
    eprintln!("  fandango-shots names --dur <sec> [--fps <n>] [--start <s>] [--step <s>] [--wipe <s>] --out <dir>");
    eprintln!("  fandango-shots chain --dur <sec> [--fps <n>] --stage <t>:<crate>[:<id>=<v>,…]… --out <dir>");
    eprintln!("  fandango-shots cover [--rule neutral|spectrum] --out <path.png>  (Gumroad カバー 1920×1080)");
    eprintln!("  fandango-shots thumb [--rule neutral|spectrum] --out <path.png>  (Gumroad サムネイル 1200×1200)");
    eprintln!("      <id>=<v> のうち pattern= だけは文字列 (gate のステップパターン)");
    eprintln!("  <name>: {}", Target::all_names().join(" "));
    std::process::exit(2);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else { usage() };
    match command.as_str() {
        "still" => {
            let mut target: Option<Target> = None;
            let mut out: Option<PathBuf> = None;
            let mut scale: f32 = 1.0;
            let mut set: BTreeMap<String, f32> = BTreeMap::new();
            while let Some(flag) = args.next() {
                match flag.as_str() {
                    "--crate" => {
                        let name = args.next().unwrap_or_else(|| usage());
                        target = Some(Target::from_name(&name).unwrap_or_else(|| {
                            eprintln!("unknown crate: {name}");
                            usage()
                        }));
                    }
                    "--out" => out = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
                    "--set" => {
                        let (id, value) = parse_set(&args.next().unwrap_or_else(|| usage()));
                        set.insert(id, value);
                    }
                    "--scale" => {
                        scale = args
                            .next()
                            .and_then(|v| v.parse::<f32>().ok())
                            .unwrap_or_else(|| usage());
                    }
                    other => {
                        eprintln!("unknown flag: {other}");
                        usage()
                    }
                }
            }
            let (Some(target), Some(out)) = (target, out) else {
                usage()
            };
            still(target, &out, scale, &set);
        }
        "anim" => {
            let mut target: Option<Target> = None;
            let mut out: Option<PathBuf> = None;
            let mut fps: u32 = 60;
            let mut scale: f32 = 1.0;
            let mut keys: Vec<anim::Key> = Vec::new();
            let mut texts: BTreeMap<String, String> = BTreeMap::new();
            while let Some(flag) = args.next() {
                match flag.as_str() {
                    "--crate" => {
                        let name = args.next().unwrap_or_else(|| usage());
                        target = Some(Target::from_name(&name).unwrap_or_else(|| {
                            eprintln!("unknown crate: {name}");
                            usage()
                        }));
                    }
                    // `#[persist]` の文字列状態。現状は gate の pattern だけ。
                    "--text" => {
                        let spec = args.next().unwrap_or_else(|| usage());
                        let Some((id, value)) = spec.split_once('=') else {
                            eprintln!("--text の書式は <id>=<文字列>: {spec}");
                            usage()
                        };
                        texts.insert(id.to_string(), value.to_string());
                    }
                    "--out" => out = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
                    "--fps" => {
                        fps = args
                            .next()
                            .and_then(|v| v.parse::<u32>().ok())
                            .filter(|v| *v > 0)
                            .unwrap_or_else(|| usage());
                    }
                    "--scale" => {
                        scale = args
                            .next()
                            .and_then(|v| v.parse::<f32>().ok())
                            .unwrap_or_else(|| usage());
                    }
                    "--key" => keys.push(parse_key(&args.next().unwrap_or_else(|| usage()))),
                    other => {
                        eprintln!("unknown flag: {other}");
                        usage()
                    }
                }
            }
            let (Some(target), Some(out)) = (target, out) else {
                usage()
            };
            let timeline = anim::Timeline::new(keys).unwrap_or_else(|e| {
                eprintln!("{e}");
                usage()
            });
            anim::render(target, &timeline, &texts, fps, scale, &out);
        }
        "tile" => {
            let mut out: Option<PathBuf> = None;
            let mut style = tile::Style::default();
            let mut set: BTreeMap<String, BTreeMap<String, f32>> = BTreeMap::new();
            let mut fps: u32 = 60;
            let mut keys: Vec<anim::Key> = Vec::new();
            while let Some(flag) = args.next() {
                match flag.as_str() {
                    "--out" => out = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
                    "--fps" => {
                        fps = args
                            .next()
                            .and_then(|v| v.parse::<u32>().ok())
                            .filter(|v| *v > 0)
                            .unwrap_or_else(|| usage());
                    }
                    "--key" => keys.push(parse_tile_key(&args.next().unwrap_or_else(|| usage()))),
                    "--set" => {
                        let spec = args.next().unwrap_or_else(|| usage());
                        let Some((krate, rest)) = spec.split_once(':') else {
                            eprintln!("tile の --set は <crate>:<id>=<0.0-1.0>: {spec}");
                            usage()
                        };
                        if Target::from_name(krate).is_none() {
                            eprintln!("unknown crate: {krate}");
                            usage()
                        }
                        let (id, value) = parse_set(rest);
                        set.entry(krate.to_string()).or_default().insert(id, value);
                    }
                    "--bg" => style.background = parse_rgb(&args.next().unwrap_or_else(|| usage())),
                    "--border" => {
                        style.border = parse_rgb(&args.next().unwrap_or_else(|| usage()))
                    }
                    "--header" => style.header_text = args.next().unwrap_or_else(|| usage()),
                    "--header-size" => {
                        style.header_size = args
                            .next()
                            .and_then(|v| v.parse::<f32>().ok())
                            .unwrap_or_else(|| usage());
                    }
                    other => {
                        eprintln!("unknown flag: {other}");
                        usage()
                    }
                }
            }
            let Some(out) = out else { usage() };
            // `--key` があれば動画。`--out` は連番 PNG のディレクトリになる。
            if keys.is_empty() {
                offscreen::with_renderer(tile::CANVAS_W, tile::CANVAS_H, |renderer| {
                    let canvas = tile::render(renderer, &style, &set);
                    offscreen::save_png(&canvas, &out);
                });
                println!("{}", out.display());
            } else {
                let timeline = anim::Timeline::new(keys).unwrap_or_else(|e| {
                    eprintln!("{e}");
                    usage()
                });
                anim::render_tile(&timeline, &set, &style, fps, &out);
            }
        }
        "cover" | "thumb" => {
            let mut out: Option<PathBuf> = None;
            let mut rule = promo::RuleStyle::Neutral;
            while let Some(flag) = args.next() {
                match flag.as_str() {
                    "--out" => out = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
                    "--rule" => {
                        let v = args.next().unwrap_or_else(|| usage());
                        rule = promo::RuleStyle::parse(&v).unwrap_or_else(|| {
                            eprintln!("--rule は neutral / spectrum: {v}");
                            usage()
                        });
                    }
                    other => {
                        eprintln!("unknown flag: {other}");
                        usage()
                    }
                }
            }
            let Some(out) = out else { usage() };
            let (w, h) = if command == "cover" {
                (promo::COVER_W, promo::COVER_H)
            } else {
                (promo::THUMB, promo::THUMB)
            };
            offscreen::with_renderer(w, h, |renderer| {
                let img = if command == "cover" {
                    promo::render_cover(renderer, rule)
                } else {
                    promo::render_thumb(renderer, rule)
                };
                offscreen::save_png(&img, &out);
            });
            println!("{}", out.display());
        }
        "names" => {
            let mut out: Option<PathBuf> = None;
            let mut fps: u32 = 60;
            let mut dur: Option<f32> = None;
            let mut start = 0.0_f32;
            let mut step = 0.5_f32;
            let mut wipe = 0.2_f32;
            let mut set: BTreeMap<String, BTreeMap<String, f32>> = BTreeMap::new();
            while let Some(flag) = args.next() {
                let num = |args: &mut dyn Iterator<Item = String>| -> f32 {
                    args.next()
                        .and_then(|v| v.parse::<f32>().ok())
                        .unwrap_or_else(|| usage())
                };
                match flag.as_str() {
                    "--out" => out = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
                    "--fps" => {
                        fps = args
                            .next()
                            .and_then(|v| v.parse::<u32>().ok())
                            .filter(|v| *v > 0)
                            .unwrap_or_else(|| usage());
                    }
                    "--dur" => dur = Some(num(&mut args)),
                    "--start" => start = num(&mut args),
                    "--step" => step = num(&mut args),
                    "--wipe" => wipe = num(&mut args),
                    "--set" => {
                        let spec = args.next().unwrap_or_else(|| usage());
                        let (krate, rest) = spec.split_once(':').unwrap_or_else(|| {
                            eprintln!("--set の書式は <crate>:<id>=<v>: {spec}");
                            usage()
                        });
                        if Target::from_name(krate).is_none() {
                            eprintln!("unknown crate: {krate}");
                            usage()
                        }
                        let (id, value) = parse_set(rest);
                        set.entry(krate.to_string()).or_default().insert(id, value);
                    }
                    other => {
                        eprintln!("unknown flag: {other}");
                        usage()
                    }
                }
            }
            let (Some(out), Some(dur)) = (out, dur) else { usage() };
            anim::render_tile_names(
                &set,
                &tile::Style::default(),
                start,
                step,
                wipe,
                dur,
                fps,
                &out,
            );
        }
        "chain" => {
            let mut out: Option<PathBuf> = None;
            let mut fps: u32 = 60;
            let mut dur: Option<f32> = None;
            let mut stages: Vec<chain::Stage> = Vec::new();
            while let Some(flag) = args.next() {
                match flag.as_str() {
                    "--out" => out = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
                    "--fps" => {
                        fps = args
                            .next()
                            .and_then(|v| v.parse::<u32>().ok())
                            .filter(|v| *v > 0)
                            .unwrap_or_else(|| usage());
                    }
                    "--dur" => {
                        dur = args
                            .next()
                            .and_then(|v| v.parse::<f32>().ok())
                            .filter(|v| *v > 0.0);
                        if dur.is_none() {
                            eprintln!("--dur は 0 より大きい秒数");
                            usage()
                        }
                    }
                    "--stage" => stages.push(parse_stage(&args.next().unwrap_or_else(|| usage()))),
                    other => {
                        eprintln!("unknown flag: {other}");
                        usage()
                    }
                }
            }
            let (Some(out), Some(dur)) = (out, dur) else { usage() };
            if stages.is_empty() {
                eprintln!("--stage が 1 つも無い");
                usage()
            }
            anim::render_chain(&stages, tile::Style::default().background, dur, fps, &out);
        }
        other => {
            eprintln!("unknown command: {other}");
            usage()
        }
    }
}

/// `<id>=<0.0-1.0>` を割る。書式違反は usage で落とす。
fn parse_set(spec: &str) -> (String, f32) {
    let Some((id, value)) = spec.split_once('=') else {
        eprintln!("--set の書式は <id>=<0.0-1.0>: {spec}");
        usage()
    };
    let Ok(value) = value.parse::<f32>() else {
        eprintln!("--set の値が数値でない: {spec}");
        usage()
    };
    (id.to_string(), value)
}

/// `<time>:<crate>[:<id>=<v>,…]` を割る。書式違反は usage で落とす。
fn parse_stage(spec: &str) -> chain::Stage {
    let Some((time, rest)) = spec.split_once(':') else {
        eprintln!("--stage の書式は <t>:<crate>[:<id>=<v>,…]: {spec}");
        usage()
    };
    let Ok(at) = time.parse::<f32>() else {
        eprintln!("--stage の時刻が数値でない: {spec}");
        usage()
    };
    let (name, params) = match rest.split_once(':') {
        Some((name, params)) => (name, params),
        None => (rest, ""),
    };
    let Some(target) = Target::from_name(name) else {
        eprintln!("unknown crate: {name}");
        usage()
    };
    let mut values = BTreeMap::new();
    let mut texts = BTreeMap::new();
    if !params.is_empty() {
        for entry in params.split(',') {
            // `pattern=…` だけは文字列。数値として解釈しない。
            if let Some(text) = entry.strip_prefix("pattern=") {
                texts.insert("pattern".to_string(), text.to_string());
                continue;
            }
            let (id, value) = parse_set(entry);
            values.insert(id, value);
        }
    }
    chain::Stage {
        target,
        at,
        values,
        texts,
    }
}

/// `<time>:<id>=<v>[,<id>=<v>]…` を割る。書式違反は usage で落とす。
fn parse_key(spec: &str) -> anim::Key {
    let Some((time, rest)) = spec.split_once(':') else {
        eprintln!("--key の書式は <time>:<id>=<0.0-1.0>[,…]: {spec}");
        usage()
    };
    let Ok(time) = time.parse::<f32>() else {
        eprintln!("--key の時刻が数値でない: {spec}");
        usage()
    };
    let values = rest.split(',').map(parse_set).collect();
    anim::Key { time, values }
}

/// tile 用のキーフレーム `<time>:<crate>:<id>=<v>[,<crate>:<id>=<v>]…` を割る。
/// crate 名の実在はここで検査する (未知なら usage で落とす)。
fn parse_tile_key(spec: &str) -> anim::Key {
    let Some((time, rest)) = spec.split_once(':') else {
        eprintln!("tile の --key の書式は <time>:<crate>:<id>=<0.0-1.0>[,…]: {spec}");
        usage()
    };
    let Ok(time) = time.parse::<f32>() else {
        eprintln!("--key の時刻が数値でない: {spec}");
        usage()
    };
    let mut values = BTreeMap::new();
    for entry in rest.split(',') {
        let Some((krate, pair)) = entry.split_once(':') else {
            eprintln!("tile の --key は <crate>:<id>=<0.0-1.0>: {entry}");
            usage()
        };
        if Target::from_name(krate).is_none() {
            eprintln!("unknown crate: {krate}");
            usage()
        }
        let (id, value) = parse_set(pair);
        values.insert(format!("{krate}:{id}"), value);
    }
    anim::Key { time, values }
}

/// `#rrggbb` を読む。
fn parse_rgb(spec: &str) -> image::Rgba<u8> {
    let hex = spec.strip_prefix('#').unwrap_or(spec);
    let Ok(v) = u32::from_str_radix(hex, 16) else {
        eprintln!("色は #rrggbb 形式: {spec}");
        usage()
    };
    if hex.len() != 6 {
        eprintln!("色は #rrggbb 形式 (6 桁): {spec}");
        usage()
    }
    image::Rgba([(v >> 16) as u8, (v >> 8) as u8, v as u8, 0xff])
}

fn still(target: Target, out: &std::path::Path, scale: f32, set: &BTreeMap<String, f32>) {
    let (w, h) = target.window_size();
    let (w, h) = (
        (w as f32 * scale).round() as u32,
        (h as f32 * scale).round() as u32,
    );
    offscreen::with_renderer(w, h, |renderer| {
        let mut scene = target.scene(set);
        let img = renderer.frame_at(scene.as_mut(), scale);
        offscreen::save_png(&img, out);
    });
    println!("{}", out.display());
}
