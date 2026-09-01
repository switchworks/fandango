# Fandango プロジェクト指示

## 最優先の設計哲学

fandango の統一軸は **代数閉形式 DSP** です。判断に迷ったら、次の優先順位へ戻ってください。

1. 恒等点は厳密に恒等であること。無処理位置で信号を変えないことをテストで確認する。
2. 有界性・単調性・DC 除去・安全性などの不変条件は、実測の偶然ではなく式の構造から導くこと。
3. 素材の絶対レベルや絶対周波数に結び付くマジックナンバーを最小化し、増やす前に代数で除去できないか検討する。
4. パラメータを増やさない。ユーザーへ正解探索を委ねず、カーブの代数で解く。
5. ヒューリスティック、LUT、補間、しきい値テーブル、カーブの辻褄合わせのための時変自動追従を導入しない。

音の実用性と解法が衝突した場合は音を優先してよい。ただし、場合分けや自動適応を積み上げず、代数の中で解決策を探すこと。非線形は許容するが、閉形式性は捨てない。非線形段を追加・変更する場合は、可能なら閉形式不定積分に基づく ADAA と数値安定性の回帰テストを用いること。

## nice-bulk 固有の不変条件

- `bass_amount = hi_amount = 0` では、サチュレータを含めて完全恒等であること。
- パラメータ ID (`bass_amount` / `hi_amount` / `bypass`)、UI、クロスオーバー周波数 (150 Hz / 4 kHz)、`VST3_CLASS_ID` を変更しない。
- `AdaaSaturator` の有理化した不定積分と f64 差分商を f32 に戻さない。小信号の桁落ちノイズと定常音クリックの回帰テストを維持する。
- env フォロワの追加的な自動追従、素材別のノブ範囲、しきい値追加で問題を覆い隠さない。
- foobar2000 版は `crates/fb2k-ffi` 経由で `nice-bulk/src/dsp.rs` を共有する。DSP 数式を二重管理しない。

## 実装・検証

- 依頼範囲外のリファクタ、整形、コメント追加をしない。既存のユーザー変更を戻さない。
- 大きな変更は先に `plans/` へ、目的、不変条件、変更禁止領域、受け入れ基準、検証手順を自己完結で記録する。
- API やビルド手順は記憶で書かず、ソース・`--help`・公式文書で確認する。
- `cargo build` 単独を完了扱いにしない。nice-bulk の変更では少なくとも次を実行する。

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p fandango-nice-bulk --release
& "$env:USERPROFILE\.cargo\bin\cargo.exe" clippy -p fandango-nice-bulk --release -- -D warnings
& "$env:USERPROFILE\.cargo\bin\cargo.exe" nih-plug bundle fandango-nice-bulk --release
```

- foobar2000 向けの DSP 変更では、上記に加えて `fandango-fb2k-ffi` と solution の Release/x64 ビルドを実行し、実機確認項目を報告する。
