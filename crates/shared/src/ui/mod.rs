//! 全プラグイン共通の egui GUI 部品 (団子スライダー・Radix パレット・ヘッダー)。

pub mod background;
pub mod curve;
pub mod formula;
pub mod header;
pub mod layout;
pub mod palette;
pub mod relief;
pub mod widgets;

pub use background::Background;
pub use curve::{transfer_plot, Axes, Baseline, CurveSpec, XScale};
pub use formula::{paint_scripted, Script};
pub use header::{header_row, install_fonts};
pub use layout::{show_shell, show_shell_with_background, Shell, WINDOW_H, WINDOW_W};
pub use palette::{apply_palette, Palette};
pub use relief::{relief_bounds, time_to_x, ReflectionField, ReflectionTap, ReliefInput};
pub use widgets::{bead_slider_row, choice_row, int_bead_slider_row};

use nih_plug_egui::egui::TextureHandle;

/// GUI 側キャッシュ。パレットは各 crate の editor が定数として指定する。
#[derive(Default)]
pub struct CommonGuiState {
    pub logo: Option<TextureHandle>,
}
