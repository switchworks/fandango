# fandango — 作業規約

## ビルドは必ず `cargo nih-plug bundle` を使う

DAW 側の VST3 フォルダから `target/bundled/*.vst3` へシンボリックリンクが張ってある。bundle を更新すれば DAW 再スキャン不要で即反映。

**必須手順** (単一 crate は名前を直に、複数 crate は `-p` で列挙する。列挙のみの形式は通らない):
```
~/.cargo/bin/cargo nih-plug bundle fandango-nice-bulk --release
```

全 crate 一括:
```
~/.cargo/bin/cargo nih-plug bundle -p fandango-spread -p fandango-raindrop -p fandango-gate -p fandango-drawl -p fandango-trip-delay -p fandango-velvet -p fandango-rotate -p fandango-crush -p fandango-tilt -p fandango-nice-bulk -p fandango-nimbus -p fandango-finalist --release
```

**禁止**: `cargo build` 単独で作業完了扱いにする。bundle 生成しないと DLL が `target/{debug,release}/` にしか置かれず DAW に届かない。

**crate → bundle 名対応**は `bundler.toml` 参照。

## 保留事項は `plans/parked-decisions.md` に必ず書く

> `plans/` は非公開（`.gitignore` 済み）。公開リポジトリには含まれない。

「気づいたが今は裁定しない」と決めたら、その場で `plans/parked-decisions.md` に 1 行足す。
後でまとめて書かない。各項目に「**何が分かれば裁定できるか**」を必ず添える。
裁定したら §決着済 へ移し、結論と根拠を残す（削除しない。同じ議論の再演を防ぐため）。

**新しい作業に入る前に本ファイルを読む。** 現に未裁定の項目が実装判断に影響する場合がある。

## Gumroad 説明文は `private/scripts/publish-gumroad.ps1` で投入する

正本は `copy.toml`。手順は `copy.toml` 編集 → `copygen all` → 本スクリプトの一本道。

```
cargo run -q -p fandango-copy --bin copygen -- all
./private/scripts/publish-gumroad.ps1
```

スクリプトと商品 ID は `private/` に置く (`.gitignore` 済み)。ID は
`private/gumroad-product-id.txt`。**`scripts/` は公開リポジトリに入るので商品 ID を
書かない。**

**禁止**: `gumroad products update --description` を素で叩く手順をユーザーに渡す。
`--description` は文字列しか受けない (`--custom-html` と違いパス不可) ため HTML を
シェル経由で渡すことになり、Windows PowerShell 5.1 では BOM 無し UTF-8 が CP932 として
読まれて日本語が全壊する (2026-09-01 に公開ページで実害)。**CLI は成功を返す** ので
投入後の照合なしには気づけない。スクリプトは UTF-8 読みを明示し、投入後に
`products view --json` から読み戻して照合語の存在を確認する。

**禁止**: Gumroad の Web エディタで説明文欄を開く。開くだけで内部形式へ正規化され、
Save すると CLI で入れた組版が失われる。確認は公開ページか `products view` で行う。

## cargo フルパス

PATH 未登録の環境では `~/.cargo/bin/cargo` を明示する（Windows/PowerShell なら `"$env:USERPROFILE\.cargoin\cargo.exe"`）。

## Rust / Windows

`~/.claude/rules/rust.md` 参照 (windows-rs 0.58、`#![windows_subsystem = "windows"]`、denormal 対策 `_mm_getcsr`/`_mm_setcsr` 等)。
