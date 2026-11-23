# foobar2000 native DSP port (`foo_dsp_fandango`)

fandango DSP を foobar2000 v2 (x64) ネイティブコンポーネントとして配布する。

## 前提

| 項目 | 場所 / 内容 |
|------|-------------|
| foobar2000 | v2 x64 (`C:\Program Files\foobar2000`) |
| SDK | `D:\wrk\fb2k-sdk\`（リポジトリ外。コミットしない） |
| WTL | `D:\wrk\fb2k-sdk\wtl10\`（`Releases\WTL10_10320_Final.zip` を展開） |
| ATL | VS Build Tools の **C++ ATL** コンポーネント |
| MSVC | PlatformToolset **v143**（SDK プロジェクトも `/p:PlatformToolset=v143` で上書き） |
| Rust staticlib | `crates/ffi` → `target/release/fandango_ffi.lib` |

`D:\wrk\fb2k-sdk\Directory.Build.props` が WTL include を全 SDK プロジェクトに注入する。

## ビルド

```powershell
# 1) Rust FFI
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --release -p fandango-ffi

# 2) C++ コンポーネント（本機では MSBuild を直パス指定する。理由は下記）
& "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\MSBuild.exe" `
  "fb2k\foo_dsp_fandango\foo_dsp_fandango.sln" `
  /p:Configuration=Release /p:Platform=x64 /p:PlatformToolset=v143 /m
```

> **注意**: 当初ここに書いていた
> `vswhere.exe -latest -requires Microsoft.Component.MSBuild -find "MSBuild/**/Bin/MSBuild.exe"`
> は、本機（VS 2022 **Build Tools**）では**空文字列を返して動かない**
> （`vswhere.exe` 自体は存在し、`-property installationPath` は正しく
> `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools` を返す）。
> 別マシンへ移す際は `vswhere -property installationPath` で導入先を確認し、
> その配下の `MSBuild\Current\Bin\MSBuild.exe` を直接指定するのが確実。

成果物: `fb2k\foo_dsp_fandango\Release\foo_dsp_fandango.dll`

DSP の数式は `crates/nice-bulk` 等の Rust 実装を `crates/ffi` の `build.rs` が
取り込む構成（`*_patched.rs` を生成）。**Rust 側の DSP を直したら C++ は無変更で
上記 1) → 2) を再実行するだけでよい**（数式の二重管理は不要）。

> **ただし自動追随するのは数式だけ。** パラメータの**型・レンジ・既定値・保存形式**を
> 変えた場合は C++ 側の追随が要る。触る箇所は
> **FFI シグネチャ（`crates/ffi/src/lib.rs`）/ ヘッダ（`crates/ffi/include/fdg_ffi.h`）/
> ダイアログと永続化（`dsp_*.cpp`）** の 3 系統。
>
> **`fdg_ffi.h` と Rust の `#[no_mangle]` の不一致はリンク時に検出されない**
> （C ABI は型を見ない）。ズレると黙って壊れるので、型を変えたら必ず目視照合すること。
>
> 保存形式を変えるときは `dsp_*.cpp` の `version` を上げ、**旧版の移行パスを書く**
> （黙って既定値へ落とさない）。
>
> 実例: crush の `rate` 実数化（2026-08-28）。VST3 側だけ直して fb2k を放置したため、
> fb2k 版では整数値しか設定できない状態が一時発生した。

## インストール（手動）

```
%APPDATA%\foobar2000-v2\user-components-x64\foo_dsp_fandango\foo_dsp_fandango.dll
```

foobar2000 を再起動 → Preferences → Components に **fandango DSP 0.1.0**、  
DSP Manager に **fandango spread**。

パッケージ: `fb2k\dist\foo_dsp_fandango.fb2k-component`（zip 拡張子変更。DLL をルートに格納）を Components 画面へドロップしても可。

## アーキテクチャ

