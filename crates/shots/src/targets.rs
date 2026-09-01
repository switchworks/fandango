//! crate ごとのディスパッチ。
//!
//! `Target` は enum なので、新しい crate を足したときに `match` が非網羅となり
//! コンパイルエラーになる (`plans/shots_v01.md` 不変条件 6)。**ワイルドカード
//! (`_ =>`) を書かないこと。**

use std::collections::BTreeMap;

use fandango_shared::param_defaults::OverridesWithText;
use nih_plug::prelude::{ParamSetter, Params};
use nih_plug_egui::egui;

use crate::gui_context::ShotsGuiContext;
use crate::offscreen::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Crush,
    Drawl,
    Finalist,
    Gate,
    NiceBulk,
    Nimbus,
    Raindrop,
    Rotate,
    Spread,
    Tilt,
    TripDelay,
    Velvet,
}

/// `crate` 名・ウィンドウサイズ・`Scene` 構築をまとめて生成する。
///
/// `$crate_mod` の `editor` は Step 1 で切り出した `WINDOW_W/H`・`new_editor_state`・
/// `build`・`draw` を持つ (`plans/shots_v01.md` §5)。
macro_rules! dispatch {
    ($($variant:ident => ($name:literal, $krate:ident, $params:ident)),+ $(,)?) => {
        impl Target {
            pub const ALL: &'static [Target] = &[$(Target::$variant),+];

            pub fn name(self) -> &'static str {
                match self {
                    $(Target::$variant => $name),+
                }
            }

            pub fn window_size(self) -> (u32, u32) {
                match self {
                    $(Target::$variant => (
                        $krate::editor::WINDOW_W,
                        $krate::editor::WINDOW_H,
                    )),+
                }
            }

            /// `set` は id → 正規化値 (0.0–1.0)。未知 id はここで panic する
            /// (黙って既定値へ落とさない)。
            pub fn scene(self, set: &BTreeMap<String, f32>) -> Box<dyn Scene> {
                self.scene_with_text(set, &BTreeMap::new())
            }

            /// `#[persist]` の文字列状態も差し替える版。現状の利用者は gate の
            /// ステップパターンだけ (`plans/promo_video_v01.md` §7 (1) 残実装 3)。
            pub fn scene_with_text(
                self,
                set: &BTreeMap<String, f32>,
                texts: &BTreeMap<String, String>,
            ) -> Box<dyn Scene> {
                match self {
                    $(Target::$variant => {
                        let plain = plain_overrides(&$krate::$params::default(), set);
                        Box::new(CrateScene {
                            params: $krate::$params::with_defaults(&OverridesWithText {
                                values: &plain,
                                texts,
                            }),
                            state: $krate::editor::new_editor_state(),
                            build: $krate::editor::build,
                            draw: $krate::editor::draw,
                        })
                    }),+
                }
            }
        }
    };
}

dispatch! {
    Crush => ("crush", fandango_crush, CrushParams),
    Drawl => ("drawl", fandango_drawl, DrawlParams),
    Finalist => ("finalist", fandango_finalist, FinalistParams),
    Gate => ("gate", fandango_gate, GateParams),
    NiceBulk => ("nice-bulk", fandango_nice_bulk, NiceBulkParams),
    Nimbus => ("nimbus", fandango_nimbus, NimbusParams),
    Raindrop => ("raindrop", fandango_raindrop, RaindropParamsNih),
    Rotate => ("rotate", fandango_rotate, RotateParams),
    Spread => ("spread", fandango_spread, SpreadParams),
    Tilt => ("tilt", fandango_tilt, TiltParams),
    TripDelay => ("trip-delay", fandango_trip_delay, TripDelayParams),
    Velvet => ("velvet", fandango_velvet, VelvetParams),
}

impl Target {
    pub fn all_names() -> Vec<&'static str> {
        Target::ALL.iter().map(|t| t.name()).collect()
    }

    pub fn from_name(name: &str) -> Option<Target> {
        Target::ALL.iter().copied().find(|t| t.name() == name)
    }
}

/// 正規化値の指定を plain 値へ直す。
///
/// `nih_plug` は値の書き込み API を公開していないので、素材生成は
/// 「既定値を差し替えて構築し直す」形を取る (`param_defaults` 参照)。plain への
/// 変換には各パラメータのレンジが要るため、既定値で 1 度作った `Params` の
/// `param_map()` を使う 2 パス構成になる。
fn plain_overrides<P: Params>(stock: &P, set: &BTreeMap<String, f32>) -> BTreeMap<String, f32> {
    let map = stock.param_map();
    let mut plain = BTreeMap::new();
    for (id, normalized) in set {
        let Some((_, ptr, _)) = map.iter().find(|(map_id, _, _)| map_id == id) else {
            let known: Vec<&str> = map.iter().map(|(id, _, _)| id.as_str()).collect();
            panic!("unknown param id: {id} (known: {})", known.join(" "));
        };
        if !(0.0..=1.0).contains(normalized) {
            panic!("param {id}: 正規化値は 0.0–1.0。受け取った値: {normalized}");
        }
        // SAFETY: `stock` はこの関数を抜けるまで生きている。
        plain.insert(id.clone(), unsafe { ptr.preview_plain(*normalized) });
    }
    plain
}

/// crate の `editor::build` / `editor::draw` をそのまま呼ぶ。描画コードは
/// プラグイン本体と同一の関数を通る (不変条件 1)。
struct CrateScene<P, S> {
    params: P,
    state: S,
    build: fn(&egui::Context, &mut S),
    draw: fn(&egui::Context, &ParamSetter, &P, &mut S),
}

impl<P, S> Scene for CrateScene<P, S> {
    fn build(&mut self, ctx: &egui::Context) {
        (self.build)(ctx, &mut self.state);
    }

    fn draw(&mut self, ctx: &egui::Context) {
        let gui_context = ShotsGuiContext;
        let setter = ParamSetter::new(&gui_context);
        (self.draw)(ctx, &setter, &self.params, &mut self.state);
    }
}

#[cfg(test)]
mod tests {
    use super::Target;

    /// `copy.toml` の 12 本と `Target` 列挙が同じ集合であること
    /// (`plans/copy_source_v01.md` 不変条件 3)。crate を足してコピーを書き漏らすと
    /// ここで落ちる。
    #[test]
    fn copy_toml_covers_every_target() {
        let copy = fandango_copy::Copy::load(&fandango_copy::repo_root())
            .expect("copy.toml が読めること");
        let mut from_copy: Vec<&str> = copy.ids();
        from_copy.sort_unstable();
        let mut from_enum: Vec<&str> = Target::ALL.iter().map(|t| t.name()).collect();
        from_enum.sort_unstable();
        assert_eq!(from_copy, from_enum);
    }
}
