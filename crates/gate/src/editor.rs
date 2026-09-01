//! gate 用 egui エディタ実装。
//!
//! 主図は「パターン文字列」「セル」「そのパターンから出る実ゲイン曲線」の 3 段。
//! 曲線は [`crate::dsp::gate_curve`] をそのまま呼んで描くので、図と DSP は
//! 構造的に乖離しない（GUI 側にゲート式を持たない）。
//!
//! パターンはホストのパラメータではなく文字列 1 本（`crate::pattern`）。
//! 入力欄がその文字列そのものなので、コピー・ペースト・共有が字面で完結する。
//!
//! **アナライザではない。** 入力信号にも再生位置にも反応しない。描いているのは
//! パラメータだけから決まる 1 周分の `gain(phi)`。拍線も `step_length` だけで
//! 決まるので、**transport を一切見ない**（`plans/gate_sync_and_bars_v01.md` §4）。

use std::sync::Arc;

use nih_plug::prelude::{Editor, Param, ParamSetter};
use nih_plug_egui::egui::{self, Align2, Color32, FontId, Mesh, Pos2, Rect, Sense, Stroke};
use nih_plug_egui::{create_egui_editor, EguiState};

use fandango_shared::ui::{
    apply_palette, bead_slider_row, choice_row, formula, install_fonts, show_shell_with_background,
    Background, Palette, Shell,
};

use crate::dsp::{gate_curve, StepCell};
use crate::pattern::{self, PatternCode, MAX_STEPS};
use crate::{GateParams, StepLengthEnum, SyncModeEnum};

pub const WINDOW_W: u32 = 800;
pub const WINDOW_H: u32 = 400;

const PALETTE: Palette = Palette::Tomato;

/// crate 固有の背景画像。暗さは `Background` 側の定数だけで決める（焼き込まない）。
const BACKGROUND: &[u8] = include_bytes!("../../../assets/backgrounds/gate.jpg");
/// 暗幕は定数どおり。この画像は平均輝度が高い（実測 86.8）が、それは欠点ではなく
/// 背景がよく見えるというだけなので厚くしない。
/// 実効輝度は主図中央 105*0.44≈46、ヘッダ・フッタ帯 71*0.275≈20（パネル地は 17）。
const VEIL_SCALE: f32 = 1.0;

/// 主図の帯配分（メイン領域高に対する比）。上から 文字列欄 / 曲線 / セル。
const TEXT_V: f32 = 0.14;
const CURVE_V: f32 = 0.52;
/// セル間の隙間（px）。
const CELL_GAP: f32 = 2.0;
/// 曲線 1 ステップあたりのサンプル数。エッジが最小幅でも折れずに読める密度。
const SAMPLES_PER_STEP: usize = 48;
/// 文字列欄の右に空ける長さ表示の幅（px）。`00/00` が収まる。
const COUNTER_W: f32 = 44.0;
/// 文字列欄の右に空ける copy / paste ボタンの幅（px）。
const CLIP_W: f32 = 104.0;
/// セル列の右に空ける `-` / `+` ボタンの幅（px）。
const STEPS_BTN_W: f32 = 26.0;
/// セル列とボタン列の間隔（px）。
const STEPS_BTN_GAP: f32 = 4.0;
/// 拍マークの高さ（曲線図の高さに対する比）。底辺から上へこの分だけ立てる。
/// 上端は透明に溶けるので、高くしても曲線を邪魔しない。
const BEAT_MARK_H_V: f32 = 0.5;
/// 拍マークの底の幅（セル何個分か）。上へ行くほど [`BEAT_MARK_CURVE`] の形で細る。
/// セル幅基準なので、セル数を変えても拍との対応が崩れない。
const BEAT_MARK_W_CELLS: f32 = 1.0;
/// 拍マーク輪郭の指数。輪郭は媒介変数 `u` で `w = W(1-u)^p`, `y = H·u^p`。
///
/// 幅と高さに**同じ指数**を掛けるので、両端で辺に対等に接し、縦横を入れ替えても
/// 重なる形になる。`p = 2` はストリングアート（正方形の 2 辺を等分して結んだ
/// 線族）の包絡線そのもので、`√x + √y = 1` を満たす放物線。
/// 上げるほど角へ寄る（超楕円の族）。
const BEAT_MARK_CURVE: f32 = 2.0;
/// 拍マークの輪郭を折れ線で近似する段数。
const BEAT_MARK_SEGMENTS: usize = 16;
/// 拍マーク根元の加算量（元色に対する倍率）。上端へ向かって 0 まで落ちる。
const BEAT_MARK_GAIN: f32 = 0.55;
/// 加算量の落ち方。`gain * (1 - t)^FADE`。大きいほど根元だけが光る。
const BEAT_MARK_FADE: f32 = 1.8;

