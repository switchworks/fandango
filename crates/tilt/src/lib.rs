//! fandango:tilt — Baxandall 型ティルト EQ。
//!
//! 低/高シェルフを pivot 周波数中心に対称連動させ、gain 1 ノブで音色を
//! シーソー反転する完全線形フィルタ。DSP は [`dsp`] モジュール参照。

pub mod dsp;
pub mod editor;

use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;

use crate::dsp::{high_shelf_coeffs, low_shelf_coeffs, BiquadCoeffs, TiltChannel};

/// build.rs で注入されるビルド時刻文字列 (`YYYYMMDD-HHMM`)。
const BUILD_TIMESTAMP: &str = env!("BUILD_TIMESTAMP");

#[derive(Params)]
pub struct TiltParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    /// header 右上の電源トグルで操作。true = バイパス中 (エフェクト無効)。
    #[id = "bypass"]
    pub bypass: BoolParam,

    #[id = "gain"]
    pub gain: FloatParam,
    #[id = "pivot"]
    pub pivot: FloatParam,
    #[id = "slope"]
    pub slope: FloatParam,
}

impl Default for TiltParams {
    fn default() -> Self {
        Self::with_defaults(fandango_shared::param_defaults::STOCK)
    }
}

impl TiltParams {
    /// 既定値を `defaults` 越しに決めて構築する。`Default::default()` は
    /// `STOCK` を渡すだけなので本体の挙動は変わらない。素材生成
    /// (`crates/shots`) だけが差し替えを使う。
    pub fn with_defaults(defaults: &dyn fandango_shared::param_defaults::Defaults) -> Self {
        Self {
            editor_state: EguiState::from_size(editor::WINDOW_W, editor::WINDOW_H),
            bypass: BoolParam::new("bypass", defaults.plain_bool("bypass", false)).make_bypass(),
            gain: FloatParam::new("gain", defaults.plain("gain", 0.0), FloatRange::Linear { min: -12.0, max: 12.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_unit(" dB")
                .with_value_to_string(formatters::v2s_f32_rounded(1)),
            pivot: FloatParam::new(
                "pivot",
                defaults.plain("pivot", 700.0),
                FloatRange::Skewed { min: 100.0, max: 5000.0, factor: FloatRange::skew_factor(-2.0) },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit(" hz")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            slope: FloatParam::new("slope", defaults.plain("slope", 0.7), FloatRange::Linear { min: 0.3, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_value_to_string(formatters::v2s_f32_rounded(2)),
        }
    }
}

pub struct TiltPlugin {
    params: Arc<TiltParams>,
    sr: f32,
    chain_l: TiltChannel,
    chain_r: TiltChannel,
    ls_coeffs: BiquadCoeffs,
    hs_coeffs: BiquadCoeffs,
    last_gain: f32,
    last_pivot: f32,
    last_slope: f32,
}

impl Default for TiltPlugin {
    fn default() -> Self {
        let sr = 48_000.0;
        let last_gain = 0.0;
        let last_pivot = 700.0;
        let last_slope = 0.7;
        Self {
            params: Arc::new(TiltParams::default()),
            sr,
            chain_l: TiltChannel::new(),
            chain_r: TiltChannel::new(),
            ls_coeffs: low_shelf_coeffs(sr, last_pivot, -last_gain, last_slope),
            hs_coeffs: high_shelf_coeffs(sr, last_pivot, last_gain, last_slope),
            last_gain,
            last_pivot,
            last_slope,
        }
    }
}

impl Plugin for TiltPlugin {
    const NAME: &'static str = "fandango - tilt";
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
        self.sr = buffer_config.sample_rate;
        self.chain_l.reset();
        self.chain_r.reset();
        self.last_gain = self.params.gain.value();
        self.last_pivot = self.params.pivot.value();
        self.last_slope = self.params.slope.value();
        self.ls_coeffs = low_shelf_coeffs(self.sr, self.last_pivot, -self.last_gain, self.last_slope);
        self.hs_coeffs = high_shelf_coeffs(self.sr, self.last_pivot, self.last_gain, self.last_slope);
        true
    }

    fn reset(&mut self) {
        self.chain_l.reset();
        self.chain_r.reset();
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

impl TiltPlugin {
    /// process() の本体。ProcessContext を取らないためテストから直接呼べる。
    fn process_buffer(&mut self, buffer: &mut Buffer) {
        // MXCSR に毎回設定。process 冒頭で確実に FTZ/DAZ を効かせる。
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
            let csr = _mm_getcsr();
            _mm_setcsr(csr | 0x8040); // bit 15 = FTZ, bit 6 = DAZ
        }

        let block_len = buffer.samples().max(1) as u32;
        let gain = self.params.gain.smoothed.next_step(block_len);
        let pivot = self.params.pivot.smoothed.next_step(block_len);
        let slope = self.params.slope.smoothed.next_step(block_len);

        if gain != self.last_gain || pivot != self.last_pivot || slope != self.last_slope {
            self.ls_coeffs = low_shelf_coeffs(self.sr, pivot, -gain, slope);
            self.hs_coeffs = high_shelf_coeffs(self.sr, pivot, gain, slope);
            self.last_gain = gain;
            self.last_pivot = pivot;
            self.last_slope = slope;
        }

        for mut ch in buffer.iter_samples() {
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

            let l_out = self.chain_l.process(l_in, &self.ls_coeffs, &self.hs_coeffs);
            let r_out = self.chain_r.process(r_in, &self.ls_coeffs, &self.hs_coeffs);

            let (mono_out_l, mono_out_r) = (l_out, r_out);
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

impl Vst3Plugin for TiltPlugin {
    // 16 バイト固定 ASCII。プラグイン識別子。
    const VST3_CLASS_ID: [u8; 16] = [0x3d,0x8a,0x6e,0x6c,0xd5,0x01,0x4b,0xf9,0x89,0xbc,0xad,0x34,0x58,0x4e,0xb9,0x88];
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Stereo];
}

// VST3 のエクスポートシンボルは cdylib 1 個につき 1 組しか置けない。素材生成
// (`crates/shots`) は 12 crate を 1 バイナリへリンクするので、そこでは切る。
#[cfg(feature = "vst3-export")]
nih_export_vst3!(TiltPlugin);

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
    fn prepared() -> TiltPlugin {
        let mut plugin = TiltPlugin::default();
        let v = plugin.params.gain.value();
        plugin.params.gain.smoothed.reset(v);
        let v = plugin.params.pivot.value();
        plugin.params.pivot.smoothed.reset(v);
        let v = plugin.params.slope.value();
        plugin.params.slope.smoothed.reset(v);
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
    const STEREO_L_WINDOW: [f32; 64] = [
        -0.49463502, -0.48961106, -0.48296288, -0.47471073,
        -0.4648863, -0.4535203, -0.44065213, -0.4263211,
        -0.41057196, -0.39346528, -0.37505376, -0.3554012,
        -0.33456752, -0.31261837, -0.28963828, -0.2656977,
        -0.24087934, -0.21525893, -0.18891759, -0.16195662,
        -0.13445856, -0.10651831, -0.078221165, -0.049657017,
        -0.020935737, 0.007854972, 0.036615826, 0.06525907,
        0.09369339, 0.12180944, 0.14952153, 0.17673424,
        0.20336452, 0.22932717, 0.25452238, 0.2788735,
        0.30229685, 0.3247267, 0.3460736, 0.3662729,
        0.38525757, 0.40296236, 0.41933745, 0.43431744,
        0.44785714, 0.4599116, 0.47043964, 0.4794112,
        0.48679033, 0.49255523, 0.49668667, 0.49917096,
        0.49999994, 0.4991706, 0.49668586, 0.49255404,
        0.4867889, 0.47940832, 0.47043884, 0.45990932,
        0.44785464, 0.43431482, 0.4193326, 0.4029618,
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
    fn prepared_with(idx: Option<usize>, n: f32) -> TiltPlugin {
        let mut p = TiltParams::default();
        if idx.is_none() || idx == Some(0) {
            let r = p.gain.range();
            let v = p.gain.preview_plain(n);
            let nm = p.gain.name().to_owned();
            p.gain = FloatParam::new(nm, v, r);
        }
        if idx.is_none() || idx == Some(1) {
            let r = p.pivot.range();
            let v = p.pivot.preview_plain(n);
            let nm = p.pivot.name().to_owned();
            p.pivot = FloatParam::new(nm, v, r);
        }
        if idx.is_none() || idx == Some(2) {
            let r = p.slope.range();
            let v = p.slope.preview_plain(n);
            let nm = p.slope.name().to_owned();
            p.slope = FloatParam::new(nm, v, r);
        }
        let mut plugin = TiltPlugin::default();
        plugin.params = Arc::new(p);
        let v = plugin.params.gain.value();
        plugin.params.gain.smoothed.reset(v);
        let v = plugin.params.pivot.value();
        plugin.params.pivot.smoothed.reset(v);
        let v = plugin.params.slope.value();
        plugin.params.slope.smoothed.reset(v);
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
