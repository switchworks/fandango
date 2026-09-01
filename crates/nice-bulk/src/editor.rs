//! nice-bulk 用 egui エディタ実装。

use std::sync::Arc;

use nih_plug::prelude::{Editor, Param, ParamSetter};
use nih_plug_egui::{create_egui_editor, egui, EguiState};

use fandango_shared::ui::{
    apply_palette, bead_slider_row, formula, install_fonts, show_shell_with_background,
    transfer_plot, Axes, Background, Baseline, CurveSpec, Palette, Shell, XScale,
};

use crate::{dsp, NiceBulkParams};

pub const WINDOW_W: u32 = 800;
pub const WINDOW_H: u32 = 400;

/// crate 固有の背景画像。暗さは `Background` 側の定数だけで決める（焼き込まない）。
const BACKGROUND: &[u8] = include_bytes!("../../../assets/backgrounds/nice-bulk.jpg");

/// ウィンドウのタイトル帯に出す表記。
pub const PLUGIN_LABEL: &str = "fandango : nice bulk";

/// エディタが持ち越す状態。
///
/// `response` は係数だけを持つ可視化用の応答 (状態なし)。構築が 1 回で済むよう
/// ここに置く。`background` は Context 依存のテクスチャなので `build` で捨てる。
pub struct EditorState {
    background: Background,
    response: dsp::VisualResponse,
}

pub fn new_editor_state() -> EditorState {
    EditorState {
        background: Background::new(BACKGROUND),
        response: dsp::VisualResponse::new(48_000.0),
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
    params: &NiceBulkParams,
    state: &mut EditorState,
) {
    apply_palette(ctx, Palette::Crimson);
    let palette = Palette::Crimson.data();
    let EditorState {
        background,
        response,
    } = state;
    show_shell_with_background(
        ctx,
        Shell {
            wordmark: PLUGIN_LABEL,
            formula: formula::NICE_BULK,
            kind: "POWER LAW",
            bypass: Some((&params.bypass, setter)),
        },
        Some(background),
        |ui, rect| {
            // 写像は GUI で書き直さず DSP の関数をそのまま呼ぶ
            // (`plans/gui_overhaul_v01.md` §Step 5 受け入れ基準)。
            let bass = params.bass_amount.modulated_plain_value();
            let hi = params.hi_amount.modulated_plain_value();
            // 横軸は周波数。低域が左、高域が右で、bass / hi の担当が
            // そのまま位置に対応する。レベル依存 (パワーローの効き) は
            // 入力レベルを変えた 3 本の広がりとして示す。
            let response = &*response;
            let curve = move |level_db: f32| {
                let a = 10.0_f32.powf(level_db / 20.0);
                move |f: f32| response.gain_db(f, a, bass, hi)
            };
            let quiet = curve(-40.0);
            let mid = curve(-24.0);
            let loud = curve(-12.0);
            transfer_plot(
                ui,
                rect.shrink(6.0),
                Axes {
                    x: (20.0, 20_000.0),
                    // 上限は g = G0_MAX·G_MAX = 16 (+24 dB) から決まる。
                    y: (-3.0, 24.0),
                    x_scale: XScale::Logarithmic,
                },
                Baseline::Level(0.0),
                "20 Hz — 20 kHz",
                &[
                    CurveSpec {
                        label: "-40 dB IN",
                        color: palette.accent[9],
                        map: &quiet,
                    },
                    CurveSpec {
                        label: "-24 dB IN",
                        color: palette.accent[8],
                        map: &mid,
                    },
                    CurveSpec {
                        label: "-12 dB IN",
                        color: palette.neutral[10],
                        map: &loud,
                    },
                ],
            );
        },
        |ui| {
            ui.columns(2, |cols| {
                bead_slider_row(&mut cols[0], "bass", &params.bass_amount, setter);
                bead_slider_row(&mut cols[1], "hi", &params.hi_amount, setter);
            });
        },
    );
}

pub fn create(
    params: Arc<NiceBulkParams>,
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
