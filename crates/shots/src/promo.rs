//! Gumroad 商品ページ用のカバー / サムネイル。
//!
//! **Gumroad の商品説明 HTML には画像を出せない**（`crates/copy/src/render_html.rs`
//! 不変条件 5: サニタイザが `<img>` を通さない）。したがって絵はカバー機能へ
//! 載せるしかなく、本モジュールがその 2 枚を生成する。
//!
//! 書体は紹介動画のテロップと同じ WDXL Lubrifont JP N を使う
//! （`tile::WORDMARK_PATH`）。動画・タイル・カバーで字面を揃えるため。

use image::RgbaImage;
use nih_plug_egui::egui;

use crate::offscreen::{Renderer, Scene};
use crate::targets::Target;
use crate::tile::{install_wordmark, WORDMARK_FAMILY};

/// カバー。Gumroad の最低要件は 1280×720 だが、床であって目標ではない。
/// 2× 表示が前提の環境で眠くならないよう 1920×1080（紹介動画と同じフレーム）で出す。
pub const COVER_W: u32 = 1920;
pub const COVER_H: u32 = 1080;

/// サムネイル。要件は正方形 600×600 以上。同じ理由で 2 倍を採る。
pub const THUMB: u32 = 1200;

/// 左右に必ず残す余白の比率。**0 にしない。**
/// Gumroad 側でカバーが角丸・トリミングされても文字が欠けないための逃げ。
const MARGIN_RATIO: f32 = 0.10;

/// 地色。`tile::Style::default()` と同じ。
const BG: egui::Color32 = egui::Color32::from_rgb(0x17, 0x17, 0x17);
/// ワードマーク。Radix gray-dark の高コントラスト側。
const FG: egui::Color32 = egui::Color32::from_rgb(0xED, 0xED, 0xED);
/// 英語タグライン。
const FG_SUB: egui::Color32 = egui::Color32::from_rgb(0xB4, 0xB4, 0xB4);
/// 日本語タグラインと byline。従属要素なので落とす。
const FG_DIM: egui::Color32 = egui::Color32::from_rgb(0x7A, 0x7A, 0x7A);

/// 無彩色の罫。**バンドルは中立の器で、色相は各プラグインが持つ**、という立場のとき用。
const RULE_NEUTRAL: egui::Color32 = egui::Color32::from_rgb(0x8A, 0x8A, 0x8A);

/// 罫の塗り方。
///
/// **単色のアクセントは選択肢に置いていない。** 12 本はそれぞれ固有の色相を持っており
/// （`fandango_shared::ui::Palette`）、`Tomato` は `gate` の色である。
/// バンドル全体を表すカバーにそれを置くと、1 本の色相をバンドルの代表へ昇格させてしまう。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RuleStyle {
    /// 無彩色 1 本。
    Neutral,
    /// 12 本ぶんの実際のアクセント色を crate 順に並べる。
    /// **色を選ばずデータから導く**ので「なぜこの色か」が発生しない。
    Spectrum,
}

impl RuleStyle {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "neutral" => Some(RuleStyle::Neutral),
            "spectrum" => Some(RuleStyle::Spectrum),
            _ => None,
        }
    }
}

/// crate 順（`Target::ALL` と同順）のアクセント色。
///
/// `targets.rs` は palette を持たないため、ここで対応を書く。
/// **`Target::ALL` と長さが違えば落ちる**ので、crate を足したときに黙ってずれない。
fn accents() -> Vec<egui::Color32> {
    use fandango_shared::ui::Palette;
    let order = [
        Palette::Orange,  // crush
        Palette::Teal,    // drawl
        Palette::Amber,   // finalist
        Palette::Tomato,  // gate
        Palette::Crimson, // nice-bulk
        Palette::Blue,    // nimbus
        Palette::Indigo,  // raindrop
        Palette::Jade,    // rotate
        Palette::Cyan,    // spread
        Palette::Amber,   // tilt
        Palette::Plum,    // trip-delay
        Palette::Violet,  // velvet
    ];
    assert_eq!(
        order.len(),
        Target::ALL.len(),
        "アクセント表と Target::ALL の本数が違う。crate を足したら本表も更新すること"
    );
    order.iter().map(|p| p.data().accent[8]).collect()
}

