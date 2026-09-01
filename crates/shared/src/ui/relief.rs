//! nimbus の反射場を、CPU 加算合成キャンバスへ描く共有部品。
//!
//! 図式は「ステレオ包絡帯 + 幾何 tap の細線」であり、**個々の拡散 tap は描かない**
//! （`plans/gui_overhaul_v01.md` §4.2）。tap 数が 16 でも 600 でも同じ絵が成立する。
//!
//! `time_scale` / `wet_gain` を**この crate で書き直さない**。DSP 側の関数を呼んだ
//! 結果を [`ReliefInput`] で受け取る（同 §4.2 受け入れ基準）。

use nih_plug_egui::egui::{self, Color32, Pos2, Rect, Stroke, TextureHandle, TextureOptions};

/// 左右の初期反射 tap。秒単位の時刻と符号付き振幅をそのまま保持する。
#[derive(Clone, Copy)]
pub struct ReflectionTap {
    pub left_time: f32,
    pub right_time: f32,
    pub left_gain: f32,
    pub right_gain: f32,
}

/// レリーフ 1 枚分の入力。時刻・ゲインの写像は DSP 側で評価済みの値を受け取る。
#[derive(Clone, Copy)]
pub struct ReliefInput<'a> {
    /// 幾何 16 tap（時刻は圧縮前の素の値。`time_scale` はここで掛ける）。
    pub taps: &'a [ReflectionTap],
    /// 拡散段のエネルギー包絡（L / R）。等間隔ビン、最大 1.0 に正規化済み。
    pub envelope_l: &'a [f32],
    pub envelope_r: &'a [f32],
    /// 拡散段の支持長 [秒]。`distance` では変わらない。
    pub envelope_span: f32,
    /// DSP の `time_scale(r)`。幾何 tap の時刻だけに掛かる。
    pub time_scale: f32,
    /// DSP の `wet_gain(r)`。全体の光量。
    pub wet_gain: f32,
    /// Dry/Wet 比。頂点位置は変えず光量のみ重み付けする。
    pub mix: f32,
    pub accent: Color32,
}

/// 通常の Painter 合成ではなく、線形 RGB の加算結果をテクスチャにして表示する。
#[derive(Default)]
pub struct ReflectionField {
    texture: Option<TextureHandle>,
    linear: Vec<[f32; 3]>,
    supersampled: Vec<[f32; 3]>,
    bloom: Vec<[f32; 3]>,
    blur: Vec<[f32; 3]>,
    rgba: Vec<u8>,
    /// 垂直ブルームの列ごとの移動和（行優先で舐めるための作業領域）。
    column_sums: Vec<[f64; 3]>,
    size: [usize; 2],
    /// 合成キャンバスの解像度（`size` × 現在の SSAA 倍率）。
    render_size: [usize; 2],
    /// 静止後にフル品質で描き直す必要があるか（§4.3 の手段 2）。
    pending_refine: bool,
    last_state: Option<FieldState>,
}

/// 静止時の SSAA 倍率。操作中は 1 に落として再描画予算を守る（§4.3）。
const FULL_SSAA: usize = 2;

#[derive(Clone, Copy, PartialEq)]
struct FieldState {
    time_scale: f32,
    wet_gain: f32,
    mix: f32,
    accent: Color32,
    background: Color32,
}

impl ReflectionField {
    /// 保持しているテクスチャを捨て、次の描画で作り直させる。
    ///
    /// **エディタの `build` コールバックから必ず呼ぶこと。** `nih_plug_egui` の
    /// `user_state` は `Arc<RwLock<T>>` としてエディタを閉じても保持される一方、
    /// `egui::Context` はウィンドウを開くたびに新しく作られる
    /// (`nih_plug_egui/src/editor.rs` の `spawn`)。古い Context に紐づいた
    /// `TextureHandle` の ID を新しい Context で描画しても何も出ないため、
    /// 再オープン時にメイン領域が空白になる。
    pub fn invalidate(&mut self) {
        self.texture = None;
        self.last_state = None;
        self.pending_refine = false;
    }