- Rust コア: `crates/spread/src/dsp.rs` 等を `#[path]` で参照（VST3 crate 無改変）
- **C++ ソースは ASCII のみ**。日本語コメントを UTF-8 (BOM 無) で書くと MSVC が
  CP932 と誤読し、後続行を飲み込んでコンパイルエラーになる
- C ABI: `crates/ffi` (`staticlib`)
- ホスト: `dsp_impl_base` + `dsp_factory_t`（1 DLL / 複数 DSP 可）

## 聞き専 UX 規約（今後の DSP 追加も同じ）

規約:

1. **設定ダイアログに Reset 必須**（左下。fb2k 用デフォルトへ戻しライブ反映）
2. **既定値は VST と別** — ニュートラル基準点をコピーしない。  
   **挿しただけで聴感が変わった** を目指す（弱〜中、ギリギリは攻めるな）
3. VST3 crate の Default は変更しない

### spread 採用既定（fb2k）

| パラメータ | fb2k 既定 | VST 参考 |
|------------|-----------|----------|
| expander | **0.66** | 0.5 = identity |
| center_amount | 1.0 | 同左 |
| bypass / inverse | off | 同左 |

### nice-bulk 採用既定（fb2k）

| パラメータ | fb2k 既定 | VST 参考 |
|------------|-----------|----------|
| bass_amount | **0.30** | 0.0 = identity |
| hi_amount | **0.30** | 0.0 = identity |
| bypass | off | 同左 |

既定を 0.75 から下げた理由: 2026-08-03 の `nice_bulk_reinforce_v01` でノブが正味ゲインを
持つようになり効きが大幅に強くなったため。ダイアログは上が Hi、下が Bass。

**聞き専向けの注意:** ノブは小レベル成分ほど強く持ち上げるため、上げるほどコンプ感が出る。
`bass` は 50% 前後までが素直に効く領域。効き量は曲のスペクトルに依存し、bass と hi で
体感差が出る（仕様。帯域エネルギーが較正点を上回る曲では持ち上げ幅が頭打ちになる）。

### crush 採用既定（fb2k）

| パラメータ | fb2k 既定 | VST 参考 |
|------------|-----------|----------|
| bits | **10.0** | 8.0 |
| rate | **2** | 1 |
| mix | **0.40** | 1.0 |
| bypass | off | 同左 |

### tilt 採用既定（fb2k）

| パラメータ | fb2k 既定 | VST 参考 |
|------------|-----------|----------|
| gain | **+1.5 dB** | 0.0 = identity |
| pivot | 700 Hz | 同左 |
| slope | 0.70 | 同左 |
| bypass | off | 同左 |

### finalist 採用既定（fb2k）

| パラメータ | fb2k 既定 | VST 参考 |
|------------|-----------|----------|
| input | **+6.0 dB** | 0.0 |
| drive | 0.0 = 厳密恒等 | 同左 |
| ceiling | -1.0 dB | 同左 |
| release | 80 ms | 同左 |
| bypass | off | 同左 |

fb2k 既定を VST と変えるのは **input のみ**（+6 dB、音量を上げてから天井で止める利便性）。
drive / ceiling / release は VST3 と同一値。

**レイテンシあり**: lookahead 2.5 ms から D = 2N-2 サンプル（N = ceil(2.5ms·sr)）
遅れる。`get_latency()` で申告し、`on_endoftrack` / `on_endofplayback` で無音を
D サンプル押し込んで遅延線を吐き切る（`need_track_change_mark()` = true）。
これを怠るとトラック末尾が D サンプル欠ける。

多チャンネルは 2ch ずつ独立インスタンス。ペア内はステレオリンク、ペア間はリンクしない。

## 範囲

- 同一 DLL: **spread / nice bulk / crush / tilt / finalist**
- spread: bypass / expander / inverse / center_amount（bass preserve 未移植）
- nice-bulk: bypass / bass_amount / hi_amount
- crush: bypass / bits / rate / mix
- tilt: bypass / gain / pivot / slope
- finalist: bypass / input / drive / ceiling / release