const WORDMARK: &str = "fandango";
const TAGLINE_EN: &str = "Closed form. So it never breaks.";
const TAGLINE_JA: &str = "式で閉じる。だから、壊れない。";
const BYLINE: &str = "by switchworks";

/// `text` が `max_w` に収まる `FontId` を返す。
///
/// **推測でサイズを決め打ちしない。** 字幅は書体と字種（英語 / 日本語）で変わるため、
/// 実際に組んで測る。幅はサイズにほぼ比例するので 1 回の補正でほぼ収束するが、
/// 収束を仮定せず上限回数まで詰める。
///
/// 縮小しかしないので、**過大な `ideal` を渡すと「収まる最大」まで詰める**動きになる。
fn fit(ctx: &egui::Context, text: &str, max_w: f32, ideal: f32) -> egui::FontId {
    let family = egui::FontFamily::Name(WORDMARK_FAMILY.into());
    let mut size = ideal;
    for _ in 0..8 {
        let id = egui::FontId::new(size, family.clone());
        let w = ctx.fonts(|f| {
            f.layout_no_wrap(text.to_owned(), id.clone(), egui::Color32::WHITE)
                .size()
                .x
        });
        if w <= max_w {
            return id;
        }
        size *= (max_w / w) * 0.99;
    }
    egui::FontId::new(size, family)
}

/// 中央寄せで縦に積む 1 要素。
enum Item {
    Text {
        text: &'static str,
        /// 収まらなければ縮める上限幅の比率（コンテンツ幅に対する）。
        width_ratio: f32,
        ideal: f32,
        color: egui::Color32,
        /// 積む高さ。実測の字面高ではなく版面上の行送りとして与える。
        height: f32,
    },
    /// 罫。幅は**直前に描いたテキストの実測幅**に対する比率で決める。
    /// 固定値だとワードマークのサイズを変えたときに比率が崩れる。
    Rule { ratio: f32, height: f32 },
    Gap(f32),
}

struct Stack {
    items: Vec<Item>,
    canvas_w: f32,
    canvas_h: f32,
    rule: RuleStyle,
}

impl Stack {
    fn draw(&self, ctx: &egui::Context) {
        let content_w = self.canvas_w * (1.0 - 2.0 * MARGIN_RATIO);
        let total: f32 = self
            .items
            .iter()
            .map(|i| match i {
                Item::Text { height, .. } => *height,
                Item::Rule { height, .. } => *height,
                Item::Gap(g) => *g,
            })
            .sum();

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(BG))
            .show(ctx, |ui| {
                let p = ui.painter();
                let cx = self.canvas_w * 0.5;
                let mut y = (self.canvas_h - total) * 0.5;
                // 直前に描いたテキストの実測幅。罫の幅がこれに従う。
                let mut last_w = self.canvas_w * 0.5;
                for item in &self.items {
                    match item {
                        Item::Text {
                            text,
                            width_ratio,
                            ideal,
                            color,
                            height,
                        } => {
                            let font = fit(ctx, text, content_w * width_ratio, *ideal);
                            last_w = ctx.fonts(|f| {
                                f.layout_no_wrap(
                                    (*text).to_owned(),
                                    font.clone(),
                                    egui::Color32::WHITE,
                                )
                                .size()
                                .x
                            });
                            p.text(
                                egui::pos2(cx, y + height * 0.5),
                                egui::Align2::CENTER_CENTER,
                                text,
                                font,
                                *color,
                            );
                            y += height;
                        }
                        Item::Rule { ratio, height } => {
                            let width = last_w * ratio;
                            let x0 = cx - width * 0.5;
                            match self.rule {
                                RuleStyle::Neutral => {
                                    p.rect_filled(
                                        egui::Rect::from_min_size(
                                            egui::pos2(x0, y),
                                            egui::vec2(width, *height),
                                        ),
                                        0.0,
                                        RULE_NEUTRAL,
                                    );
                                }
                                RuleStyle::Spectrum => {
                                    let cols = accents();
                                    let n = cols.len() as f32;
                                    for (i, c) in cols.iter().enumerate() {
                                        // 区画の境界を実数で求めてから塗る。端数を最後へ
                                        // 寄せると罫の右端が縮むので、両端を必ず一致させる。
                                        let a = x0 + width * (i as f32) / n;
                                        let b = x0 + width * (i as f32 + 1.0) / n;
                                        p.rect_filled(
                                            egui::Rect::from_min_size(
                                                egui::pos2(a, y),
                                                egui::vec2(b - a, *height),
                                            ),
                                            0.0,
                                            *c,
                                        );
                                    }
                                }
                            }
                            y += height;
                        }
                        Item::Gap(g) => y += g,
                    }
                }
            });
    }
}

