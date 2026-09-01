//! ビーズ・レールと enum/bool のセグメント選択。

use nih_plug::prelude::{Param, ParamSetter};
use nih_plug_egui::egui::{
    self, Align2, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2,
};

/// ラベルと固定値表示を持つ、可動真円 1 個のビーズ・レール。
pub fn bead_slider_row(
    ui: &mut egui::Ui,
    label: &str,
    param: &nih_plug::params::FloatParam,
    setter: &ParamSetter,
) -> egui::Response {
    bead_slider_generic(ui, label, param, setter)
}

/// `IntParam` 版。真円は 1 個のまま、離散値はレール上でスナップする。
pub fn int_bead_slider_row(
    ui: &mut egui::Ui,
    label: &str,
    param: &nih_plug::params::IntParam,
    setter: &ParamSetter,
) -> egui::Response {
    bead_slider_generic(ui, label, param, setter)
}

fn bead_slider_generic<P: Param>(
    ui: &mut egui::Ui,
    label: &str,
    param: &P,
    setter: &ParamSetter,
) -> egui::Response
where
    P::Plain: PartialEq,
{
    const H: f32 = 44.0;
    const VALUE_W: f32 = 88.0;
    const RAIL_INSET: f32 = 12.0;
    const DOT_R: f32 = 10.0;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, H), Sense::hover());
    let value_rect = Rect::from_min_size(
        egui::pos2(rect.right() - VALUE_W, rect.top()),
        Vec2::new(VALUE_W, 18.0),
    );
    #[cfg(feature = "direct_value_entry")]
    {
        let value_id = ui.make_persistent_id((label, "bead-value"));
        let mut value_response = ui.interact(value_rect, value_id, Sense::click());
        if value_response.double_clicked() {
            let plain = param.normalized_value_to_string(param.modulated_normalized_value(), false);
            ui.memory_mut(|m| m.data.insert_temp(edit_buf_id(&value_response), plain));
        }
        if show_text_edit_overlay(ui, value_rect, &mut value_response, param, setter) {
            return value_response;
        }
    }

    let rail_y = rect.bottom() - 12.0;
    let rail_left = rect.left() + RAIL_INSET;
    let rail_right = rect.right() - RAIL_INSET;
    let rail_w = rail_right - rail_left;
    let hit_rect = Rect::from_center_size(
        egui::pos2((rail_left + rail_right) * 0.5, rail_y),
        Vec2::new(rail_w, 28.0),
    );
    let rail_id = ui.make_persistent_id((label, "bead-rail"));
    let mut response = ui.interact(hit_rect, rail_id, Sense::click_and_drag());
    // ホイールはレール・値表示のどちらの上でも効かせる (rect 全体でホバー判定)。
    let scroll_id = ui.make_persistent_id((label, "bead-scroll"));
    let mut scroll_response = ui.interact(rect, scroll_id, Sense::hover());
    apply_scroll(ui, &mut scroll_response, param, setter);
    let normalized = param.modulated_normalized_value();
    let travel_left = rail_left + DOT_R;
    let travel_right = rail_right - DOT_R;
    let travel = travel_right - travel_left;
    let pos_to_norm = |pos: Pos2| ((pos.x - travel_left) / travel).clamp(0.0, 1.0);

    if response.drag_started() {
        setter.begin_set_parameter(param);
    }
    if response.double_clicked() {
        snap_to_step(&mut response, param, setter);
    } else if response.clicked_by(egui::PointerButton::Secondary) {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, param.default_plain_value());
        setter.end_set_parameter(param);
        response.mark_changed();
    } else if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let plain = param.preview_plain(pos_to_norm(pos));
            if plain != param.modulated_plain_value() {
                setter.set_parameter(param, plain);
                response.mark_changed();
            }
        }
    } else if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            setter.begin_set_parameter(param);
            setter.set_parameter(param, param.preview_plain(pos_to_norm(pos)));
            setter.end_set_parameter(param);
            response.mark_changed();
        }
    }
    if response.drag_stopped() {
        setter.end_set_parameter(param);
    }

    let bead_x = travel_left + travel * normalized;
    let visuals = ui.visuals().clone();
    let painter = ui.painter();
    painter.text(
        rect.left_top(),
        Align2::LEFT_TOP,
        label,
        FontId::monospace(12.0),
        visuals.weak_text_color(),
    );
    painter.text(
        value_rect.right_top(),
        Align2::RIGHT_TOP,
        param.to_string(),
        FontId::monospace(12.0),
        visuals.strong_text_color(),
    );
    let rail_stroke = Stroke::new(3.0, visuals.widgets.noninteractive.bg_stroke.color);
    painter.line_segment(
        [
            egui::pos2(rail_left, rail_y),
            egui::pos2(rail_right, rail_y),
        ],
        rail_stroke,
    );
    let accent = visuals.widgets.active.bg_stroke.color;
    painter.line_segment(
        [egui::pos2(rail_left, rail_y), egui::pos2(bead_x, rail_y)],
        Stroke::new(3.0, accent),
    );
    let fill = if response.dragged() {
        visuals.widgets.active.bg_fill
    } else {
        visuals.widgets.inactive.bg_fill
    };
    painter.circle_filled(egui::pos2(bead_x, rail_y), DOT_R, fill);
    painter.circle_stroke(egui::pos2(bead_x, rail_y), DOT_R, Stroke::new(1.5, accent));
    response
}

