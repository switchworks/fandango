//! fandango:gate — 無状態トランスゲート (最大 32 ステップ)。

pub mod dsp;
pub mod editor;
pub mod pattern;

use std::sync::{Arc, Mutex};

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;

/// build.rs が注入するビルド時刻。GUI 右上に出して DLL の鮮度を目視確認する。
const BUILD_TIMESTAMP: &str = env!("BUILD_TIMESTAMP");

use crate::dsp::{PhaseSync, StepCell, StepGate, StepLength, SyncMode};
use crate::pattern::PatternCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum StepLengthEnum {
    #[id = "1_4"]
    #[name = "1/4"]
    Quarter,
    #[id = "1_8"]
    #[name = "1/8"]
    Eighth,
    #[id = "1_16"]
    #[name = "1/16"]
    Sixteenth,
    #[id = "1_32"]
    #[name = "1/32"]
    ThirtySecond,
    #[id = "1_8t"]
    #[name = "1/8T"]
    EighthTriplet,
    #[id = "1_16t"]
    #[name = "1/16T"]
    SixteenthTriplet,
}

impl StepLengthEnum {
    fn to_dsp(self) -> StepLength {
        match self {
            StepLengthEnum::Quarter => StepLength::Quarter,
            StepLengthEnum::Eighth => StepLength::Eighth,
            StepLengthEnum::Sixteenth => StepLength::Sixteenth,
            StepLengthEnum::ThirtySecond => StepLength::ThirtySecond,
            StepLengthEnum::EighthTriplet => StepLength::EighthTriplet,
            StepLengthEnum::SixteenthTriplet => StepLength::SixteenthTriplet,
        }
    }
}

/// パターンの位相原点。GUI とホストのオートメーションに出る面。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum SyncModeEnum {
    #[id = "song"]
    #[name = "song"]
    Song,
    #[id = "bar"]
    #[name = "bar"]
    Bar,
    #[id = "note"]
    #[name = "note"]
    Note,
}

impl SyncModeEnum {
    fn to_dsp(self) -> SyncMode {
        match self {
            SyncModeEnum::Song => SyncMode::Song,
            SyncModeEnum::Bar => SyncMode::Bar,
            SyncModeEnum::Note => SyncMode::Note,
        }
    }
}

#[derive(Params)]
pub struct GateParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    /// header 右上の電源トグルで操作。true = バイパス中 (エフェクト無効)。
    #[id = "bypass"]
    pub bypass: BoolParam,

    #[id = "step_len"]
    pub step_length: EnumParam<StepLengthEnum>,
    /// 位相原点。既定は `bar`（パターン頭が常に小節頭に来る）。
    #[id = "sync"]
    pub sync_mode: EnumParam<SyncModeEnum>,
    #[id = "duty"]
    pub duty: FloatParam,
    #[id = "edge"]
    pub edge: FloatParam,
    #[id = "shape"]
    pub shape: FloatParam,
    #[id = "depth"]
    pub depth: FloatParam,
    /// パターンは文字列 1 本で持つ。ホストのパラメータ一覧には出さないが状態は保存される
    /// (`plans/gate_gui_v01.md` §8)。音声スレッドはこの中の `AtomicU64` を読む。
    #[persist = "pattern"]
    pub pattern: PatternCode,
    /// `-` で削ったセルの控え。`+` で押し戻すためだけに使う **GUI 専用の補助データ**。
    /// 音声スレッドは触らない。本体（`pattern`）と食い違ったら常に本体を優先し、
    /// こちらを捨てる（`crate::pattern::sanitize_tail`）。
    #[persist = "pattern-tail"]
    pub pattern_tail: Arc<Mutex<String>>,
}

impl Default for GateParams {
    fn default() -> Self {
        Self::with_defaults(fandango_shared::param_defaults::STOCK)
    }
}

