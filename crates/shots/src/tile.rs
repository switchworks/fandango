//! 12 crate を 1 枚に並べた 1920×1080 の一覧 (`plans/shots_v01.md` Step 5)。
//!
//! 各タイルは 800×400 を 0.5 縮小した 400×200。横は外周と列間を 64 の 5 分割
//! (64×5 + 400×4 = 1920)、縦はヘッダ 208 + (200 + 行間 64)×3 - 64 + 下 144 = 1080。
//!
//! **タイルの中身は各 crate の `editor::draw` をそのまま縮小したもの**であり、
//! タイル用に描き足したり線幅を変えたりはしない (不変条件 2)。

use std::collections::BTreeMap;

use image::imageops::FilterType;
use image::{Rgba, RgbaImage};
use nih_plug_egui::egui;

use crate::offscreen::{Renderer, Scene};
use crate::targets::Target;

pub const CANVAS_W: u32 = 1920;
pub const CANVAS_H: u32 = 1080;

/// タイル 1 枚の大きさ。800×400 の整数半減。
pub const TILE_W: u32 = 400;
pub const TILE_H: u32 = 200;

/// 外周・列間・行間。横は 64×5 + 400×4 = 1920 でちょうど埋まる。
pub const GAP: u32 = 64;
/// ヘッダ帯の高さ (= タイル群の上端)。文字は帯の中央に置くので、この値を増やすと
/// タイル群が下がり、ヘッダ文字はその半分だけ下がる。
pub const HEADER_H: u32 = 208;

pub const COLS: u32 = 4;
pub const ROWS: u32 = 3;

/// 見た目の調整値。コマンドラインから上書きする。
pub struct Style {
    pub background: Rgba<u8>,
    pub border: Rgba<u8>,
    pub header_text: String,
    pub header_size: f32,
    pub header_color: egui::Color32,
    /// タイル中央へ重ねる crate 名の大きさ (`plans/promo_video_v01.md` §4 型 B)。
    pub name_size: f32,
    /// 同、縁取りの太さ。地色で縁取ると GUI の描画に溶けず文字が立つ。
    pub name_outline: f32,
    pub name_color: egui::Color32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            background: Rgba([0x17, 0x17, 0x17, 0xff]),
            border: Rgba([0x33, 0x33, 0x33, 0xff]),
            header_text: "fandango".to_string(),
            header_size: 112.0,
            header_color: egui::Color32::from_rgb(0xc4, 0xc4, 0xc4),
            name_size: 80.0,
            name_outline: 9.0,
            name_color: egui::Color32::WHITE,
        }
    }
}

/// `set` は crate 名 → (id → 正規化値)。指定のない crate は既定値のまま。
pub fn render(
    renderer: &mut Renderer,
    style: &Style,
    set: &BTreeMap<String, BTreeMap<String, f32>>,
) -> RgbaImage {
    render_with_names(renderer, style, set, &BTreeMap::new())
}