struct StackScene(Stack);

impl Scene for StackScene {
    fn build(&mut self, ctx: &egui::Context) {
        install_wordmark(ctx);
    }
    fn draw(&mut self, ctx: &egui::Context) {
        self.0.draw(ctx);
    }
}

/// カバー 1920×1080。ワードマーク + 罫 + 英日タグライン + byline。
///
/// byline を入れるのは、**カバーがページから切り離されて一人歩きするため**
/// （OGP のシェアカード、KVR の掲載画像、記事のサムネイル）。
/// 商品ページ上では Gumroad 側の表示と重複するが、単体で出たときに作者が消える方が損。
pub fn render_cover(renderer: &mut Renderer, rule: RuleStyle) -> RgbaImage {
    renderer.resize(COVER_W, COVER_H);
    let mut scene = StackScene(Stack {
        canvas_w: COVER_W as f32,
        canvas_h: COVER_H as f32,
        rule,
        items: vec![
            Item::Text {
                text: WORDMARK,
                // コンテンツ幅の 0.86 まで詰める。`ideal` は過大値で、`fit` の縮小に
                // 幅から逆算させる（サムネイルと同じ手）。
                width_ratio: 0.86,
                ideal: 1000.0,
                color: FG,
                // `g` のディセンダが罫に当たらないよう、字面より広く取る。
                height: 472.0,
            },
            Item::Gap(38.0),
            Item::Rule {
                ratio: 0.62,
                height: 8.0,
            },
            Item::Gap(64.0),
            Item::Text {
                text: TAGLINE_EN,
                width_ratio: 1.0,
                ideal: 78.0,
                color: FG_SUB,
                height: 90.0,
            },
            Item::Gap(14.0),
            Item::Text {
                text: TAGLINE_JA,
                width_ratio: 0.9,
                ideal: 56.0,
                color: FG_DIM,
                height: 70.0,
            },
            Item::Gap(64.0),
            Item::Text {
                text: BYLINE,
                width_ratio: 0.6,
                ideal: 42.0,
                color: FG_DIM,
                height: 50.0,
            },
        ],
    });
    renderer.frame(&mut scene)
}

/// サムネイル 1200×1200。**ワードマークと罫だけ。**
///
/// 一覧グリッドでは 100 px 前後まで縮む。その寸法でタグラインは 4 px 程度の字面になり
/// 潰れて読めないだけなので、載せない。byline も同じ理由で入れない。
pub fn render_thumb(renderer: &mut Renderer, rule: RuleStyle) -> RgbaImage {
    renderer.resize(THUMB, THUMB);
    let mut scene = StackScene(Stack {
        canvas_w: THUMB as f32,
        canvas_h: THUMB as f32,
        rule,
        items: vec![
            Item::Text {
                text: WORDMARK,
                width_ratio: 1.0,
                // **意図的に過大な値を渡している。** `fit` は縮小しかしないので、
                // 収まらない値から入れることで「余白を残して最大まで詰める」動きになる。
                // サムネイルは一覧で 100 px 前後まで縮むため、字面を稼ぐ方を優先する。
                ideal: 1000.0,
                color: FG,
                height: 352.0,
            },
            Item::Gap(38.0),
            Item::Rule {
                ratio: 0.62,
                height: 9.0,
            },
        ],
    });
    renderer.frame(&mut scene)
}