impl GateParams {
    /// 既定値を `defaults` 越しに決めて構築する。`Default::default()` は `STOCK` を
    /// 渡すだけなので本体の挙動は変わらない。素材生成 (`crates/shots`) だけが
    /// 差し替えを使う。
    pub fn with_defaults(defaults: &dyn fandango_shared::param_defaults::Defaults) -> Self {
        Self {
            editor_state: EguiState::from_size(editor::WINDOW_W, editor::WINDOW_H),
            bypass: BoolParam::new("bypass", defaults.plain_bool("bypass", false)).make_bypass(),
            step_length: EnumParam::new(
                "Step Length",
                fandango_shared::param_defaults::enum_default(
                    defaults,
                    "step_len",
                    StepLengthEnum::Sixteenth,
                ),
            ),
            sync_mode: EnumParam::new(
                "Sync",
                fandango_shared::param_defaults::enum_default(
                    defaults,
                    "sync",
                    SyncModeEnum::Bar,
                ),
            ),
            duty: FloatParam::new(
                "Duty",
                defaults.plain("duty", 0.60),
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            edge: FloatParam::new(
                "Edge",
                defaults.plain("edge", 0.35),
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            shape: FloatParam::new(
                "Shape",
                defaults.plain("shape", 0.0),
                FloatRange::Linear {
                    min: -1.0,
                    max: 1.0,
                },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            depth: FloatParam::new(
                "Depth",
                defaults.plain("depth", 0.95),
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            pattern: {
                // パターンは `#[persist]` であってパラメータではないため、
                // `Defaults::plain` では届かない。素材生成 (`crates/shots`) から
                // 差し替えるための唯一の経路がここ。
                let text = defaults.text("pattern", crate::pattern::DEFAULT_PATTERN);
                let code = PatternCode::default();
                assert!(
                    code.store_str(&text),
                    "pattern の差し替え文字列を解釈できない: {text:?}"
                );
                code
            },
            pattern_tail: Arc::new(Mutex::new(String::new())),
        }
    }
}

/// モノ経路で右チャンネル役を務めるスクラッチの既定容量。
/// initialize() でホストの max_buffer_size に合わせて拡張する。
const DEFAULT_SCRATCH_LEN: usize = 4096;

pub struct GatePlugin {
    params: Arc<GateParams>,
    dsp: StepGate,
    /// モノ入力時にのみ使う右チャンネル用スクラッチ。
    /// 確保は initialize()/Default のみ。process() 内では触らない。
    scratch: Vec<f32>,
    /// host が停止中でも step を自走させるためのローカル位置。
    local_pos_beats: f64,
    /// 直近の NoteOn の拍位置。`SyncMode::Note` の位相原点。
    /// `None` = 未受信で、この間は Song と同じ原点（MIDI が届かない環境で
    /// 無音同然になるのを避ける）。
    ///
    /// NoteOn の位置はサンプル精度で反映する（`process` がイベント境界で
    /// 区間を切って `process_buffer` を呼ぶ）。ブロック境界で丸めると
    /// ホストのバッファサイズで音が変わってしまうため。
    note_origin_beats: Option<f64>,
}

impl Default for GatePlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(GateParams::default()),
            dsp: StepGate::new(),
            scratch: vec![0.0; DEFAULT_SCRATCH_LEN],
            local_pos_beats: 0.0,
            note_origin_beats: None,
        }
    }
}

impl Plugin for GatePlugin {
    const NAME: &'static str = "fandango - gate";
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

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
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
        self.dsp.prepare(buffer_config.sample_rate);
        self.local_pos_beats = 0.0;
        self.note_origin_beats = None;
        let needed = buffer_config.max_buffer_size as usize;
        if self.scratch.len() < needed {
            self.scratch.resize(needed, 0.0);
        }
        true
    }

