//! drawl 用 egui エディタ実装。
//!
//! 主図は「声道の周波数応答」。`dsp::chain_response` が返すアナログ原型
//! `H(jw)` をそのまま描くので、図と実装は構造的に乖離しない。
//! 縦軸に掛かる正規化スカラも `dsp::never_boost_scale` 本体を呼ぶため、
//! 「どの母音でも 0 dB を超えない」という設計がそのまま図に出る。
//!
//! **アナライザではない。** 入力信号には一切反応しない。曲線を決めるのは
//! パラメータだけ。

use std::sync::Arc;

use nih_plug::prelude::{Editor, Param, ParamSetter};
use nih_plug_egui::{create_egui_editor, egui, EguiState};

use fandango_shared::ui::{
    apply_palette, bead_slider_row, formula, install_fonts, show_shell_with_background,
    transfer_plot, Axes, Background, Baseline, CurveSpec, Palette, Shell, XScale,
};

use crate::dsp;
use crate::DrawlParams;

pub const WINDOW_W: u32 = 800;
pub const WINDOW_H: u32 = 400;

/// crate 固有の背景画像。暗さは `Background` 側の定数だけで決める（焼き込まない）。
const BACKGROUND: &[u8] = include_bytes!("../../../assets/backgrounds/drawl.jpg");

/// 周波数軸の両端 (Hz)。
const F_MIN: f32 = 20.0;
const F_MAX: f32 = 20_000.0;

/// 縦軸 (dB)。Q 上端 8 での共振ピーク実測 ≈ +27 dB を収める。
const DB_MIN: f32 = -36.0;
const DB_MAX: f32 = 30.0;

/// エディタが持ち越す状態。Context 依存のテクスチャを抱えるので `build` で必ず捨てる。
pub struct EditorState {
    background: Background,
}

/// パラメータ 1 組から応答曲線を作る。
/// 帯域幅倍率 `b_mul` を差し替えればジッタ端の曲線になる。
fn response_at(f1: f32, f2: f32, b_mul: f32, mix: f32) -> impl Fn(f32) -> f32 {
    let hp = f1 / dsp::HP_DIVISOR;
    let (q1, q2) = dsp::q_pair(f1, f2, b_mul);
    let scale = dsp::never_boost_scale(f1, f2, q1, q2, hp);
    move |f_hz: f32| {
        let (re, im) = dsp::chain_response(f_hz, f1, f2, q1, q2, hp);
        // dry/wet は時間領域の和なので図でも複素和で合成する。
        // 振幅だけを補間すると位相打ち消しが消え、mix 中間で嘘になる。
        let wr = 1.0 - mix + mix * scale * re;
        let wi = mix * scale * im;
        20.0 * (wr * wr + wi * wi).sqrt().max(1e-6).log10()
    }
}

/// ウィンドウのタイトル帯に出す表記。
pub const PLUGIN_LABEL: &str = "fandango : drawl";

pub fn new_editor_state() -> EditorState {
    EditorState {
        background: Background::new(BACKGROUND),
    }
}

/// ウィンドウを開くたびに **新しい Context** で呼ばれる。
/// 持ち越したテクスチャは古い Context のものなので必ず捨てる。
pub fn build(ctx: &egui::Context, state: &mut EditorState) {
    install_fonts(ctx);
    state.background.invalidate();
}

pub fn draw(
    ctx: &egui::Context,
    setter: &ParamSetter,
    params: &DrawlParams,
    state: &mut EditorState,
) {
    apply_palette(ctx, Palette::Teal);
    let palette = Palette::Teal.data();
    let EditorState { background } = state;
    show_shell_with_background(
        ctx,
        Shell {
            wordmark: PLUGIN_LABEL,
            formula: formula::DRAWL,
            kind: "FORMANT",
            bypass: Some((&params.bypass, setter)),
        },
        Some(background),
        |ui, rect| {
            let bypass = params.bypass.value();
            let (f1, f2) = dsp::vowel_formants(params.vowel.modulated_plain_value());
            let jitter = params.jitter.modulated_plain_value();
            // bypass 中は wet 経路が通らない。図も dry（平坦）に落とす。
            let mix = if bypass {
                0.0
            } else {
                params.mix.modulated_plain_value()
            };

            let main = response_at(f1, f2, 1.0, mix);
            // `b_mul` の下端 = B が狭い = Q の上端。ラベルは Q 基準なので入れ替える。
            let (b_lo, b_hi) = dsp::b_mul_range(jitter);
            let lo = response_at(f1, f2, b_hi, mix);
            let hi = response_at(f1, f2, b_lo, mix);

            let accent = if bypass {
                palette.neutral[8]
            } else {
                palette.accent[9]
            };
            let faint = palette.neutral[6];

            let mut curves = Vec::with_capacity(3);
            // jitter = 0 では 3 本が重なり、面の重ね塗りで濃さだけが変わる。
            // 見た目が jitter に依らず一定になるよう端の 2 本を落とす。
            if jitter > 0.0 {
                curves.push(CurveSpec {
                    label: "Q LO",
                    color: faint,
                    map: &lo,
                });
                curves.push(CurveSpec {
                    label: "Q HI",
                    color: faint,
                    map: &hi,
                });
            }
            curves.push(CurveSpec {
                label: "F1 · F2",
                color: accent,
                map: &main,
            });

            transfer_plot(
                ui,
                rect.shrink(6.0),
                Axes {
                    x: (F_MIN, F_MAX),
                    y: (DB_MIN, DB_MAX),
                    x_scale: XScale::Logarithmic,
                },
                Baseline::Level(0.0),
                "20 Hz — 20 kHz",
                &curves,
            );
        },
        |ui| {
            ui.columns(2, |cols| {
                bead_slider_row(&mut cols[0], "vowel", &params.vowel, setter);
                bead_slider_row(&mut cols[1], "jitter", &params.jitter, setter);
            });
            ui.columns(2, |cols| {
                bead_slider_row(&mut cols[0], "mix", &params.mix, setter);
            });
        },
    );
}

pub fn create(
    params: Arc<DrawlParams>,
    editor_state: Arc<EguiState>,
    _build_timestamp: &'static str,
) -> Option<Box<dyn Editor>> {
    create_egui_editor(
        editor_state,
        new_editor_state(),
        |ctx, state: &mut EditorState| build(ctx, state),
        move |ctx, setter, state| draw(ctx, setter, &params, state),
    )
}
