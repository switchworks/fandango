//! raindrop 用 egui エディタ実装。
//!
//! 主図はタップ列（`plans/gui_overhaul_v01.md` §Step 6 クラス2）。時間軸を左→右に取り、
//! 反射 1 個を同心円（水面の波紋）で描く。半径は振幅であり、振幅は DSP の定数
//! [`dsp::G_MAIN`] と feedback から出す。GUI 側で減衰則を作らない。
//!
//! **アナライザではない。** 入力信号には一切反応しない。描くのはパラメータだけで
//! 決まる静的なインパルス応答の図解。

use std::sync::Arc;

use nih_plug::prelude::{Editor, Param, ParamSetter};
use nih_plug_egui::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke};
use nih_plug_egui::{create_egui_editor, EguiState};

use fandango_shared::ui::{
    apply_palette, bead_slider_row, formula, install_fonts, show_shell_with_background, Background,
    Palette, Shell,
};

use crate::{dsp, RaindropParamsNih};

pub const WINDOW_W: u32 = 800;
pub const WINDOW_H: u32 = 400;

/// crate 固有の背景画像。暗さは `Background` 側の定数だけで決める（焼き込まない）。
const BACKGROUND: &[u8] = include_bytes!("../../../assets/backgrounds/raindrop.jpg");
/// この画像は他 crate より平均輝度が低い（実測 6.4 / 他は 18〜27）ため暗幕を薄くする。
const VEIL_SCALE: f32 = 0.25;

/// 時間軸の右端 (ms)。TIME を動かすとタップ間隔が変わるよう軸は固定する。
const AXIS_MS: f32 = 600.0;
/// 描くタップ数の上限。
const MAX_TAPS: usize = 24;
/// これより小さい振幅のタップは描かない。
const AMP_FLOOR: f32 = 0.012;
/// 振幅 1.0 に対応する波紋の外半径 (px)。
const RIPPLE_R: f32 = 34.0;
/// 波紋の輪の本数。
const RINGS: usize = 3;
/// レーン間隔に混ぜる実測比（`sqrt(alpha^2+h^2)`）の割合。残りは単調な直線。
const VIS_BLEND: f32 = 0.5;
/// 直線側の切片（alpha=0 でも Haas が広げているので 0 にしない）。
const VIS_BASE: f32 = 0.45;
/// 直線側の傾き。`VIS_BASE + VIS_TILT` が alpha=1 の値。
const VIS_TILT: f32 = 0.55;
/// Haas の +/- を振り分ける上下距離の下限 (px)。
const HAAS_MARK_GAP: f32 = 6.0;
/// Haas 分数遅延の時間 [ms]。dsp 側が ms で持つのでそのまま使う。
const HAAS_MS: f32 = dsp::HAAS_TAU_MS as f32;

/// エディタが持ち越す状態。Context 依存のテクスチャを抱えるので `build` で必ず捨てる。
pub struct EditorState {
    background: Background,
}

/// ウィンドウのタイトル帯に出す表記。
pub const PLUGIN_LABEL: &str = "fandango : raindrop";

