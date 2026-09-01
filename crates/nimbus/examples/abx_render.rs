//! ABX 用オフラインレンダ（`plans/parked-decisions.md`「nimbus — 「音が良い」の帰属先」）。
//!
//! 同一素材を 2 系統に通して raw f32 で書き出す。モードで組を選ぶ:
//!   `geom` — 現行 nimbus vs 対照系（`ErFir::control_shuffled`。
//!            幾何だけを潰し総エネルギーは一致）。幾何が可聴かを測る
//!   `dry`  — dry vs 現行 nimbus。装置そのものが可聴かと、選好を測る
//!
//! センド構成を再現する。dry + `SEND_DB` で送った wet を戻して加算する。
//! nimbus 側は `mix = 1.0`（センド用の発注仕様）で回す。
//!
//! 入出力は raw f32 LE インターリーブ 2ch（WAV 入出力とレベル合わせは
//! `tools/nimbus_abx.py` が持つ。ここは DSP だけを担当する）。
//!
//! 実行: cargo run -p fandango-nimbus --example abx_render --release --
//!         <in.raw> <a.raw> <b.raw> <geom|dry> <send_db>

use fandango_nimbus::dsp::{hf_shelf_coeffs, hpf_coeffs, ErFir};
use std::io::{Read, Write};

const SR: f32 = 48_000.0;
const DISTANCE: f32 = 0.5;
const MIX: f32 = 1.0;
const CONTROL_SEED: u64 = 0x5DEE_CE66_D3A9_1B47;

fn render(input: &[(f32, f32)], mut er: ErFir, send_db: f32) -> Vec<(f32, f32)> {
    let shelf = hf_shelf_coeffs(SR, DISTANCE);
    let hpf = hpf_coeffs(SR);
    let send = 10.0_f32.powf(send_db / 20.0);
    input
        .iter()
        .map(|&(l, r)| {
            let (wl, wr) = er.process(l * send, r * send, DISTANCE, MIX, &shelf, &hpf);
            (l + wl, r + wr)
        })
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(
        args.len(),
        6,
        "usage: abx_render <in.raw> <out_a.raw> <out_b.raw> <geom|dry> <send_db>"
    );
    let mode = args[4].as_str();
    let send_db: f32 = args[5].parse().expect("send_db が数値でない");

    let mut bytes = Vec::new();
    std::fs::File::open(&args[1])
        .expect("入力を開けない")
        .read_to_end(&mut bytes)
        .expect("入力を読めない");
    assert_eq!(bytes.len() % 8, 0, "raw f32 ステレオではない");
    let input: Vec<(f32, f32)> = bytes
        .chunks_exact(8)
        .map(|c| {
            (
                f32::from_le_bytes(c[0..4].try_into().unwrap()),
                f32::from_le_bytes(c[4..8].try_into().unwrap()),
            )
        })
        .collect();

    // `geom`: 現行 nimbus vs 幾何を潰した対照（幾何の可聴性を測る）
    // `dry`:  dry vs 現行 nimbus（装置そのものの可聴性と選好を測る）
    let outputs: [Vec<(f32, f32)>; 2] = match mode {
        "geom" => [
            render(&input, ErFir::new(SR), send_db),
            render(&input, ErFir::control_shuffled(SR, CONTROL_SEED), send_db),
        ],
        "dry" => [input.clone(), render(&input, ErFir::new(SR), send_db)],
        other => panic!("未知のモード: {other}"),
    };

    for (path, out) in [&args[2], &args[3]].into_iter().zip(outputs) {
        let mut buf = Vec::with_capacity(out.len() * 8);
        for (l, r) in out {
            buf.extend_from_slice(&l.to_le_bytes());
            buf.extend_from_slice(&r.to_le_bytes());
        }
        std::fs::File::create(path)
            .expect("出力を作れない")
            .write_all(&buf)
            .expect("出力を書けない");
    }
}
