//! fandango:crush — ADAA ビットクラッシャー。
//!
//! mid-tread 量子化器に 1 次 ADAA (Antiderivative Anti-Aliasing) を適用し、
//! エイリアシングを抑えた「濁らないローファイ」を作る。DSP は [`dsp`] 参照。

pub mod dsp;
pub mod editor;

use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;

use crate::dsp::CrushChannel;

/// build.rs で注入されるビルド時刻文字列 (`YYYYMMDD-HHMM`)。
const BUILD_TIMESTAMP: &str = env!("BUILD_TIMESTAMP");

#[derive(Params)]
pub struct CrushParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    /// header 右上の電源トグルで操作。true = バイパス中 (エフェクト無効)。
    #[id = "bypass"]
    pub bypass: BoolParam,

    #[id = "bits"]
    pub bits: FloatParam,
    /// **値は `log2(rate)`（オクターブ数 0.0..=5.0）**。実係数は [`rate_from_octaves`] で得る。
    #[id = "rate"]
    pub rate: FloatParam,
    #[id = "mix"]
    pub mix: FloatParam,
}

impl Default for CrushParams {
    fn default() -> Self {
        Self::with_defaults(fandango_shared::param_defaults::STOCK)
    }
}

impl CrushParams {
    /// 既定値を `defaults` 越しに決めて構築する。`Default::default()` は `STOCK` を
    /// 渡すだけなので本体の挙動は変わらない。素材生成 (`crates/shots`) だけが
    /// 差し替えを使う。
    pub fn with_defaults(defaults: &dyn fandango_shared::param_defaults::Defaults) -> Self {
        Self {
            editor_state: EguiState::from_size(editor::WINDOW_W, editor::WINDOW_H),
            bypass: BoolParam::new("bypass", defaults.plain_bool("bypass", false)).make_bypass(),
            bits: FloatParam::new(
                "bits",
                defaults.plain("bits", 8.0),
                FloatRange::Linear {
                    min: 1.0,
                    max: 16.0,
                },
            )
            .with_smoother(SmoothingStyle::Linear(10.0))
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            // **値は `rate` そのものではなく `log2(rate)`（オクターブ数 0.0..=5.0）**。
            // DSP へ渡すときに `2^p` を取る (`rate_from_octaves`)。
            //
            // なぜ log 領域で持つか: 実効レートは `fs / rate` なので、`rate` を線形に
            // 持つと刻みが対数軸で不均等になる（1→2 が 1.00 oct、31→32 が 0.046 oct。
            // 刻みが対数軸で厳密に等比になるようにするため）。
            // `FloatRange::Skewed` は `plain = min + (max-min)·n^(1/factor)` なので
            // `min = 1` のオフセットが効いて `n→0` で傾きが潰れ、**線形より悪化する**
            // （実測: 刻み比 線形 31.5 に対し `skew_factor(-1.0)` で 179.6、
            //  `skew_factor(-2.0)` で 1.7e8）。比 32 のレンジを等比にできる `factor` は無い。
            // 値を log2 で持てば刻み比は厳密に 1.00 になり、手で選ぶ定数も要らない
            // （至上命題 優先順位 3）。恒等点は `p = 0` ⇔ `rate = 1` で厳密。
            rate: FloatParam::new(
                "rate",
                defaults.plain("rate", 0.0),
                FloatRange::Linear { min: 0.0, max: RATE_MAX_OCTAVES },
            )
            .with_value_to_string(Arc::new(|p| format!("{:.2}", rate_from_octaves(p))))
            .with_string_to_value(Arc::new(|s| {
                s.trim().parse::<f32>().ok().map(|r| r.max(1.0).log2())
            })),
            mix: FloatParam::new(
                "mix",
                defaults.plain("mix", 1.0),
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(5.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
        }
    }
}

/// `rate` パラメータの上限 [オクターブ]。`2^5 = 32` で従来の上限と一致する。
const RATE_MAX_OCTAVES: f32 = 5.0;

/// `rate` パラメータ（オクターブ数）から S&H ダウンサンプル係数を得る。
/// `p = 0` で厳密に `1.0`（恒等点）。
#[inline]
fn rate_from_octaves(p: f32) -> f32 {
    p.max(0.0).exp2()
}

pub struct CrushPlugin {
    params: Arc<CrushParams>,
    ch_l: CrushChannel,
    ch_r: CrushChannel,
}

impl Default for CrushPlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(CrushParams::default()),
            ch_l: CrushChannel::new(),
            ch_r: CrushChannel::new(),
        }
    }
}

