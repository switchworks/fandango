//! パラメータ掃引のアニメーション (`plans/shots_v01.md` Step 4)。
//!
//! 掃引は「往復」「複数パラメータ同時」を含むので、キーフレーム列 + 線形補間で
//! 表す。単方向掃引はキーフレーム 2 点の特殊ケースになる。

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use image::Rgba;

use crate::chain;
use crate::offscreen;
use crate::targets::Target;
use crate::tile;

/// `ffmpeg` の既定の位置。PATH に頼らず絶対パスで叩く。
pub const FFMPEG: &str = r"C:\ffmpeg\bin\ffmpeg.exe";

/// 1 つの時刻と、そこで指定されたパラメータ (id → 正規化値)。
#[derive(Debug)]
pub struct Key {
    pub time: f32,
    pub values: BTreeMap<String, f32>,
}

/// キーフレーム列。時刻の昇順。
#[derive(Debug)]
pub struct Timeline {
    keys: Vec<Key>,
}

impl Timeline {
    /// 時刻順に並べ替えて検査する。書式違反・時刻の重複はエラーで落とす。
    pub fn new(mut keys: Vec<Key>) -> Result<Self, String> {
        if keys.len() < 2 {
            return Err(format!("--key は 2 つ以上要る (受け取った数: {})", keys.len()));
        }
        keys.sort_by(|a, b| a.time.partial_cmp(&b.time).expect("key time is NaN"));
        for pair in keys.windows(2) {
            if pair[0].time == pair[1].time {
                return Err(format!("--key の時刻が重複している: {}", pair[0].time));
            }
        }
        if keys[0].time < 0.0 {
            return Err(format!("--key の時刻は 0 以上: {}", keys[0].time));
        }
        Ok(Self { keys })
    }

    /// 最後のキーの時刻 (秒)。
    pub fn duration(&self) -> f32 {
        self.keys.last().expect("keys is empty").time
    }

    /// この掃引に現れるパラメータ id をすべて集める。
    fn ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self
            .keys
            .iter()
            .flat_map(|k| k.values.keys().map(String::as_str))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// 時刻 `t` におけるパラメータ値。
    ///
    /// 補間はパラメータごとに独立で、そのパラメータを指定しているキーだけを見る
    /// (§8「各キーで指定されたパラメータのみ、その区間で補間される」)。指定範囲の
    /// 外側では端の値を保持する。どのキーにも現れない id は返さない
    /// (= 既定値のまま)。
    pub fn sample(&self, t: f32) -> BTreeMap<String, f32> {
        let mut out = BTreeMap::new();
        for id in self.ids() {
            let points: Vec<(f32, f32)> = self
                .keys
                .iter()
                .filter_map(|k| k.values.get(id).map(|v| (k.time, *v)))
                .collect();
            let value = interpolate(&points, t);
            out.insert(id.to_string(), value);
        }
        out
    }
}

/// 昇順の (時刻, 値) 列を線形補間する。範囲外は端の値を保持する。
fn interpolate(points: &[(f32, f32)], t: f32) -> f32 {
    let (first_t, first_v) = points[0];
    if t <= first_t {
        return first_v;
    }
    for pair in points.windows(2) {
        let ((t0, v0), (t1, v1)) = (pair[0], pair[1]);
        if t <= t1 {
            let a = (t - t0) / (t1 - t0);
            return v0 + (v1 - v0) * a;
        }
    }
    points.last().expect("points is empty").1
}

/// フレーム数。両端を含むので +1。
fn frame_count(timeline: &Timeline, fps: u32) -> u32 {
    assert!(fps > 0, "--fps は 1 以上");
    (timeline.duration() * fps as f32).round() as u32 + 1
}

/// 連番 PNG (`<out>/%05d.png`) と mp4 (`<out>.mp4`) を書く。
pub fn render(
    target: Target,
    timeline: &Timeline,
    texts: &BTreeMap<String, String>,
    fps: u32,
    scale: f32,
    out: &Path,
) {
    let frames = frame_count(timeline, fps);

    let (w, h) = target.window_size();
    let (w, h) = (
        (w as f32 * scale).round() as u32,
        (h as f32 * scale).round() as u32,
    );

    std::fs::create_dir_all(out).unwrap_or_else(|e| panic!("create_dir_all {}: {e}", out.display()));

    offscreen::with_renderer(w, h, |renderer| {
        for i in 0..frames {
            let t = i as f32 / fps as f32;
            let set = timeline.sample(t);
            let mut scene = target.scene_with_text(&set, texts);
            let img = renderer.frame_at(scene.as_mut(), scale);
            offscreen::save_png(&img, &out.join(format!("{i:05}.png")));
        }
    });

    encode(fps, out, frames);
}