/// エディタが持ち越す状態。
///
/// `text` は**編集中の下書き**であり、パターンの正本ではない。正本は
/// [`PatternCode`]（`#[persist]` の対象）。下書きは解釈できた時だけ正本へ反映する。
pub struct EditorState {
    text: String,
    /// 直近の paste で起きた失敗の表示。成功したら消す（黙って握り潰さないため）。
    /// **英字のみ**。`install_fonts` の既定フォントに日本語グリフが無い（□になる）。
    notice: Option<String>,
    background: Background,
}

/// ウィンドウのタイトル帯に出す表記。
pub const PLUGIN_LABEL: &str = "fandango : gate";

pub fn new_editor_state() -> EditorState {
    EditorState {
        text: String::new(),
        notice: None,
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
    params: &GateParams,
    state: &mut EditorState,
) {
    apply_palette(ctx, PALETTE);
    let palette = PALETTE.data();
    // 背景と下書きを別々に借りる（同じ `state` を二重に借りないため）。
    let EditorState {
        text,
        notice,
        background,
    } = state;
    // 1 拍あたりのステップ数。`step_length` だけで決まるので transport は要らない。
    // 1/4 なら 1.0（＝毎セルが拍頭）、1/16 なら 4.0、1/8T なら 3.0。
    let steps_per_beat = 1.0 / params.step_length.value().to_dsp().beats_per_step();
    show_shell_with_background(
        ctx,
        Shell {
            wordmark: PLUGIN_LABEL,
            formula: formula::GATE,
            kind: "STATELESS",
            bypass: Some((&params.bypass, setter)),
        },
        Some(background),
        |ui, rect| {
            let rect = rect.shrink(6.0);
            let (cells, len) = params.pattern.load();
            let cells = &cells[..len];

            let text_rect =
                Rect::from_min_size(rect.min, egui::vec2(rect.width(), rect.height() * TEXT_V));
            // 曲線図とセル図で横方向の基準を揃える。セル図は右端に `-`/`+` の列を
            // 空けるので、曲線図からも同じ幅を引かないと小節線の x が食い違う。
            let plot_w = (rect.width() - STEPS_BTN_W - STEPS_BTN_GAP).max(1.0);
            let curve_rect = Rect::from_min_size(
                egui::pos2(rect.left(), text_rect.bottom()),
                egui::vec2(plot_w, rect.height() * CURVE_V),
            );
            let grid_rect =
                Rect::from_min_max(egui::pos2(rect.left(), curve_rect.bottom()), rect.max);

            // 削ったセルの控え。GUI スレッドだけが触る。
            let mut tail = match params.pattern_tail.lock() {
                Ok(tail) => tail,
                // 他スレッドが panic した痕跡。握り潰さず中身を捨てて続ける
                // （補助データなので、壊れた控えを復元しようとしない）。
                Err(poisoned) => {
                    let mut tail = poisoned.into_inner();
                    tail.clear();
                    tail
                }
            };
            // 保存から読んだ直後は本体と食い違いうる。本体を優先して整える。
            let sanitized = pattern::sanitize_tail(&tail, len);
            if *tail != sanitized {
                *tail = sanitized;
            }

            pattern_text_row(
                ui,
                text_rect,
                &params.pattern,
                text,
                &mut tail,
                notice,
                palette.accent[10],
            );
            paint_curve(
                ui,
                curve_rect,
                cells,
                CurveInput {
                    duty: params.duty.modulated_plain_value(),
                    edge: params.edge.modulated_plain_value(),
                    shape: params.shape.modulated_plain_value(),
                    depth: params.depth.modulated_plain_value(),
                    accent: palette.accent[8],
                    guide: palette.neutral[5],
                    beat: palette.neutral[9],
                    steps_per_beat,
                },
            );
            pattern_grid(
                ui,
                grid_rect,
                &params.pattern,
                cells,
                text,
                &mut tail,
                CellColors {
                    accent: palette.accent[9],
                    idle: palette.neutral[4],
                    glyph: palette.neutral[8],
                },
            );
        },
        |ui| {
            // step と sync を左右に分ける。行を増やすとスライダーがはみ出す。
            ui.columns(2, |cols| {
                choice_row(
                    &mut cols[0],
                    "step",
                    &params.step_length,
                    setter,
                    &[
                        ("1/4", StepLengthEnum::Quarter),
                        ("1/8", StepLengthEnum::Eighth),
                        ("1/16", StepLengthEnum::Sixteenth),
                        ("1/32", StepLengthEnum::ThirtySecond),
                        ("1/8T", StepLengthEnum::EighthTriplet),
                        ("1/16T", StepLengthEnum::SixteenthTriplet),
                    ],
                );
                choice_row(
                    &mut cols[1],
                    "sync",
                    &params.sync_mode,
                    setter,
                    &[
                        ("song", SyncModeEnum::Song),
                        ("bar", SyncModeEnum::Bar),
                        ("note", SyncModeEnum::Note),
                    ],
                );
            });
            ui.columns(2, |cols| {
                bead_slider_row(&mut cols[0], "duty", &params.duty, setter);
                bead_slider_row(&mut cols[1], "edge", &params.edge, setter);
            });
            ui.columns(2, |cols| {
                bead_slider_row(&mut cols[0], "shape", &params.shape, setter);
                bead_slider_row(&mut cols[1], "depth", &params.depth, setter);
            });
        },
    );
}

pub fn create(
    params: Arc<GateParams>,
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

/// パターン文字列の入力欄。
///
/// 下書きが解釈できた時だけ正本へ書き、解釈できない間は枠を警告色にして**書かない**
/// （壊れた文字列を黙って既定へ落とさない）。フォーカスが無い間は正本から下書きを
/// 引き直すので、プリセット読込やセルのクリックが欄に反映される。
fn pattern_text_row(
    ui: &mut egui::Ui,
    rect: Rect,
    code: &PatternCode,
    text: &mut String,
    tail: &mut String,
    notice: &mut Option<String>,
    warn: Color32,
) {
    // 欄の右に copy / paste と長さ表示の場所を空ける。重ねずに退けるので文字が衝突しない。
    let field_rect = Rect::from_min_size(
        rect.min,
        egui::vec2(rect.width() - COUNTER_W - CLIP_W, rect.height()),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(field_rect));
    let id = child.make_persistent_id("gate-pattern-text");
    let focused = child.memory(|m| m.has_focus(id));
    if !focused {
        *text = code.to_text();
    }

    let valid = pattern::parse(&*text).is_some();
    let response = child.add_sized(
        egui::vec2(field_rect.width(), rect.height().min(24.0)),
        egui::TextEdit::singleline(&mut *text)
            .id(id)
            .font(FontId::monospace(14.0))
            .hint_text("| = trig   - = hold   _ = rest"),
    );
    if response.changed() {
        let before = code.load().1;
        if code.store_str(&*text) {
            // 解釈できた時だけ正本へ書き、下書きを正規形へ揃える。
            // 改行やタブ込みで貼られた文字列がここで 1 行に畳まれる。
            *text = code.to_text();
            if code.load().1 != before {
                // 長さが外から変わった。古い控えは本体と繋がらないので捨てる。
                tail.clear();
            }
        }
    }
    if !valid {
        ui.painter().rect_stroke(
            response.rect,
            2.0,
            Stroke::new(1.0, warn),
            egui::StrokeKind::Outside,
        );
    }

    clipboard_buttons(
        ui,
        Rect::from_min_size(
            egui::pos2(field_rect.right(), response.rect.top()),
            egui::vec2(CLIP_W, response.rect.height()),
        ),
        code,
        text,
        tail,
        notice,
    );

    // 実効長 / 上限。上限を超えた分も数えて見せる（黙って切らない）ので、
    // どれだけ余っている・はみ出しているかが数で分かる。
    let used = text.chars().filter(|c| !c.is_whitespace()).count();
    let color = if used > MAX_STEPS {
        warn
    } else {
        ui.visuals().weak_text_color()
    };
    ui.painter().text(
        egui::pos2(rect.right(), response.rect.center().y),
        Align2::RIGHT_CENTER,
        format!("{used}/{MAX_STEPS}"),
        FontId::monospace(12.0),
        color,
    );

    // 失敗した事実を出す（黙って無視しない）。成功すれば消える。
    if let Some(msg) = notice.as_deref() {
        ui.painter().text(
            egui::pos2(rect.left(), rect.bottom()),
            Align2::LEFT_BOTTOM,
            msg,
            FontId::proportional(11.0),
            warn,
        );
    }
}

/// copy / paste ボタン。キー入力を受け取れないホストでもパターンを出し入れできる唯一の経路。
///
/// copy は egui のクリップボード出力に載せる。paste は egui 側に能動的な読み取り API が
/// 無い（読み取りがキーイベント判定の内側でしか呼ばれない）ため、`copypasta` を直接叩く。
fn clipboard_buttons(
    ui: &mut egui::Ui,
    rect: Rect,
    code: &PatternCode,
    text: &mut String,
    tail: &mut String,
    notice: &mut Option<String>,
) {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 4.0;
    if child.button("copy").on_hover_text("copy pattern to clipboard").clicked() {
        // 下書きではなく正本から出す（解釈できない文字列をコピーさせない）。
        child.ctx().copy_text(code.to_text());
        *notice = None;
    }
    if child
        .button("paste")
        .on_hover_text("paste pattern from clipboard")
        .clicked()
    {
        match read_clipboard() {
            Ok(pasted) => {
                let before = code.load().1;
                if code.store_str(&pasted) {
                    *text = code.to_text();
                    *notice = None;
                    if code.load().1 != before {
                        // 長さが外から変わった。古い控えは本体と繋がらない。
                        tail.clear();
                    }
                } else {
                    // 正本は書き換えない。読めなかった文字列は下書きへ入れて枠を警告色にする。
                    *text = pasted;
                    *notice = Some("paste: not a valid pattern".to_owned());
                }
            }
            Err(e) => *notice = Some(format!("paste error: {e}")),
        }
    }
}

/// クリップボードのテキストを読む。ハンドルは押下時だけ作り、抱え続けない。
fn read_clipboard() -> Result<String, String> {
    use copypasta::ClipboardProvider;
    let mut ctx = copypasta::ClipboardContext::new().map_err(|e| e.to_string())?;
    ctx.get_contents().map_err(|e| e.to_string())
}

/// 主図 1 枚分の入力。
struct CurveInput {
    duty: f32,
    edge: f32,
    shape: f32,
    depth: f32,
    accent: Color32,
    guide: Color32,
    /// 拍マークの色。**加算合成**（`additive`）で、根元が最も濃く上端で 0 に溶ける。
    /// 輪郭線は引かない（単色しか持てず、上端まで濃さが残って「Y の脚」に見えるため）。
    beat: Color32,
    /// 1 拍あたりのステップ数。0 以下・非有限なら拍マークを出さない。
    steps_per_beat: f64,
}

/// セルの色。
struct CellColors {
    accent: Color32,
    idle: Color32,
    glyph: Color32,
}

/// パターン 1 周分のゲイン曲線。縦軸は 0..1 固定（`depth` の効きを図に出すため）。
fn paint_curve(ui: &egui::Ui, rect: Rect, cells: &[StepCell], input: CurveInput) {
    let painter = ui.painter_at(rect);
    let steps = cells.len().max(1);

    let y_of = |g: f32| rect.bottom() - g * rect.height();

    // 0 と 1 の基準線。曲線がどこまで開いているかを読む枠。
    for g in [0.0_f32, 1.0] {
        painter.line_segment(
            [
                Pos2::new(rect.left(), y_of(g)),
                Pos2::new(rect.right(), y_of(g)),
            ],
            Stroke::new(1.0, weak(input.guide, 50)),
        );
    }
    // ステップ境界。曲線とセルの横方向整合を目で確かめるための格子。
    // 濃淡は拍線で付けるので、ここでは一律に薄く引く
    // （もとの `k % 4` は step = 1/16 を前提にした拍線であり、他の step で嘘になる）。
    for k in 0..=steps {
        let x = rect.left() + rect.width() * (k as f32 / steps as f32);
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1.0, weak(input.guide, 30)),
        );
    }
    // 拍マーク。**最初のセルを拍頭とみなした**位置に、底辺から上へ払う 2 本の斜線
    // （「人」の字の形）を出す。縦線と違い曲線を横切らないので、曲線の可読性を損なわない。
    let mark_h = rect.height() * BEAT_MARK_H_V;
    // 幅は**右side のみ**へ伸ばす。左辺が垂直線になって拍頭の x を鋭く示し、
    // 形の流れが時間の進行方向と一致する。
    let mark_w = rect.width() / steps as f32 * BEAT_MARK_W_CELLS;
    for x in beat_lines(rect.left(), rect.width(), steps, input.steps_per_beat) {
        // 辺が内側へ反るので凸多角形ではない。凸前提の `convex_polygon` は使えず、
        // 曲線の塗りと同じく Mesh を自前で張る。
        //
        // 濃さは**頂点色の勾配**で作る。加算合成では「薄い線を重ねた密度」と
        // 「濃さを直接指定する」が同じ値を生むので、密度で描く必要がない。
        // 輪郭線を引かないのは、線が単色しか持てず上端まで濃さが残るため。
        let mut mesh = Mesh::default();
        for i in 0..=BEAT_MARK_SEGMENTS {
            let u = i as f32 / BEAT_MARK_SEGMENTS as f32;
            let hv = u.powf(BEAT_MARK_CURVE);
            let y = rect.bottom() - mark_h * hv;
            let w = mark_w * (1.0 - u).powf(BEAT_MARK_CURVE);
            // 濃さは高さの比で決める（媒介変数ではなく実際の高さに従わせる）。
            let c = additive(input.beat, BEAT_MARK_GAIN * (1.0 - hv).powf(BEAT_MARK_FADE));
            mesh.colored_vertex(Pos2::new(x, y), c);
            mesh.colored_vertex(Pos2::new(x + w, y), c);
        }
        for i in 0..BEAT_MARK_SEGMENTS {
            let a = (i * 2) as u32;
            mesh.add_triangle(a, a + 1, a + 2);
            mesh.add_triangle(a + 1, a + 2, a + 3);
        }
        painter.add(egui::Shape::mesh(mesh));
    }

    let n = steps * SAMPLES_PER_STEP;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let pos = steps as f64 * (i as f64 / n as f64);
            let g = gate_curve(cells, pos, input.duty, input.edge, input.shape);
            let gain = g * input.depth + (1.0 - input.depth);
            Pos2::new(
                rect.left() + rect.width() * (i as f32 / n as f32),
                y_of(gain),
            )
        })
        .collect();

    // 曲線下を塗る。面積 = 通る量。depth=0 で全面 = 何も削っていないことが読める。
    let mut mesh = Mesh::default();
    let fill = weak(input.accent, 60);
    for p in &pts {
        mesh.colored_vertex(*p, fill);
        mesh.colored_vertex(Pos2::new(p.x, rect.bottom()), fill);
    }
    for i in 0..n {
        let a = (i * 2) as u32;
        mesh.add_triangle(a, a + 1, a + 2);
        mesh.add_triangle(a + 1, a + 2, a + 3);
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.add(egui::Shape::line(pts, Stroke::new(2.0, input.accent)));
}

