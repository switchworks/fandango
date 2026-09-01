//! fandango:velvet — ヴェルベットノイズ・デコリレータ。
//!
//! 完全時不変の疎 FIR 畳み込みでモノ〜ナロー素材に空気感を付加する。
//! DSP は [`dsp`] モジュール参照。

pub mod dsp;
pub mod editor;

use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;

use crate::dsp::{
    build_sequence, color_to_cutoff_hz, energy_norm, lp_alpha, DecorrelatorState, OnePoleLp,
    VelvetSequence, DENSITY,
};

/// build.rs で注入されるビルド時刻文字列 (`YYYYMMDD-HHMM`)。
const BUILD_TIMESTAMP: &str = env!("BUILD_TIMESTAMP");

/// L チャンネル用シーケンス xorshift64* シード (固定定数)。
/// 受け入れ基準 4 (モノ互換、1/3 oct 平均 ±3 dB 超の谷無し) を満たすよう
/// 実測選定済み (width=35% デフォルト、length=30ms デフォルトで最大偏差
/// 1.157 dB、60 候補中最良)。変更禁止 (決定論・モノ互換の再現性が破綻する)。
const L_SEED: u64 = 0x98C94E6B3AA7E83D;
/// R チャンネル用シーケンス xorshift64* シード (固定定数)。L_SEED 同様。
const R_SEED: u64 = 0xB86598FE74D7396A;

/// 5 段の length 全種を事前生成したシーケンスセット (片チャンネル分)。
struct SequenceSet {
    seqs: [VelvetSequence; 5],
    norms: [f32; 5],
}

impl SequenceSet {
    fn build(seed: u64, fs: f32) -> Self {
        let lengths = [
            LengthMs::Ms10,
            LengthMs::Ms20,
            LengthMs::Ms30,
            LengthMs::Ms40,
            LengthMs::Ms50,
        ];
        let seqs = lengths.map(|l| build_sequence(seed, fs, l.seconds(), DENSITY));
        let norms = std::array::from_fn(|i| energy_norm(&seqs[i]));
        Self { seqs, norms }
    }
}

/// シーケンス長 5 段 (10/20/30/40/50 ms)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum LengthMs {
    #[id = "10"]
    #[name = "10 ms"]
    Ms10,
    #[id = "20"]
    #[name = "20 ms"]
    Ms20,
    #[id = "30"]
    #[name = "30 ms"]
    Ms30,
    #[id = "40"]
    #[name = "40 ms"]
    Ms40,
    #[id = "50"]
    #[name = "50 ms"]
    Ms50,
}

impl LengthMs {
    pub fn seconds(self) -> f32 {
        match self {
            LengthMs::Ms10 => 0.010,
            LengthMs::Ms20 => 0.020,
            LengthMs::Ms30 => 0.030,
            LengthMs::Ms40 => 0.040,
            LengthMs::Ms50 => 0.050,
        }
    }

    pub fn index(self) -> usize {
        match self {
            LengthMs::Ms10 => 0,
            LengthMs::Ms20 => 1,
            LengthMs::Ms30 => 2,
            LengthMs::Ms40 => 3,
            LengthMs::Ms50 => 4,
        }
    }
}

#[derive(Params)]
pub struct VelvetParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    /// header 右上の電源トグルで操作。true = バイパス中 (エフェクト無効)。
    #[id = "bypass"]
    pub bypass: BoolParam,

    #[id = "width"]
    pub width: FloatParam,
    #[id = "length"]
    pub length: EnumParam<LengthMs>,
    #[id = "color"]
    pub color: FloatParam,
}

impl Default for VelvetParams {
    fn default() -> Self {
        Self::with_defaults(fandango_shared::param_defaults::STOCK)
    }
}