    /// ステレオ包絡帯（拡散段）に幾何 tap の細線を重ねて描く。
    ///
    /// 面の重なりは線形 RGB に加算してから一度だけトーンマップする。
    /// 壁面や残響尾を示すものではない。
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, input: ReliefInput<'_>) {
        let painter = ui.painter_at(rect);
        let visual = ui.visuals();
        // パネルも枠も敷かない。背景画像とその暗幕がそのまま透けるよう、
        // 発光部だけを alpha 付きで重ねる（背景 = 透明黒）。
        let draw_rect = rect.shrink(5.0);
        let size = [draw_rect.width().round().max(1.0) as usize, draw_rect.height().round().max(1.0) as usize];
        let state = FieldState {
            time_scale: input.time_scale,
            wet_gain: input.wet_gain,
            mix: input.mix,
            accent: input.accent,
            background: Color32::TRANSPARENT,
        };
        // 状態が変わらない限り、合成・CPU コピー・GL 転送のいずれも行わない
        // （§4.3 の受け入れ基準）。テクスチャ生成は最初の 1 回だけ内側で行う。
        //
        // 変化中は SSAA を 1 に落として予算内に収め、静止した次のフレームで
        // フル品質へ描き直す（§4.3 の手段 2）。描き直しは 1 回だけで、
        // その後は状態が変わるまで何もしない。
        let changed = self.size != size || self.last_state != Some(state);
        if changed || self.pending_refine || self.texture.is_none() {
            let ssaa = if changed { 1 } else { FULL_SSAA };
            self.render(size, input, state, ssaa);
            self.pending_refine = ssaa != FULL_SSAA;
            if self.pending_refine {
                ui.ctx().request_repaint();
            }
            self.last_state = Some(state);
            let image = egui::ColorImage::from_rgba_unmultiplied(size, &self.rgba);
            match &mut self.texture {
                Some(texture) => texture.set(image, TextureOptions::LINEAR),
                None => {
                    self.texture = Some(ui.ctx().load_texture(
                        "nimbus-reflection-field",
                        image,
                        TextureOptions::LINEAR,
                    ));
                }
            }
        }
        if let Some(texture) = &self.texture {
            painter.image(
                texture.id(),
                draw_rect,
                egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        let origin = Pos2::new(draw_rect.left() + 12.0, draw_rect.center().y);
        painter.line_segment(
            [
                Pos2::new(origin.x, draw_rect.top() + 14.0),
                Pos2::new(origin.x, draw_rect.bottom() - 14.0),
            ],
            Stroke::new(1.0, visual.widgets.noninteractive.bg_stroke.color),
        );
        painter.text(
            Pos2::new(draw_rect.left() + 3.0, draw_rect.top() + 3.0),
            egui::Align2::LEFT_TOP,
            "REFLECTION FIELD",
            egui::FontId::monospace(9.0),
            visual.weak_text_color(),
        );
        painter.text(
            Pos2::new(origin.x + 4.0, origin.y - 16.0),
            egui::Align2::LEFT_CENTER,
            "L",
            egui::FontId::monospace(9.0),
            visual.weak_text_color(),
        );
        painter.text(
            Pos2::new(origin.x + 4.0, origin.y + 16.0),
            egui::Align2::LEFT_CENTER,
            "R",
            egui::FontId::monospace(9.0),
            visual.weak_text_color(),
        );
        painter.text(
            Pos2::new(origin.x + 3.0, draw_rect.bottom() - 3.0),
            egui::Align2::CENTER_BOTTOM,
            "DIRECT",
            egui::FontId::monospace(9.0),
            visual.strong_text_color(),
        );
        painter.text(
            Pos2::new(draw_rect.right() - 2.0, draw_rect.bottom() - 3.0),
            egui::Align2::RIGHT_BOTTOM,
            "TIME →",
            egui::FontId::monospace(9.0),
            visual.weak_text_color(),
        );
    }

    /// egui を介さずレリーフを合成し、RGBA8 を返す。GUI を開けない環境での
    /// 数値検証・目視用スナップショット生成に使う（描画経路は `show` と同一）。
    pub fn render_rgba(
        &mut self,
        size: [usize; 2],
        input: ReliefInput<'_>,
        background: Color32,
    ) -> &[u8] {
        self.render_rgba_at(size, input, background, FULL_SSAA)
    }

    /// [`Self::render_rgba`] の SSAA 指定版。操作中の低品質パスを測るのに使う。
    pub fn render_rgba_at(
        &mut self,
        size: [usize; 2],
        input: ReliefInput<'_>,
        background: Color32,
        ssaa: usize,
    ) -> &[u8] {
        self.render(
            size,
            input,
            FieldState {
                time_scale: input.time_scale,
                wet_gain: input.wet_gain,
                mix: input.mix,
                accent: input.accent,
                background,
            },
            ssaa,
        );
        &self.rgba
    }

    fn render(
        &mut self,
        size: [usize; 2],
        input: ReliefInput<'_>,
        state: FieldState,
        ssaa: usize,
    ) {
        self.size = size;
        let supersampled_size = [size[0] * ssaa, size[1] * ssaa];
        self.render_size = supersampled_size;
        let pixel_count = size[0] * size[1];
        let supersampled_count = supersampled_size[0] * supersampled_size[1];
        self.linear.resize(pixel_count, [0.0; 3]);
        self.supersampled.resize(supersampled_count, [0.0; 3]);
        self.bloom.resize(pixel_count, [0.0; 3]);
        self.blur.resize(pixel_count, [0.0; 3]);
        self.rgba.resize(pixel_count * 4, 0);
        let background = srgb_to_linear(state.background);
        let bg_alpha = state.background.a();
        self.supersampled.fill(background);

        let light = state.wet_gain * state.mix;
        if light <= 0.0 {
            self.downsample_and_map(size, ssaa, background, bg_alpha);
            return;
        }
        let accent = srgb_to_linear(state.accent);
        let scale_up = ssaa as f32;
        let width = supersampled_size[0] as f32;
        let height = supersampled_size[1] as f32;
        // 左端は DIRECT の軸、右端は余白。
        let (left, right) = relief_bounds(size, ssaa);
        let center = height * 0.5;
        debug_assert!(width > right, "描画幅 {width} が右端 {right} を下回っている");

        // 時間軸は √t。幾何 tap（〜10 ms）と拡散段（300 ms）で 30 倍の開きが
        // あり、線形軸では幾何 tap が左端 3% に潰れて共在署名が読めない。
        // √t は t = 0 で 0、単調増加、原点で有限（対数と違い t = 0 を扱える）。
        let span = input
            .envelope_span
            .max(max_tap_time(input.taps) * input.time_scale)
            .max(1.0e-6);
        let x_of = |t: f32| time_to_x(left, right, span, t);

        // 帯は中央の DIRECT 軸から gap だけ離し、外側へ厚みを持たせる。
        let gap = 5.0 * scale_up;
        let max_thickness = (center - gap - 4.0 * scale_up).max(1.0);

        // 拡散包絡帯。1 列ずつ厚みを決めて塗る（個々の tap は描かない）。
        let columns = (right - left).ceil().max(1.0);
        for column in 0..columns as usize {
            let x = left + column as f32;
            // x → t は √ の逆写像。
            let u = column as f32 / columns;
            let t = u * u * span;
            let u_next = (column as f32 + 1.0) / columns;
            // 1 列が受け持つ時間幅。√軸では右へ行くほど広くなるため、
            // 見かけの明るさが時間幅に比例しないよう密度で補正する。
            let dt = (u_next * u_next * span - t).max(1.0e-9);
            let density = (span / (columns * dt)).sqrt().min(2.0);
            for (envelope, sign) in [(input.envelope_l, -1.0_f32), (input.envelope_r, 1.0)] {
                let value = sample_envelope(envelope, t / span);
                let thickness = value * max_thickness;
                if thickness <= 0.0 {
                    continue;
                }
                let base = center + sign * gap;
                self.add_column(x, base, sign * thickness, accent, light * density * 0.155);
            }
        }

        // 幾何 tap の細線。共在署名として帯の上に重ねる。
        let max_gain = input
            .taps
            .iter()
            .map(|tap| tap.left_gain.abs().max(tap.right_gain.abs()))
            .fold(1.0e-6_f32, f32::max);
        // 基準線は帯の中央高さなので、伸び幅は半分未満に取る。
        // これを超えると細線が帯の外へ出て、端で切れて見える。
        let tap_reach = max_thickness * 0.45;
        for tap in input.taps {
            for (time, gain, sign) in [
                (tap.left_time, tap.left_gain, -1.0_f32),
                (tap.right_time, tap.right_gain, 1.0),
            ] {
                let x = x_of(time * input.time_scale);
                // 極性は伸びる向きで示す。基準線は帯の中央高さ。
                // 帯と同じく L/R は鏡像に置く。`sign` を掛けないと、帯が鏡像なのに
                // 細線だけ上下同じ向きへ伸びて対称性が崩れる。
                let base = center + sign * (gap + max_thickness * 0.5);
                let reach = sign * -gain / max_gain * tap_reach;
                self.add_line(x, base, reach, 1.0 * scale_up, accent, light * 0.85);
            }
        }
        self.downsample_and_map(size, ssaa, background, bg_alpha);
    }

    /// 加算合成の縦帯。厚み方向の外側へ向かって輝度を落とす（縁の勾配）。
    fn add_column(&mut self, x: f32, base: f32, extent: f32, color: [f32; 3], intensity: f32) {
        let [render_w, render_h] = self.render_size;
        let column = x.round() as isize;
        if column < 0 || column >= render_w as isize {
            return;
        }
        let steps = extent.abs().ceil() as usize;
        for step in 0..steps {
            let y = base + extent.signum() * step as f32;
            if y < 0.0 || y >= render_h as f32 {
                continue;
            }
            let u = step as f32 / extent.abs();
            let falloff = 1.0 - u * u;
            let pixel = &mut self.supersampled[y as usize * render_w + column as usize];
            for channel in 0..3 {
                pixel[channel] += color[channel] * intensity * falloff;
            }
        }
    }

    /// 幾何 tap 1 本の細線。`reach` の符号が極性を表す。
    fn add_line(
        &mut self,
        x: f32,
        base: f32,
        reach: f32,
        width: f32,
        color: [f32; 3],
        intensity: f32,
    ) {
        let [render_w, render_h] = self.render_size;
        let half = (width * 0.5).max(0.5);
        let steps = reach.abs().ceil().max(1.0) as usize;
        for step in 0..=steps {
            let y = base + reach.signum() * step as f32;
            if y < 0.0 || y >= render_h as f32 {
                continue;
            }
            let mut offset = -half;
            while offset <= half {
                let column = (x + offset).round();
                if column >= 0.0 && column < render_w as f32 {
                    let pixel = &mut self.supersampled[y as usize * render_w + column as usize];
                    for channel in 0..3 {
                        pixel[channel] += color[channel] * intensity;
                    }
                }
                offset += 1.0;
            }
        }
    }

    /// `ssaa == FULL_SSAA` のときだけブルームを掛ける。操作中の低品質パスでは
    /// 省いて予算を守る（§4.3 の手段 2）。止めた次のフレームでフル品質へ
    /// 描き直されるため、静止画としてのブルームは失われない。
    /// `bg_alpha` は背景の不透明度。0 なら発光の明るさをそのまま alpha に載せ、
    /// 暗い画素は下の背景画像を透かす。255 なら従来どおり不透明画像になる。
    fn downsample_and_map(
        &mut self,
        size: [usize; 2],
        ssaa: usize,
        background: [f32; 3],
        bg_alpha: u8,
    ) {
        let supersampled_w = size[0] * ssaa;
        let weight = 1.0 / (ssaa * ssaa) as f32;
        for y in 0..size[1] {
            for x in 0..size[0] {
                let mut color = [0.0; 3];
                for sy in 0..ssaa {
                    for sx in 0..ssaa {
                        let sample =
                            self.supersampled[(y * ssaa + sy) * supersampled_w + x * ssaa + sx];
                        for channel in 0..3 {
                            color[channel] += sample[channel] * weight;
                        }
                    }
                }
                self.linear[y * size[0] + x] = color;
            }
        }
        if ssaa == FULL_SSAA {
            self.apply_bloom(background);
        }
        for index in 0..size[0] * size[1] {
            let color = self.linear[index];
            let (r, g, b) = (mapped_u8(color[0]), mapped_u8(color[1]), mapped_u8(color[2]));
            // 非プリマルチプライドで返すので、alpha を上げても RGB は変えない。
            let luma = r.max(g).max(b);
            let alpha = bg_alpha as u16 + (255 - bg_alpha as u16) * luma as u16 / 255;
            self.rgba[index * 4..index * 4 + 4].copy_from_slice(&[r, g, b, alpha as u8]);
        }
    }
}

/// 時刻 `t` [秒] のスーパーサンプル座標。時間軸は √t（`render` の注記を参照）。
/// 描画と検証で同じ写像を使うため、内部からもテストからもこの関数を呼ぶ。
pub fn time_to_x(left: f32, right: f32, span: f32, t: f32) -> f32 {
    left + (right - left) * (t / span).clamp(0.0, 1.0).sqrt()
}

/// `render` が使う描画領域の左右端（スーパーサンプル座標）。検証用に公開する。
pub fn relief_bounds(size: [usize; 2], ssaa: usize) -> (f32, f32) {
    let scale = ssaa as f32;
    (12.0 * scale, size[0] as f32 * scale - 6.0 * scale)
}

fn max_tap_time(taps: &[ReflectionTap]) -> f32 {
    taps.iter()
        .map(|tap| tap.left_time.max(tap.right_time))
        .fold(0.0_f32, f32::max)
}

/// 正規化位置 `u ∈ [0,1]` の包絡値。ビン間は線形補間する。
fn sample_envelope(envelope: &[f32], u: f32) -> f32 {
    if envelope.is_empty() || !(0.0..1.0).contains(&u) {
        return 0.0;
    }
    let position = u * (envelope.len() - 1) as f32;
    let index = position.floor() as usize;
    let fraction = position - index as f32;
    let next = envelope.get(index + 1).copied().unwrap_or(0.0);
    envelope[index] + fraction * (next - envelope[index])
}

impl ReflectionField {
    /// しきい値超過分をぼかして加算する。ぼかしは半径 `RADIUS` のボックス
    /// （水平 → 垂直の分離型）であり、端は値を繰り返して埋める。
    ///
    /// 各画素で窓を舐め直すと 1 画素あたり `2·RADIUS+1` 回の加算になり、
    /// 再描画時間の支配項になる（実測 13.2 ms。`plans/gui_overhaul_v01.md` §4.3）。
    /// 移動和にすると窓幅に依存しない 2 演算で同じ平均が出る。累積誤差を
    /// 避けるため和は f64 で持つ。
    fn apply_bloom(&mut self, background: [f32; 3]) {
        const THRESHOLD: f32 = 0.08;
        const RADIUS: isize = 5;
        const STRENGTH: f32 = 1.65;
        let window = (2 * RADIUS + 1) as f64;
        let (w, h) = (self.size[0], self.size[1]);

        for (source, bloom) in self.linear.iter().zip(self.bloom.iter_mut()) {
            for channel in 0..3 {
                bloom[channel] = (source[channel] - background[channel] - THRESHOLD).max(0.0);
            }
        }

        // 水平パス。行内は連続アクセスなので素直に移動和を回す。
        let clamp_x = |x: isize| x.clamp(0, w as isize - 1) as usize;
        for y in 0..h {
            let row = y * w;
            let mut sum = [0.0_f64; 3];
            for dx in -RADIUS..=RADIUS {
                let sample = self.bloom[row + clamp_x(dx)];
                for channel in 0..3 {
                    sum[channel] += sample[channel] as f64;
                }
            }
            for x in 0..w {
                let leaving = self.bloom[row + clamp_x(x as isize - RADIUS)];
                let entering = self.bloom[row + clamp_x(x as isize + RADIUS + 1)];
                for channel in 0..3 {
                    self.blur[row + x][channel] = (sum[channel] / window) as f32;
                    sum[channel] += entering[channel] as f64 - leaving[channel] as f64;
                }
            }
        }

        // 垂直パス。列ごとに縦断するとストライドアクセスでキャッシュを外すため、
        // 幅 w の移動和を行バッファに持ち、y を外側にして行優先で舐める。
        let clamp_y = |y: isize| y.clamp(0, h as isize - 1) as usize;
        self.column_sums.clear();
        self.column_sums.resize(w, [0.0_f64; 3]);
        for dy in -RADIUS..=RADIUS {
            let row = clamp_y(dy) * w;
            for x in 0..w {
                let sample = self.blur[row + x];
                for (sum, value) in self.column_sums[x].iter_mut().zip(sample.iter()) {
                    *sum += *value as f64;
                }
            }
        }
        for y in 0..h {
            let leaving_row = clamp_y(y as isize - RADIUS) * w;
            let entering_row = clamp_y(y as isize + RADIUS + 1) * w;
            for x in 0..w {
                let sum = self.column_sums[x];
                let target = &mut self.linear[y * w + x];
                for channel in 0..3 {
                    target[channel] += (sum[channel] / window) as f32 * STRENGTH;
                }
                let leaving = self.blur[leaving_row + x];
                let entering = self.blur[entering_row + x];
                for channel in 0..3 {
                    self.column_sums[x][channel] +=
                        entering[channel] as f64 - leaving[channel] as f64;
                }
            }
        }
    }
}

fn srgb_to_linear(color: Color32) -> [f32; 3] {
    [color.r(), color.g(), color.b()].map(|component| {
        let value = component as f32 / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    })
}

/// `linear_to_srgb(tone_map(value))` と**厳密に同じ u8** を、`powf` / `exp` を
/// 呼ばずに返す。
///
/// 合成写像は単調増加なので、出力 u8 が `v` になる入力の下限 256 個を
/// 一度だけ求めておけば、以降は二分探索で決まる。近似ではなく逆写像であり、
/// `mapped_u8_matches_the_direct_composition` が全域一致を実測している。
/// 画素あたり 3 回の超越関数呼び出しが再描画時間の支配項だったため
/// （実測 14.3 ms / 59 万回。`plans/gui_overhaul_v01.md` §4.3）。
fn mapped_u8(value: f32) -> u8 {
    static BOUNDS: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    let bounds = BOUNDS.get_or_init(|| {
        let mut table = [0.0_f32; 256];
        for (v, slot) in table.iter_mut().enumerate().skip(1) {
            // u8 の丸め境界 (v − 0.5)/255 を sRGB → linear → tone_map の逆で戻す。
            let srgb = (v as f64 - 0.5) / 255.0;
            let linear = if srgb <= 0.04045 {
                srgb / 12.92
            } else {
                ((srgb + 0.055) / 1.055).powf(2.4)
            };
            // tone_map(x) = 1 − exp(−2.5x) の逆写像。
            *slot = (-(1.0 - linear).ln() / 2.5) as f32;
        }
        table
    });
    // NaN も 0 に落とす（比較が偽になる側へ倒す）。
    if value <= bounds[1] || value.is_nan() {
        return 0;
    }
    (bounds.partition_point(|bound| *bound <= value) - 1) as u8
}

#[allow(dead_code)]
fn linear_to_srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let srgb = if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (srgb * 255.0 + 0.5) as u8
}

#[allow(dead_code)]
fn tone_map(value: f32) -> f32 {
    1.0 - (-value * 2.5).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LUT 版が元の合成写像と全域で一致すること（近似を持ち込んでいないこと）。
    #[test]
    fn mapped_u8_matches_the_direct_composition() {
        let direct = |value: f32| linear_to_srgb(tone_map(value));
        let mut worst = 0.0_f32;
        for step in 0..200_001 {
            let value = step as f32 / 20_000.0;
            let (fast, slow) = (mapped_u8(value), direct(value));
            if fast != slow {
                worst = worst.max(value);
            }
            assert_eq!(fast, slow, "value={value} で {fast} != {slow}");
        }
        assert_eq!(mapped_u8(0.0), direct(0.0));
        assert_eq!(mapped_u8(-1.0), direct(-1.0));
        assert_eq!(mapped_u8(100.0), direct(100.0));
        assert_eq!(worst, 0.0);
    }
}
