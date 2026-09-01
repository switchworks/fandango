//! fandango:spread — 完全線形ステレオワイドナー + Bass Preserve 拡張。
//!
//! DSP は [`dsp`] モジュール参照 (幅カーブを有理関数の閉形式で与える)。
//! Bass Preserve は LR4 4次クロスオーバーで低域を触らず高域のみ M/S 処理する
//! 独自拡張。

pub mod dsp;
pub mod editor;

use std::sync::Arc;

use fandango_shared::Lr4Crossover;
use nih_plug::prelude::*;
use nih_plug_egui::EguiState;

use crate::dsp::Coefs;

/// build.rs で注入されるビルド時刻文字列 (`YYYYMMDD-HHMM`)。
const BUILD_TIMESTAMP: &str = env!("BUILD_TIMESTAMP");

/// Bass Preserve カットオフ選択肢。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum BassPreserve {
    #[id = "off"]
    #[name = "off"]
    Off,
    #[id = "60"]
    #[name = "60 hz"]
    Hz60,
    #[id = "100"]
    #[name = "100 hz"]
    Hz100,
    #[id = "150"]
    #[name = "150 hz"]
    Hz150,
    #[id = "200"]
    #[name = "200 hz"]
    Hz200,
}

impl BassPreserve {
    fn cutoff_hz(self) -> Option<f32> {
        match self {
            BassPreserve::Off => None,
            BassPreserve::Hz60 => Some(60.0),
            BassPreserve::Hz100 => Some(100.0),
            BassPreserve::Hz150 => Some(150.0),
            BassPreserve::Hz200 => Some(200.0),
        }
    }
}

#[derive(Params)]
pub struct SpreadParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    /// header 右上の電源トグルで操作。true = バイパス中 (エフェクト無効)。
    #[id = "bypass"]
    pub bypass: BoolParam,

    #[id = "expander"]
    pub expander: FloatParam,
    #[id = "inverse"]
    pub inverse: BoolParam,
    #[id = "center_amount"]
    pub center_amount: FloatParam,
    #[id = "bass_preserve"]
    pub bass_preserve: EnumParam<BassPreserve>,
}

impl Default for SpreadParams {
    fn default() -> Self {
        Self::with_defaults(fandango_shared::param_defaults::STOCK)
    }
}