/// crate 名を各タイル中央へ重ねる版 (`plans/promo_video_v01.md` §5.5 C4)。
///
/// `wipe` は crate 名 → ワイプ量 0.0–1.0。**下線から上へ**開く割合で、
/// 0.0 なら何も描かず、1.0 で全部出る。表に無い crate は 0.0 (＝出さない)。
///
/// ffmpeg の `drawtext` にはクリップ矩形が無く、`crop` は式をフレームごとに再評価せず、
/// `overlay` はフレーム端でしか切れない。よって**この描画は shots 側でしか書けない**
/// (`plans/promo_video_v01.md` §10.2 の実測)。egui の `Painter::with_clip_rect` を使う。
pub fn render_with_names(
    renderer: &mut Renderer,
    style: &Style,
    set: &BTreeMap<String, BTreeMap<String, f32>>,
    wipe: &BTreeMap<String, f32>,
) -> RgbaImage {
    assert_eq!(
        Target::ALL.len() as u32,
        COLS * ROWS,
        "タイルは {COLS}×{ROWS} 固定。crate 数が変わったらレイアウトを決め直すこと"
    );

    let mut canvas = RgbaImage::from_pixel(CANVAS_W, CANVAS_H, style.background);

    let empty = BTreeMap::new();
    for (i, target) in Target::ALL.iter().enumerate() {
        let (w, h) = target.window_size();
        renderer.resize(w, h);
        let mut scene = target.scene(set.get(target.name()).unwrap_or(&empty));
        let shot = renderer.frame(scene.as_mut());

        let tile = image::imageops::resize(&shot, TILE_W, TILE_H, FilterType::Lanczos3);
        let col = i as u32 % COLS;
        let row = i as u32 / COLS;
        let x = GAP + col * (TILE_W + GAP);
        let y = HEADER_H + row * (TILE_H + GAP);
        image::imageops::overlay(&mut canvas, &tile, x as i64, y as i64);
        draw_border(&mut canvas, x, y, style.border);
    }

    // ヘッダ帯は egui で描く。プラグイン本体と同じフォントを通すため。
    renderer.resize(CANVAS_W, HEADER_H);
    let mut header = HeaderScene { style };
    let band = renderer.frame(&mut header);
    image::imageops::overlay(&mut canvas, &band, 0, 0);

    // crate 名の重畳。タイルを描いた後の 1 枚に、透明背景で重ねる。
    if wipe.values().any(|v| *v > 0.0) {
        let layer = names_layer(renderer, style, wipe);
        image::imageops::overlay(&mut canvas, &layer, 0, 0);
    }

    canvas
}

/// crate 名だけを透明背景で描いた 1920×1080 の層を返す。
///
/// タイル本体（12 crate の GUI）は掃引しない限りフレーム間で変わらないので、
/// 動画では**下地を 1 枚作って使い回し、この層だけを毎フレーム描く**。
/// 下地を毎フレーム描くと 12 crate を描き直すことになり 30 倍以上遅い
/// (実測 2026-08-31: 下地込み 約 4 秒/フレーム → 層のみ 約 0.1 秒/フレーム)。
pub fn names_layer(
    renderer: &mut Renderer,
    style: &Style,
    wipe: &BTreeMap<String, f32>,
) -> RgbaImage {
    renderer.resize(CANVAS_W, CANVAS_H);
    let mut names = NamesScene { style, wipe };
    renderer.frame_transparent(&mut names)
}

/// タイルの左上座標 (読み順)。
pub fn tile_origin(i: usize) -> (u32, u32) {
    let col = i as u32 % COLS;
    let row = i as u32 / COLS;
    (GAP + col * (TILE_W + GAP), HEADER_H + row * (TILE_H + GAP))
}

/// crate 名を 12 タイルの中央へ重ね、下線から上へワイプする層。
struct NamesScene<'a> {
    style: &'a Style,
    wipe: &'a BTreeMap<String, f32>,
}

