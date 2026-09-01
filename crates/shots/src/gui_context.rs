//! `ParamSetter` を作るためだけの `GuiContext` 実装。
//!
//! ショット生成では GUI からパラメータを書き戻すことがないので、書き込み系は
//! すべて呼ばれてはならない。呼ばれたら黙って無視せず panic で落とす。

use nih_plug::prelude::{GuiContext, ParamPtr, PluginApi, PluginState};

pub struct ShotsGuiContext;

impl GuiContext for ShotsGuiContext {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Vst3
    }

    fn request_resize(&self) -> bool {
        false
    }

    unsafe fn raw_begin_set_parameter(&self, _param: ParamPtr) {
        panic!("shots: GUI からのパラメータ操作は起こらない");
    }

    unsafe fn raw_set_parameter_normalized(&self, _param: ParamPtr, _normalized: f32) {
        panic!("shots: GUI からのパラメータ操作は起こらない");
    }

    unsafe fn raw_end_set_parameter(&self, _param: ParamPtr) {
        panic!("shots: GUI からのパラメータ操作は起こらない");
    }

    fn get_state(&self) -> PluginState {
        panic!("shots: 状態のシリアライズは行わない");
    }

    fn set_state(&self, _state: PluginState) {
        panic!("shots: 状態の復元は行わない");
    }
}
