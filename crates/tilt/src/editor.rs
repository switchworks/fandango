//! tilt 用 egui エディタ実装。

use std::sync::Arc;

use nih_plug::prelude::{Editor, Param, ParamSetter};
use nih_plug_egui::{create_egui_editor, egui, EguiState};

use fandango_shared::ui::{
    apply_palette, bead_slider_row, formula, install_fonts, show_shell_with_background,
    transfer_plot, Axes, Background, Baseline, CurveSpec, Palette, Shell, XScale,
};

use crate::dsp;
use crate::TiltParams;

pub const WINDOW_W: u32 = 800;
pub const WINDOW_H: u32 = 400;

/// crate 固有の背景画像。暗さは `Background` 側の定数だけで決める（焼き込まない）。
const BACKGROUND: &[u8] = include_bytes!("../../../assets/backgrounds/tilt.jpg");

/// 図で使うサンプルレート。係数は `fs` に依存するので基準値を置く
/// （実行時 `sr` は GUI スレッドから見えない）。
const SR_REF: f32 = 48_000.0;

/// 周波数軸の両端 (Hz)。
const F_MIN: f32 = 20.0;
const F_MAX: f32 = 20_000.0;

/// 縦軸 (dB)。gain 上端 ±12 dB がシェルフ棚で頭打ちになるので余白 2 dB を足す。
const DB_MIN: f32 = -14.0;
const DB_MAX: f32 = 14.0;

/// ウィンドウのタイトル帯に出す表記。
pub const PLUGIN_LABEL: &str = "fandango : tilt";

/// エディタが持ち越す状態。Context 依存のテクスチャを抱えるので `build` で必ず捨てる。
pub struct EditorState {
    background: Background,
}

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

/// パラメータ 1 組から応答曲線を作る。低シェルフ (-gain) と高シェルフ (+gain) の
/// 縦列なので、振幅は積 = dB では和。
fn response_at(gain: f32, pivot: f32, slope: f32) -> impl Fn(f32) -> f32 {
    let ls = dsp::low_shelf_coeffs(SR_REF, pivot, -gain, slope);
    let hs = dsp::high_shelf_coeffs(SR_REF, pivot, gain, slope);
    move |f_hz: f32| {
        let amp = dsp::biquad_amp(&ls, f_hz, SR_REF) * dsp::biquad_amp(&hs, f_hz, SR_REF);
        20.0 * amp.max(1e-6).log10()
    }
}

pub fn draw(
    ctx: &egui::Context,
    setter: &ParamSetter,
    params: &TiltParams,
    state: &mut EditorState,
) {
    apply_palette(ctx, Palette::Amber);
    let palette = Palette::Amber.data();
    let EditorState { background } = state;
    show_shell_with_background(
        ctx,
        Shell {
            wordmark: PLUGIN_LABEL,
            formula: formula::TILT,
            kind: "SHELF",
            bypass: Some((&params.bypass, setter)),
        },
        Some(background),
        |ui, rect| {
            let bypass = params.bypass.value();
            // bypass 中は素通り。図も平坦 (gain = 0) に落とす。
            let gain = if bypass {
                0.0
            } else {
                params.gain.modulated_plain_value()
            };
            let pivot = params.pivot.modulated_plain_value();
            let slope = params.slope.modulated_plain_value();

            let main = response_at(gain, pivot, slope);
            let accent = if bypass {
                palette.neutral[8]
            } else {
                palette.accent[9]
            };

            let curves = [CurveSpec {
                label: "TILT",
                color: accent,
                map: &main,
            }];

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
                bead_slider_row(&mut cols[0], "gain", &params.gain, setter);
                bead_slider_row(&mut cols[1], "pivot", &params.pivot, setter);
            });
            ui.columns(2, |cols| {
                bead_slider_row(&mut cols[0], "slope", &params.slope, setter);
            });
        },
    );
}

pub fn create(
    params: Arc<TiltParams>,
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
