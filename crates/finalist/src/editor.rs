//! finalist 用 egui エディタ実装。

use std::sync::Arc;

use nih_plug::prelude::{Editor, Param, ParamSetter};
use nih_plug_egui::{create_egui_editor, egui, EguiState};

use fandango_shared::ui::{
    apply_palette, bead_slider_row, formula, install_fonts, show_shell_with_background,
    transfer_plot, Axes, Background, Baseline, CurveSpec, Palette, Shell, XScale,
};

use crate::{dsp, FinalistParams};

pub const WINDOW_W: u32 = 800;
pub const WINDOW_H: u32 = 400;

/// crate 固有の背景画像。暗さは `Background` 側の定数だけで決める（焼き込まない）。
const BACKGROUND: &[u8] = include_bytes!("../../../assets/backgrounds/finalist.jpg");

/// 曲線の下端。これより下は表示しない（ゼロ入力の -inf dB を潰す）。
const FLOOR_DB: f32 = -48.0;

/// ウィンドウのタイトル帯に出す表記。
pub const PLUGIN_LABEL: &str = "fandango : finalist";

/// 持ち越す状態は背景テクスチャのみ。
pub type EditorState = Background;

pub fn new_editor_state() -> EditorState {
    Background::new(BACKGROUND)
}

/// ウィンドウを開くたびに **新しい Context** で呼ばれる。
/// 持ち越したテクスチャは古い Context のものなので必ず捨てる。
pub fn build(ctx: &egui::Context, background: &mut EditorState) {
    install_fonts(ctx);
    background.invalidate();
}

pub fn draw(
    ctx: &egui::Context,
    setter: &ParamSetter,
    params: &FinalistParams,
    background: &mut EditorState,
) {
    apply_palette(ctx, Palette::Amber);
    let palette = Palette::Amber.data();
    show_shell_with_background(
        ctx,
        Shell {
            wordmark: PLUGIN_LABEL,
            formula: formula::FINALIST,
            kind: "MAXIMIZER",
            bypass: Some((&params.bypass, setter)),
        },
        Some(background),
        |ui, rect| {
            // 整形関数は GUI で書き直さず DSP の `drive_shape` をそのまま呼ぶ
            // （曲線と実際の音が食い違わないことを実装で保証するため）。
            // 描くのは**静的**伝達特性。lookahead 窓・release は時間軸の量なので
            // ここには現れない（release ノブは曲線を動かさない）。
            let input_gain = 10.0_f32.powf(params.input_db.modulated_plain_value() / 20.0);
            let k = params.drive.modulated_plain_value();
            let ceiling_db = params.ceiling_db.modulated_plain_value();
            let ceiling = 10.0_f32.powf(ceiling_db / 20.0);

            // drive 段だけ（リミッタ前）。入力ゲイン込み。
            let shaped = move |x_db: f32| {
                let u = 10.0_f32.powf(x_db / 20.0) * input_gain;
                to_db(dsp::drive_shape(u, k))
            };
            // 天井まで含めた出力。静的信号ではゲイン経路は
            // g = ceiling / max(|f|, ceiling) に落ち着く。
            let limited = move |x_db: f32| {
                let u = 10.0_f32.powf(x_db / 20.0) * input_gain;
                to_db(dsp::drive_shape(u, k).abs().min(ceiling))
            };
            transfer_plot(
                ui,
                rect.shrink(6.0),
                Axes {
                    x: (FLOOR_DB, 12.0),
                    y: (FLOOR_DB, 12.0),
                    x_scale: XScale::Linear,
                },
                Baseline::Identity,
                "-48 dB — +12 dB IN",
                &[
                    CurveSpec {
                        label: "DRIVE",
                        color: palette.neutral[10],
                        map: &shaped,
                    },
                    CurveSpec {
                        label: "OUT",
                        color: palette.accent[9],
                        map: &limited,
                    },
                ],
            );
        },
        |ui| {
            ui.columns(2, |cols| {
                bead_slider_row(&mut cols[0], "input", &params.input_db, setter);
                bead_slider_row(&mut cols[0], "ceiling", &params.ceiling_db, setter);
                bead_slider_row(&mut cols[1], "drive", &params.drive, setter);
                bead_slider_row(&mut cols[1], "release", &params.release_ms, setter);
            });
        },
    );
}

pub fn create(
    params: Arc<FinalistParams>,
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

/// 線形振幅 → dB。0 近傍は表示下端で止める。
fn to_db(v: f32) -> f32 {
    let a = v.abs();
    if a <= 0.0 {
        FLOOR_DB
    } else {
        (20.0 * a.log10()).max(FLOOR_DB)
    }
}