impl Plugin for CrushPlugin {
    const NAME: &'static str = "fandango - crush";
    const VENDOR: &'static str = "switchworks";
    const URL: &'static str = "https://github.com/switchworks/fandango";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        aux_input_ports: &[],
        aux_output_ports: &[],
        names: PortNames::const_default(),
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(
            self.params.clone(),
            self.params.editor_state.clone(),
            BUILD_TIMESTAMP,
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.ch_l.set_sample_rate(buffer_config.sample_rate);
        self.ch_r.set_sample_rate(buffer_config.sample_rate);
        self.ch_l.reset();
        self.ch_r.reset();
        // 経路 F (区間平均) の因果版は出力が 1 サンプル遅れる。
        // 無報告で飲むとパラレル経路で dry とブレンドしたときに 1 サンプルのコムが出るため、
        // ホストへ報告して補償させる。
        _context.set_latency_samples(1);
        true
    }

    fn reset(&mut self) {
        self.ch_l.reset();
        self.ch_r.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.process_buffer(buffer);
        ProcessStatus::Normal
    }
}

impl CrushPlugin {
    /// process() の本体。ProcessContext を取らないためテストから直接呼べる。
    fn process_buffer(&mut self, buffer: &mut Buffer) {
        // MXCSR に毎回設定。process 冒頭で確実に FTZ/DAZ を効かせる。
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
            let csr = _mm_getcsr();
            _mm_setcsr(csr | 0x8040); // bit 15 = FTZ, bit 6 = DAZ
        }

        for mut ch in buffer.iter_samples() {
            let bits = self.params.bits.smoothed.next();
            let rate = rate_from_octaves(self.params.rate.value());
            let mix = self.params.mix.smoothed.next();
            if self.params.bypass.value() {
                continue;
            }

            let Some(&mut l_in) = ch.get_mut(0) else {
                continue;
            };
            let mono = ch.len() < 2;
            let r_in = if mono {
                l_in
            } else {
                match ch.get_mut(1) {
                    Some(&mut v) => v,
                    None => l_in,
                }
            };

            let (mono_out_l, mono_out_r) = (self.ch_l.process(l_in, bits, rate, mix), self.ch_r.process(r_in, bits, rate, mix));
            if mono {
                if let Some(s) = ch.get_mut(0) {
                    *s = 0.5 * (mono_out_l + mono_out_r);
                }
            } else {
                if let Some(s) = ch.get_mut(0) {
                    *s = mono_out_l;
                }
                if let Some(s) = ch.get_mut(1) {
                    *s = mono_out_r;
                }
            }
        }

    }
}

impl Vst3Plugin for CrushPlugin {
    // 16 バイト固定 ASCII。プラグイン識別子。
    const VST3_CLASS_ID: [u8; 16] = [
        0xee, 0xfc, 0x1d, 0xe5, 0x75, 0xe1, 0x43, 0x51, 0xb0, 0x50, 0xbc, 0x76, 0x7d, 0x3e, 0xe2,
        0x94,
    ];
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Stereo];
}

// VST3 のエクスポートシンボルは cdylib 1 個につき 1 組しか置けない。素材生成
// (`crates/shots`) は 12 crate を 1 バイナリへリンクするので、そこでは切る。
#[cfg(feature = "vst3-export")]
nih_export_vst3!(CrushPlugin);

#[cfg(test)]
mod rate_param_tests {
    use super::*;

    /// 恒等点。`rate` パラメータの下端 `p = 0` は厳密に `rate = 1.0`
    /// （至上命題 優先順位 1）。`exp2` の定義から従うので、実装の副産物ではない。
    #[test]
    fn rate_identity_at_zero_octaves() {
        assert_eq!(rate_from_octaves(0.0), 1.0);
        assert_eq!(rate_from_octaves(-1.0), 1.0, "下限クランプが効いていない");
    }