/// セルの直接編集。左クリックで Rest -> Trig -> Hold の巡回、右クリックで Rest。
///
/// セル数はパターン長そのもの。長さの増減は文字列側で行う（ここでは変えない）。
fn pattern_grid(
    ui: &mut egui::Ui,
    rect: Rect,
    code: &PatternCode,
    cells: &[StepCell],
    text: &mut String,
    tail: &mut String,
    colors: CellColors,
) {
    let len = cells.len();
    if len == 0 {
        return;
    }
    // 右端に `-` / `+` の列を空ける。操作対象（セル数）の隣に置く。
    let buttons_rect =
        Rect::from_min_max(egui::pos2(rect.right() - STEPS_BTN_W, rect.top()), rect.max);
    let rect = Rect::from_min_max(
        rect.min,
        egui::pos2(buttons_rect.left() - STEPS_BTN_GAP, rect.bottom()),
    );
    let cell_w = rect.width() / len as f32;
    let h = (rect.height() - 4.0).max(1.0);
    // 短いパターンでセルが間延びしないよう、隙間は幅に比例させる。
    let gap = CELL_GAP.min(cell_w * 0.12);

    let mut next: Option<Vec<StepCell>> = None;
    for (k, cell) in cells.iter().enumerate() {
        let cell_rect = Rect::from_min_size(
            egui::pos2(rect.left() + cell_w * k as f32, rect.top() + 4.0),
            egui::vec2(cell_w, h),
        )
        .shrink(gap);
        let id = ui.make_persistent_id(("gate-cell", k));
        let response = ui.interact(cell_rect, id, Sense::click());

        if response.clicked() || response.clicked_by(egui::PointerButton::Secondary) {
            let replaced = if response.clicked_by(egui::PointerButton::Secondary) {
                StepCell::Rest
            } else {
                match cell {
                    StepCell::Rest => StepCell::Trig,
                    StepCell::Trig => StepCell::Hold,
                    StepCell::Hold => StepCell::Rest,
                }
            };
            let mut edited = cells.to_vec();
            edited[k] = replaced;
            next = Some(edited);
        }

        let painter = ui.painter_at(cell_rect);
        let (fill, mark) = match cell {
            StepCell::Rest => (weak(colors.idle, 90), "_"),
            StepCell::Trig => (colors.accent, "|"),
            StepCell::Hold => (weak(colors.accent, 130), "-"),
        };
        painter.rect_filled(cell_rect, 2.0, fill);
        if response.hovered() {
            painter.rect_stroke(
                cell_rect,
                2.0,
                Stroke::new(1.0, colors.glyph),
                egui::StrokeKind::Inside,
            );
        }
        // セルが細い時は字を出さない（潰れて読めないものを描かない）。
        if cell_w >= 12.0 {
            painter.text(
                cell_rect.center(),
                Align2::CENTER_CENTER,
                mark,
                FontId::monospace(14.0),
                colors.glyph,
            );
        }
    }

    if let Some(edited) = step_count_buttons(ui, buttons_rect, cells, tail) {
        next = Some(edited);
    }

    if let Some(edited) = next {
        code.store(&edited);
        // 入力欄の下書きも即座に追随させる（フォーカス中でもクリックが反映されるように）。
        *text = code.to_text();
    }
}

