//! trip-delay 用 egui エディタ実装。

use std::sync::Arc;

use nih_plug::prelude::{Editor, Param, ParamSetter};
use nih_plug_egui::egui::{self, Color32, Pos2, Rect, Stroke};
use nih_plug_egui::{create_egui_editor, EguiState};

use fandango_shared::ui::{
    apply_palette, bead_slider_row, formula, install_fonts, show_shell_with_background, Background,
    Palette, Shell,
};

use crate::dsp::level_ui_to_amp;
use crate::TripDelayParams;

/// crate 固有の背景画像。暗さは `Background` 側の定数だけで決める（焼き込まない）。
const BACKGROUND: &[u8] = include_bytes!("../../../assets/backgrounds/trip-delay.jpg");

pub const WINDOW_W: u32 = 800;
pub const WINDOW_H: u32 = 400;

/// 時間軸の右端 (拍)。テンポは GUI から見えないが、タップ位置は全て拍の分数なので
/// 拍で目盛れば図はテンポに依らず実機と一致する。
const AXIS_BEATS: f32 = 3.0;

/// 縦軸の下限 (dB)。線形のままだと level 下端 (-24 dB) が dry の 6% となり
/// 図に出ないため、対数で取る。帰還の減衰が等間隔の階段になるのもこの軸の効果。
const FLOOR_DB: f32 = 48.0;

/// 描く帰還段数の上限。振幅が床に達するか軸の右端を越えれば早く止まる。
const MAX_ECHO: usize = 32;

/// エディタが持ち越す状態。Context 依存のテクスチャを抱えるので `build` で必ず捨てる。
pub struct EditorState {
    background: Background,
}

/// 図に渡す値。
struct TapInput {
    level: f32,
    feedback: f32,
    bypass: bool,
}

/// ウィンドウのタイトル帯に出す表記。
pub const PLUGIN_LABEL: &str = "fandango : trip delay";

pub fn new_editor_state() -> EditorState {
    EditorState {
        // 元画像が既存で最も明るい (実測 lum 88 / chroma 26)。定数どおりの幕だと
        // メイン中央の実効透過が 0.44 となり、明度と同率で chroma も半減して
        // 色が抜ける。幕を薄くして色彩を残す (画像側は触らない)。
        background: Background::new(BACKGROUND).with_veil_scale(0.55),
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
    params: &TripDelayParams,
    state: &mut EditorState,
) {
    apply_palette(ctx, Palette::Plum);
    let EditorState { background } = state;
    show_shell_with_background(
        ctx,
        Shell {
            wordmark: PLUGIN_LABEL,
            formula: formula::TRIP_DELAY,
            kind: "POLYRHYTHM DELAY",
            bypass: Some((&params.bypass, setter)),
        },
        Some(background),
        |ui, rect| {
            paint_taps(
                ui,
                rect.shrink(6.0),
                TapInput {
                    level: params.level.modulated_plain_value(),
                    feedback: params.feedback.modulated_plain_value(),
                    bypass: params.bypass.value(),
                },
            );
        },
        |ui| {
            ui.columns(2, |cols| {
                bead_slider_row(&mut cols[0], "level", &params.level, setter);
                bead_slider_row(&mut cols[1], "feedback", &params.feedback, setter);
            });
        },
    );
}

pub fn create(
    params: Arc<TripDelayParams>,
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

/// L / R のエコー列を上下 2 レーンに描く。
///
/// 位置は `tau_L,n = (2+3n)/8 T` / `tau_R,n = 3(n+1)/8 T`、高さは `a b^n` を
/// dB で取ったもの。帰還が両チャンネルとも 3/8 拍周期という DSP の構造から
/// 導いた閉形式で、実機のタップ列と一致する。
fn paint_taps(ui: &egui::Ui, rect: Rect, input: TapInput) {
    let painter = ui.painter_at(rect);
    let palette = Palette::Plum.data();
    let grid = palette.neutral[5];
    // 文字は罫線と同じ色だと背景に沈む。罫線は暗いまま、文字だけ明るい段へ上げる。
    let label = palette.neutral[11];
    let dry_col = palette.neutral[8];
    let accent = palette.accent[10];

    let lane_h = rect.height() * 0.30;
    let lane_l = rect.center().y - rect.height() * 0.06;
    let lane_r = rect.center().y + rect.height() * 0.06;
    let x_of = |beats: f32| rect.left() + (beats / AXIS_BEATS) * rect.width();
    // 振幅 → 高さ。0 dB で満杯、`FLOOR_DB` 下で 0。
    let h_of = |amp: f32| -> f32 {
        if amp <= 0.0 {
            return 0.0;
        }
        let db = 20.0 * amp.log10();
        ((1.0 + db / FLOOR_DB).clamp(0.0, 1.0)) * lane_h
    };

    // 1/8 拍の目盛りと拍線。
    let faint = Color32::from_rgba_unmultiplied(grid.r(), grid.g(), grid.b(), 50);
    let mut e = 1usize;
    while (e as f32) / 8.0 < AXIS_BEATS {
        let x = x_of(e as f32 / 8.0);
        let on_beat = e % 8 == 0;
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1.0, if on_beat { grid } else { faint }),
        );
        e += 1;
    }
    for y in [lane_l, lane_r] {
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(1.0, grid),
        );
    }

    // dry は常に素通り (DSP は dry passthrough + wet 加算)。t=0 に満杯の線。
    for (y, dir) in [(lane_l, -1.0), (lane_r, 1.0)] {
        let x = x_of(0.0) + 1.0;
        painter.line_segment(
            [Pos2::new(x, y), Pos2::new(x, y + dir * lane_h)],
            Stroke::new(2.0, dry_col),
        );
    }

    if !input.bypass {
        let a = level_ui_to_amp(input.level);
        let b = input.feedback;
        for (y, dir, first_eighth) in [(lane_l, -1.0f32, 2.0f32), (lane_r, 1.0, 3.0)] {
            let mut tops: Vec<Pos2> = Vec::new();
            for n in 0..MAX_ECHO {
                let beats = (first_eighth + 3.0 * n as f32) / 8.0;
                if beats > AXIS_BEATS {
                    break;
                }
                let amp = a * b.powi(n as i32);
                let h = h_of(amp);
                if h <= 0.0 {
                    break;
                }
                let x = x_of(beats);
                let top = Pos2::new(x, y + dir * h);
                painter.line_segment([Pos2::new(x, y), top], Stroke::new(2.0, accent));
                tops.push(top);
            }
            // 減衰包絡。dB 軸なので b が一定なら直線になる。
            if tops.len() >= 2 {
                let env = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 110);
                painter.add(egui::Shape::line(tops, Stroke::new(1.0, env)));
            }
        }
    }

    painter.text(
        Pos2::new(rect.left() + 2.0, lane_l - lane_h),
        egui::Align2::LEFT_TOP,
        "L  2/8",
        egui::FontId::proportional(11.0),
        label,
    );
    painter.text(
        Pos2::new(rect.left() + 2.0, lane_r + lane_h),
        egui::Align2::LEFT_BOTTOM,
        "R  3/8",
        egui::FontId::proportional(11.0),
        label,
    );
    painter.text(
        Pos2::new(rect.right(), rect.bottom()),
        egui::Align2::RIGHT_BOTTOM,
        "0 — 3 beats",
        egui::FontId::proportional(11.0),
        label,
    );
}