    /// 上端は従来の `rate = 32` と一致する。
    #[test]
    fn rate_top_matches_legacy_max() {
        assert_eq!(rate_from_octaves(RATE_MAX_OCTAVES), 32.0);
    }

    /// **刻みが対数軸で厳密に等比**であること。
    /// パラメータを `log2(rate)` で持つので `d(log2 rate)/dp = 1` が恒等的に成り立ち、
    /// スライダー上の等距離が実効レートの等比変化になる。
    /// `FloatRange::Skewed` ではこれが作れない（実測: 刻み比 線形 31.5、
    /// `skew_factor(-1.0)` で 179.6、`skew_factor(-2.0)` で 1.7e8）。
    #[test]
    fn rate_steps_are_geometric() {
        let n = 1000;
        let step: Vec<f64> = (0..n)
            .map(|i| {
                let p0 = RATE_MAX_OCTAVES as f64 * i as f64 / n as f64;
                let p1 = RATE_MAX_OCTAVES as f64 * (i + 1) as f64 / n as f64;
                (rate_from_octaves(p1 as f32) as f64).log2()
                    - (rate_from_octaves(p0 as f32) as f64).log2()
            })
            .collect();
        let mx = step.iter().cloned().fold(f64::MIN, f64::max);
        let mn = step.iter().cloned().fold(f64::MAX, f64::min);
        // f32 の丸めぶんだけ許容する。等比なら比は 1.0。
        assert!(mx / mn < 1.01, "刻み比 {:.4} が等比から外れている", mx / mn);
    }

    /// 表示文字列は `rate` の実係数を返す（パラメータ値そのものではない）。
    #[test]
    fn rate_display_shows_actual_factor() {
        let p = CrushParams::default();
        assert_eq!(p.rate.normalized_value_to_string(0.0, false), "1.00");
        assert_eq!(p.rate.normalized_value_to_string(1.0, false), "32.00");
        assert_eq!(p.rate.normalized_value_to_string(0.2, false), "2.00");
    }
}

#[cfg(test)]
mod mono_tests {
    use super::*;