/// セル数の `-` / `+`。端（1 セル / [`MAX_STEPS`] セル）では押せない。
///
/// 増減した列を返すだけで、書き込みはセルクリックと同じ経路に合流させる。
/// `-` で削ったセルは `tail` に積み、`+` はそこから戻す（削って戻すと元に復る）。
fn step_count_buttons(
    ui: &mut egui::Ui,
    rect: Rect,
    cells: &[StepCell],
    tail: &mut String,
) -> Option<Vec<StepCell>> {
    let len = cells.len();
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.y = 2.0;
    let h = ((rect.height() - 2.0) * 0.5).max(1.0);
    let size = egui::vec2(rect.width(), h);

    let mut edited = None;
    if child
        .add_enabled(len < MAX_STEPS, egui::Button::new("+").min_size(size))
        .on_hover_text("add one cell")
        .clicked()
    {
        // 直前に `-` で削ったセルがあればそれを戻す。無ければ Rest。
        edited = Some(pattern::grow_with_tail(cells, tail));
    }
    if child
        .add_enabled(len > 1, egui::Button::new("-").min_size(size))
        .on_hover_text("remove one cell")
        .clicked()
    {
        // 削ったセルは控えへ積む（`+` で戻せるように）。
        edited = Some(pattern::shrink_with_tail(cells, tail));
    }
    edited
}

