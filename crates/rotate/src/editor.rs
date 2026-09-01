//! rotate 用 egui エディタ実装。
//!
//! 主図は「非対称波形のピークが落ちる」図（`plans/gui_overhaul_v01.md` §Step 6 クラス2）。
//! 時間軸を左→右に取り、固定の非対称波（基音＋第2倍音）を [`AllpassChain`] に
//! そのまま通した結果を描く。振幅特性は全周波数で厳密に 1 なので周波数応答は図に
//! ならない。図に出るのは位相回転の結果として起きる**波形の変形とピーク値の低下**、
//! つまり rotate を挿す理由そのもの。
//!
//! **アナライザではない。** 入力信号には一切反応しない。入力は GUI 側で作る固定波で、
//! 形を決めるのは freq / stages だけ。

use std::sync::Arc;

use nih_plug::prelude::{Editor, Param, ParamSetter};
use nih_plug_egui::egui::{self, Color32, Mesh, Pos2, Rect, Stroke};
use nih_plug_egui::{create_egui_editor, EguiState};

use fandango_shared::ui::{
    apply_palette, bead_slider_row, choice_row, formula, install_fonts, show_shell_with_background,
    Background, Palette, Shell,
};

use crate::dsp::AllpassChain;
use crate::{RotateParams, Stages};

pub const WINDOW_W: u32 = 800;
pub const WINDOW_H: u32 = 400;

/// crate 固有の背景画像。暗さは `Background` 側の定数だけで決める（焼き込まない）。
const BACKGROUND: &[u8] = include_bytes!("../../../assets/backgrounds/rotate.jpg");
/// 平均輝度は実測 10.4（crush 3.4 / raindrop 6.4 / nimbus 27.2）。線が細く高輝度なので
/// 既定の暗幕では潰れる一方、素通しでは主図と競合する。中間に置く。
const VEIL_SCALE: f32 = 0.45;

/// 図の中で使う仮想サンプルレート。実機の sr には依存させない（図は固定の題材）。
const VIS_SR: f32 = 48_000.0;
/// 題材の基音 (Hz)。`freq` の可動域 50–2000 全体で図が動くよう実測で選んだ
/// (基音 120 では freq 800 以上でピークが動かず、スライダー上半分が死ぬ)。
const F0: f32 = 450.0;
/// 1 周期あたりのサンプル数。
const PERIOD: usize = (VIS_SR / F0) as usize;
/// 描く周期数。
const CYCLES: usize = 3;
/// 空回しする周期数。オールパス縦列の過渡を落とす。
const WARMUP_CYCLES: usize = 8;
/// 第2倍音の振幅と位相。位相 π/2 が非対称性の源で、これを 0 にすると波が奇関数に
/// なりピークが対称化する (= 位相回転してもピークが下がらず図が成立しない)。
const H2: f32 = 0.6;
const H2_PHASE: f32 = std::f32::consts::FRAC_PI_2;
/// 第3倍音。片側の尖りを深くしてピーク低下量を読める大きさにする。
const H3: f32 = 0.35;
const H3_PHASE: f32 = std::f32::consts::PI;
/// 入力の全体振幅。正規化後にこれを掛ける。
const AMP: f32 = 0.9;
/// 縦軸の表示範囲（振幅）。
const Y_SPAN: f32 = 1.05;
/// 主図の中心の縦位置（メイン領域高に対する比）。
const CENTER_V: f32 = 0.5;
/// 主図の高さ（メイン領域高に対する比）。
const PLOT_V: f32 = 0.82;

/// 題材の非対称波（正規化前）。基音＋第2倍音。正のピークだけが尖る。
fn source(i: usize) -> f32 {
    let th = i as f32 / PERIOD as f32 * std::f32::consts::TAU;
    th.sin() + H2 * (2.0 * th + H2_PHASE).sin() + H3 * (3.0 * th + H3_PHASE).sin()
}

/// ウィンドウのタイトル帯に出す表記。
pub const PLUGIN_LABEL: &str = "fandango : rotate";

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
    params: &RotateParams,
    state: &mut EditorState,
) {
    apply_palette(ctx, Palette::Jade);
    let palette = Palette::Jade.data();
    let EditorState { background } = state;
    show_shell_with_background(
        ctx,
        Shell {
            wordmark: PLUGIN_LABEL,
            formula: formula::ROTATE,
            kind: "ALLPASS",
            bypass: Some((&params.bypass, setter)),
        },
        Some(background),
        |ui, rect| {
            paint_rotation(
                ui,
                rect.shrink(6.0),
                RotateInput {
                    freq: params.freq.modulated_plain_value(),
                    stages: params.stages.value().count(),
                    bypass: params.bypass.value(),
                    accent: palette.accent[8],
                    highlight: palette.accent[10],
                    guide: palette.neutral[5],
                },
            );
        },
        |ui| {
            ui.columns(2, |cols| {
                bead_slider_row(&mut cols[0], "freq", &params.freq, setter);
            });
            choice_row(
                ui,
                "stages",
                &params.stages,
                setter,
                &[
                    ("2", Stages::Two),
                    ("4", Stages::Four),
                    ("6", Stages::Six),
                    ("8", Stages::Eight),
                ],
            );
        },
    );
}