impl VelvetParams {
    /// 既定値を `defaults` 越しに決めて構築する。`Default::default()` は `STOCK` を
    /// 渡すだけなので本体の挙動は変わらない。素材生成 (`crates/shots`) だけが
    /// 差し替えを使う。
    pub fn with_defaults(defaults: &dyn fandango_shared::param_defaults::Defaults) -> Self {
        Self {
            editor_state: EguiState::from_size(editor::WINDOW_W, editor::WINDOW_H),
            bypass: BoolParam::new("bypass", defaults.plain_bool("bypass", false)).make_bypass(),
            width: FloatParam::new(
                "width",
                defaults.plain("width", 0.35),
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            length: EnumParam::new(
                "length",
                fandango_shared::param_defaults::enum_default(defaults, "length", LengthMs::Ms30),
            ),
            color: FloatParam::new(
                "color",
                defaults.plain("color", 0.0),
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
        }
    }
}

pub struct VelvetPlugin {
    params: Arc<VelvetParams>,
    sr: f32,
    seqs_l: SequenceSet,
    seqs_r: SequenceSet,
    decorr_l: DecorrelatorState,
    decorr_r: DecorrelatorState,
    lp_l: OnePoleLp,
    lp_r: OnePoleLp,
}

/// `DecorrelatorState` のリングバッファ確保サイズ (50 ms 相当 + 余裕)。
/// length 切替時も再確保しない (仕様通りポインタ切替のみ)。
fn max_length_samples(sr: f32) -> u32 {
    (LengthMs::Ms50.seconds() * sr).ceil() as u32 + 64
}

impl Default for VelvetPlugin {
    fn default() -> Self {
        let sr = 48_000.0;
        Self {
            params: Arc::new(VelvetParams::default()),
            sr,
            seqs_l: SequenceSet::build(L_SEED, sr),
            seqs_r: SequenceSet::build(R_SEED, sr),
            decorr_l: DecorrelatorState::new(max_length_samples(sr)),
            decorr_r: DecorrelatorState::new(max_length_samples(sr)),
            lp_l: OnePoleLp::new(),
            lp_r: OnePoleLp::new(),
        }
    }
}

impl Plugin for VelvetPlugin {
    const NAME: &'static str = "fandango - velvet";
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
        self.seqs_l = SequenceSet::build(L_SEED, self.sr);
        self.seqs_r = SequenceSet::build(R_SEED, self.sr);
        self.decorr_l = DecorrelatorState::new(max_length_samples(self.sr));
        self.decorr_r = DecorrelatorState::new(max_length_samples(self.sr));
        self.lp_l.reset();
        self.lp_r.reset();
        true
    }

    fn reset(&mut self) {
        self.decorr_l.reset();
        self.decorr_r.reset();
        self.lp_l.reset();
        self.lp_r.reset();
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

impl VelvetPlugin {
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
            let width = self.params.width.smoothed.next();
            let color = self.params.color.smoothed.next();
            let length_idx = self.params.length.value().index();
            let alpha = lp_alpha(color_to_cutoff_hz(color), self.sr);
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
            let mid = 0.5 * (l_in + r_in);

            let seq_l = &self.seqs_l.seqs[length_idx];
            let seq_r = &self.seqs_r.seqs[length_idx];
            let norm_l = self.seqs_l.norms[length_idx];
            let norm_r = self.seqs_r.norms[length_idx];

            let wet_l = self.decorr_l.process(mid, seq_l) * norm_l;
            let wet_r = self.decorr_r.process(mid, seq_r) * norm_r;
            let wet_l = self.lp_l.process(wet_l, alpha);
            let wet_r = self.lp_r.process(wet_r, alpha);

            // 1 極 LP `y[n] = (1-a)·y[n-1] + a·x[n]` の白色雑音パワーゲインは
            // 閉形式 `a / (2 - a)`。`energy_norm` は疎 FIR のタップ包絡しか見ておらず
            // 後段の LP を計上していないため、その逆数をここで掛けて
            // 「wet 単体 RMS = 入力等価」の宣言を信号経路の全段へ広げる。
            // 補償は不足を埋める操作であり、0 dB を超える方向ではない。
            // 新しい定数は導入していない (`alpha` は既存の `lp_alpha` の戻り値)。
            // 白色前提の近似である点は drawl の `I5` 正規化と同じ扱い。
            //
            // `alpha = 1 - exp(-2π·fc/fs)` は `fc = color_to_cutoff_hz(color) >= 2 kHz`、
            // `fs > 0` である限り厳密に正なのでゼロ除算は起きない。クランプで
            // 誤魔化さず、前提が崩れたら debug ビルドで落とす。
            debug_assert!(alpha > 0.0, "lp_alpha が非正: alpha={alpha}");
            let lp_comp = ((2.0 - alpha) / alpha).sqrt();
            let wet_l = wet_l * lp_comp;
            let wet_r = wet_r * lp_comp;

            let (mono_out_l, mono_out_r) = (l_in + width * wet_l, r_in + width * wet_r);
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

impl Vst3Plugin for VelvetPlugin {
    // 16 バイト固定 ASCII。プラグイン識別子。
    const VST3_CLASS_ID: [u8; 16] = [
        0x0e, 0x2c, 0xcd, 0x68, 0xf9, 0x5a, 0x4a, 0x28, 0xb7, 0xd8, 0xd2, 0x7e, 0xa2, 0x43, 0xb4,
        0x7a,
    ];
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Stereo];
}

// VST3 のエクスポートシンボルは cdylib 1 個につき 1 組しか置けない。素材生成
// (`crates/shots`) は 12 crate を 1 バイナリへリンクするので、そこでは切る。
#[cfg(feature = "vst3-export")]
nih_export_vst3!(VelvetPlugin);

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
    fn prepared() -> VelvetPlugin {
        let mut plugin = VelvetPlugin::default();
        let v = plugin.params.width.value();
        plugin.params.width.smoothed.reset(v);
        let v = plugin.params.color.value();
        plugin.params.color.smoothed.reset(v);
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

    /// 基準となるステレオ出力 L のサンプル 2048..2112。
    /// 過渡部を避けた定常部で採る (finalist は lookahead 120 サンプル分が先頭ゼロ)。
    /// 以降の Step でこの値が変わったら音を変えている。
    ///
    /// 初版は Step 1 時点 (2026-08-22)。
    /// 2026-08-24 更新: `color` の 1 極 LP のエネルギー損失を正規化に折り込んだため
    /// (`plans/invariant_path_v01.md`)。`energy_norm` は疎 FIR のタップ包絡しか
    /// 見ておらず、宣言していた「wet 単体 RMS = 入力等価」が後段の LP で破れていた。
    ///
    /// 変更が補償量そのものであることを確認済み。既定 `color = 0` では
    /// `alpha = 0.9270509`、補償 `sqrt((2-alpha)/alpha) = 1.0758154` (+0.6348 dB) で、
    /// `new = dry + (old - dry) * 1.0758154` が f32 精度 (誤差 ~1e-7) で全サンプル成立する。
    /// すなわち dry は無改変で wet 経路だけが補償倍された。`width = 0` の bit-exact 恒等も不変。
    const STEREO_L_WINDOW: [f32; 64] = [
        -0.77547127, -0.7621925, -0.7463864, -0.72810245,
        -0.7074055, -0.68436295, -0.65905213, -0.6315552,
        -0.6019585, -0.5703699, -0.53689003, -0.50163203,
        -0.46470866, -0.4262371, -0.38635832, -0.34519863,
        -0.30289668, -0.25958848, -0.21541059, -0.17052607,
        -0.12507652, -0.079214744, -0.033088394, 0.013157185,
        0.059350636, 0.10534696, 0.150991, 0.19613679,
        0.24064136, 0.28433925, 0.32709503, 0.36876112,
        0.4092081, 0.4483063, 0.48590955, 0.5219024,
        0.5561599, 0.58858365, 0.6190485, 0.6474601,
        0.67372566, 0.69775313, 0.71947545, 0.7388056,
        0.75568557, 0.77006066, 0.78187907, 0.79111046,
        0.7977138, 0.80167246, 0.8029722, 0.8016094,
        0.79758924, 0.79092205, 0.78163326, 0.7697513,
        0.7553173, 0.7383782, 0.71898997, 0.6972184,
        0.67313355, 0.646817, 0.61835366, 0.5878415,
    ];
}

#[cfg(test)]
mod nan_tests {
    use super::*;
    use nih_plug::params::Param;

    const PARAM_COUNT: usize = 2;

    /// `idx` (None なら全部) のパラメータを normalized 値 `n` に据えた plugin を作る。
    /// ParamMut が crate 外へ公開されていないため、パラメータ自体を作り直す。
    #[allow(unused_variables, unused_mut)]
    fn prepared_with(idx: Option<usize>, n: f32) -> VelvetPlugin {
        let mut p = VelvetParams::default();
        if idx.is_none() || idx == Some(0) {
            let r = p.width.range();
            let v = p.width.preview_plain(n);
            let nm = p.width.name().to_owned();
            p.width = FloatParam::new(nm, v, r);
        }
        if idx.is_none() || idx == Some(1) {
            let r = p.color.range();
            let v = p.color.preview_plain(n);
            let nm = p.color.name().to_owned();
            p.color = FloatParam::new(nm, v, r);
        }
        let mut plugin = VelvetPlugin::default();
        plugin.params = Arc::new(p);
        let v = plugin.params.width.value();
        plugin.params.width.smoothed.reset(v);
        let v = plugin.params.color.value();
        plugin.params.color.smoothed.reset(v);
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
