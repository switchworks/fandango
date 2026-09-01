# fandango — cargo 依存以外の資産とライセンス

`licenses/THIRD-PARTY.md` は `cargo license` の出力から**自動生成**されるため、
crate 以外の資産（フォント等）は本ファイルへ手で記録する。
THIRD-PARTY.md を再生成しても本ファイルは残る。

---

## フォント

### WDXL Lubrifont JP N — Regular

**リポジトリには含めない。** `assets/fonts/` は `.gitignore` 済み（実体 10 MB、再取得できるため）。
配布物にも含まれない。

- 置き場所: `assets/fonts/WDXLLubrifontJPN-Regular.ttf`
- 取得元: <https://github.com/NightFurySL2001/WD-XL-font>
- バージョン: 2.001（`name` テーブル id 5 より、実測 2026-08-26）
- ライセンス: **SIL Open Font License 1.1**（同リポジトリの `OFL.txt`）
- 著作権表示（`name` テーブル id 0 / 9 より、実測 2026-08-26）:
  - `Copyright 2025 The WDXL Lubrifont Project Authors`
  - `[WDXL Lubrifont] Copyright 2020-2022 (c) NightFurySL2001, Skr-ZERO`
  - `[ZCOOL QingKe HuangYou] Copyright 2018-2022 (c) The ZCOOL QingKe HuangYou Project Authors`

**用途**: 紹介動画のテロップのみ（`plans/promo_video_v01.md` §4 フォント）。
ffmpeg の `drawtext` で使う。**プラグイン本体の GUI では使わない**
（`crates/shared/src/ui/header.rs::install_fonts` は変更しない）。

**環境を作り直したときの手順**:

1. 上の取得元から `WDXLLubrifontJPN-Regular.ttf` を取る
2. `assets/fonts/` へ置く（`scripts/preview-tile-names.ps1` の既定パス）
3. 同じ場所へ上流の `OFL.txt` も置く（義務ではないが出所を残すため）

**OFL 上の位置づけ**: フォントで描画した成果物（動画・画像）の利用に制限はない。
OFL が縛るのは**フォントソフトウェア自体の再配布**であり、本リポジトリは `.ttf` を配らないため
同梱義務は生じない。`.ttf` を誰かに渡す状況になったら `OFL.txt` を必ず添えること。

---

## foobar2000 コンポーネントの依存（VST3 版には含まれない）

`fb2k/foo_dsp_fandango` のビルドにのみ関わる。**VST3 の配布物には一切含まれない。**
SDK 一式はリポジトリ外（`D:\wrk\fb2k-sdk\`）に置き、コミットしない。
以下は 2026-09-01 に実物のライセンスファイルを読んで記録した。

### foobar2000 SDK — version 2025-03-07

- 取得元: <https://www.foobar2000.org/SDK>
- ライセンス: **BSD 系**（`sdk-license.txt`。再配布可・ソース再配布時は著作権表示を保持・
  作者名を推奨表示に使わない）。**GPL-3 と互換**
- 著作権表示: `Copyright (c) 2002-2025, Peter Pawlowski. All rights reserved.`

### pfc — SDK 同梱

- ライセンス: **zlib**（`pfc/pfc-license.txt`）。GPL-3 と互換
- 著作権表示: `Copyright (C) 2002-2024 Peter Pawlowski`

### libPPUI — SDK 同梱

- ライセンス: **zlib**（`libPPUI/libPPUI-license.txt`）。GPL-3 と互換
- 著作権表示: `Copyright (C) 2002-2024 Peter Pawlowski`

### WTL 10 — `WTL10_10320_Final`

- 取得元: <https://sourceforge.net/projects/wtl/>
- ライセンス: **Microsoft Public License (Ms-PL)**（`wtl10/MS-PL.txt`）
- **GPL-3 とは非互換**（FSF の見解）。設定ダイアログが `CDialogImpl` を使うため
  リンクは避けられない。→ **`fb2k/LICENSE-EXCEPTION.md` の追加許諾でこれを解消している**

### ATL / MSVC ランタイム

Windows 版のビルドツールチェーンに同梱されるもの。GPL-3 §1 の
**System Libraries**（対応するソースの提供義務から除かれる範囲）に当たると解している。
本項は法律判断ではなく、Windows 向け GPL ソフトウェアで一般に取られている整理の記録。

### 配布物への同梱

fb2k 版の zip には上記 4 件の**ライセンス本文を同梱する**（BSD・zlib・Ms-PL とも
再配布時の表示を条件にしているため）。VST3 版の zip には含めない（使っていないため）。