    /// 決定論的テスト信号 (440 Hz / 48 kHz)。
    fn test_signal(n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin() * 0.5)
            .collect()
    }

    /// ホストの initialize 相当。smoother を現在値へ揃えてから返す。
    fn prepared() -> CrushPlugin {
        let mut plugin = CrushPlugin::default();
        let v = plugin.params.bits.value();
        plugin.params.bits.smoothed.reset(v);
        let v = plugin.params.rate.value();
        plugin.params.rate.smoothed.reset(v);
        let v = plugin.params.mix.value();
        plugin.params.mix.smoothed.reset(v);
        plugin.reset();
        plugin
    }

    /// `x` を L=R として与えたステレオ経路の出力。
    fn run_stereo(x: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let mut plugin = prepared();
        let mut chans = vec![x.to_vec(), x.to_vec()];
        {
            let mut buffer = Buffer::default();
            unsafe {
                buffer.set_slices(x.len(), |slices| {
                    let (a, b) = chans.split_at_mut(1);
                    *slices = vec![a[0].as_mut_slice(), b[0].as_mut_slice()];
                });
            }
            plugin.process_buffer(&mut buffer);
        }
        let r = chans.pop().unwrap();
        let l = chans.pop().unwrap();
        (l, r)
    }

    /// `x` をモノ経路 (1ch) に通した出力。
    fn run_mono(x: &[f32]) -> Vec<f32> {
        let mut plugin = prepared();
        let mut chans = vec![x.to_vec()];
        {
            let mut buffer = Buffer::default();
            unsafe {
                buffer.set_slices(x.len(), |slices| {
                    *slices = vec![chans[0].as_mut_slice()];
                });
            }
            plugin.process_buffer(&mut buffer);
        }
        chans.pop().unwrap()
    }

    #[test]
    fn mono_matches_stereo_average() {
        let x = test_signal(4096);
        let mono = run_mono(&x);
        let (l, r) = run_stereo(&x);
        for i in 0..x.len() {
            let expected = 0.5 * (l[i] + r[i]);
            assert!(
                (mono[i] - expected).abs() <= 1e-6,
                "sample {i}: mono={} expected={}",
                mono[i],
                expected
            );
        }
    }

    #[test]
    fn stereo_output_unchanged() {
        let x = test_signal(4096);
        let (l, _r) = run_stereo(&x);
        let window: Vec<f32> = l[2048..2112].to_vec();
        assert_eq!(window, STEREO_L_WINDOW.to_vec());
    }

    /// Step 1 時点 (2026-08-22) のステレオ出力 L のサンプル 2048..2112。
    /// 過渡部を避けた定常部で採る (finalist は lookahead 120 サンプル分が先頭ゼロ)。
    /// 以降の Step でこの値が変わったら音を変えている。
    /// 2026-08-26 更新: ADAA 差分商の f64 化により最大 4.47e-6 (-107 dBFS) 変化。
    /// 変化方向は誤差減少 (dsp.rs `no_click_on_steady_tone_16bit` 参照)。
    /// 2026-08-27 更新: S&H の区間平均化 (経路 F) により**列全体が 1 サンプル後退**。
    /// 変化は純粋なシフトのみで、**1 サンプルずらすと最大絶対差 0e0 (完全一致)**。
    /// 素の最大差 2.89e-2 はシフトによる見かけの値であって波形の変化ではない。
    /// この 1 サンプルは `set_latency_samples(1)` でホストへ報告する。
    /// 2026-08-29 更新: DC ブロッカーのカットオフを閉形式で決め直し
    /// (7.639 Hz → 3.052 Hz、dsp.rs `DC_BLOCK_FC_HZ` 参照) 最大絶対差 5.64e-3
    /// (-44.98 dBFS)。差はコーナーを下げたことによる低域残留の**復帰**のみで、
    /// 差分伝達 `H_new - H_old` は 4.8 Hz で最大 0.429、1 kHz で 4.59e-3。
    /// 新しい成分は生じない (両者とも DC に零点を持つ 1 極 HP)。
    const STEREO_L_WINDOW: [f32; 64] = [
        -0.5032735, -0.4953151, -0.49420518, -0.48707664,
        -0.47961935, -0.4697445, -0.45931435, -0.4471378,
        -0.43355328, -0.41812888, -0.4013772, -0.3836333,
        -0.36454287, -0.3440791, -0.32238555, -0.2999087,
        -0.2763614, -0.25194207, -0.22667679, -0.20024367,
        -0.17386694, -0.14632182, -0.118829295, -0.09030247,
        -0.062143102, -0.033357926, -0.0044720266, 0.024071205,
        0.05288865, 0.08150676, 0.109403685, 0.13775152,
        0.16467793, 0.19202009, 0.21793261, 0.24330717,
        0.2681714, 0.29192632, 0.31481633, 0.33672607,
        0.35703957, 0.376725, 0.39457393, 0.41153976,
        0.4273474, 0.4415776, 0.45359483, 0.4653918,
        0.47452313, 0.48260817, 0.48875168, 0.4931902,
        0.49656594, 0.5006379, 0.50043786, 0.49596667,
        0.49219662, 0.48736528, 0.48083347, 0.4723676,
        0.4628621, 0.45069742, 0.43832365, 0.423746,
    ];
}

#[cfg(test)]
mod nan_tests {
    use super::*;
    use nih_plug::params::Param;

    const PARAM_COUNT: usize = 3;