pub fn create(
    params: Arc<RotateParams>,
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

/// エディタが持ち越す状態。Context 依存のテクスチャを抱えるので `build` で必ず捨てる。
pub struct EditorState {
    background: Background,
}

/// 主図 1 枚分の入力。
struct RotateInput {
    freq: f32,
    stages: usize,
    bypass: bool,
    accent: Color32,
    highlight: Color32,
    guide: Color32,
}

fn paint_rotation(ui: &egui::Ui, rect: Rect, input: RotateInput) {
    let painter = ui.painter_at(rect);

    let plot_h = rect.height() * PLOT_V;
    let center_y = rect.top() + rect.height() * CENTER_V;
    let n = PERIOD * CYCLES;

    // 正規化。dry のピークが常に AMP になるので、図の縦位置は freq/stages だけで動く。
    let src_peak = (0..PERIOD).map(|i| source(i).abs()).fold(0.0f32, f32::max);
    let x_in = |i: usize| AMP * source(i) / src_peak;

    let x_of = |i: usize| rect.left() + (i as f32 / (n - 1) as f32) * rect.width();
    let y_of = |v: f32| center_y - (v / Y_SPAN).clamp(-1.0, 1.0) * plot_h * 0.5;

    painter.line_segment(
        [
            Pos2::new(rect.left(), center_y),
            Pos2::new(rect.right(), center_y),
        ],
        Stroke::new(1.0, weak(input.guide, 60)),
    );

    let dry: Vec<Pos2> = (0..n).map(|i| Pos2::new(x_of(i), y_of(x_in(i)))).collect();
    let dry_peak = (0..n).map(|i| x_in(i).abs()).fold(0.0f32, f32::max);

    if input.bypass {
        painter.add(egui::Shape::line(
            dry,
            Stroke::new(1.5, weak(input.guide, 140)),
        ));
        peak_line(&painter, rect, y_of(dry_peak), weak(input.guide, 90));
        return;
    }

    // wet。DSP 本体をそのまま通す。GUI 側で位相回転則を再実装しない。
    let mut chain = AllpassChain::new();
    for i in 0..(PERIOD * WARMUP_CYCLES) {
        chain.process(x_in(i % PERIOD), input.freq, VIS_SR, input.stages);
    }
    let wet_v: Vec<f32> = (0..n)
        .map(|i| chain.process(x_in(i), input.freq, VIS_SR, input.stages))
        .collect();
    let wet: Vec<Pos2> = (0..n).map(|i| Pos2::new(x_of(i), y_of(wet_v[i]))).collect();
    let wet_peak = wet_v.iter().fold(0.0f32, |m, v| m.max(v.abs()));

    // dry と wet の隔たりを面で塗る。位相が回っていなければ面積 0。
    let mut mesh = Mesh::default();
    let fill = weak(input.accent, 70);
    for i in 0..n {
        mesh.colored_vertex(dry[i], fill);
        mesh.colored_vertex(wet[i], fill);
    }
    for i in 0..(n - 1) {
        let a = (i * 2) as u32;
        mesh.add_triangle(a, a + 1, a + 2);
        mesh.add_triangle(a + 1, a + 2, a + 3);
    }
    painter.add(egui::Shape::mesh(mesh));

    painter.add(egui::Shape::line(
        dry,
        Stroke::new(1.0, weak(input.guide, 110)),
    ));
    painter.add(egui::Shape::line(wet, Stroke::new(2.5, input.accent)));

    // ピークの水平線 2 本。上が dry、下が wet。落差が rotate の効き。
    peak_line(&painter, rect, y_of(dry_peak), weak(input.guide, 90));
    peak_line(&painter, rect, y_of(wet_peak), weak(input.highlight, 170));
}

/// ピーク値を示す水平の破線。数値は出さない（図は測定値を主張しない）。
fn peak_line(painter: &egui::Painter, rect: Rect, y: f32, color: Color32) {
    let dash = 6.0;
    let gap = 5.0;
    let mut x = rect.left();
    while x < rect.right() {
        let x2 = (x + dash).min(rect.right());
        painter.line_segment([Pos2::new(x, y), Pos2::new(x2, y)], Stroke::new(1.0, color));
        x = x2 + gap;
    }
}

/// 色のアルファだけを差し替える。
fn weak(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}
