//! fandango:drawl — 連続 F1/F2 母音フィルタ + Q ジッタ (MVP)。

pub mod dsp;
pub mod editor;

use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;

use crate::dsp::DrawlDsp;

/// build.rs で注入されるビルド時刻文字列 (`YYYYMMDD-HHMM`)。
const BUILD_TIMESTAMP: &str = env!("BUILD_TIMESTAMP");

#[derive(Params)]
pub struct DrawlParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    /// header 右上の電源トグルで操作。true = バイパス中 (エフェクト無効)。
    #[id = "bypass"]
    pub bypass: BoolParam,

    /// Vowel Position 0..4 (0=U, 1=O, 2=A, 3=E, 4=I、間は補間)。
    #[id = "vowel"]
    pub vowel: FloatParam,
    /// Q Jitter 0..1。
    #[id = "jitter"]
    pub jitter: FloatParam,
    /// Mix 0..1。
    #[id = "mix"]
    pub mix: FloatParam,
}

impl Default for DrawlParams {
    fn default() -> Self {
        Self::with_defaults(fandango_shared::param_defaults::STOCK)
    }
}

impl DrawlParams {
    /// 既定値を `defaults` 越しに決めて構築する。`Default::default()` は `STOCK` を
    /// 渡すだけなので本体の挙動は変わらない。素材生成 (`crates/shots`) だけが
    /// 差し替えを使う。
    pub fn with_defaults(defaults: &dyn fandango_shared::param_defaults::Defaults) -> Self {
        Self {
            editor_state: EguiState::from_size(editor::WINDOW_W, editor::WINDOW_H),
            bypass: BoolParam::new("bypass", defaults.plain_bool("bypass", false)).make_bypass(),
            vowel: FloatParam::new(
                "Vowel",
                defaults.plain("vowel", crate::dsp::VOWEL_A),
                FloatRange::Linear { min: 0.0, max: 4.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(Arc::new(|v: f32| {
                let names = crate::dsp::VOWEL_NAMES;
                let idx = v.floor() as usize;
                let frac = v - idx as f32;
                if frac < 0.05 || idx >= 4 {
                    names[idx.min(4)].to_string()
                } else {
                    format!(
                        "{}→{} {:.0}%",
                        names[idx],
                        names[(idx + 1).min(4)],
                        frac * 100.0
                    )
                }
            })),
            jitter: FloatParam::new(
                "Q Jitter",
                defaults.plain("jitter", 0.2),
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            mix: FloatParam::new(
                "Mix",
                defaults.plain("mix", 1.0),
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
        }
    }
}

/// モノ経路で右チャンネル役を務めるスクラッチの既定容量。
/// initialize() でホストの max_buffer_size に合わせて拡張する。
const DEFAULT_SCRATCH_LEN: usize = 4096;

pub struct DrawlPlugin {
    params: Arc<DrawlParams>,
    dsp: DrawlDsp,
    /// モノ入力時にのみ使う右チャンネル用スクラッチ。
    /// 確保は initialize()/Default のみ。process() 内では触らない。
    scratch: Vec<f32>,
}

impl Default for DrawlPlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(DrawlParams::default()),
            dsp: DrawlDsp::new(48000.0),
            scratch: vec![0.0; DEFAULT_SCRATCH_LEN],
        }
    }
}

impl Plugin for DrawlPlugin {
    const NAME: &'static str = "fandango - drawl";
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
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

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
        self.dsp.set_sr(buffer_config.sample_rate);
        self.dsp.reset();
        let needed = buffer_config.max_buffer_size as usize;
        if self.scratch.len() < needed {
            self.scratch.resize(needed, 0.0);
        }
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
        if self.params.bypass.value() {
            return ProcessStatus::Normal;
        }

        let vowel = self.params.vowel.value();
        let jitter = self.params.jitter.value();
        let mix = self.params.mix.value();

        self.process_buffer(buffer, vowel, jitter, mix);
        ProcessStatus::Normal
    }
}

impl DrawlPlugin {
    /// process() の本体。ProcessContext を取らないためテストから直接呼べる。
    fn process_buffer(&mut self, buffer: &mut Buffer, vowel: f32, jitter: f32, mix: f32) {
        // MXCSR に毎回設定。process 冒頭で確実に FTZ/DAZ を効かせる。
        // 無音へ切り替えた後のテイルで denormal が滞留するのを実測したため
        // (Step 4 / no_nan_no_denormal の jitter_min / square_then_silence)。
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
            let csr = _mm_getcsr();
            _mm_setcsr(csr | 0x8040); // bit 15 = FTZ, bit 6 = DAZ
        }

