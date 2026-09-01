//! fandango:raindrop — 極短窓 IIR comb + Haas-shuffle + WIDTH。

pub mod dsp;
pub mod editor;

use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;

use crate::dsp::{RaindropDsp, RaindropParams};

/// build.rs で注入されるビルド時刻文字列 (`YYYYMMDD-HHMM`)。
const BUILD_TIMESTAMP: &str = env!("BUILD_TIMESTAMP");

#[derive(Params)]
pub struct RaindropParamsNih {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    /// header 右上の電源トグルで操作。true = バイパス中 (エフェクト無効)。
    #[id = "bypass"]
    pub bypass: BoolParam,

    /// 遅延時間 (ms)。
    #[id = "time"]
    pub time: FloatParam,
    /// フィードバック 0..0.90。
    #[id = "feedback"]
    pub feedback: FloatParam,
    /// WIDTH 0..1 (0=Mid コア, 1=ステレオ独立)。
    #[id = "width"]
    pub width: FloatParam,
    /// MIX 0..1 (Dry/Wet)。
    #[id = "mix"]
    pub mix: FloatParam,
}

impl Default for RaindropParamsNih {
    fn default() -> Self {
        Self::with_defaults(fandango_shared::param_defaults::STOCK)
    }
}

impl RaindropParamsNih {
    /// 既定値を `defaults` 越しに決めて構築する。`Default::default()` は `STOCK` を
    /// 渡すだけなので本体の挙動は変わらない。素材生成 (`crates/shots`) だけが
    /// 差し替えを使う。
    pub fn with_defaults(defaults: &dyn fandango_shared::param_defaults::Defaults) -> Self {
        Self {
            editor_state: EguiState::from_size(editor::WINDOW_W, editor::WINDOW_H),
            bypass: BoolParam::new("bypass", defaults.plain_bool("bypass", false)).make_bypass(),
            time: FloatParam::new(
                "Time",
                defaults.plain("time", 75.0),
                FloatRange::Linear {
                    min: 10.0,
                    max: 200.0,
                },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
            feedback: FloatParam::new(
                "Feedback",
                defaults.plain("feedback", 0.50),
                FloatRange::Linear {
                    min: 0.0,
                    max: 0.90,
                },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            width: FloatParam::new(
                "Width",
                defaults.plain("width", 0.0),
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            mix: FloatParam::new(
                "Mix",
                defaults.plain("mix", 0.50),
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
        }
    }
}

pub struct RaindropPlugin {
    params: Arc<RaindropParamsNih>,
    sr: f32,
    dsp: RaindropDsp,
}

impl Default for RaindropPlugin {
    fn default() -> Self {
        let sr = 48_000.0;
        let max_delay = max_delay_samples(sr);
        Self {
            params: Arc::new(RaindropParamsNih::default()),
            sr,
            dsp: RaindropDsp::new(max_delay),
        }
    }
}

fn max_delay_samples(sr: f32) -> usize {
    // TIME 最大 200 ms + 余裕
    ((sr * 0.220) as usize).max(1024)
}

impl Plugin for RaindropPlugin {
    const NAME: &'static str = "fandango - raindrop";
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
        _layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sr = buffer_config.sample_rate;
        self.dsp.set_sample_rate(self.sr);
        self.dsp.resize(max_delay_samples(self.sr));
        true
    }

    fn reset(&mut self) {
        self.dsp.reset();
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

impl RaindropPlugin {
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
            let time_ms = self.params.time.smoothed.next();
            let feedback = self.params.feedback.smoothed.next();
            let width = self.params.width.smoothed.next();
            let mix = self.params.mix.smoothed.next();
            if self.params.bypass.value() {
                continue;
            }

            let delay_samples = ((time_ms * 1e-3) * self.sr).round() as usize;
            let p = RaindropParams {
                delay_samples: delay_samples.max(1),
                feedback,
                width,
                mix,
            };

            let Some(&mut l) = ch.get_mut(0) else {
                continue;
            };
            let mono = ch.len() < 2;
            let r = if mono {
                l
            } else {
                match ch.get_mut(1) {
                    Some(&mut v) => v,
                    None => l,
                }
            };
            let (out_l, out_r) = self.dsp.process_sample(l, r, p);
            let (mono_out_l, mono_out_r) = (out_l, out_r);
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

impl Vst3Plugin for RaindropPlugin {
    const VST3_CLASS_ID: [u8; 16] = [
        0x72, 0x15, 0x79, 0xd0, 0x20, 0x06, 0x48, 0x81, 0x88, 0x17, 0xad, 0xab, 0x68, 0xd4, 0xa3,
        0x32,
    ];
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Delay];
}

// VST3 のエクスポートシンボルは cdylib 1 個につき 1 組しか置けない。素材生成
// (`crates/shots`) は 12 crate を 1 バイナリへリンクするので、そこでは切る。
#[cfg(feature = "vst3-export")]
nih_export_vst3!(RaindropPlugin);

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
    fn prepared() -> RaindropPlugin {
        let mut plugin = RaindropPlugin::default();
        let v = plugin.params.time.value();
        plugin.params.time.smoothed.reset(v);
        let v = plugin.params.feedback.value();
        plugin.params.feedback.smoothed.reset(v);
        let v = plugin.params.width.value();
        plugin.params.width.smoothed.reset(v);
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
    const STEREO_L_WINDOW: [f32; 64] = [
        -0.49463582, -0.48961186, -0.48296368, -0.47471154,
        -0.4648871, -0.45352104, -0.44065282, -0.4263217,
        -0.41057244, -0.39346567, -0.3750541, -0.35540143,
        -0.33456767, -0.31261846, -0.28963822, -0.2656975,
        -0.24087903, -0.21525851, -0.18891709, -0.16195607,
        -0.13445798, -0.106517725, -0.07822057, -0.049656425,
        -0.020935144, 0.007855561, 0.03661641, 0.06525965,
        0.093693964, 0.12181001, 0.14952211, 0.1767348,
        0.20336506, 0.22932768, 0.25452286, 0.27887398,
        0.30229726, 0.32472703, 0.34607387, 0.36627308,
        0.38525763, 0.40296236, 0.4193374, 0.4343173,
        0.4478569, 0.45991135, 0.47043934, 0.47941095,
        0.48679012, 0.49255505, 0.49668655, 0.49917093,
        0.5, 0.49917075, 0.49668613, 0.49255443,
        0.48678935, 0.47940886, 0.47043943, 0.45990998,
        0.44785535, 0.43431556, 0.4193334, 0.40296257,
    ];
}

#[cfg(test)]
mod nan_tests {
    use super::*;
    use nih_plug::params::Param;

    const PARAM_COUNT: usize = 4;

    /// `idx` (None なら全部) のパラメータを normalized 値 `n` に据えた plugin を作る。
    /// ParamMut が crate 外へ公開されていないため、パラメータ自体を作り直す。
    #[allow(unused_variables, unused_mut)]
    fn prepared_with(idx: Option<usize>, n: f32) -> RaindropPlugin {
        let mut p = RaindropParamsNih::default();
        if idx.is_none() || idx == Some(0) {
            let r = p.time.range();
            let v = p.time.preview_plain(n);
            let nm = p.time.name().to_owned();
            p.time = FloatParam::new(nm, v, r);
        }
        if idx.is_none() || idx == Some(1) {
            let r = p.feedback.range();
            let v = p.feedback.preview_plain(n);
            let nm = p.feedback.name().to_owned();
            p.feedback = FloatParam::new(nm, v, r);
        }
        if idx.is_none() || idx == Some(2) {
            let r = p.width.range();
            let v = p.width.preview_plain(n);
            let nm = p.width.name().to_owned();
            p.width = FloatParam::new(nm, v, r);
        }
        if idx.is_none() || idx == Some(3) {
            let r = p.mix.range();
            let v = p.mix.preview_plain(n);
            let nm = p.mix.name().to_owned();
            p.mix = FloatParam::new(nm, v, r);
        }
        let mut plugin = RaindropPlugin::default();
        plugin.params = Arc::new(p);
        let v = plugin.params.time.value();
        plugin.params.time.smoothed.reset(v);
        let v = plugin.params.feedback.value();
        plugin.params.feedback.smoothed.reset(v);
        let v = plugin.params.width.value();
        plugin.params.width.smoothed.reset(v);
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
