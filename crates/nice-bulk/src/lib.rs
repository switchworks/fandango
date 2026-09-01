//! fandango:nice-bulk — 帯域別パワーローコンプレッション。
//!
//! Bass(≤150Hz)/Hi(≥4kHz) 帯域を LR4 3-way クロスオーバーで分離し、
//! 小音量成分ほど持ち上げる冪乗則ゲインを掛けて再合成する。ゲイン有界。
//! 安全網のソフトサチュレータはブーストする帯域の**内側**に置き、天井を
//! その帯域の静的メイクアップに比例させる。これにより mid は非線形段を
//! 一切通らず (帯域間変調ゼロ)、ノブを上げてもサチュレーション深度が
//! 増えない (`plans/nice_bulk_band_saturation_v01.md`)。
//! 設計経緯は `plans/nice_bulk_v01.md` 参照。DSP は [`dsp`] モジュール参照。

pub mod dsp;
pub mod editor;

use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;

use crate::dsp::NiceBulkChannel;

/// build.rs で注入されるビルド時刻文字列 (`YYYYMMDD-HHMM`)。
const BUILD_TIMESTAMP: &str = env!("BUILD_TIMESTAMP");

#[derive(Params)]
pub struct NiceBulkParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    /// header 右上の電源トグルで操作。true = バイパス中 (エフェクト無効)。
    #[id = "bypass"]
    pub bypass: BoolParam,

    #[id = "bass_amount"]
    pub bass_amount: FloatParam,
    #[id = "hi_amount"]
    pub hi_amount: FloatParam,
}

impl Default for NiceBulkParams {
    fn default() -> Self {
        Self::with_defaults(fandango_shared::param_defaults::STOCK)
    }
}