    /// `idx` (None なら全部) のパラメータを normalized 値 `n` に据えた plugin を作る。
    /// ParamMut が crate 外へ公開されていないため、パラメータ自体を作り直す。
    #[allow(unused_variables, unused_mut)]
    fn prepared_with(idx: Option<usize>, n: f32) -> CrushPlugin {
        let mut p = CrushParams::default();
        if idx.is_none() || idx == Some(0) {
            let r = p.bits.range();
            let v = p.bits.preview_plain(n);
            let nm = p.bits.name().to_owned();
            p.bits = FloatParam::new(nm, v, r);
        }
        if idx.is_none() || idx == Some(1) {
            let r = p.rate.range();
            let v = p.rate.preview_plain(n);
            let nm = p.rate.name().to_owned();
            p.rate = FloatParam::new(nm, v, r);
        }
        if idx.is_none() || idx == Some(2) {
            let r = p.mix.range();
            let v = p.mix.preview_plain(n);
            let nm = p.mix.name().to_owned();
            p.mix = FloatParam::new(nm, v, r);
        }
        let mut plugin = CrushPlugin::default();
        plugin.params = Arc::new(p);
        let v = plugin.params.bits.value();
        plugin.params.bits.smoothed.reset(v);
        let v = plugin.params.rate.value();
        plugin.params.rate.smoothed.reset(v);
        let v = plugin.params.mix.value();
        plugin.params.mix.smoothed.reset(v);
        plugin.reset();
        plugin
    }

    /// denormal 滞留の検出: 非正規化数の非ゼロが連続 1000 サンプル以上続いたら false。
    ///
    /// 閾値は `f32::MIN_POSITIVE` (≈1.18e-38)。CPU を焼くのは非正規化数であって
    /// 単に小さい正規化数ではない (計画書の「1e-30 級」はこの意図の近似表現)。
    /// FTZ/DAZ が効いていれば非正規化数は 0 へ潰れるので、これが出るなら対策漏れ。
    fn no_denormal_run(xs: &[f32]) -> bool {
        let mut run = 0usize;
        for &v in xs {
            if v != 0.0 && v.abs() < f32::MIN_POSITIVE {
                run += 1;
                if run >= 1000 {
                    return false;
                }
            } else {
                run = 0;
            }
        }
        true
    }

    /// 入力パターン: 無音 / フルスケール矩形波 / 極小信号 / 無音へ切替後のテイル。
    /// 第 3 要素は denormal 滞留を判定するか。`tiny` は入力自体が denormal なので
    /// 出力が denormal なのは正常 (恒等通過) であり、判定対象から外す。
    fn hostile_inputs() -> Vec<(&'static str, Vec<f32>, bool)> {
        let n = 4096;
        let square: Vec<f32> = (0..n)
            .map(|i| if (i / 64) % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let tiny: Vec<f32> = (0..n)
            .map(|i| if i % 2 == 0 { 1e-30 } else { -1e-30 })
            .collect();
        let mut tail = square.clone();
        tail.extend(std::iter::repeat(0.0).take(8192));
        vec![
            ("silence", vec![0.0f32; n], true),
            ("square", square, true),
            ("tiny", tiny, false),
            ("square_then_silence", tail, true),
        ]
    }

    #[test]
    fn no_nan_no_denormal() {
        let mut settings: Vec<(String, Option<usize>, f32)> =
            vec![("all_min".into(), None, 0.0), ("all_max".into(), None, 1.0)];
        for idx in 0..PARAM_COUNT {
            settings.push((format!("param{idx}_min"), Some(idx), 0.0));
            settings.push((format!("param{idx}_max"), Some(idx), 1.0));
        }

        for (label, idx, n) in &settings {
            for (name, x, check_denormal) in hostile_inputs() {
                let mut plugin = prepared_with(*idx, *n);
                let mut chans = vec![x.clone(), x.clone()];
                {
                    let mut buffer = Buffer::default();
                    unsafe {
                        buffer.set_slices(x.len(), |slices| {
                            let (a, b) = chans.split_at_mut(1);
                            *slices = vec![a[0].as_mut_slice(), b[0].as_mut_slice()];
                        });
                    }
                    plugin.process_buffer(&mut buffer);
                }
                for (ci, out) in chans.iter().enumerate() {
                    for (i, v) in out.iter().enumerate() {
                        assert!(v.is_finite(), "{label} / {name} / ch{ci} sample {i}: {v}");
                    }
                    if check_denormal {
                        assert!(
                            no_denormal_run(out),
                            "{label} / {name} / ch{ci}: denormal が 1000 サンプル以上滞留"
                        );
                    }
                }
            }
        }
    }
}
