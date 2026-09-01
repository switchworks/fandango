//! 中央上の式透かし。下付きは Unicode 下付きブロックを使わず自前描画する。

use nih_plug_egui::egui::{self, Align2, Color32, FontId, Painter, Pos2};

#[derive(Clone, Copy)]
pub enum Script {
    Base,
    Sub,
    Sup,
}

/// 下付き・上付きの縮小率。両者で共通。
const SCRIPT_SCALE: f32 = 0.68;
/// 下付きのベースラインからの下げ幅（`base_size` に対する比）。
const SUB_OFFSET: f32 = 0.30;
/// 上付きの上げ幅。下付きより浅いのは、大文字・数字の高さが
/// ベースライン上にしか伸びず、同じ量だと離れて見えるため。
const SUP_OFFSET: f32 = 0.26;

/// これ以上は縮めない下限のフォントサイズ。読めなくなるより衝突を選ぶ。
const MIN_SIZE: f32 = 10.0;

/// セグメント列を `base_size` で描いた時の総送り幅。
pub fn scripted_width(painter: &Painter, segments: &[(&str, Script)], base_size: f32) -> f32 {
    let mut total_w = 0.0;
    for &(text, script) in segments {
        let font = font_id(script, base_size);
        let galley = painter.layout_no_wrap(text.to_owned(), font, Color32::WHITE);
        total_w += galley.size().x;
    }
    total_w
}

/// セグメント列を中央寄せして描く。送り幅は `Painter::text` の返り値 `width()` のみ使う。
///
/// `max_width` を超える場合はフォントを縮めて収める（下限 [`MIN_SIZE`]）。式の長さは
/// crate ごとに違い、ヘッダ左のワードマークと右の種別表示に挟まれた幅は固定なので、
/// 呼び出し側で個別に調整させず描画側で吸収する。
pub fn paint_scripted(
    painter: &Painter,
    center: Pos2,
    segments: &[(&str, Script)],
    base_size: f32,
    color: Color32,
    max_width: f32,
) {
    let full_w = scripted_width(painter, segments, base_size);
    let base_size = if full_w > max_width && full_w > 0.0 {
        (base_size * max_width / full_w).max(MIN_SIZE)
    } else {
        base_size
    };

    let total_w = scripted_width(painter, segments, base_size);

    let mut x = center.x - total_w * 0.5;
    for &(text, script) in segments {
        let font = font_id(script, base_size);
        let y = match script {
            Script::Base => center.y,
            Script::Sub => center.y + base_size * SUB_OFFSET,
            Script::Sup => center.y - base_size * SUP_OFFSET,
        };
        let rect = painter.text(egui::pos2(x, y), Align2::LEFT_CENTER, text, font, color);
        x += rect.width();
    }
}

fn font_id(script: Script, base_size: f32) -> FontId {
    let size = match script {
        Script::Base => base_size,
        Script::Sub | Script::Sup => base_size * SCRIPT_SCALE,
    };
    FontId::monospace(size)
}

pub const NONE: &[(&str, Script)] = &[];

/// `y = W(r) SUM g_i x(t - tau_i)`
pub const NIMBUS: &[(&str, Script)] = &[
    ("y = W(r) SUM g", Script::Base),
    ("i", Script::Sub),
    (" x(t - tau", Script::Base),
    ("i", Script::Sub),
    (")", Script::Base),
];

/// `a = 4(1-t)/(3-2t)`
pub const SPREAD: &[(&str, Script)] = &[("a = 4(1-t)/(3-2t)", Script::Base)];

/// `H(z) = H_LS(z) H_HS(z)`
pub const TILT: &[(&str, Script)] = &[
    ("H(z) = H", Script::Base),
    ("LS", Script::Sub),
    ("(z) H", Script::Base),
    ("HS", Script::Sub),
    ("(z)", Script::Base),
];

