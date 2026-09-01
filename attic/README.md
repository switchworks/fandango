# attic — 退役した crate の凍結置き場

出荷対象から外したが、参考実装として残すソース。**ビルド対象外**。
workspace の `exclude` に入れてあるので `cargo build` / `cargo check` は触らない。

`version.workspace = true` 等の workspace 継承が効かないため、ここに置いた crate は
単体でもビルドできない。復帰させる場合は `crates/` へ戻し、ルート `Cargo.toml` の
`members` へ再登録して `exclude` から外すこと。

## `excite` / `excite-ab` / `excite_ui` (2026-08-14 退役)

- `:excite` は ADAA + Even/Odd 倍音制御のエキサイター。後継は `:nice-bulk`
  (帯域別パワーローコンプ)。経緯は `plans/nice_bulk_v01.md` §経緯
- `excite-ab` は excite のオフライン A/B 比較 CLI (`publish = false`)
- `excite_ui` は excite 専用の共通 param/editor。元位置は `crates/shared/src/excite_ui/`
- `crates/crush/src/dsp.rs` と `crates/nice-bulk/src/dsp.rs` のコメントが
  「`crates/excite/src/dsp.rs` から転記」と書いているが、その転記元は現在ここ