impl Scene for NamesScene<'_> {
    fn build(&mut self, ctx: &egui::Context) {
        install_wordmark(ctx);
    }

    fn draw(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::TRANSPARENT))
            .show(ctx, |ui| {
                for (i, target) in Target::ALL.iter().enumerate() {
                    let amount = self.wipe.get(target.name()).copied().unwrap_or(0.0);
                    if amount <= 0.0 {
                        continue;
                    }
                    let amount = amount.min(1.0);
                    let (x, y) = tile_origin(i);
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(x as f32, y as f32),
                        egui::vec2(TILE_W as f32, TILE_H as f32),
                    );
                    // 下線から上へ開く。クリップ矩形の上端だけを動かす。
                    let clip = egui::Rect::from_min_max(
                        egui::pos2(rect.min.x, rect.max.y - rect.height() * amount),
                        rect.max,
                    );
                    let painter = ui.painter().with_clip_rect(clip);
                    let text = target.name().to_uppercase();
                    let font = egui::FontId::new(
                        self.style.name_size,
                        egui::FontFamily::Name(WORDMARK_FAMILY.into()),
                    );
                    // 縁取りは地色で 8 方向へずらして先に描く。
                    // egui の `text` に縁取りの口が無いための実装 (`Stroke` は図形用)。
                    let o = self.style.name_outline;
                    let bg = self.style.background;
                    let outline = egui::Color32::from_rgb(bg[0], bg[1], bg[2]);
                    for (dx, dy) in [
                        (-1.0, 0.0),
                        (1.0, 0.0),
                        (0.0, -1.0),
                        (0.0, 1.0),
                        (-0.7, -0.7),
                        (0.7, -0.7),
                        (-0.7, 0.7),
                        (0.7, 0.7),
                    ] {
                        painter.text(
                            rect.center() + egui::vec2(dx * o, dy * o),
                            egui::Align2::CENTER_CENTER,
                            &text,
                            font.clone(),
                            outline,
                        );
                    }
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        &text,
                        font,
                        self.style.name_color,
                    );
                }
            });
    }
}

/// タイルの外周 1px。0.5 縮小で弱った輪郭を立てる。
fn draw_border(canvas: &mut RgbaImage, x: u32, y: u32, color: Rgba<u8>) {
    for dx in 0..TILE_W + 2 {
        canvas.put_pixel(x + dx - 1, y - 1, color);
        canvas.put_pixel(x + dx - 1, y + TILE_H, color);
    }
    for dy in 0..TILE_H + 2 {
        canvas.put_pixel(x - 1, y + dy - 1, color);
        canvas.put_pixel(x + TILE_W, y + dy - 1, color);
    }
}

struct HeaderScene<'a> {
    style: &'a Style,
}

/// ヘッダ帯だけで使うワードマーク用の書体（`plans/promo_video_v01.md` §4）。
/// **GUI 側の `install_fonts` は触らない。** ここで追加する家族は
/// `HeaderScene` の `FontId` からしか参照されないため、既存 GUI の描画結果は 1 px も変わらない。
pub(crate) const WORDMARK_FAMILY: &str = "wordmark";
const WORDMARK_PATH: &str = "assets/fonts/WDXLLubrifontJPN-Regular.ttf";

/// ワードマーク書体を読み、`WORDMARK_FAMILY` として登録する。
///
/// **無言でフォールバックしない。** 欠落は環境の不備であり、
/// 既定書体で描くと出力が静かに別物になるため、置き場所を示して落とす。
pub(crate) fn install_wordmark(ctx: &egui::Context) {
    let bytes = std::fs::read(WORDMARK_PATH).unwrap_or_else(|e| {
        panic!(
            "ワードマーク書体を読めない: {WORDMARK_PATH}: {e}
             licenses/NON-CARGO.md の手順で取得し、リポジトリ直下からの相対パスに置くこと"
        )
    });
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        WORDMARK_FAMILY.to_owned(),
        std::sync::Arc::new(egui::FontData::from_owned(bytes)),
    );
    fonts.families.insert(
        egui::FontFamily::Name(WORDMARK_FAMILY.into()),
        vec![WORDMARK_FAMILY.to_owned()],
    );
    ctx.set_fonts(fonts);
}

impl Scene for HeaderScene<'_> {
    fn build(&mut self, ctx: &egui::Context) {
        install_wordmark(ctx);
    }

    fn draw(&mut self, ctx: &egui::Context) {
        let bg = self.style.background;
        let fill = egui::Color32::from_rgb(bg[0], bg[1], bg[2]);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(fill))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &self.style.header_text,
                    egui::FontId::new(
                        self.style.header_size,
                        egui::FontFamily::Name(WORDMARK_FAMILY.into()),
                    ),
                    self.style.header_color,
                );
            });
    }
}
