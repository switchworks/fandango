//! excite 系プラグイン共通 egui エディタ実装。骨格とスライダーのみ追随する。

use std::sync::Arc;

use nih_plug::prelude::Editor;
use nih_plug_egui::{create_egui_editor, EguiState};

use crate::ui::{
    apply_palette, bead_slider_row, choice_row, formula, install_fonts, show_shell, Palette, Shell,
};

use super::params::{CharacterEnum, ExciteParams};

pub(crate) const WINDOW_W: u32 = 800;
pub(crate) const WINDOW_H: u32 = 400;

pub fn create(
    params: Arc<ExciteParams>,
    editor_state: Arc<EguiState>,
    plugin_label: &'static str,
    _build_timestamp: &'static str,
) -> Option<Box<dyn Editor>> {
    create_egui_editor(
        editor_state,
        (),
        |ctx, _state| install_fonts(ctx),
        move |ctx, setter, _state| {
            apply_palette(ctx, Palette::Tomato);
            show_shell(
                ctx,
                Shell {
                    wordmark: plugin_label,
                    formula: formula::NONE,
                    kind: "",
                    bypass: Some((&params.bypass, setter)),
                },
                |_ui, _rect| {},
                |ui| {
                    ui.columns(2, |cols| {
                        bead_slider_row(&mut cols[0], "bass", &params.bass, setter);
                        bead_slider_row(&mut cols[1], "hi", &params.hi, setter);
                    });
                    ui.columns(2, |cols| {
                        bead_slider_row(&mut cols[0], "bass_legacy_mix", &params.bass_legacy_mix, setter);
                        bead_slider_row(&mut cols[1], "hi_legacy_mix", &params.hi_legacy_mix, setter);
                    });
                    choice_row(
                        ui,
                        "character",
                        &params.character,
                        setter,
                        &[
                            ("warm", CharacterEnum::Warm),
                            ("neutral", CharacterEnum::Neutral),
                            ("bright", CharacterEnum::Bright),
                        ],
                    );
                },
            );
        },
    )
}