/// 拍線の x 座標。**最初のセルを拍頭とみなす**（`plans/gate_sync_and_bars_v01.md` §4-2）。
///
/// `steps_per_beat` は `1 / beats_per_step` なので、3 連（1/8T → 3.0）でも整数になる。
/// 非整数になっても端数位置にそのまま返す（丸めて嘘の位置に引かない）。
/// **左端（0）も出す。** 拍頭は等間隔に現れるべきで、端だけ省くと規則性が崩れる。
/// 右端（`steps`）は次の周の 0 と同じ位置なので出さない（二重に描かない）。
fn beat_lines(left: f32, width: f32, steps: usize, steps_per_beat: f64) -> Vec<f32> {
    if !steps_per_beat.is_finite() || steps_per_beat <= 0.0 || steps == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut b = 0.0;
    while b < steps as f64 {
        out.push(left + width * (b / steps as f64) as f32);
        b += steps_per_beat;
    }
    out
}

/// 加算合成の色を作る。egui は premultiplied alpha なので **alpha = 0 が加算**を意味する
/// （`ecolor::Color32` の doc: "alpha=0 means the color is to be treated as an additive color"）。
/// `gain` で加算量を絞る。1.0 では曲線より前に出て眩しい。
fn additive(c: Color32, gain: f32) -> Color32 {
    let f = |v: u8| (v as f32 * gain).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb_additive(f(c.r()), f(c.g()), f(c.b()))
}