    fn reset(&mut self) {
        self.dsp.reset();
        self.local_pos_beats = 0.0;
        self.note_origin_beats = None;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let transport = context.transport();
        let sr = transport.sample_rate;
        let tempo = transport.tempo.unwrap_or(120.0) as f32;

        let pos_beats = transport.pos_beats().unwrap_or(self.local_pos_beats);

        // 拍子。取れない / 0 の時は 4/4 とみなす（黙って握り潰すのではなく、
        // 拍子不明時の既定として明示的に決めた値）。
        let beats_per_bar = match (
            transport.time_sig_numerator,
            transport.time_sig_denominator,
        ) {
            (Some(num), Some(den)) if num > 0 && den > 0 => num as f64 / den as f64 * 4.0,
            _ => 4.0,
        };
        let dt_beats = tempo as f64 / 60.0 / sr as f64;
        let sync_mode = self.params.sync_mode.value().to_dsp();

        let step_length = self.params.step_length.value().to_dsp();
        let duty = self.params.duty.value();
        let edge = self.params.edge.value();
        let shape = self.params.shape.value();
        // bypass は既存の `depth = 0` 経路を再利用する (depth=0 は bit-exact 恒等)。
        // 新しい分岐を作らないため step 位置は進み続ける = 解除時に位相が飛ばない。
        let depth = if self.params.bypass.value() {
            0.0
        } else {
            self.params.depth.value()
        };

        // 1 回の atomic load でセルと長さが同時に決まる（引き裂けない）。
        let (steps, len) = self.params.pattern.load();
        let steps = &steps[..len];

        // NoteOn の位置でブロックを区間に切り、区間ごとに原点を確定させる。
        // これをやらないと NoteOn より前のサンプルにも新しい原点が遡って効き、
        // ホストのバッファサイズで音が変わる（ブロック分割不変性の破れ）。
        //
        // Song / Bar では原点が拍位置の関数なので、区間に切っても出力は変わらない
        // （dsp の A4 テストがそれを保証する）。
        let total = buffer.samples();
        let mut start = 0usize;
        let mut pending = context.next_event();
        while start < total {
            let mut end = total;
            while let Some(event) = pending {
                let timing = event.timing() as usize;
                if timing > start {
                    // 次の境界。ここまでを現在の原点で処理する。
                    end = timing.min(total);
                    break;
                }
                if let NoteEvent::NoteOn { timing, .. } = event {
                    self.note_origin_beats = Some(pos_beats + timing as f64 * dt_beats);
                }
                pending = context.next_event();
            }

            let sync = PhaseSync {
                mode: sync_mode,
                beats_per_bar,
                // 未受信は 0.0 = Song と同じ原点。無音同然にしない。
                note_origin_beats: self.note_origin_beats.unwrap_or(0.0),
            };
            self.process_buffer(
                buffer,
                start..end,
                steps,
                step_length,
                pos_beats + start as f64 * dt_beats,
                sync,
                tempo,
                duty,
                edge,
                shape,
                depth,
            );
            start = end;
        }

        // host 停止中でも step 進行
        self.local_pos_beats = pos_beats + total as f64 * dt_beats;

        ProcessStatus::Normal
    }
}

impl GatePlugin {
    /// process() の本体。ProcessContext を取らないためテストから直接呼べる。
    ///
    /// `range` はブロック内で処理する区間。NoteOn の位置でここを切ることで、
    /// 原点の切り替わりをサンプル精度で反映する。`pos_beats` は**区間先頭**の拍位置。
    #[allow(clippy::too_many_arguments)]
    fn process_buffer(
        &mut self,
        buffer: &mut Buffer,
        range: std::ops::Range<usize>,
        steps: &[StepCell],
        step_length: StepLength,
        pos_beats: f64,
        sync: PhaseSync,
        tempo: f32,
        duty: f32,
        edge: f32,
        shape: f32,
        depth: f32,
    ) -> usize {
        // MXCSR に毎回設定。process 冒頭で確実に FTZ/DAZ を効かせる。
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
            let csr = _mm_getcsr();
            _mm_setcsr(csr | 0x8040); // bit 15 = FTZ, bit 6 = DAZ
        }

