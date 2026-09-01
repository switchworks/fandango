//! パラメータ既定値の差し替えフック。
//!
//! `nih_plug` はパラメータ値の書き込み API を外部へ公開していない
//! (`ParamMut` も `ParamPtr::set_normalized_value` も `wrapper::state::deserialize_object` も
//! すべて `pub(crate)`)。したがって、いったん作った `Params` の値を後から動かす経路はない。
//!
//! そこで **構築時に既定値を差し替える**。各 crate の `Params` は
//! `with_defaults(&Defaults)` を持ち、パラメータの既定値を書いている箇所で
//! [`Defaults::plain`] を通す。`Default::default()` は [`STOCK`] を渡すだけなので、
//! プラグイン本体の挙動は一切変わらない。
//!
//! 用途は `crates/shots` の素材生成 (`plans/shots_v01.md` Step 2)。

/// パラメータ id ごとの既定 plain 値を決める。
///
/// `bool` パラメータは `0.0` / `1.0` で表す (閾値 `> 0.5`)。
pub trait Defaults {
    /// `id` の既定値を返す。差し替えないなら `stock` をそのまま返す。
    fn plain(&self, id: &str, stock: f32) -> f32;

    /// `bool` パラメータ用の薄い包み。閾値は `BoolParam::preview_plain` と同じ
    /// `> 0.5` に揃える。
    fn plain_bool(&self, id: &str, stock: bool) -> bool {
        self.plain(id, if stock { 1.0 } else { 0.0 }) > 0.5
    }

    /// `enum` パラメータ用。値はバリアント index (`EnumParam::preview_plain` と同じ)。
    fn plain_index(&self, id: &str, stock: usize) -> usize {
        self.plain(id, stock as f32).round().max(0.0) as usize
    }

    /// `#[persist]` で保持される文字列状態の差し替え。
    ///
    /// 数値パラメータではないので [`plain`](Self::plain) では届かない。
    /// 現状の利用者は gate のステップパターンだけ
    /// (`crates/gate/src/lib.rs` の `pattern`、`plans/promo_video_v01.md` §7 (1) 残実装 3)。
    /// 差し替えないなら `stock` をそのまま返す。
    fn text(&self, _id: &str, stock: &str) -> String {
        stock.to_string()
    }
}

/// `EnumParam` の既定値を差し替える。`Defaults` は `dyn` で渡すので、
/// ジェネリックはこの自由関数側に置く。
pub fn enum_default<E: nih_plug::prelude::Enum + Copy>(
    defaults: &dyn Defaults,
    id: &str,
    stock: E,
) -> E {
    E::from_index(defaults.plain_index(id, stock.to_index()))
}

/// 何も差し替えない。`Default::default()` が使う。
pub struct Stock;

impl Defaults for Stock {
    fn plain(&self, _id: &str, stock: f32) -> f32 {
        stock
    }
}

/// `Default::default()` から渡す唯一の実体。
pub const STOCK: &Stock = &Stock;

/// id → 文字列の対応表つきで差し替える。表にない id は既定のまま。
///
/// [`Overrides`] と分けているのは、文字列を要るのが gate のパターンだけであり、
/// 既存の呼び出し側 (`crates/shots` の `still` / `anim` / `tile`) を変えないため。
pub struct OverridesWithText<'a> {
    pub values: &'a std::collections::BTreeMap<String, f32>,
    pub texts: &'a std::collections::BTreeMap<String, String>,
}

impl Defaults for OverridesWithText<'_> {
    fn plain(&self, id: &str, stock: f32) -> f32 {
        self.values.get(id).copied().unwrap_or(stock)
    }

    fn text(&self, id: &str, stock: &str) -> String {
        self.texts
            .get(id)
            .cloned()
            .unwrap_or_else(|| stock.to_string())
    }
}

/// id → plain 値の対応表で差し替える。表にない id は既定のまま。
///
/// 未知 id の検出は表を作る側の責務 (`crates/shots` が `param_map()` と突き合わせる)。
/// ここで黙って無視しても気づけないため、この型に検査は持たせない。
pub struct Overrides<'a>(pub &'a std::collections::BTreeMap<String, f32>);

impl Defaults for Overrides<'_> {
    fn plain(&self, id: &str, stock: f32) -> f32 {
        self.0.get(id).copied().unwrap_or(stock)
    }
}
