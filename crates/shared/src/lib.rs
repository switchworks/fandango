//! fandango 共通 DSP ユーティリティ。
//!
//! - [`DelayLine`] 分数遅延 (線形補間) 対応リングバッファ
//! - [`ms`] Mid/Side 変換
//! - [`biquad`] Direct Form II Transposed 双二次フィルタ
//! - [`lr4`] Linkwitz-Riley 4次クロスオーバー
//! - [`velvet`] ヴェルベットノイズ (OVN) タップ列生成と疎 FIR
//! - [`ui`] 全プラグイン共通 egui GUI 部品 (団子スライダー・Radix パレット・ヘッダー)
//! - [`param_defaults`] パラメータ既定値の差し替えフック (素材生成用)

pub mod biquad;
pub mod delay;
pub mod lr4;
pub mod ms;
pub mod param_defaults;
pub mod ui;
pub mod velvet;

pub use biquad::Biquad;
pub use delay::DelayLine;
pub use lr4::Lr4Crossover;
