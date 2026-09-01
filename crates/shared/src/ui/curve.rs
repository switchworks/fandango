//! 伝達曲線・応答曲線の共通描画（`plans/gui_overhaul_v01.md` Step 3）。
//!
//! データソースは呼び出し側のクロージャのみ。**オーディオスレッドとの通信を
//! 一切持たない。** パラメータの現在値を GUI 側で読み、その場で評価する。
//!
//! 細線は説明、面は訴求を担う。恒等線からの隔たりを面で塗ることで、
//! 「どれだけ持ち上げているか」が面積として読める。

use nih_plug_egui::egui::{self, Color32, Mesh, Pos2, Rect, Stroke};

/// 1 本の曲線。`map` は入力 dB を受け取り出力 dB を返す。
///
/// 入力軸が何を指すかは呼び出し側の意味論に従う。nice-bulk では
/// **帯域包絡のレベル**であって瞬時振幅ではない（包絡追従器を通った後の量）。
pub struct CurveSpec<'a> {
    pub label: &'a str,
    pub color: Color32,
    pub map: &'a dyn Fn(f32) -> f32,
}

/// 横軸のスケール。
#[derive(Clone, Copy, PartialEq)]
pub enum XScale {
    /// 線形。両端の単位は呼び出し側の意味論（dB など）。
    Linear,
    /// 対数。周波数軸用（20 Hz〜20 kHz など）。両端は正の値であること。
    Logarithmic,
}

/// 軸の範囲。縦軸は常に dB。
#[derive(Clone, Copy)]
pub struct Axes {
    pub x: (f32, f32),
    pub y: (f32, f32),
    pub x_scale: XScale,
}

impl Default for Axes {
    fn default() -> Self {
        Self {
            x: (-60.0, 0.0),
            y: (-60.0, 12.0),
            x_scale: XScale::Linear,
        }
    }
}

impl Axes {
    /// 正規化位置 `t ∈ [0,1]` に対応する x 値。
    fn x_at(&self, t: f32) -> f32 {
        match self.x_scale {
            XScale::Linear => self.x.0 + (self.x.1 - self.x.0) * t,
            XScale::Logarithmic => {
                self.x.0 * (self.x.1 / self.x.0).powf(t)
            }
        }
    }

    /// x 値の正規化位置。
    fn t_of(&self, x: f32) -> f32 {
        match self.x_scale {
            XScale::Linear => (x - self.x.0) / (self.x.1 - self.x.0),
            XScale::Logarithmic => {
                (x / self.x.0).log10() / (self.x.1 / self.x.0).log10()
            }
        }
    }
}

/// 面を塗る基準線。曲線との隔たりが「装置の仕事量」を表す。
#[derive(Clone, Copy)]
pub enum Baseline {
    /// `y = x`。入出力を同じ単位で取る伝達曲線用。
    Identity,
    /// 一定レベルの水平線。ゲイン曲線なら 0 dB を渡す。
    Level(f32),
}

