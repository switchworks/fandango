//! crush 用 egui エディタ実装。
//!
//! 主図は「連続波が段へ落ちる」図。
//! 時間軸を左→右に取り、固定の正弦 1 周期強を [`CrushChannel`] に通した結果を描く。
//! 描画に使う値は DSP 本体そのものなので、図と実装は構造的に乖離しない。
//!
//! **アナライザではない。** 入力信号には一切反応しない。入力は GUI 側で作る固定の
//! 正弦であり、形を決めるのはパラメータだけ。

use std::sync::Arc;

use nih_plug::prelude::{Editor, Param, ParamSetter};
use nih_plug_egui::egui::{self, Color32, Mesh, Pos2, Rect, Stroke};
use nih_plug_egui::{create_egui_editor, EguiState};

use fandango_shared::ui::{
    apply_palette, bead_slider_row, formula, install_fonts,
    show_shell_with_background, Background, Palette, Shell,
};

use crate::dsp::CrushChannel;
use crate::CrushParams;

pub const WINDOW_W: u32 = 800;
pub const WINDOW_H: u32 = 400;

/// crate 固有の背景画像。暗さは `Background` 側の定数だけで決める（焼き込まない）。
const BACKGROUND: &[u8] = include_bytes!("../../../assets/backgrounds/crush.jpg");
/// この画像は既存背景より平均輝度が低い（実測 3.1 / raindrop 6.3 / nimbus 27.2）ため
/// 暗幕をさらに薄くする。
const VEIL_SCALE: f32 = 0.15;

/// 1 周期あたりのサンプル数。`rate` 最大 32 で 1 周期 4 段になるよう選ぶ。
const PERIOD: usize = 128;
/// 描く周期数。段の繰り返しが読める最小限。
const CYCLES: f32 = 1.5;
/// 描画前に空回しする周期数。DC ブロッカーと S&H の位相を落ち着かせる。
const WARMUP_CYCLES: usize = 2;
/// 入力正弦の振幅。
const AMP: f32 = 0.9;
/// 縦軸の表示範囲（振幅）。量子化で段が外へ出るため入力振幅より広く取る。
const Y_SPAN: f32 = 1.15;
/// 主図の中心の縦位置（メイン領域高に対する比）。背景の階段が下端にあるので上へ寄せる。
const CENTER_V: f32 = 0.44;
/// 主図の高さ（メイン領域高に対する比）。
const PLOT_V: f32 = 0.78;

/// `bits` の視覚専用写像の傾き。実 bits をそのまま使うと 8 bit で 256 段になり
/// 図では段が見えない（＝パラメータを動かしても図が変わらない）ため圧縮する。
/// **単調**であることが要件で、段数そのものは主張しない（数値は表示しない）。
const VIS_BITS_SLOPE: f32 = 4.0 / 15.0;

/// 実 bits → 視覚用 bits。`b=1 → 1`、`b=16 → 5`。単調増加。
fn vis_bits(b: f32) -> f32 {
    1.0 + (b - 1.0) * VIS_BITS_SLOPE
}

/// エディタが持ち越す状態。Context 依存のテクスチャを抱えるので `build` で必ず捨てる。
pub struct EditorState {
    background: Background,
}

/// ウィンドウのタイトル帯に出す表記。
pub const PLUGIN_LABEL: &str = "fandango : crush";

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
    params: &CrushParams,
    state: &mut EditorState,
) {
    apply_palette(ctx, Palette::Orange);
    let palette = Palette::Orange.data();
    let EditorState { background } = state;
    show_shell_with_background(
        ctx,
        Shell {
            wordmark: PLUGIN_LABEL,
            formula: formula::CRUSH,
            kind: "ANTIDERIVATIVE",
            bypass: Some((&params.bypass, setter)),
        },
        Some(background),
        |ui, rect| {
            paint_steps(
                ui,
                rect.shrink(6.0),
                StepInput {
                    bits: params.bits.modulated_plain_value(),
                    rate: smoothed_rate(ui, crate::rate_from_octaves(
                        params.rate.modulated_plain_value(),
                    )),
                    mix: params.mix.modulated_plain_value(),
                    bypass: params.bypass.value(),
                    accent: palette.accent[8],
                    highlight: palette.accent[10],
                    guide: palette.neutral[5],
                },
            );
        },
        |ui| {
            ui.columns(2, |cols| {
                bead_slider_row(&mut cols[0], "bits", &params.bits, setter);
                bead_slider_row(&mut cols[1], "rate", &params.rate, setter);
            });
            ui.columns(2, |cols| {
                bead_slider_row(&mut cols[0], "mix", &params.mix, setter);
            });
        },
    );
}

