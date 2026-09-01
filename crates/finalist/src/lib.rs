use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;

pub mod dsp;
pub mod editor;

use dsp::Finalist;

/// build.rs で注入されるビルド時刻文字列 (`YYYYMMDD-HHMM`)。
const BUILD_TIMESTAMP: &str = env!("BUILD_TIMESTAMP");

#[derive(Params)]
pub struct FinalistParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    /// true = バイパス中 (エフェクト無効)。
    #[id = "bypass"]
    pub bypass: BoolParam,

    /// リミッタへの突っ込み量。
    #[id = "input"]
    pub input_db: FloatParam,
    /// 多項式ステージ量。0 = 純リミッタ (厳密恒等)。
    #[id = "drive"]
    pub drive: FloatParam,
    /// 天井。
    #[id = "ceiling"]
    pub ceiling_db: FloatParam,
    /// one-pole release。
    #[id = "release"]
    pub release_ms: FloatParam,
}

impl Default for FinalistParams {
    fn default() -> Self {
        Self::with_defaults(fandango_shared::param_defaults::STOCK)
    }
}

impl FinalistParams {
    /// 既定値を `defaults` 越しに決めて構築する。`Default::default()` は `STOCK` を
    /// 渡すだけなので本体の挙動は変わらない。素材生成 (`crates/shots`) だけが
    /// 差し替えを使う。
    pub fn with_defaults(defaults: &dyn fandango_shared::param_defaults::Defaults) -> Self {
        Self {
            editor_state: EguiState::from_size(editor::WINDOW_W, editor::WINDOW_H),
            bypass: BoolParam::new("bypass", defaults.plain_bool("bypass", false)).make_bypass(),
            input_db: FloatParam::new(
                "input",
                defaults.plain("input", 0.0),
                FloatRange::Linear {
                    min: -12.0,
                    max: 12.0,
                },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
            drive: FloatParam::new(
                "drive",
                defaults.plain("drive", 0.0),
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            ceiling_db: FloatParam::new(
                "ceiling",
                defaults.plain("ceiling", -1.0),
                FloatRange::Linear {
                    min: -12.0,
                    max: 0.0,
                },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
            release_ms: FloatParam::new(
                "release",
                defaults.plain("release", 80.0),
                FloatRange::Skewed {
                    min: 10.0,
                    max: 500.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
        }
    }
}

pub struct FinalistPlugin {
    params: Arc<FinalistParams>,
    dsp: Finalist,
}

impl Default for FinalistPlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(FinalistParams::default()),
            dsp: Finalist::new(48_000.0),
        }
    }
}

impl Plugin for FinalistPlugin {
    const NAME: &'static str = "fandango - finalist";
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
        context: &mut impl InitContext<Self>,
    ) -> bool {
        self.dsp = Finalist::new(buffer_config.sample_rate);
        context.set_latency_samples(self.dsp.latency_samples());
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

impl FinalistPlugin {
    /// process() の本体。ProcessContext を取らないためテストから直接呼べる。
    fn process_buffer(&mut self, buffer: &mut Buffer) {
        // MXCSR に毎回設定。process 冒頭で確実に FTZ/DAZ を効かせる。
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
            let csr = _mm_getcsr();
            _mm_setcsr(csr | 0x8040); // bit 15 = FTZ, bit 6 = DAZ
        }

        // ceiling と release はブロック内で一定にする。天井保証の導出が
        // 窓内で ceiling 一定を前提にしているため、サンプル単位で動かさない。
        let ceiling = util::db_to_gain(self.params.ceiling_db.value());
        let release_coeff = self.dsp.release_coeff(self.params.release_ms.value());
        let bypass = self.params.bypass.value();

        for mut channel_samples in buffer.iter_samples() {
            let input_gain = util::db_to_gain(self.params.input_db.smoothed.next());
            let drive = self.params.drive.smoothed.next();

            let Some(&mut l) = channel_samples.get_mut(0) else {
                continue;
            };
            let mono = channel_samples.len() < 2;
            let r = if mono {
                l
            } else {
                match channel_samples.get_mut(1) {
                    Some(&mut v) => v,
                    None => l,
                }
            };
            // バイパス時も遅延線と検出器は通す（ホストのレイテンシ補正と整合させ、
            // 復帰時に検出窓が古い値で埋まっているのを避けるため）。
            let (out_l, out_r) =
                self.dsp
                    .process(l, r, input_gain, drive, ceiling, release_coeff, bypass);
            if mono {
                if let Some(s) = channel_samples.get_mut(0) {
                    *s = 0.5 * (out_l + out_r);
                }
            } else {
                if let Some(s) = channel_samples.get_mut(0) {
                    *s = out_l;
                }
                if let Some(s) = channel_samples.get_mut(1) {
                    *s = out_r;
                }
            }
        }
    }
}

impl Vst3Plugin for FinalistPlugin {
    const VST3_CLASS_ID: [u8; 16] = [
        0x6f, 0x2c, 0x91, 0xd4, 0x5b, 0x83, 0x47, 0x1a, 0x8e, 0xc7, 0x3d, 0x40, 0xa9, 0x16, 0xbb,
        0x52,
    ];
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Dynamics];
}

// VST3 のエクスポートシンボルは cdylib 1 個につき 1 組しか置けない。素材生成
// (`crates/shots`) は 12 crate を 1 バイナリへリンクするので、そこでは切る。
#[cfg(feature = "vst3-export")]
nih_export_vst3!(FinalistPlugin);

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
    fn prepared() -> FinalistPlugin {
        let mut plugin = FinalistPlugin::default();
        let v = plugin.params.input_db.value();
        plugin.params.input_db.smoothed.reset(v);
        let v = plugin.params.drive.value();
        plugin.params.drive.smoothed.reset(v);
        let v = plugin.params.ceiling_db.value();
        plugin.params.ceiling_db.smoothed.reset(v);
        let v = plugin.params.release_ms.value();
        plugin.params.release_ms.smoothed.reset(v);
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
        -0.27232105, -0.29600745, -0.3187122, -0.34036008,
        -0.3608792, -0.3802041, -0.39826554, -0.41500625,
        -0.4303707, -0.44430798, -0.4567734, -0.46772233,
        -0.4771202, -0.48493582, -0.49114335, -0.49572262,
        -0.4986573, -0.4999383, -0.49956146, -0.49752757,
        -0.49384394, -0.48852265, -0.48158133, -0.47304296,
        -0.4629345, -0.45129207, -0.4381531, -0.4235611,
        -0.4075645, -0.39021394, -0.3715716, -0.35169703,
        -0.33065617, -0.30851883, -0.2853552, -0.26124832,
        -0.23627508, -0.2105183, -0.1840634, -0.1569945,
        -0.12940852, -0.1013934, -0.07304204, -0.044448458,
        -0.015703661, 0.013089409, 0.041839074, 0.07044999,
        0.09882729, 0.12688054, 0.15450929, 0.18162566,
        0.20813973, 0.23396356, 0.25901482, 0.28320372,
        0.3064535, 0.32868698, 0.3498305, 0.3698165,
        0.38857338, 0.40604168, 0.42216346, 0.43688527,
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
    fn prepared_with(idx: Option<usize>, n: f32) -> FinalistPlugin {
        let mut p = FinalistParams::default();
        if idx.is_none() || idx == Some(0) {
            let r = p.input_db.range();
            let v = p.input_db.preview_plain(n);
            let nm = p.input_db.name().to_owned();
            p.input_db = FloatParam::new(nm, v, r);
        }
        if idx.is_none() || idx == Some(1) {
            let r = p.drive.range();
            let v = p.drive.preview_plain(n);
            let nm = p.drive.name().to_owned();
            p.drive = FloatParam::new(nm, v, r);
        }
        if idx.is_none() || idx == Some(2) {
            let r = p.ceiling_db.range();
            let v = p.ceiling_db.preview_plain(n);
            let nm = p.ceiling_db.name().to_owned();
            p.ceiling_db = FloatParam::new(nm, v, r);
        }
        if idx.is_none() || idx == Some(3) {
            let r = p.release_ms.range();
            let v = p.release_ms.preview_plain(n);
            let nm = p.release_ms.name().to_owned();
            p.release_ms = FloatParam::new(nm, v, r);
        }
        let mut plugin = FinalistPlugin::default();
        plugin.params = Arc::new(p);
        let v = plugin.params.input_db.value();
        plugin.params.input_db.smoothed.reset(v);
        let v = plugin.params.drive.value();
        plugin.params.drive.smoothed.reset(v);
        let v = plugin.params.ceiling_db.value();
        plugin.params.ceiling_db.smoothed.reset(v);
        let v = plugin.params.release_ms.value();
        plugin.params.release_ms.smoothed.reset(v);
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