impl SpreadParams {
    /// 既定値を `defaults` 越しに決めて構築する。`Default::default()` は `STOCK` を
    /// 渡すだけなので本体の挙動は変わらない。素材生成 (`crates/shots`) だけが
    /// 差し替えを使う。
    pub fn with_defaults(defaults: &dyn fandango_shared::param_defaults::Defaults) -> Self {
        Self {
            editor_state: EguiState::from_size(editor::WINDOW_W, editor::WINDOW_H),
            bypass: BoolParam::new("bypass", defaults.plain_bool("bypass", false)).make_bypass(),
            expander: FloatParam::new(
                "expander",
                defaults.plain("expander", 0.5),
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            inverse: BoolParam::new("inverse", defaults.plain_bool("inverse", false)),
            center_amount: FloatParam::new(
                "center mix",
                defaults.plain("center_amount", 1.0),
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            bass_preserve: EnumParam::new(
                "bass preserve",
                fandango_shared::param_defaults::enum_default(
                    defaults,
                    "bass_preserve",
                    BassPreserve::Off,
                ),
            ),
        }
    }
}

pub struct SpreadPlugin {
    params: Arc<SpreadParams>,
    sr: f32,
    lr4_l: Lr4Crossover,
    lr4_r: Lr4Crossover,
    active_cutoff: Option<f32>,
}

impl Default for SpreadPlugin {
    fn default() -> Self {
        let sr = 48_000.0;
        Self {
            params: Arc::new(SpreadParams::default()),
            sr,
            lr4_l: Lr4Crossover::new(sr, 100.0),
            lr4_r: Lr4Crossover::new(sr, 100.0),
            active_cutoff: None,
        }
    }
}

impl Plugin for SpreadPlugin {
    const NAME: &'static str = "fandango - spread";
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
        self.active_cutoff = None;
        self.lr4_l.reset();
        self.lr4_r.reset();
        true
    }

    fn reset(&mut self) {
        self.lr4_l.reset();
        self.lr4_r.reset();
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

impl SpreadPlugin {
    /// process() の本体。ProcessContext を取らないためテストから直接呼べる。
    fn process_buffer(&mut self, buffer: &mut Buffer) {
        // MXCSR に毎回設定。process 冒頭で確実に FTZ/DAZ を効かせる。
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
            let csr = _mm_getcsr();
            _mm_setcsr(csr | 0x8040); // bit 15 = FTZ, bit 6 = DAZ
        }

        let bass = self.params.bass_preserve.value();
        let desired_cutoff = bass.cutoff_hz();
        if desired_cutoff != self.active_cutoff {
            if let Some(fc) = desired_cutoff {
                self.lr4_l.set_cutoff(self.sr, fc);
                self.lr4_r.set_cutoff(self.sr, fc);
                self.lr4_l.reset();
                self.lr4_r.reset();
            }
            self.active_cutoff = desired_cutoff;
        }

        for mut ch in buffer.iter_samples() {
            let t = self.params.expander.smoothed.next();
            let inv = self.params.inverse.value();
            let center_amount = self.params.center_amount.smoothed.next();
            if self.params.bypass.value() {
                continue;
            }
            let k = Coefs::from_params(t, center_amount);

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
            let (l0, r0) = if inv { (r_in, l_in) } else { (l_in, r_in) };

            let (out_l, out_r) = match desired_cutoff {
                None => (k.a * l0 + k.c * r0, k.c * l0 + k.a * r0),
                Some(_) => {
                    let (l_low, l_high) = self.lr4_l.split(l0);
                    let (r_low, r_high) = self.lr4_r.split(r0);
                    let l_hi_out = k.a * l_high + k.c * r_high;
                    let r_hi_out = k.c * l_high + k.a * r_high;
                    (l_low + l_hi_out, r_low + r_hi_out)
                }
            };

            if mono {
                if let Some(s) = ch.get_mut(0) {
                    *s = 0.5 * (out_l + out_r);
                }
            } else {
                if let Some(s) = ch.get_mut(0) {
                    *s = out_l;
                }
                if let Some(s) = ch.get_mut(1) {
                    *s = out_r;
                }
            }
        }

    }
}

impl Vst3Plugin for SpreadPlugin {
    // 16 バイト固定 ASCII。プラグイン識別子。
    const VST3_CLASS_ID: [u8; 16] = [
        0x1d, 0xe8, 0x67, 0x05, 0x5d, 0x6c, 0x4d, 0x11, 0x80, 0xf3, 0x20, 0x20, 0x1a, 0x5c, 0xb1,
        0x80,
    ];
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Stereo];
}

// VST3 のエクスポートシンボルは cdylib 1 個につき 1 組しか置けない。素材生成
// (`crates/shots`) は 12 crate を 1 バイナリへリンクするので、そこでは切る。
#[cfg(feature = "vst3-export")]
nih_export_vst3!(SpreadPlugin);

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
    fn prepared() -> SpreadPlugin {
        let mut plugin = SpreadPlugin::default();
        let v = plugin.params.expander.value();
        plugin.params.expander.smoothed.reset(v);
        let v = plugin.params.center_amount.value();
        plugin.params.center_amount.smoothed.reset(v);
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

    const PARAM_COUNT: usize = 2;

    /// `idx` (None なら全部) のパラメータを normalized 値 `n` に据えた plugin を作る。
    /// ParamMut が crate 外へ公開されていないため、パラメータ自体を作り直す。
    #[allow(unused_variables, unused_mut)]
    fn prepared_with(idx: Option<usize>, n: f32) -> SpreadPlugin {
        let mut p = SpreadParams::default();
        if idx.is_none() || idx == Some(0) {
            let r = p.expander.range();
            let v = p.expander.preview_plain(n);
            let nm = p.expander.name().to_owned();
            p.expander = FloatParam::new(nm, v, r);
        }
        if idx.is_none() || idx == Some(1) {
            let r = p.center_amount.range();
            let v = p.center_amount.preview_plain(n);
            let nm = p.center_amount.name().to_owned();
            p.center_amount = FloatParam::new(nm, v, r);
        }
        let mut plugin = SpreadPlugin::default();
        plugin.params = Arc::new(p);
        let v = plugin.params.expander.value();
        plugin.params.expander.smoothed.reset(v);
        let v = plugin.params.center_amount.value();
        plugin.params.center_amount.smoothed.reset(v);
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