        let slice = buffer.as_slice();
        match slice.len() {
            0 => {}
            1 => {
                // モノ: (x, x) を与えたステレオ処理の L/R 平均を返す。
                // 右チャンネル役はスクラッチ。容量を超えるブロックは分割して回す。
                let Self { dsp, scratch, .. } = self;
                let cap = scratch.len().max(1);
                let out = &mut slice[0][..];
                let mut off = 0;
                while off < out.len() {
                    let n = (out.len() - off).min(cap);
                    let chunk = &mut out[off..off + n];
                    let right = &mut scratch[..n];
                    right.copy_from_slice(chunk);
                    dsp.process_block(chunk, right, vowel, jitter, mix);
                    for i in 0..n {
                        chunk[i] = 0.5 * (chunk[i] + right[i]);
                    }
                    off += n;
                }
            }
            _ => {
                let (left_slice, right_and_rest) = slice.split_at_mut(1);
                let (right_slice, _rest) = right_and_rest.split_at_mut(1);
                self.dsp.process_block(
                    &mut left_slice[0][..],
                    &mut right_slice[0][..],
                    vowel,
                    jitter,
                    mix,
                );
            }
        }
    }
}

impl Vst3Plugin for DrawlPlugin {
    const VST3_CLASS_ID: [u8; 16] = [
        0xcb, 0x27, 0xb0, 0xdf, 0x08, 0xb0, 0x46, 0x30, 0x97, 0x12, 0xc5, 0x67, 0xa4, 0xc2, 0x8e,
        0x00,
    ];
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Filter];
}

// VST3 のエクスポートシンボルは cdylib 1 個につき 1 組しか置けない。素材生成
// (`crates/shots`) は 12 crate を 1 バイナリへリンクするので、そこでは切る。
#[cfg(feature = "vst3-export")]
nih_export_vst3!(DrawlPlugin);

#[cfg(test)]
mod mono_tests {
    use super::*;