/// 色のアルファだけを差し替える。
fn weak(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 拍線は「最初のセルが拍頭」の仮定で等間隔に入る。左端も出す。
    #[test]
    fn beat_lines_positions() {
        // 1/16: 1 拍 = 4 ステップ。16 セルなら 0/4/8/12 の 4 本
        // （16 は次の周の 0 と同じ位置なので出さない）。
        let lines = beat_lines(0.0, 160.0, 16, 4.0);
        assert_eq!(lines, vec![0.0, 40.0, 80.0, 120.0]);
        // 1/4: 1 拍 = 1 ステップ。全セル境界に入る（右端を除く）。
        assert_eq!(beat_lines(0.0, 160.0, 16, 1.0).len(), 16);
        // 1/8T: 1 拍 = 3 ステップ。
        let lines = beat_lines(0.0, 120.0, 12, 3.0);
        assert_eq!(lines, vec![0.0, 30.0, 60.0, 90.0]);
    }

    /// `steps_per_beat` が step の実値と一致すること（決め打ちを残さない）。
    #[test]
    fn steps_per_beat_matches_step_length() {
        use crate::dsp::StepLength;
        for (len, expected) in [
            (StepLength::Quarter, 1.0),
            (StepLength::Eighth, 2.0),
            (StepLength::Sixteenth, 4.0),
            (StepLength::ThirtySecond, 8.0),
            (StepLength::EighthTriplet, 3.0),
            (StepLength::SixteenthTriplet, 6.0),
        ] {
            let got = 1.0 / len.beats_per_step();
            assert!((got - expected).abs() < 1e-9, "{len:?}: {got} != {expected}");
        }
    }

    /// 不正な入力で線を出さない（パニックも無限ループもしない）。
    #[test]
    fn beat_lines_rejects_degenerate_input() {
        assert!(beat_lines(0.0, 160.0, 16, 0.0).is_empty());
        assert!(beat_lines(0.0, 160.0, 16, -4.0).is_empty());
        assert!(beat_lines(0.0, 160.0, 16, f64::NAN).is_empty());
        assert!(beat_lines(0.0, 160.0, 16, f64::INFINITY).is_empty());
        assert!(beat_lines(0.0, 160.0, 0, 4.0).is_empty());
    }
}
