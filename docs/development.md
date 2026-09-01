# 開発 — ビルドと目視確認

`README.md` から分離した開発者向けの手順。

---

## ビルド

VST3 bundle のみ (複数 crate は `-p` で列挙する。列挙のみの形式は通らない):

```
cargo nih-plug bundle -p fandango-spread -p fandango-raindrop -p fandango-gate -p fandango-drawl -p fandango-trip-delay -p fandango-velvet -p fandango-rotate -p fandango-crush -p fandango-tilt -p fandango-nice-bulk -p fandango-nimbus -p fandango-finalist --release
```

生成物: `target/bundled/*.vst3`

### GUI プレビュー exe (開発用)

DAW を起動せずに、実プラグインと同じ GUI を単独ウィンドウで開ける exe をまとめて生成する。
nih-plug の standalone ラッパーを使っており、モックではなく実物の editor が開く。

PowerShell:

```
.\scripts\build-previews.ps1
```

- 全 GUI crate の preview exe (`target/release/<crate>_preview.exe`) と VST3 bundle を生成する
- `-Only <crate>` で 1 つに絞れる (例: `.\scripts\build-previews.ps1 -Only nimbus`)
- `-SkipBundle` で bundle を省略し preview exe だけ作る
- 対象は GUI を持つ crate (全 12 crate。`gate` `nimbus` を含む)
- preview exe は開発・目視確認専用であり、配布物には含めない

---

## ライセンスの経緯

fandango は **GPL-3.0-or-later** で配布する。

nih-plug 本体は ISC だが、`nih_export_vst3!()` が使う
[vst3-sys](https://github.com/RustAudio/vst3-sys) バインディングが GPLv3 であり、
これを自前実装へ差し替えない限り、生成される VST3 は GPLv3 に準拠する必要がある。
fandango はこの経路をそのまま使っているため、バンドル全体を GPL-3.0-or-later で配布する。

配布バイナリに対応するソースは、リリースごとに切るバージョンタグで提示する。