    /// 決定論的テスト信号 (440 Hz / 48 kHz)。
    fn test_signal(n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin() * 0.5)
            .collect()
    }

    fn params_of(plugin: &DrawlPlugin) -> (f32, f32, f32) {
        (
            plugin.params.vowel.value(),
            plugin.params.jitter.value(),
            plugin.params.mix.value(),
        )
    }

    /// `x` を L=R として与えたステレオ経路の出力。
    fn run_stereo(x: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let mut plugin = DrawlPlugin::default();
        let (v, j, m) = params_of(&plugin);
        let mut chans = vec![x.to_vec(), x.to_vec()];
        {
            let mut buffer = Buffer::default();
            unsafe {
                buffer.set_slices(x.len(), |slices| {
                    let (a, b) = chans.split_at_mut(1);
                    *slices = vec![a[0].as_mut_slice(), b[0].as_mut_slice()];
                });
            }
            plugin.process_buffer(&mut buffer, v, j, m);
        }
        let r = chans.pop().unwrap();
        let l = chans.pop().unwrap();
        (l, r)
    }

    /// `x` をモノ経路 (1ch) に通した出力。`scratch_len` でスクラッチ容量を絞れる。
    fn run_mono_with_scratch(x: &[f32], scratch_len: usize) -> Vec<f32> {
        let mut plugin = DrawlPlugin::default();
        plugin.scratch = vec![0.0; scratch_len];
        let (v, j, m) = params_of(&plugin);
        let mut chans = vec![x.to_vec()];
        {
            let mut buffer = Buffer::default();
            unsafe {
                buffer.set_slices(x.len(), |slices| {
                    *slices = vec![chans[0].as_mut_slice()];
                });
            }
            plugin.process_buffer(&mut buffer, v, j, m);
        }
        chans.pop().unwrap()
    }

    #[test]
    fn mono_matches_stereo_average() {
        let x = test_signal(4096);
        let mono = run_mono_with_scratch(&x, DEFAULT_SCRATCH_LEN);
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

    /// スクラッチ容量を超えるブロックでも panic せず、分割前と同じ出力になる。
    #[test]
    fn mono_block_longer_than_scratch() {
        let x = test_signal(4096);
        let full = run_mono_with_scratch(&x, DEFAULT_SCRATCH_LEN);
        let split = run_mono_with_scratch(&x, 128);
        for i in 0..x.len() {
            assert!(
                (full[i] - split[i]).abs() <= 1e-6,
                "sample {i}: full={} split={}",
                full[i],
                split[i]
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

    /// **never-boost 正規化 (2026-08-27) 時点**のステレオ出力 L のサンプル 2048..2112。
    ///
    /// 旧基準値 (Q の F1/F2 分離 2026-08-26 時点) から意図的に更新した。
    /// 白色雑音の全帯域電力を 1 に揃える等式正規化から `|sH| <= 1` の不等式
    /// (never-boost) へ転換し、**音を変える変更を承認済みで入れたため**
    /// (`plans/drawl_never_boost_v01.md` §2-4)。振幅は旧基準比で約 -24.2 dB。
    /// 以降の Step でこの値が変わったら音を変えている。
    ///
    /// **案 F (Fant 系 `B(f) = 80 + 120 f/5000`) へ退避する場合、この値も再取得が要る。**
    const STEREO_L_WINDOW: [f32; 64] = [
        -0.01849085, -0.017694907, -0.016839901, -0.015928656,
        -0.014964179, -0.013949662, -0.012888468, -0.011784116,
        -0.010640275, -0.009460748, -0.008249462, -0.0070104497,
        -0.005747842, -0.0044658524, -0.0031687613, -0.0018609025,
        -0.00054664793, 0.0007696069, 0.0020834573, 0.0033905057,
        0.004686375, 0.005966725, 0.0072272643, 0.00846377,
        0.009672099, 0.010848204, 0.0119881425, 0.013088097,
        0.014144385, 0.0151534695, 0.016111976, 0.0170167,
        0.017864615, 0.018652894, 0.01937891, 0.02004025,
        0.02063471, 0.021160325, 0.021615354, 0.0219983,
        0.022307906, 0.02254317, 0.02270333, 0.02278788,
        0.022796577, 0.022729423, 0.022586677, 0.022368852,
        0.022076713, 0.021711271, 0.02127378, 0.020765737,
        0.020188868, 0.019545134, 0.018836707, 0.018065976,
        0.01723554, 0.016348183, 0.015406881, 0.014414784,
        0.013375207, 0.012291619, 0.01116763, 0.010006981,
    ];
}

#[cfg(test)]
mod nan_tests {
    use super::*;
    use nih_plug::params::Param;

    /// denormal 滞留の検出: 非正規化数の非ゼロが連続 1000 サンプル以上続いたら false。
    /// 閾値は `f32::MIN_POSITIVE`。CPU を焼くのは非正規化数であって小さい正規化数ではない。
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

    /// 第 3 要素は denormal 滞留を判定するか (`tiny` は入力自体が denormal)。
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
        let base = DrawlParams::default();
        let ends = |p: &FloatParam| (p.preview_plain(0.0), p.preview_plain(1.0));
        let (v_min, v_max) = ends(&base.vowel);
        let (j_min, j_max) = ends(&base.jitter);
        let (m_min, m_max) = ends(&base.mix);
        let (v_d, j_d, m_d) = (base.vowel.value(), base.jitter.value(), base.mix.value());

        let settings: Vec<(&str, (f32, f32, f32))> = vec![
            ("all_min", (v_min, j_min, m_min)),
            ("all_max", (v_max, j_max, m_max)),
            ("vowel_min", (v_min, j_d, m_d)),
            ("vowel_max", (v_max, j_d, m_d)),
            ("jitter_min", (v_d, j_min, m_d)),
            ("jitter_max", (v_d, j_max, m_d)),
            ("mix_min", (v_d, j_d, m_min)),
            ("mix_max", (v_d, j_d, m_max)),
        ];

        for (label, (vowel, jitter, mix)) in settings {
            for (name, x, check_denormal) in hostile_inputs() {
                let mut plugin = DrawlPlugin::default();
                let mut chans = vec![x.clone(), x.clone()];
                {
                    let mut buffer = Buffer::default();
                    unsafe {
                        buffer.set_slices(x.len(), |slices| {
                            let (a, b) = chans.split_at_mut(1);
                            *slices = vec![a[0].as_mut_slice(), b[0].as_mut_slice()];
                        });
                    }
                    plugin.process_buffer(&mut buffer, vowel, jitter, mix);
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