/// 無彩色の等幅テキストによる enum/bool セグメント選択。選択中のみアクセント色。
pub fn choice_row<P: Param>(
    ui: &mut egui::Ui,
    label: &str,
    param: &P,
    setter: &ParamSetter,
    options: &[(&str, P::Plain)],
) -> egui::Response
where
    P::Plain: Copy + PartialEq,
{
    const H: f32 = 22.0;
    const VALUE_W: f32 = 88.0;
    let w = ui.available_width();
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(w, H), Sense::click());
    let value_rect = Rect::from_min_size(
        egui::pos2(rect.right() - VALUE_W, rect.top()),
        Vec2::new(VALUE_W, H),
    );
    #[cfg(feature = "direct_value_entry")]
    {
        let value_id = ui.make_persistent_id((label, "choice-value"));
        let mut value_response = ui.interact(value_rect, value_id, Sense::click());
        if value_response.double_clicked() {
            let plain = param.normalized_value_to_string(param.modulated_normalized_value(), false);
            ui.memory_mut(|m| m.data.insert_temp(edit_buf_id(&value_response), plain));
        }
        if show_text_edit_overlay(ui, value_rect, &mut value_response, param, setter) {
            return value_response;
        }
    }

    if response.clicked_by(egui::PointerButton::Secondary) {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, param.default_plain_value());
        setter.end_set_parameter(param);
        response.mark_changed();
    }

    let current = param.modulated_plain_value();
    let visuals = ui.visuals().clone();
    let accent = visuals.widgets.active.bg_stroke.color;
    let weak = visuals.weak_text_color();
    let font = FontId::monospace(12.0);
    let label_rect = ui.painter().text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        label,
        font.clone(),
        weak,
    );
    ui.painter().text(
        value_rect.right_center(),
        Align2::RIGHT_CENTER,
        param.to_string(),
        font.clone(),
        visuals.strong_text_color(),
    );

    let mut x = label_rect.right() + 16.0;
    for &(opt_label, value) in options {
        let galley = ui
            .painter()
            .layout_no_wrap(opt_label.to_owned(), font.clone(), weak);
        let opt_w = galley.size().x + 12.0;
        let opt_rect = Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(opt_w, H));
        let opt_id = ui.make_persistent_id((label, opt_label, "choice-opt"));
        let opt_resp = ui.interact(opt_rect, opt_id, Sense::click());
        if opt_resp.clicked() && current != value {
            setter.begin_set_parameter(param);
            setter.set_parameter(param, value);
            setter.end_set_parameter(param);
            response.mark_changed();
        }
        let color = if current == value { accent } else { weak };
        ui.painter().text(
            opt_rect.center(),
            Align2::CENTER_CENTER,
            opt_label,
            font.clone(),
            color,
        );
        x += opt_w + 8.0;
    }
    response
}

/// 表示文字列から plain 値を数値として取り出す。単位なしで整形させた上で
/// パースするので、`FloatParam`/`IntParam` のような数値パラメータでのみ成功し、
/// enum など非数値パラメータでは `None` を返す (呼び出し側で機能を無効化する)。
fn plain_from_display<P: Param>(param: &P, normalized: f32) -> Option<f32> {
    param
        .normalized_value_to_string(normalized, false)
        .trim()
        .parse::<f32>()
        .ok()
}

/// 表示の小数桁数。ホイール通常操作の 1 ノッチ幅に使う。
fn display_decimals<P: Param>(param: &P, normalized: f32) -> usize {
    param
        .normalized_value_to_string(normalized, false)
        .split_once('.')
        .map(|(_, frac)| frac.trim().len())
        .unwrap_or(0)
}

