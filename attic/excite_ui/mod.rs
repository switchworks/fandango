//! excite 系プラグイン (現行 tanh 版・Chebyshev 実験版) 共通の param/editor。
//! `plans/excite_chebyshev.md` Step 0 で決定した共有化方針の実装。

pub mod editor;
pub mod params;

pub use params::{CharacterEnum, ExciteParams};
