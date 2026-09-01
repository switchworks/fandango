//! fandango:excite — ADAA + Even/Odd + envelope-driven 2 ノブエキサイター。
//!
//! Params/editor は `fandango_shared::excite_ui` を Chebyshev 実験版と共有
//! (`plans/excite_chebyshev.md` Step 0)。

pub mod dsp;

use std::sync::Arc;

use nih_plug::prelude::*;

use fandango_shared::excite_ui::{editor, CharacterEnum, ExciteParams};

use crate::dsp::{Character, ExciteChannel};

fn character_to_dsp(c: CharacterEnum) -> Character {
    match c {
        CharacterEnum::Warm => Character::Warm,
        CharacterEnum::Neutral => Character::Neutral,
        CharacterEnum::Bright => Character::Bright,
    }
}

/// build.rs で注入されるビルド時刻文字列 (`YYYYMMDD-HHMM`)。
/// DAW が読込中の DLL バージョン目視用。
const BUILD_TIMESTAMP: &str = env!("BUILD_TIMESTAMP");

pub struct ExcitePlugin {
    params: Arc<ExciteParams>,
    ch_l: ExciteChannel,
    ch_r: ExciteChannel,
}

impl Default for ExcitePlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(ExciteParams::default()),
            ch_l: ExciteChannel::new(48000.0),
            ch_r: ExciteChannel::new(48000.0),
        }
    }
}

impl Plugin for ExcitePlugin {
    const NAME: &'static str = "fandango - excite";
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
            "fandango : excite",
            BUILD_TIMESTAMP,
        )
    }

    fn initialize(
        &mut self,
        _layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        let sr = buffer_config.sample_rate;
        self.ch_l.set_sr(sr);
        self.ch_r.set_sr(sr);
        self.ch_l.reset();
        self.ch_r.reset();
        true
    }

    fn reset(&mut self) {
        self.ch_l.reset();
        self.ch_r.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // FTZ (Flush-To-Zero) + DAZ (Denormals-Are-Zero) を audio thread の
        // MXCSR に毎回設定。MXCSR は per-thread のため initialize (別thread) では
        // 反映されない事があり、process 冒頭で確実に効かせる。
        // オーバヘッド ≈ 1 cycle/block、負担ほぼ無し。
        #[cfg(target_arch = "x86_64")]
        unsafe {
            use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
            let csr = _mm_getcsr();
            _mm_setcsr(csr | 0x8040); // bit 15 = FTZ, bit 6 = DAZ
        }

        for mut ch_samples in buffer.iter_samples() {
            let bass_db = self.params.bass.smoothed.next();
            let hi_db = self.params.hi.smoothed.next();
            let bass_legacy_mix = self.params.bass_legacy_mix.smoothed.next();
            let hi_legacy_mix = self.params.hi_legacy_mix.smoothed.next();
            // bypass 中も smoother は進めておく (解除時の値ジャンプ回避)。
            if self.params.bypass.value() {
                continue;
            }
            // dB → linear 変換。0 dB (bypass) 近傍で 0.01、40 dB (full) で 1.0。
            let bass = if bass_db < 0.5 { 0.0 } else { 10.0_f32.powf((bass_db - 40.0) / 20.0) };
            let hi = if hi_db < 0.5 { 0.0 } else { 10.0_f32.powf((hi_db - 40.0) / 20.0) };
            let character = character_to_dsp(self.params.character.value());

            let mut iter = ch_samples.iter_mut();
            if let Some(l) = iter.next() {
                *l = self.ch_l.process(*l, bass, hi, character, bass_legacy_mix, hi_legacy_mix);
            }
            if let Some(r) = iter.next() {
                *r = self.ch_r.process(*r, bass, hi, character, bass_legacy_mix, hi_legacy_mix);
            }
        }
        ProcessStatus::Normal
    }
}

impl Vst3Plugin for ExcitePlugin {
    const VST3_CLASS_ID: [u8; 16] = [0xf5,0x54,0x66,0x61,0x6c,0x97,0x43,0x63,0xab,0xf0,0xf3,0xc1,0xcc,0xf4,0xec,0x8a];
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Distortion];
}

nih_export_vst3!(ExcitePlugin);