pub fn create(
    params: Arc<CrushParams>,
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

/// 図に使う `rate` を時間方向に平滑化する。
///
/// `rate` をオートメーションすると段の位置が毎フレーム動き、図が細かく震えて読めない
/// （オーナー報告 2026-08-27）。**描画だけを均す。DSP には一切触れない。**
/// 実効レートは対数軸で効くので、平滑化も `log2` 領域で行う（線形で均すと
/// 低 `rate` 側だけ鈍る）。
///
/// 時定数はフレームレートに依らないよう `stable_dt` から係数を作る。
/// 初回フレームと非有限値では平滑化せず目標値をそのまま採る。
fn smoothed_rate(ui: &egui::Ui, target: f32) -> f32 {
    /// 図が追いつく時定数 [秒]。見た目だけの値で、音には影響しない。
    const TAU: f32 = 0.12;

    let id = egui::Id::new("crush_vis_rate_log2");
    let goal = target.max(1.0).log2();
    let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.1);
    let prev: Option<f32> = ui.data(|d| d.get_temp(id));
    let next = match prev {
        Some(p) if p.is_finite() && dt > 0.0 => {
            let k = 1.0 - (-dt / TAU).exp();
            p + (goal - p) * k
        }
        _ => goal,
    };
    ui.data_mut(|d| d.insert_temp(id, next));
    // 目標に十分近づいたら吸い付かせる。残差で段が半端な位置に留まらないように。
    if (next - goal).abs() < 1e-4 {
        goal.exp2()
    } else {
        // 追従中は次フレームを要求する。入力が止まった直後に再描画が来ないと
        // 図が途中の位置で固まるため。
        ui.ctx().request_repaint();
        next.exp2()
    }
}

/// 主図 1 枚分の入力。
struct StepInput {
    /// 実 bits（1.0..=16.0）。図には [`vis_bits`] を通した値を使う。
    bits: f32,
    /// S&H ダウンサンプル係数（1.0..=32.0、実数）。
    rate: f32,
    /// wet の混合比。
    mix: f32,
    bypass: bool,
    accent: Color32,
    highlight: Color32,
    guide: Color32,
}

fn paint_steps(ui: &egui::Ui, rect: Rect, input: StepInput) {
    let painter = ui.painter_at(rect);

    let plot_h = rect.height() * PLOT_V;
    let center_y = rect.top() + rect.height() * CENTER_V;
    let n = (PERIOD as f32 * CYCLES) as usize;

    let x_of = |i: usize| rect.left() + (i as f32 / (n - 1) as f32) * rect.width();
    let y_of = |v: f32| center_y - (v / Y_SPAN).clamp(-1.0, 1.0) * plot_h * 0.5;

    // 中心線。振幅 0 の位置を示すだけの説明線。
    painter.line_segment(
        [
            Pos2::new(rect.left(), center_y),
            Pos2::new(rect.right(), center_y),
        ],
        Stroke::new(1.0, weak(input.guide, 60)),
    );

    // 入力正弦（dry）。bypass 時はこれだけを描く。
    let sine = |i: usize| AMP * (i as f32 / PERIOD as f32 * std::f32::consts::TAU).sin();
    let dry: Vec<Pos2> = (0..n).map(|i| Pos2::new(x_of(i), y_of(sine(i)))).collect();

    if input.bypass {
        painter.add(egui::Shape::line(
            dry,
            Stroke::new(1.5, weak(input.guide, 140)),
        ));
        return;
    }

    // wet。DSP 本体をそのまま通す。GUI 側で量子化則を作らない。
    // bits だけは視覚専用の写像を挟む（実 bits では段が図に出ないため）。
    let b = vis_bits(input.bits);
    let mut ch = CrushChannel::new();
    for i in 0..(PERIOD * WARMUP_CYCLES) {
        ch.process(sine(i), b, input.rate, input.mix);
    }
    let wet: Vec<Pos2> = (0..n)
        .map(|i| {
            let y = ch.process(sine(i), b, input.rate, input.mix);
            Pos2::new(x_of(i), y_of(y))
        })
        .collect();

    // dry と wet の隔たりを面で塗る。mix=0 なら面積 0 = 何もしていないことが読める。
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

    // 細線は説明、太いシルエットは訴求。
    painter.add(egui::Shape::line(
        dry,
        Stroke::new(1.0, weak(input.guide, 110)),
    ));
    painter.add(egui::Shape::line(
        wet.clone(),
        Stroke::new(2.5, input.accent),
    ));

    // 段の踏み面の折れ点を粒で示す。rate を上げるほど疎になる。
    let step = input.rate.max(1.0).round() as usize;
    for i in (0..n).step_by(step.max(1)) {
        painter.circle_filled(wet[i], 2.0, input.highlight);
    }
}

/// 色のアルファだけを差し替える。
fn weak(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}