/// `y = (F(x) - F(x_n-1))/(x - x_n-1)`
pub const CRUSH: &[(&str, Script)] = &[
    ("y = (F(x) - F(x", Script::Base),
    ("n-1", Script::Sub),
    ("))/(x - x", Script::Base),
    ("n-1", Script::Sub),
    (")", Script::Base),
];

/// `H(z) = ((a + 1/z)/(1 + a/z))^N`
pub const ROTATE: &[(&str, Script)] = &[
    ("H(z) = ((a + 1/z)/(1 + a/z))", Script::Base),
    ("N", Script::Sup),
];

/// `u = (env/env_typ)^-alpha`
pub const NICE_BULK: &[(&str, Script)] = &[
    ("u = (env/env", Script::Base),
    ("typ", Script::Sub),
    (")", Script::Base),
    ("-alpha", Script::Sup),
];

/// `y = SUM s_k x(t - t_k), s_k = +-1`
pub const VELVET: &[(&str, Script)] = &[
    ("y = SUM s", Script::Base),
    ("k", Script::Sub),
    (" x(t - t", Script::Base),
    ("k", Script::Sub),
    ("), s", Script::Base),
    ("k", Script::Sub),
    (" = +-1", Script::Base),
];

/// `H(s) = s/(s+w_h) PROD w_i^2/(s^2+(w_i/Q_i)s+w_i^2)`
///
/// 前段 1 次ハイパス × 共振ローパス 2 段直列 (声道の全極モデル)。
/// `crates/drawl/src/dsp.rs` の `chain_response` / `chain_power` と同じ式で、
/// 主図に描いている曲線そのもの。
pub const DRAWL: &[(&str, Script)] = &[
    ("H(s) = s/(s+w", Script::Base),
    ("h", Script::Sub),
    (") PROD w", Script::Base),
    ("i", Script::Sub),
    ("2", Script::Sup),
    ("/(s", Script::Base),
    ("2", Script::Sup),
    ("+(w", Script::Base),
    ("i", Script::Sub),
    ("/Q", Script::Base),
    ("i", Script::Sub),
    (")s+w", Script::Base),
    ("i", Script::Sub),
    ("2", Script::Sup),
    (")", Script::Base),
];

/// `tau_n = tau_0 + 3n/8 T, a_n = a b^n`
///
/// 帰還は両チャンネルとも 3/8 拍周期なので、エコー列は初期タップ
/// `tau_0`（L = 2/8 T, R = 3/8 T）からの等間隔 + 等比減衰という閉形式になる。
/// 主図に描いている点列そのもの (`crates/trip-delay/src/editor.rs`)。
/// `tau_0` の非対称は図のレーン見出しが担うので式には書かない（ヘッダ幅に収めるため）。
pub const TRIP_DELAY: &[(&str, Script)] = &[
    ("tau", Script::Base),
    ("n", Script::Sub),
    (" = tau", Script::Base),
    ("0", Script::Sub),
    (" + 3n/8 T, a", Script::Base),
    ("n", Script::Sub),
    (" = a b", Script::Base),
    ("n", Script::Sup),
];

/// `y(t) = x(t) + g y(t - T)`
pub const RAINDROP: &[(&str, Script)] = &[("y(t) = x(t) + g y(t - T)", Script::Base)];

/// `y = min(f(g x), c)`
pub const FINALIST: &[(&str, Script)] = &[("y = min(f(g x), c)", Script::Base)];

/// `y = x[g(phi) d + 1 - d]`
///
/// `g` は無状態のゲート曲線 (`crates/gate/src/dsp.rs` の `gate_curve`)、
/// `phi` は transport から導く絶対ステップ位相、`d` は depth。
/// `g = 1` または `d = 0` で恒等 (`plans/gate_v01.md` §4 I5 / I6)。
pub const GATE: &[(&str, Script)] = &[("y = x[g(phi) d + 1 - d]", Script::Base)];
