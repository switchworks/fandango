//! fandango:rotate — フェーズローテータ。
//!
//! 1 次オールパス縦列で位相のみ回転し、非対称波形のピークを音色を変えずに
//! 下げる。DSP は [`dsp`] モジュール参照。

pub mod dsp;
pub mod editor;

use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;

use crate::dsp::AllpassChain;

/// build.rs で注入されるビルド時刻文字列 (`YYYYMMDD-HHMM`)。
const BUILD_TIMESTAMP: &str = env!("BUILD_TIMESTAMP");

/// オールパス段数 4 種 (enum)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum Stages {
    #[id = "2"]
    #[name = "2"]
    Two,
    #[id = "4"]
    #[name = "4"]
    Four,
    #[id = "6"]
    #[name = "6"]
    Six,
    #[id = "8"]
    #[name = "8"]
    Eight,
}

impl Stages {
    pub fn count(self) -> usize {
        match self {
            Stages::Two => 2,
            Stages::Four => 4,
            Stages::Six => 6,
            Stages::Eight => 8,
        }
    }
}

#[derive(Params)]
pub struct RotateParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    /// header 右上の電源トグルで操作。true = バイパス中 (エフェクト無効)。
    #[id = "bypass"]
    pub bypass: BoolParam,

    #[id = "stages"]
    pub stages: EnumParam<Stages>,
    #[id = "freq"]
    pub freq: FloatParam,
}

impl Default for RotateParams {
    fn default() -> Self {
        Self::with_defaults(fandango_shared::param_defaults::STOCK)
    }
}

impl RotateParams {
    /// 既定値を `defaults` 越しに決めて構築する。`Default::default()` は `STOCK` を
    /// 渡すだけなので本体の挙動は変わらない。素材生成 (`crates/shots`) だけが
    /// 差し替えを使う。
    pub fn with_defaults(defaults: &dyn fandango_shared::param_defaults::Defaults) -> Self {
        Self {
            editor_state: EguiState::from_size(editor::WINDOW_W, editor::WINDOW_H),
            bypass: BoolParam::new("bypass", defaults.plain_bool("bypass", false)).make_bypass(),
            stages: EnumParam::new(
                "stages",
                fandango_shared::param_defaults::enum_default(defaults, "stages", Stages::Four),
            ),
            freq: FloatParam::new(
                "freq",
                defaults.plain("freq", 700.0),
                FloatRange::Skewed {
                    min: 50.0,
                    max: 2000.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit(" hz")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
        }
    }
}

pub struct RotatePlugin {
    params: Arc<RotateParams>,
    sr: f32,
    chain_l: AllpassChain,
    chain_r: AllpassChain,
}

impl Default for RotatePlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(RotateParams::default()),
            sr: 48_000.0,
            chain_l: AllpassChain::new(),
            chain_r: AllpassChain::new(),
        }
    }
}

impl Plugin for RotatePlugin {
    const NAME: &'static str = "fandango - rotate";
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

impl RotatePlugin {
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
            let freq = self.params.freq.smoothed.next();
            let stages = self.params.stages.value().count();
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

            let out_l = self.chain_l.process(l_in, freq, self.sr, stages);
            let out_r = self.chain_r.process(r_in, freq, self.sr, stages);

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

impl Vst3Plugin for RotatePlugin {
    // 16 バイト固定 ASCII。プラグイン識別子。
    const VST3_CLASS_ID: [u8; 16] = [
        0xc7, 0x24, 0x2b, 0xe5, 0xd2, 0xcb, 0x48, 0xe5, 0x9b, 0xae, 0x80, 0x16, 0x06, 0xa0, 0x97,
        0x28,
    ];
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Stereo];
}

// VST3 のエクスポートシンボルは cdylib 1 個につき 1 組しか置けない。素材生成
// (`crates/shots`) は 12 crate を 1 バイナリへリンクするので、そこでは切る。
#[cfg(feature = "vst3-export")]
nih_export_vst3!(RotatePlugin);

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
    fn prepared() -> RotatePlugin {
        let mut plugin = RotatePlugin::default();
        let v = plugin.params.freq.value();
        plugin.params.freq.smoothed.reset(v);
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
    /// 2026-08-24 更新: `freq` の既定値を 200 Hz → 700 Hz に変更したため。
    /// 変更は crest factor の実測に基づく意図的なもので、旧既定 (stages=4 / 200 Hz) は
    /// 合成非対称波の基音 5 点平均で 0.98 dB しか低減せず、ほぼ効いていなかった。
    /// 700 Hz では 3.47 dB。根拠は `plans/bundle_review_v01_fable.md` の実測記録 3。
    /// 切り戻し確認済み: 200 Hz に戻すと旧基準値で通る (原因が本変更のみであることの確認)。
    const STEREO_L_WINDOW: [f32; 64] = [
        0.18137386, 0.20789418, 0.23372582, 0.2587839,
        0.28298125, 0.3062403, 0.32848275, 0.34963796,
        0.3696366, 0.38840428, 0.40588441, 0.42201713,
        0.43675366, 0.45004573, 0.4618382, 0.4720998,
        0.48079407, 0.48789826, 0.49338895, 0.4972348,
        0.49943268, 0.49997234, 0.49885887, 0.49609596,
        0.49167848, 0.48563164, 0.47797245, 0.46873325,
        0.4579444, 0.44562745, 0.43183383, 0.41660646,
        0.40000233, 0.38207608, 0.3628742, 0.34247,
        0.3209287, 0.29833132, 0.27473712, 0.25023273,
        0.22489917, 0.19881861, 0.1720847, 0.14477444,
        0.11698468, 0.088807404, 0.06033498, 0.031665742,
        0.0028883219, -0.025898278, -0.05459881, -0.08311814,
        -0.11136174, -0.13923618, -0.16664883, -0.19350874,
        -0.21972692, -0.24521726, -0.26989323, -0.29367423,
        -0.31648147, -0.33823922, -0.35887673, -0.37832168,
    ];
}

#[cfg(test)]
mod nan_tests {
    use super::*;
    use nih_plug::params::Param;

    const PARAM_COUNT: usize = 1;

    /// `idx` (None なら全部) のパラメータを normalized 値 `n` に据えた plugin を作る。
    /// ParamMut が crate 外へ公開されていないため、パラメータ自体を作り直す。
    #[allow(unused_variables, unused_mut)]
    fn prepared_with(idx: Option<usize>, n: f32) -> RotatePlugin {
        let mut p = RotateParams::default();
        if idx.is_none() || idx == Some(0) {
            let r = p.freq.range();
            let v = p.freq.preview_plain(n);
            let nm = p.freq.name().to_owned();
            p.freq = FloatParam::new(nm, v, r);
        }
        let mut plugin = RotatePlugin::default();
        plugin.params = Arc::new(p);
        let v = plugin.params.freq.value();
        plugin.params.freq.smoothed.reset(v);
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