        let slice = buffer.as_slice();
        match slice.len() {
            0 => 0,
            1 => {
                // モノ: (x, x) を与えたステレオ処理の L/R 平均を返す。
                // 右チャンネル役はスクラッチ。容量を超えるブロックは分割して回す。
                // 分割してもゲート位相は pos_beats を進めて連続させる。
                let Self { dsp, scratch, .. } = self;
                let cap = scratch.len().max(1);
                let out = &mut slice[0][range];
                let total = out.len();
                let mut off = 0;
                while off < total {
                    let n = (total - off).min(cap);
                    let chunk = &mut out[off..off + n];
                    let right = &mut scratch[..n];
                    right.copy_from_slice(chunk);
                    let chunk_pos = pos_beats + off as f64 * (tempo as f64 / 60.0 / dsp.sample_rate() as f64);
                    dsp.process(
                        chunk,
                        right,
                        steps,
                        step_length,
                        chunk_pos,
                        sync,
                        tempo,
                        duty,
                        edge,
                        shape,
                        depth,
                    );
                    for i in 0..n {
                        chunk[i] = 0.5 * (chunk[i] + right[i]);
                    }
                    off += n;
                }
                total
            }
            _ => {
                let (left_right, _rest) = slice.split_at_mut(2);
                let (left_slice, right_slice) = left_right.split_at_mut(1);
                let out_l = &mut left_slice[0][range.clone()];
                let out_r = &mut right_slice[0][range];
                self.dsp.process(
                    out_l,
                    out_r,
                    steps,
                    step_length,
                    pos_beats,
                    sync,
                    tempo,
                    duty,
                    edge,
                    shape,
                    depth,
                );
                out_l.len()
            }
        }
    }
}

impl Vst3Plugin for GatePlugin {
    const VST3_CLASS_ID: [u8; 16] = [
        0x9e, 0x97, 0x28, 0x2d, 0x87, 0x4a, 0x4b, 0x87, 0x8a, 0xc9, 0xf4, 0xfe, 0x0d, 0x0b, 0x67,
        0x42,
    ];
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Modulation];
}

// VST3 のエクスポートシンボルは cdylib 1 個につき 1 組しか置けない。素材生成
// (`crates/shots`) は 12 crate を 1 バイナリへリンクするので、そこでは切る。
#[cfg(feature = "vst3-export")]
nih_export_vst3!(GatePlugin);

#[cfg(test)]
mod mono_tests {
    use super::*;