impl NiceBulkParams {
    /// 既定値を `defaults` 越しに決めて構築する。`Default::default()` は `STOCK` を
    /// 渡すだけなので本体の挙動は変わらない。素材生成 (`crates/shots`) だけが
    /// 差し替えを使う。
    pub fn with_defaults(defaults: &dyn fandango_shared::param_defaults::Defaults) -> Self {
        Self {
            editor_state: EguiState::from_size(editor::WINDOW_W, editor::WINDOW_H),
            bypass: BoolParam::new("bypass", defaults.plain_bool("bypass", false)).make_bypass(),
            bass_amount: FloatParam::new(
                "bass_amount",
                defaults.plain("bass_amount", 0.0),
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit("%")
            .with_value_to_string(Arc::new(|v| format!("{:.0}", v * 100.0)))
            .with_string_to_value(Arc::new(|s| {
                s.trim()
                    .trim_end_matches('%')
                    .trim()
                    .parse::<f32>()
                    .ok()
                    .map(|v| v / 100.0)
            })),
            hi_amount: FloatParam::new(
                "hi_amount",
                defaults.plain("hi_amount", 0.0),
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit("%")
            .with_value_to_string(Arc::new(|v| format!("{:.0}", v * 100.0)))
            .with_string_to_value(Arc::new(|s| {
                s.trim()
                    .trim_end_matches('%')
                    .trim()
                    .parse::<f32>()
                    .ok()
                    .map(|v| v / 100.0)
            })),
        }
    }
}

pub struct NiceBulkPlugin {
    params: Arc<NiceBulkParams>,
    sr: f32,
    chain_l: NiceBulkChannel,
    chain_r: NiceBulkChannel,
}

impl Default for NiceBulkPlugin {
    fn default() -> Self {
        let sr = 48_000.0;
        Self {
            params: Arc::new(NiceBulkParams::default()),
            sr,
            chain_l: NiceBulkChannel::new(sr),
            chain_r: NiceBulkChannel::new(sr),
        }
    }
}

impl Plugin for NiceBulkPlugin {
    const NAME: &'static str = "fandango - nice bulk";
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
        self.chain_l.set_sr(self.sr);
        self.chain_r.set_sr(self.sr);
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

impl NiceBulkPlugin {
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
            let bass_amount = self.params.bass_amount.smoothed.next();
            let hi_amount = self.params.hi_amount.smoothed.next();
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

            let out_l = self.chain_l.process(l_in, bass_amount, hi_amount);
            let out_r = self.chain_r.process(r_in, bass_amount, hi_amount);

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

impl Vst3Plugin for NiceBulkPlugin {
    // 16 バイト固定 ASCII。プラグイン識別子。
    const VST3_CLASS_ID: [u8; 16] = [
        0x8c, 0x89, 0x43, 0xfc, 0xd6, 0x39, 0x46, 0xec, 0xb3, 0x58, 0x2e, 0x59, 0xeb, 0x0a, 0x0b,
        0x72,
    ];
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Stereo];
}

// VST3 のエクスポートシンボルは cdylib 1 個につき 1 組しか置けない。素材生成
// (`crates/shots`) は 12 crate を 1 バイナリへリンクするので、そこでは切る。
#[cfg(feature = "vst3-export")]
nih_export_vst3!(NiceBulkPlugin);

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
    fn prepared() -> NiceBulkPlugin {
        let mut plugin = NiceBulkPlugin::default();
        let v = plugin.params.bass_amount.value();
        plugin.params.bass_amount.smoothed.reset(v);
        let v = plugin.params.hi_amount.value();
        plugin.params.hi_amount.smoothed.reset(v);
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
        // 更新時の説明責任: 差分を必ず出力してから比較する。
        let max_d = window
            .iter()
            .zip(STEREO_L_WINDOW.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        let rms_old = (STEREO_L_WINDOW.iter().map(|v| v * v).sum::<f32>() / 64.0).sqrt();
        println!("STEREO_L_WINDOW 最大差分 = {max_d:.9} ({:.2} dB rel)",
                 20.0 * (max_d / rms_old).max(1e-12).log10());
        println!("新しい値:");
        for row in window.chunks(4) {
            println!("        {}",
                row.iter().map(|v| format!("{v:?},")).collect::<Vec<_>>().join(" "));
        }
        assert_eq!(window, STEREO_L_WINDOW.to_vec());
    }

    /// ステレオ出力 L のサンプル 2048..2112。過渡部を避けた定常部で採る。
    /// **この値が変わったら音を変えている。** 更新するときは必ず
    /// `stereo_output_unchanged` が出す最大差分を先に読み、変更内容から説明が
    /// つくことを確認すること（説明できない差分は別の変更が混ざった証拠）。
    ///
    /// 更新履歴:
    /// - 2026-08-22 初版（Step 1 時点）
    /// - 2026-08-30 `SPLIT_BASS_HZ` を 150 → 141 へ（オーナー聴取で決定）。
    ///   最大差分 0.032111（-20.47 dB rel）。`bass_split_sensitivity` の
    ///   「141 vs 150 = -19.71 dB rel」と 0.8 dB 以内で一致し、
    ///   **分割点の移動だけで説明がつくことを確認済み**
    const STEREO_L_WINDOW: [f32; 64] = [
        -0.35541952, -0.33460522, -0.31268048, -0.28971803,
        -0.26579747, -0.24099448, -0.2153925, -0.18907396,
        -0.16212726, -0.13464837, -0.10672095, -0.07843973,
        -0.04989498, -0.02118353, 0.0075905556, 0.036342315,
        0.064973496, 0.09339347, 0.12150507, 0.14920443,
        0.17641239, 0.20303537, 0.22898993, 0.25418627,
        0.27852955, 0.30195296, 0.32437515, 0.34572658,
        0.3659326, 0.38491488, 0.4026245, 0.4189991,
        0.43398893, 0.4475404, 0.4595984, 0.47013557,
        0.4791139, 0.48651016, 0.4922842, 0.49642813,
        0.49892685, 0.49977013, 0.49896032, 0.49648905,
        0.49237302, 0.48662493, 0.4792627, 0.47031364,
        0.45980105, 0.44776464, 0.43424368, 0.4192829,
        0.4029316, 0.38524377, 0.36627835, 0.3460982,
        0.32477027, 0.30236462, 0.2789574, 0.25462463,
        0.22944717, 0.20350876, 0.17689452, 0.14969602,
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
    fn prepared_with(idx: Option<usize>, n: f32) -> NiceBulkPlugin {
        let mut p = NiceBulkParams::default();
        if idx.is_none() || idx == Some(0) {
            let r = p.bass_amount.range();
            let v = p.bass_amount.preview_plain(n);
            let nm = p.bass_amount.name().to_owned();
            p.bass_amount = FloatParam::new(nm, v, r);
        }
        if idx.is_none() || idx == Some(1) {
            let r = p.hi_amount.range();
            let v = p.hi_amount.preview_plain(n);
            let nm = p.hi_amount.name().to_owned();
            p.hi_amount = FloatParam::new(nm, v, r);
        }
        let mut plugin = NiceBulkPlugin::default();
        plugin.params = Arc::new(p);
        let v = plugin.params.bass_amount.value();
        plugin.params.bass_amount.smoothed.reset(v);
        let v = plugin.params.hi_amount.value();
        plugin.params.hi_amount.smoothed.reset(v);
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