/// タイル一覧を動画のフレーム列として出す (`plans/shots_v01.md` §9 `--anim`)。
///
/// `timeline` のパラメータ id は `<crate>:<id>` の形。`base` は掃引に現れない
/// crate / id の固定値 (`--set` 由来) で、キーフレームの値が上書きする。
pub fn render_tile(
    timeline: &Timeline,
    base: &BTreeMap<String, BTreeMap<String, f32>>,
    style: &tile::Style,
    fps: u32,
    out: &Path,
) {
    let frames = frame_count(timeline, fps);

    std::fs::create_dir_all(out).unwrap_or_else(|e| panic!("create_dir_all {}: {e}", out.display()));

    offscreen::with_renderer(tile::CANVAS_W, tile::CANVAS_H, |renderer| {
        for i in 0..frames {
            let t = i as f32 / fps as f32;
            let set = merge_by_crate(base, &timeline.sample(t));
            let canvas = tile::render(renderer, style, &set);
            offscreen::save_png(&canvas, &out.join(format!("{i:05}.png")));
        }
    });

    encode(fps, out, frames);
}

/// 12 本タイルへ crate 名を順次ワイプインさせる (型 B, `plans/promo_video_v01.md` §5.5 C4)。
///
/// n 番目 (読み順) の名前は `start + n·step` から `wipe` 秒かけて下線から上へ開く。
/// 一度出たら消えない。GUI は静止 (C4 は掃引しない、同 §5.5 C4 の裁定)。
pub fn render_tile_names(
    base: &BTreeMap<String, BTreeMap<String, f32>>,
    style: &tile::Style,
    start: f32,
    step: f32,
    wipe: f32,
    duration: f32,
    fps: u32,
    out: &Path,
) {
    assert!(fps > 0, "--fps は 1 以上");
    assert!(wipe > 0.0, "--wipe は 0 より大きい必要がある");
    assert!(duration > 0.0, "--dur は 0 より大きい必要がある");
    let frames = (duration * fps as f32).round() as u32;

    std::fs::create_dir_all(out).unwrap_or_else(|e| panic!("create_dir_all {}: {e}", out.display()));

    offscreen::with_renderer(tile::CANVAS_W, tile::CANVAS_H, |renderer| {
        // 下地（12 crate のタイル + ヘッダ帯）は掃引しないので 1 枚だけ作って使い回す。
        // C4 は掃引しない（`plans/promo_video_v01.md` §5.5 C4 の裁定）ため成立する。
        let base_canvas = tile::render(renderer, style, base);
        for i in 0..frames {
            let t = i as f32 / fps as f32;
            let mut amounts = BTreeMap::new();
            for (n, target) in Target::ALL.iter().enumerate() {
                let at = start + n as f32 * step;
                let amount = ((t - at) / wipe).clamp(0.0, 1.0);
                amounts.insert(target.name().to_string(), amount);
            }
            let mut canvas = base_canvas.clone();
            if amounts.values().any(|v| *v > 0.0) {
                let layer = tile::names_layer(renderer, style, &amounts);
                image::imageops::overlay(&mut canvas, &layer, 0, 0);
            }
            offscreen::save_png(&canvas, &out.join(format!("{i:05}.png")));
        }
    });

    encode(fps, out, frames);
}

/// チェーン表示 (型 C) の連番 PNG と mp4 を書く (`plans/promo_video_v01.md` §5.5 C9)。
///
/// 段は `Stage::at` の時刻に 1 枚ずつ現れる。掃引はしない (C9 の裁定) ので `Timeline` を取らず、
/// 尺は引数で受ける。
pub fn render_chain(stages: &[chain::Stage], background: Rgba<u8>, duration: f32, fps: u32, out: &Path) {
    assert!(fps > 0, "--fps は 1 以上");
    assert!(duration > 0.0, "--dur は 0 より大きい必要がある");
    let frames = (duration * fps as f32).round() as u32;

    std::fs::create_dir_all(out).unwrap_or_else(|e| panic!("create_dir_all {}: {e}", out.display()));

    offscreen::with_renderer(chain::CANVAS_W, chain::CANVAS_H, |renderer| {
        for i in 0..frames {
            let t = i as f32 / fps as f32;
            let canvas = chain::frame_at(renderer, stages, background, t);
            offscreen::save_png(&canvas, &out.join(format!("{i:05}.png")));
        }
    });

    encode(fps, out, frames);
}