/// そのパラメータにとって「切りのいい刻み」を plain range の幅から代数的に導出する。
/// 素材ごとの定数を置かないための規則: 全域を 20 分割した粗さを基準に、
/// 1/2/5 系列へ丸める。bits(幅15)→1.0、mix(幅1)→0.05、dB(幅48)→2.0、ms(幅2000)→100。
fn snap_step<P: Param>(param: &P) -> Option<f32> {
    let lo = plain_from_display(param, 0.0)?;
    let hi = plain_from_display(param, 1.0)?;
    let width = (hi - lo).abs();
    if !width.is_finite() || width <= 0.0 {
        return None;
    }
    let base = width / 20.0;
    let mag = 10f32.powf(base.log10().floor());
    let mantissa = base / mag;
    // 境界は 1/2/5/10 の幾何平均。等比の系列なので算術中点だと系列端 (7.5 等) が
    // 境界にちょうど乗り、f32 の丸め次第で刻みが揺れる。
    let m = if mantissa < 1.414 {
        1.0
    } else if mantissa < 3.162 {
        2.0
    } else if mantissa < 7.071 {
        5.0
    } else {
        10.0
    };
    Some(m * mag)
}

/// plain 値を正規化値へ戻す。`preview_normalized` は `P::Plain` を要求して
/// ジェネリックには扱えないため、表示と同じ書式の文字列を経由して逆写像を得る
/// (skew 付き range でも `string_to_normalized_value` が正しく戻す)。
fn plain_to_normalized<P: Param>(param: &P, plain: f32, decimals: usize) -> Option<f32> {
    param.string_to_normalized_value(&format!("{plain:.decimals$}"))
}

/// ホイールによる値変更。ホバー中のみ反応し、通常は表示桁の最小単位、
/// Shift 併用で [`snap_step`] 刻み。数値化できないパラメータでは何もしない。
fn apply_scroll<P: Param>(
    ui: &egui::Ui,
    response: &mut egui::Response,
    param: &P,
    setter: &ParamSetter,
) {
    if !response.hovered() {
        return;
    }
    let (scroll, shift) = ui.input(|i| (i.raw_scroll_delta.y, i.modifiers.shift));
    if scroll == 0.0 {
        return;
    }
    let normalized = param.modulated_normalized_value();
    let Some(current) = plain_from_display(param, normalized) else {
        return;
    };
    let decimals = display_decimals(param, normalized);
    let step = if shift {
        match snap_step(param) {
            Some(step) => step,
            None => return,
        }
    } else {
        10f32.powi(-(decimals as i32))
    };
    let target = current + step * scroll.signum();
    let Some(next) = plain_to_normalized(param, target, decimals) else {
        return;
    };
    if next != normalized {
        setter.begin_set_parameter(param);
        setter.set_parameter_normalized(param, next);
        setter.end_set_parameter(param);
        response.mark_changed();
    }
}

/// 現在値を [`snap_step`] の倍数へ四捨五入する。レール上のダブルクリック用。
fn snap_to_step<P: Param>(response: &mut egui::Response, param: &P, setter: &ParamSetter) {
    let normalized = param.modulated_normalized_value();
    let (Some(current), Some(step)) = (plain_from_display(param, normalized), snap_step(param))
    else {
        return;
    };
    let decimals = display_decimals(param, normalized);
    let snapped = (current / step).round() * step;
    let Some(next) = plain_to_normalized(param, snapped, decimals) else {
        return;
    };
    if next != normalized {
        setter.begin_set_parameter(param);
        setter.set_parameter_normalized(param, next);
        setter.end_set_parameter(param);
        response.mark_changed();
    }
}

/// ダブルクリックで直接入力欄をオーバレイ表示するための編集中バッファ。
/// `egui` の temp memory に `Response::id` キーで保持する (per-widget、永続化不要)。
#[cfg_attr(not(feature = "direct_value_entry"), allow(dead_code))]
fn edit_buf_id(response: &egui::Response) -> egui::Id {
    response.id.with("value_slider_edit_buf")
}

