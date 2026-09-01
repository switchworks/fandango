use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;

pub mod dsp;
pub mod editor;

use dsp::{hf_shelf_coeffs, hpf_coeffs, ErFir, HpfCoeffs, ShelfCoeffs};

#[derive(Params)]
pub struct NimbusParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    /// header 右上の電源トグルで操作。true = バイパス中 (エフェクト無効)。
    #[id = "bypass"]
    pub bypass: BoolParam,

    #[id = "distance"]
    pub distance: FloatParam,
    #[id = "mix"]
    pub mix: FloatParam,
}

impl Default for NimbusParams {
    fn default() -> Self {
        Self::with_defaults(fandango_shared::param_defaults::STOCK)
    }
}

impl NimbusParams {
    /// 既定値を `defaults` 越しに決めて構築する。`Default::default()` は `STOCK` を
    /// 渡すだけなので本体の挙動は変わらない。素材生成 (`crates/shots`) だけが
    /// 差し替えを使う。
    pub fn with_defaults(defaults: &dyn fandango_shared::param_defaults::Defaults) -> Self {
        Self {
            editor_state: EguiState::from_size(editor::WINDOW_W, editor::WINDOW_H),
            bypass: BoolParam::new("bypass", defaults.plain_bool("bypass", false)).make_bypass(),
            distance: FloatParam::new(
                "Distance",
                defaults.plain("distance", 0.5),
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

pub struct NimbusPlugin {
    params: Arc<NimbusParams>,
    er_fir: ErFir,
    sample_rate: f32,
    /// 高域シェルフ係数。tan / powf を含むためブロック単位で更新する
    /// （tilt と同じ方式）。W(r) と tap 時刻スケールは乗算のみなのでサンプル単位。
    shelf: ShelfCoeffs,
    /// wet HPF 係数。`f_hp` は r 非依存・固定なので SR 確定時に 1 回だけ組む（§3.4）。
    hpf: HpfCoeffs,
    last_distance: f32,
}

impl Default for NimbusPlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(NimbusParams::default()),
            er_fir: ErFir::new(48_000.0),
            sample_rate: 48_000.0,
            shelf: hf_shelf_coeffs(48_000.0, 0.0),
            hpf: hpf_coeffs(48_000.0),
            last_distance: 0.0,
        }
    }
}

impl Plugin for NimbusPlugin {
    const NAME: &'static str = "fandango - nimbus";
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
        editor::create(self.params.clone(), self.params.editor_state.clone())
    }

    fn initialize(
        &mut self,
        _layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        self.er_fir = ErFir::new(buffer_config.sample_rate);
        self.last_distance = self.params.distance.value();
        self.shelf = hf_shelf_coeffs(self.sample_rate, self.last_distance);
        self.hpf = hpf_coeffs(self.sample_rate);
        true
    }

    fn reset(&mut self) {
        self.er_fir.reset();
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

impl NimbusPlugin {
    /// process() の本体。ProcessContext を取らないためテストから直接呼べる。
    fn process_buffer(&mut self, buffer: &mut Buffer) {
        // MXCSR に毎回設定。process 冒頭で確実に FTZ/DAZ を効かせる。
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
            let csr = _mm_getcsr();
            _mm_setcsr(csr | 0x8040); // bit 15 = FTZ, bit 6 = DAZ
        }

        let mut block_start = true;
        for mut channel_samples in buffer.iter_samples() {
            let distance = self.params.distance.smoothed.next();
            let mix = self.params.mix.smoothed.next();
            // bypass は既存の `mix = 0` 経路を再利用する (mix=0 は bit-exact 恒等)。
            // 新しい分岐を作らないため、wet の計算と遅延線の進行はそのまま続く
            // = 解除時に不連続が出ない。smoother は上で必ず進めている。
            let mix = if self.params.bypass.value() { 0.0 } else { mix };

            // シェルフ係数は tan / powf を含むためブロック先頭の distance で
            // 1 回だけ組む（tilt と同じ方式）。W(r) と tap 時刻スケールは
            // 乗算のみなのでサンプル単位で追従する。
            if block_start {
                block_start = false;
                if distance != self.last_distance {
                    self.shelf = hf_shelf_coeffs(self.sample_rate, distance);
                    self.last_distance = distance;
                }
            }
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
            let (out_l, out_r) = self
                .er_fir
                .process(l, r, distance, mix, &self.shelf, &self.hpf);
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

impl Vst3Plugin for NimbusPlugin {
    const VST3_CLASS_ID: [u8; 16] = [
        0x03, 0xf1, 0x7a, 0x3a, 0x96, 0x2e, 0x4e, 0x99, 0xbd, 0xba, 0x50, 0xd6, 0xd0, 0x32, 0xf4,
        0xc8,
    ];
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Fx,
        Vst3SubCategory::Spatial,
        Vst3SubCategory::Stereo,
    ];
}

// VST3 のエクスポートシンボルは cdylib 1 個につき 1 組しか置けない。素材生成
// (`crates/shots`) は 12 crate を 1 バイナリへリンクするので、そこでは切る。
#[cfg(feature = "vst3-export")]
nih_export_vst3!(NimbusPlugin);

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
    fn prepared() -> NimbusPlugin {
        let mut plugin = NimbusPlugin::default();
        let v = plugin.params.distance.value();
        plugin.params.distance.smoothed.reset(v);
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
        -0.7612985, -0.75025845, -0.73661864, -0.72042286,
        -0.70172215, -0.6805863, -0.65708387, -0.63130456,
        -0.60332847, -0.5732556, -0.5411866, -0.5072262,
        -0.4715007, -0.43411773, -0.4002962, -0.3658563,
        -0.33060113, -0.29445305, -0.25738725, -0.21945402,
        -0.18071687, -0.14126849, -0.10122201, -0.06068635,
        -0.0147226155, 0.032238007, 0.07973269, 0.12739672,
        0.1749463, 0.22211812, 0.2687021, 0.31450802,
        0.35934907, 0.40306994, 0.44549724, 0.48648795,
        0.5259004, 0.5635992, 0.5994579, 0.6333456,
        0.66515064, 0.6947676, 0.7220989, 0.7470488,
        0.769529, 0.78946394, 0.8067858, 0.82144666,
        0.833384, 0.8425594, 0.8489383, 0.8524986,
        0.84817713, 0.8402432, 0.8290419, 0.8148074,
        0.7977201, 0.77793604, 0.75555634, 0.7307064,
        0.70348513, 0.67399967, 0.64236385, 0.6086714,
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
    fn prepared_with(idx: Option<usize>, n: f32) -> NimbusPlugin {
        let mut p = NimbusParams::default();
        if idx.is_none() || idx == Some(0) {
            let r = p.distance.range();
            let v = p.distance.preview_plain(n);
            let nm = p.distance.name().to_owned();
            p.distance = FloatParam::new(nm, v, r);
        }
        if idx.is_none() || idx == Some(1) {
            let r = p.mix.range();
            let v = p.mix.preview_plain(n);
            let nm = p.mix.name().to_owned();
            p.mix = FloatParam::new(nm, v, r);
        }
        let mut plugin = NimbusPlugin::default();
        plugin.params = Arc::new(p);
        let v = plugin.params.distance.value();
        plugin.params.distance.smoothed.reset(v);
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