/// `<crate>:<id>` 形式の掃引値を crate ごとの表へ畳み、`base` へ重ねる。
/// crate 名の実在は呼び出し側 (CLI のパース) で検査済み。
fn merge_by_crate(
    base: &BTreeMap<String, BTreeMap<String, f32>>,
    sampled: &BTreeMap<String, f32>,
) -> BTreeMap<String, BTreeMap<String, f32>> {
    let mut out = base.clone();
    for (key, value) in sampled {
        let (krate, id) = key
            .split_once(':')
            .unwrap_or_else(|| panic!("tile の掃引 id は <crate>:<id>: {key}"));
        out.entry(krate.to_string())
            .or_default()
            .insert(id.to_string(), *value);
    }
    out
}

/// 連番 PNG を無音 mp4 へ束ねる。`ffmpeg` が無ければエラーで落とす。
fn encode(fps: u32, dir: &Path, frames: u32) {
    if !Path::new(FFMPEG).is_file() {
        panic!("ffmpeg が見つからない: {FFMPEG}");
    }
    let mp4 = dir.with_extension("mp4");
    let status = Command::new(FFMPEG)
        .arg("-y")
        .args(["-framerate", &fps.to_string()])
        .arg("-i")
        .arg(dir.join("%05d.png"))
        .args(["-c:v", "libx264"])
        .args(["-pix_fmt", "yuv420p"])
        .args(["-crf", "18"])
        .arg(&mp4)
        .status()
        .unwrap_or_else(|e| panic!("{FFMPEG} の起動に失敗: {e}"));
    if !status.success() {
        panic!("ffmpeg が異常終了した: {status}");
    }
    println!("{} ({frames} frames)", dir.display());
    println!("{}", mp4.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(time: f32, values: &[(&str, f32)]) -> Key {
        Key {
            time,
            values: values
                .iter()
                .map(|(id, v)| (id.to_string(), *v))
                .collect(),
        }
    }

    #[test]
    fn 往復掃引を線形補間する() {
        let tl = Timeline::new(vec![
            key(0.0, &[("gain", 0.0)]),
            key(2.0, &[("gain", 1.0)]),
            key(4.0, &[("gain", 0.0)]),
        ])
        .expect("Timeline::new");
        assert_eq!(tl.duration(), 4.0);
        assert_eq!(tl.sample(0.0)["gain"], 0.0);
        assert_eq!(tl.sample(1.0)["gain"], 0.5);
        assert_eq!(tl.sample(2.0)["gain"], 1.0);
        assert_eq!(tl.sample(3.0)["gain"], 0.5);
        assert_eq!(tl.sample(4.0)["gain"], 0.0);
    }

    #[test]
    fn 指定のないキーでは端の値を保持する() {
        let tl = Timeline::new(vec![
            key(0.0, &[("gain", 0.0), ("pivot", 0.5)]),
            key(2.0, &[("gain", 1.0)]),
        ])
        .expect("Timeline::new");
        // pivot は 1 点しかないので全区間で 0.5。
        assert_eq!(tl.sample(0.0)["pivot"], 0.5);
        assert_eq!(tl.sample(2.0)["pivot"], 0.5);
        assert_eq!(tl.sample(1.0)["gain"], 0.5);
    }

    #[test]
    fn タイルの掃引値はcrateごとに畳まれる() {
        let mut base = BTreeMap::new();
        base.insert(
            "tilt".to_string(),
            BTreeMap::from([("slope".to_string(), 0.2)]),
        );
        let sampled = BTreeMap::from([
            ("tilt:gain".to_string(), 0.75),
            ("crush:bits".to_string(), 0.1),
        ]);
        let merged = merge_by_crate(&base, &sampled);
        assert_eq!(merged["tilt"]["slope"], 0.2);
        assert_eq!(merged["tilt"]["gain"], 0.75);
        assert_eq!(merged["crush"]["bits"], 0.1);
    }

    #[test]
    fn 時刻の重複はエラー() {
        let err = Timeline::new(vec![key(1.0, &[("gain", 0.0)]), key(1.0, &[("gain", 1.0)])])
            .expect_err("重複が通ってしまった");
        assert!(err.contains("重複"), "{err}");
    }

    #[test]
    fn キーが1つならエラー() {
        Timeline::new(vec![key(0.0, &[("gain", 0.0)])]).expect_err("1 点が通ってしまった");
    }
}