/// 伝達曲線を描く。恒等線（`y = x`）からの隔たりを面で塗る。
///
/// サンプル点数は幅ピクセル数程度で足りる（同計画書 Step 3）。
pub fn transfer_plot(
    ui: &egui::Ui,
    rect: Rect,
    axes: Axes,
    baseline: Baseline,
    axis_label: &str,
    curves: &[CurveSpec<'_>],
) {
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    let grid = visuals.widgets.noninteractive.bg_stroke.color;

    let to_screen = |x: f32, y_db: f32| -> Pos2 {
        let u = axes.t_of(x).clamp(0.0, 1.0);
        let v = ((y_db - axes.y.0) / (axes.y.1 - axes.y.0)).clamp(0.0, 1.0);
        Pos2::new(rect.left() + rect.width() * u, rect.bottom() - rect.height() * v)
    };

    // 背景グリッド（中性色の低ステップ）。
    let faint = Color32::from_rgba_unmultiplied(grid.r(), grid.g(), grid.b(), 40);
    let vertical_at = |x: f32| {
        let sx = to_screen(x, 0.0).x;
        painter.line_segment(
            [Pos2::new(sx, rect.top()), Pos2::new(sx, rect.bottom())],
            Stroke::new(1.0, faint),
        );
    };
    match axes.x_scale {
        XScale::Linear => {
            let mut level = (axes.x.0 / 12.0).ceil() * 12.0;
            while level <= axes.x.1 {
                vertical_at(level);
                level += 12.0;
            }
        }
        XScale::Logarithmic => {
            // ディケードごとの 1,2,5 目盛り。
            let mut decade = 10f32.powf(axes.x.0.log10().floor());
            while decade <= axes.x.1 {
                for step in [1.0_f32, 2.0, 5.0] {
                    let f = decade * step;
                    if f >= axes.x.0 && f <= axes.x.1 {
                        vertical_at(f);
                    }
                }
                decade *= 10.0;
            }
        }
    }
    let mut level = (axes.y.0 / 12.0).ceil() * 12.0;
    while level <= axes.y.1 {
        let y = to_screen(0.0, level).y;
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(1.0, faint),
        );
        // 0 dB だけ数値を添える。全部に振ると図が数字で埋まる。
        if level.abs() < 0.5 {
            painter.text(
                Pos2::new(rect.right() - 4.0, y - 2.0),
                egui::Align2::RIGHT_BOTTOM,
                "0 dB",
                egui::FontId::monospace(9.0),
                Color32::from_rgba_unmultiplied(grid.r(), grid.g(), grid.b(), 170),
            );
        }
        level += 12.0;
    }

    // 基準線。ここからの隔たりが装置の仕事量そのもの。
    let base_at = |x: f32| match baseline {
        Baseline::Identity => x,
        Baseline::Level(level) => level,
    };
    let base_stroke = Stroke::new(
        1.0,
        Color32::from_rgba_unmultiplied(grid.r(), grid.g(), grid.b(), 170),
    );
    painter.line_segment(
        [
            to_screen(axes.x.0, base_at(axes.x.0)),
            to_screen(axes.x.1, base_at(axes.x.1)),
        ],
        base_stroke,
    );

    let samples = rect.width().round().max(2.0) as usize;
    for curve in curves {
        let mut line = Vec::with_capacity(samples);
        let mut mesh = Mesh::default();
        // 面は訴求だが、一様に塗ると背景画像を覆い尽くす。曲線側を濃く、
        // 基準線側を透明にして、主役を曲線に置いたまま面積を稼ぐ。
        let near = Color32::from_rgba_unmultiplied(
            curve.color.r(),
            curve.color.g(),
            curve.color.b(),
            72,
        );
        let far = Color32::from_rgba_unmultiplied(
            curve.color.r(),
            curve.color.g(),
            curve.color.b(),
            0,
        );
        for index in 0..samples {
            let t = index as f32 / (samples - 1) as f32;
            let x = axes.x_at(t);
            let y_db = (curve.map)(x);
            let point = to_screen(x, y_db);
            line.push(point);
            // 基準線との間を面で埋める。
            let base = to_screen(x, base_at(x));
            let start = mesh.vertices.len() as u32;
            mesh.colored_vertex(point, near);
            mesh.colored_vertex(base, far);
            if index > 0 {
                mesh.add_triangle(start - 2, start - 1, start);
                mesh.add_triangle(start - 1, start, start + 1);
            }
        }
        painter.add(mesh);
        painter.add(egui::Shape::line(line, Stroke::new(1.6, curve.color)));
    }

    // 凡例は基準線より下（曲線が来ない側）へ横に並べる。上へ置くと
    // 最もゲインが大きい領域と重なって読めない。
    let legend_y = to_screen(axes.x.0, base_at(axes.x.0)).y + 6.0;
    let mut legend_x = rect.left() + 6.0;
    for curve in curves {
        let galley = painter.layout_no_wrap(
            curve.label.to_owned(),
            egui::FontId::monospace(10.0),
            curve.color,
        );
        let width = galley.size().x;
        painter.galley(Pos2::new(legend_x, legend_y), galley, curve.color);
        legend_x += width + 12.0;
    }
    painter.text(
        Pos2::new(rect.right() - 4.0, rect.bottom() - 3.0),
        egui::Align2::RIGHT_BOTTOM,
        axis_label,
        egui::FontId::monospace(9.0),
        visuals.weak_text_color(),
    );
}