/// 直接入力欄オーバレイ。ツマミと同じ矩形・見た目 (背景+枠) の上に frameless な
/// `TextEdit` を重ねる — スライダー全体でなくツマミだけがテキスト編集モードに
/// 切り替わったように見せる。編集中なら `true` を返し、呼び出し側はスライダー
/// 本体の描画・ドラッグ処理をスキップする。確定 (Enter) で `setter` 経由の値反映。
#[cfg_attr(not(feature = "direct_value_entry"), allow(dead_code))]
fn show_text_edit_overlay<P: Param>(
    ui: &mut egui::Ui,
    thumb_rect: Rect,
    response: &mut egui::Response,
    param: &P,
    setter: &ParamSetter,
) -> bool {
    let id = edit_buf_id(response);
    let Some(mut buf) = ui.memory(|m| m.data.get_temp::<String>(id)) else {
        return false;
    };

    // TextEdit 自体に固定 id を持たせる (ui.put の自動採番だと request_focus 対象との
    // 紐付けが不安定になりうるため)。
    let text_edit_id = id.with("widget");

    // ツマミと同じ見た目 (背景+枠) を先に描いてから、frame なし TextEdit を重ねる。
    let visuals = ui.visuals().clone();
    let painter = ui.painter();
    painter.rect_filled(thumb_rect, 2.0, visuals.widgets.inactive.bg_fill);
    painter.rect_stroke(
        thumb_rect,
        2.0,
        Stroke::new(1.0, visuals.widgets.active.bg_stroke.color),
        StrokeKind::Middle,
    );

    let te_resp = ui.put(
        thumb_rect,
        egui::TextEdit::singleline(&mut buf)
            .id(text_edit_id)
            .horizontal_align(egui::Align::Center)
            .font(FontId::monospace(12.0))
            .text_color(visuals.strong_text_color())
            .frame(false),
    );
    // フォーカスが外れていれば毎フレーム要求し続ける (開始フレームで一度だけ
    // request_focus すると、その反映が次フレーム以降になる egui の挙動と噛み合わず
    // 取りこぼすことがあるため)。
    if !ui.memory(|m| m.has_focus(text_edit_id)) {
        ui.memory_mut(|m| m.request_focus(text_edit_id));
    }

    // TextEdit 自身は Enter/Esc で自動的に lost_focus() しない (シングルラインでも
    // 改行キー扱いのままフォーカスは保持される) ため、lost_focus() 待ちにせず毎フレーム
    // 直接キー押下を見て、確定/キャンセル時は surrender_focus で明示的に終了させる。
    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        if let Some(normalized) = param.string_to_normalized_value(buf.trim()) {
            setter.begin_set_parameter(param);
            setter.set_parameter_normalized(param, normalized);
            setter.end_set_parameter(param);
            response.mark_changed();
        }
        ui.memory_mut(|m| {
            m.data.remove::<String>(id);
            m.surrender_focus(text_edit_id);
        });
    } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        ui.memory_mut(|m| {
            m.data.remove::<String>(id);
            m.surrender_focus(text_edit_id);
        });
    } else if te_resp.lost_focus() {
        // クリック外など、キー操作以外でフォーカスが外れた場合も編集終了 (値は破棄)。
        ui.memory_mut(|m| m.data.remove::<String>(id));
    } else {
        ui.memory_mut(|m| m.data.insert_temp(id, buf));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use nih_plug::prelude::{FloatParam, FloatRange, IntParam, IntRange, formatters};

    fn f(min: f32, max: f32, decimals: usize) -> FloatParam {
        FloatParam::new("t", min, FloatRange::Linear { min, max })
            .with_value_to_string(formatters::v2s_f32_rounded(decimals))
    }

    fn assert_step(param: &FloatParam, expected: f32) {
        let got = snap_step(param).expect("numeric param");
        // 10^floor(log10(..)) の指数演算で最下位ビットがずれるため、値としての一致を見る。
        assert!(
            (got - expected).abs() <= expected * 1e-5,
            "expected {expected}, got {got}"
        );
    }

    #[test]
    fn snap_step_follows_range_width() {
        assert_step(&f(1.0, 16.0, 2), 1.0); // crush bits
        assert_step(&f(0.0, 1.0, 2), 0.05); // mix
        assert_step(&f(-24.0, 24.0, 1), 2.0); // dB
        assert_step(&f(0.0, 2000.0, 1), 100.0); // ms
    }

    #[test]
    fn snap_step_handles_int_params() {
        let p = IntParam::new("t", 0, IntRange::Linear { min: 0, max: 100 });
        assert_eq!(snap_step(&p), Some(5.0));
    }

    #[test]
    fn display_decimals_matches_formatter() {
        assert_eq!(display_decimals(&f(1.0, 16.0, 2), 0.5), 2);
        assert_eq!(display_decimals(&f(1.0, 16.0, 0), 0.5), 0);
    }

    #[test]
    fn plain_roundtrips_through_display() {
        let p = f(1.0, 16.0, 2);
        let n = plain_to_normalized(&p, 8.0, 2).unwrap();
        assert_eq!(plain_from_display(&p, n), Some(8.0));
    }
}
