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

## cargo フルパス

PATH 未登録の環境では `~/.cargo/bin/cargo` を明示する（Windows/PowerShell なら `"$env:USERPROFILE\.cargoin\cargo.exe"`）。

## Rust / Windows

`~/.claude/rules/rust.md` 参照 (windows-rs 0.58、`#![windows_subsystem = "windows"]`、denormal 対策 `_mm_getcsr`/`_mm_setcsr` 等)。