    /// 決定論的テスト信号 (440 Hz / 48 kHz)。
    fn test_signal(n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin() * 0.5)
            .collect()
    }

    const TEMPO: f32 = 120.0;

    /// 検証対象の sync mode。`Note` は NoteOn 未受信で `Song` と同一なので省く。
    const SYNCS: [SyncModeEnum; 2] = [SyncModeEnum::Song, SyncModeEnum::Bar];

    /// `process_buffer` を呼ぶ。`scratch_len` でスクラッチ容量を絞れる。
    fn run(x: &[f32], channels: usize, scratch_len: usize) -> Vec<Vec<f32>> {
        run_sync(x, channels, scratch_len, SyncModeEnum::Song)
    }

    fn run_sync(
        x: &[f32],
        channels: usize,
        scratch_len: usize,
        sync_mode: SyncModeEnum,
    ) -> Vec<Vec<f32>> {
        let mut plugin = GatePlugin::default();
        plugin.dsp.prepare(48_000.0);
        plugin.scratch = vec![0.0; scratch_len];
        let step_length = plugin.params.step_length.value().to_dsp();
        let duty = plugin.params.duty.value();
        let edge = plugin.params.edge.value();
        let shape = plugin.params.shape.value();
        let depth = plugin.params.depth.value();
        let (steps, len) = plugin.params.pattern.load();
        let steps: Vec<_> = steps[..len].to_vec();

        let mut chans: Vec<Vec<f32>> = (0..channels).map(|_| x.to_vec()).collect();
        {
            let mut buffer = Buffer::default();
            unsafe {
                buffer.set_slices(x.len(), |slices| {
                    *slices = chans.iter_mut().map(|c| c.as_mut_slice()).collect();
                });
            }
            plugin.process_buffer(
                &mut buffer,
                0..x.len(),
                &steps,
                step_length,
                0.0,
                PhaseSync {
                    mode: sync_mode.to_dsp(),
                    ..PhaseSync::default()
                },
                TEMPO,
                duty,
                edge,
                shape,
                depth,
            );
        }
        chans
    }

    /// NoteOn の位置でブロックを区間に切っても出力が変わらないこと。
    ///
    /// これが破れると、ホストのバッファサイズや MIDI の有無で音が変わる
    /// （ブロック分割不変性の破れ）。Song / Bar は原点が拍位置の関数なので
    /// 区間の切り方に依らない。
    #[test]
    fn split_ranges_match_whole_block() {
        let x = test_signal(4096);
        for sync in SYNCS {
            let whole = run_ranges(&x, 2, sync, &[4096]);
            for splits in [
                vec![64, 4096],
                vec![1, 2, 3, 1000, 4096],
                vec![512, 1024, 1536, 2048, 2560, 3072, 3584, 4096],
            ] {
                let split = run_ranges(&x, 2, sync, &splits);
                assert_eq!(whole, split, "sync={sync:?} splits={splits:?} で出力が変わった");
            }
        }
    }

    /// `process_buffer` を `bounds` の区切りで複数回呼ぶ。各区間の `pos_beats` は
    /// 区間先頭の拍位置（process() が渡すのと同じ形）。
    fn run_ranges(
        x: &[f32],
        channels: usize,
        sync_mode: SyncModeEnum,
        bounds: &[usize],
    ) -> Vec<Vec<f32>> {
        let mut plugin = GatePlugin::default();
        plugin.dsp.prepare(48_000.0);
        let step_length = plugin.params.step_length.value().to_dsp();
        let duty = plugin.params.duty.value();
        let edge = plugin.params.edge.value();
        let shape = plugin.params.shape.value();
        let depth = plugin.params.depth.value();
        let (steps, len) = plugin.params.pattern.load();
        let steps: Vec<_> = steps[..len].to_vec();
        let dt_beats = TEMPO as f64 / 60.0 / 48_000.0;

        let mut chans: Vec<Vec<f32>> = (0..channels).map(|_| x.to_vec()).collect();
        {
            let mut buffer = Buffer::default();
            unsafe {
                buffer.set_slices(x.len(), |slices| {
                    *slices = chans.iter_mut().map(|c| c.as_mut_slice()).collect();
                });
            }
            let mut start = 0usize;
            for &end in bounds {
                plugin.process_buffer(
                    &mut buffer,
                    start..end,
                    &steps,
                    step_length,
                    start as f64 * dt_beats,
                    PhaseSync {
                        mode: sync_mode.to_dsp(),
                        ..PhaseSync::default()
                    },
                    TEMPO,
                    duty,
                    edge,
                    shape,
                    depth,
                );
                start = end;
            }
        }
        chans
    }

    #[test]
    fn mono_matches_stereo_average() {
        let x = test_signal(4096);
        for sync in SYNCS {
            let mono = run_sync(&x, 1, DEFAULT_SCRATCH_LEN, sync);
            let st = run_sync(&x, 2, DEFAULT_SCRATCH_LEN, sync);
            for i in 0..x.len() {
                let expected = 0.5 * (st[0][i] + st[1][i]);
                assert!(
                    (mono[0][i] - expected).abs() <= 1e-6,
                    "sync={sync:?} sample {i}: mono={} expected={}",
                    mono[0][i],
                    expected
                );
            }
        }
    }

    /// スクラッチ容量を超えるブロックでも panic せず、分割前と同じ出力になる。
    #[test]
    fn mono_block_longer_than_scratch() {
        let x = test_signal(4096);
        let full = run(&x, 1, DEFAULT_SCRATCH_LEN);
        let split = run(&x, 1, 128);
        for i in 0..x.len() {
            assert!(
                (full[0][i] - split[0][i]).abs() <= 1e-6,
                "sample {i}: full={} split={}",
                full[0][i],
                split[0][i]
            );
        }
    }

    #[test]
    fn stereo_output_unchanged() {
        let x = test_signal(4096);
        let st = run(&x, 2, DEFAULT_SCRATCH_LEN);
        let window: Vec<f32> = st[0][2048..2112].to_vec();
        assert_eq!(window, STEREO_L_WINDOW.to_vec());
    }

    /// Step 1 時点 (2026-08-22) のステレオ出力 L のサンプル 2048..2112。
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
        let base = GateParams::default();
        let ends = |p: &FloatParam| (p.preview_plain(0.0), p.preview_plain(1.0));
        let (du_min, du_max) = ends(&base.duty);
        let (e_min, e_max) = ends(&base.edge);
        let (s_min, s_max) = ends(&base.shape);
        let (de_min, de_max) = ends(&base.depth);
        let (du_d, e_d, s_d, de_d) = (
            base.duty.value(),
            base.edge.value(),
            base.shape.value(),
            base.depth.value(),
        );

        let settings: Vec<(&str, (f32, f32, f32, f32))> = vec![
            ("all_min", (du_min, e_min, s_min, de_min)),
            ("all_max", (du_max, e_max, s_max, de_max)),
            ("duty_min", (du_min, e_d, s_d, de_d)),
            ("duty_max", (du_max, e_d, s_d, de_d)),
            ("edge_min", (du_d, e_min, s_d, de_d)),
            ("edge_max", (du_d, e_max, s_d, de_d)),
            ("shape_min", (du_d, e_d, s_min, de_d)),
            ("shape_max", (du_d, e_d, s_max, de_d)),
            ("depth_min", (du_d, e_d, s_d, de_min)),
            ("depth_max", (du_d, e_d, s_d, de_max)),
        ];

        let step_length = base.step_length.value().to_dsp();
        let (steps, len) = base.pattern.load();
        let steps: Vec<_> = steps[..len].to_vec();

        for (label, (duty, edge, shape, depth)) in settings {
            for sync_mode in [SyncModeEnum::Song, SyncModeEnum::Bar] {
            for (name, x, check_denormal) in hostile_inputs() {
                let mut plugin = GatePlugin::default();
                plugin.dsp.prepare(48_000.0);
                let mut chans = vec![x.clone(), x.clone()];
                {
                    let mut buffer = Buffer::default();
                    unsafe {
                        buffer.set_slices(x.len(), |slices| {
                            let (a, b) = chans.split_at_mut(1);
                            *slices = vec![a[0].as_mut_slice(), b[0].as_mut_slice()];
                        });
                    }
                    plugin.process_buffer(
                        &mut buffer,
                        0..x.len(),
                        &steps,
                        step_length,
                        0.0,
                        PhaseSync {
                            mode: sync_mode.to_dsp(),
                            ..PhaseSync::default()
                        },
                        120.0,
                        duty,
                        edge,
                        shape,
                        depth,
                    );
                }
                for (ci, out) in chans.iter().enumerate() {
                    for (i, v) in out.iter().enumerate() {
                        assert!(
                            v.is_finite(),
                            "{label} / {name} / {sync_mode:?} / ch{ci} sample {i}: {v}"
                        );
                    }
                    if check_denormal {
                        assert!(
                            no_denormal_run(out),
                            "{label} / {name} / {sync_mode:?} / ch{ci}: denormal が 1000 サンプル以上滞留"
                        );
                    }
                }
            }
            }
        }
    }
}
