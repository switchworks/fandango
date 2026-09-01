//! excite 系プラグイン共通パラメータ定義。
//!
//! `bass`/`hi`/`character` の param id は現行 excite と Chebyshev 実験版とで
//! 完全一致させる（DAW セッション互換・合流コスト最小化のため、
//! `plans/excite_chebyshev.md` Step 0 決定事項）。値の DSP 側解釈は各 crate
//! が個別に持ってよい。

use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum CharacterEnum {
    #[id = "warm"]
    #[name = "warm"]
    Warm,
    #[id = "neutral"]
    #[name = "neutral"]
    Neutral,
    #[id = "bright"]
    #[name = "bright"]
    Bright,
}

#[derive(Params)]
pub struct ExciteParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    /// header 右上の電源トグルで操作。true = バイパス中 (エフェクト無効)。
    #[id = "bypass"]
    pub bypass: BoolParam,

    #[id = "bass"]
    pub bass: FloatParam,
    #[id = "hi"]
    pub hi: FloatParam,
    #[id = "character"]
    pub character: EnumParam<CharacterEnum>,

    /// Chebyshev ⇔ Legacy (tanh) の帯域別 mix (0.0=Chebyshev, 1.0=Legacy)。
    /// 名前を「上げると Legacy 度が上がる」に一致させるため
    /// (2026-07-13 命名修正、旧 `bass_mix`/`hi_mix` から改名)。
    /// デフォルト 0.0 = Chebyshev (ユーザー選好により既定に採用)。
    #[id = "bass_legacy_mix"]
    pub bass_legacy_mix: FloatParam,
    #[id = "hi_legacy_mix"]
    pub hi_legacy_mix: FloatParam,
}

impl Default for ExciteParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(super::editor::WINDOW_W, super::editor::WINDOW_H),
            bypass: BoolParam::new("bypass", false),
            // Bass/Hi は内部 0..40 dB で表現。0 dB=bypass、40 dB=full effect。
            // DAW automation lane 上でも直接 dB 単位で編集可能。
            // DSP 側で linear 変換 (`10^((db-40)/20)`) してから使用。
            bass: FloatParam::new(
                "bass",
                0.0,
                FloatRange::Linear { min: 0.0, max: 40.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit(" dB")
            .with_value_to_string(Arc::new(|db| format!("{db:.1}")))
            .with_string_to_value(Arc::new(|s| s.trim().trim_end_matches("dB").trim().parse().ok())),
            hi: FloatParam::new(
                "hi",
                0.0,
                FloatRange::Linear { min: 0.0, max: 40.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit(" dB")
            .with_value_to_string(Arc::new(|db| format!("{db:.1}")))
            .with_string_to_value(Arc::new(|s| s.trim().trim_end_matches("dB").trim().parse().ok())),
            character: EnumParam::new("character", CharacterEnum::Warm),
            bass_legacy_mix: FloatParam::new(
                "bass_legacy_mix",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit("%")
            .with_value_to_string(Arc::new(|v| format!("{:.0}", v * 100.0)))
            .with_string_to_value(Arc::new(|s| {
                s.trim().trim_end_matches('%').trim().parse::<f32>().ok().map(|v| v / 100.0)
            })),
            hi_legacy_mix: FloatParam::new(
                "hi_legacy_mix",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit("%")
            .with_value_to_string(Arc::new(|v| format!("{:.0}", v * 100.0)))
            .with_string_to_value(Arc::new(|s| {
                s.trim().trim_end_matches('%').trim().parse::<f32>().ok().map(|v| v / 100.0)
            })),
        }
    }
}