pub fn new_editor_state() -> EditorState {
    EditorState {
        background: Background::new(BACKGROUND).with_veil_scale(VEIL_SCALE),
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
    params: &RaindropParamsNih,
    state: &mut EditorState,
) {
    apply_palette(ctx, Palette::Indigo);
    let palette = Palette::Indigo.data();
    let EditorState { background } = state;
    show_shell_with_background(
        ctx,
        Shell {
            wordmark: PLUGIN_LABEL,
            formula: formula::RAINDROP,
            kind: "HAAS",
            bypass: Some((&params.bypass, setter)),
        },
        Some(background),
        |ui, rect| {
            let width = params.width.modulated_plain_value();
            paint_taps(
                ui,
                rect.shrink(6.0),
                TapInput {
                    time_ms: params.time.modulated_plain_value(),
                    feedback: params.feedback.modulated_plain_value(),
                    width,
                    // Haas 量は DSP と同じ式。WIDTH に逆相関。
                    haas: (1.0 - width) * dsp::HAAS_MAX_AMOUNT,
                    mix: params.mix.modulated_plain_value(),
                    bypass: params.bypass.value(),
                    accent: palette.accent[8],
                    highlight: palette.accent[10],
                    guide: palette.neutral[5],
                    label: palette.neutral[9],
                },
            );
        },
        |ui| {
            ui.columns(2, |cols| {
                bead_slider_row(&mut cols[0], "time", &params.time, setter);
                bead_slider_row(&mut cols[1], "feedback", &params.feedback, setter);
            });
            ui.columns(2, |cols| {
                bead_slider_row(&mut cols[0], "width", &params.width, setter);
                bead_slider_row(&mut cols[1], "mix", &params.mix, setter);
            });
        },
    );
}

pub fn create(
    params: Arc<RaindropParamsNih>,
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

/// 主図 1 枚分の入力。
struct TapInput {
    /// 反射間隔 (ms)。
    time_ms: f32,
    /// 反射ごとの比。
    feedback: f32,
    /// WIDTH `alpha`。コム入力の側成分ゲイン = L/R 2 本のコムの独立度。
    width: f32,
    /// Haas-shuffle 量 `(1 - width) * HAAS_MAX_AMOUNT`。
    haas: f32,
    /// wet の加算量。dry は常に 1.0（`out = x + mix * wet`）。
    mix: f32,
    bypass: bool,
    accent: Color32,
    highlight: Color32,
    guide: Color32,
    label: Color32,
}

fn paint_taps(ui: &egui::Ui, rect: Rect, input: TapInput) {
    let painter = ui.painter_at(rect);

    let lane_gap = rect.height() * 0.18 * lane_spread(input.width);
    let lane_l = rect.center().y - lane_gap;
    let lane_r = rect.center().y + lane_gap;
    let x_of = |ms: f32| rect.left() + (ms / AXIS_MS).clamp(0.0, 1.0) * rect.width();

    // 100 ms ごとの目盛りと、L / R レーンの基準線。
    for step in 1..(AXIS_MS as usize / 100) {
        let x = x_of(step as f32 * 100.0);
        painter.line_segment(
            [
                Pos2::new(x, rect.top() + 8.0),
                Pos2::new(x, rect.bottom() - 8.0),
            ],
            Stroke::new(1.0, input.guide),
        );
    }
    for (side, name) in [(-1.0_f32, "L"), (1.0, "R")] {
        let y = rect.center().y + side * lane_gap;
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(1.0, input.guide),
        );
        painter.text(
            Pos2::new(rect.left() + 2.0, y - 4.0),
            Align2::LEFT_BOTTOM,
            name,
            FontId::monospace(10.0),
            input.label,
        );
    }

    let (accent, highlight) = if input.bypass {
        (input.guide, input.label)
    } else {
        (input.accent, input.highlight)
    };

    // dry。mix に依らず常に 1.0 で出る（`out = x + mix * wet`）ので大きさは変えない。
    for y in [lane_l, lane_r] {
        ripple(&painter, Pos2::new(x_of(0.0), y), RIPPLE_R, highlight);
    }
    painter.text(
        Pos2::new(x_of(0.0), rect.bottom()),
        Align2::LEFT_BOTTOM,
        "dry",
        FontId::monospace(10.0),
        input.label,
    );

    // 反射列。振幅は wet の第 n 反射 = G_MAIN * feedback^(n-1)、mix で加算量が決まる。
    let mut shown = 0usize;
    for n in 1..=MAX_TAPS {
        let t = input.time_ms * n as f32;
        if t > AXIS_MS {
            break;
        }
        let amp = input.mix * dsp::G_MAIN * input.feedback.powi(n as i32 - 1);
        if amp < AMP_FLOOR {
            break;
        }
        let x = x_of(t);
        for y in [lane_l, lane_r] {
            ripple(&painter, Pos2::new(x, y), RIPPLE_R * amp, accent);
        }
        // Haas-shuffle: mid 由来の側成分を +HAAS_MS だけ遅らせ、L に加算 / R に減算する。
        // 減算側は輪を描かず短い縦棒（極性反転）で示す。
        if input.haas > 0.0 {
            let hx = x_of(t + HAAS_MS);
            let hr = RIPPLE_R * amp * input.haas;
            // レーンが近い（S/M 最小付近）ときも +/- が潰れないよう最低限離す。
            let hy_gap = lane_gap.max(HAAS_MARK_GAP);
            let (hy_l, hy_r) = (rect.center().y - hy_gap, rect.center().y + hy_gap);
            ripple(&painter, Pos2::new(hx, hy_l), hr, highlight);
            painter.line_segment(
                [Pos2::new(hx, hy_r - hr), Pos2::new(hx, hy_r + hr)],
                Stroke::new(1.0, highlight),
            );
        }
        shown += 1;
    }

    painter.text(
        Pos2::new(rect.right(), rect.bottom()),
        Align2::RIGHT_BOTTOM,
        format!(
            "0 — {:.0} ms   {shown} taps   haas +{HAAS_MS:.1} ms x{:.2}   dual {:.2}",
            AXIS_MS, input.haas, input.width
        ),
        FontId::monospace(10.0),
        input.label,
    );
}

/// レーン間隔（視覚専用の写像。**測定量ではない**）。
///
/// 実際の wet の Side/Mid 振幅比は `sqrt(alpha^2 + h^2)`（`h = (1-alpha)*HAAS_MAX_AMOUNT`）で、
/// alpha=0 → 0.90、alpha=0.45 → 0.67（最小）、alpha=1 → 1.00 という**非単調**な形になる。
/// これは「Haas による広がり」から「L/R 独立コムによる広がり」への乗り換えの谷であって、
/// 幅そのものが失われるわけではない（谷は約 -2.5 dB）。谷をそのまま写すと「WIDTH を
/// 上げたのに狭くなる」と読まれるため、実測比と単調な直線を半々で混ぜ、谷を浅く残した
/// なだらかな曲線にする（0.675 → 0.654 @0.2 → 0.684 @0.45 → 0.79 @0.7 → 1.0）。
///
/// **代償**: 谷の深さが実際より浅い。比の数値は表示しない（表示すると間隔と食い違い、
/// 図が嘘になる）。真の量は footer の `haas` / `dual` で示す。
fn lane_spread(alpha: f32) -> f32 {
    let haas = (1.0 - alpha) * dsp::HAAS_MAX_AMOUNT;
    let sm = (alpha * alpha + haas * haas).sqrt();
    VIS_BLEND * sm + (1.0 - VIS_BLEND) * (VIS_BASE + VIS_TILT * alpha)
}

/// 波紋 1 個。外半径 `r` の同心円を内側ほど明るく描く。
fn ripple(painter: &egui::Painter, center: Pos2, r: f32, color: Color32) {
    if r <= 0.5 {
        return;
    }
    for i in 0..RINGS {
        let k = (i + 1) as f32 / RINGS as f32;
        let alpha = (255.0 * (1.0 - 0.55 * k)) as u8;
        painter.circle_stroke(
            center,
            r * k,
            Stroke::new(1.0, color.gamma_multiply(alpha as f32 / 255.0)),
        );
    }
    // 中心の落下点。
    painter.circle_filled(center, 1.5, color);
}
