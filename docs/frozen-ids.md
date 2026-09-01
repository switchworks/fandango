# fandango — 公開後変更禁止の識別子一覧

**公開後変更禁止。変更＝既存プロジェクトが壊れる。**

- `VST3_CLASS_ID` を変えると、保存済みプロジェクトからプラグインが見つからなくなる
- パラメータ ID を変えると、保存済みの値が読めなくなる
- `bundler.toml` の表示名を変えると、ホストによっては別プラグイン扱いになる

凍結の効力は**公開の瞬間**から発生する。公開前は変更してよいが、変更したら本ファイルを再生成する。

- **バージョン: `1.0.0`**（workspace 一括。ルート `Cargo.toml`。12 crate が同時に上がる）
- 公開後は単調増加させる（一部ホストが更新判定に使う）
- 生成: ソースからの機械抽出（2026-08-22）
- ID 形式は commit `8bbeb1f` でランダム UUIDv4 へ移行済み。旧 ASCII 形式 `*b"SwxFandango:XXXX"` は廃止

---

## `spread` — 表示名 `fandango - spread`

```
VST3_CLASS_ID = [0x1d, 0xe8, 0x67, 0x05, 0x5d, 0x6c, 0x4d, 0x11, 0x80, 0xf3, 0x20, 0x20, 0x1a, 0x5c, 0xb1, 0x80]
```

パラメータ:

- `bypass` — フィールド `bypass` / 型 `BoolParam` / 既定 `defaults.plain_bool("bypass", false)`
- `expander` — フィールド `expander` / 型 `FloatParam` / 既定 `defaults.plain("expander", 0.5)`
- `inverse` — フィールド `inverse` / 型 `BoolParam` / 既定 `defaults.plain_bool("inverse", false)`
- `center_amount` — フィールド `center_amount` / 型 `FloatParam` / 既定 `defaults.plain("center_amount", 1.0)`
- `bass_preserve` — フィールド `bass_preserve` / 型 `EnumParam<BassPreserve>` / 既定 `BassPreserve::Off`

## `raindrop` — 表示名 `fandango - raindrop`

```
VST3_CLASS_ID = [0x72, 0x15, 0x79, 0xd0, 0x20, 0x06, 0x48, 0x81, 0x88, 0x17, 0xad, 0xab, 0x68, 0xd4, 0xa3, 0x32]
```

パラメータ:

- `bypass` — フィールド `bypass` / 型 `BoolParam` / 既定 `defaults.plain_bool("bypass", false)`
- `time` — フィールド `time` / 型 `FloatParam` / 既定 `defaults.plain("time", 75.0)`
- `feedback` — フィールド `feedback` / 型 `FloatParam` / 既定 `defaults.plain("feedback", 0.50)`
- `width` — フィールド `width` / 型 `FloatParam` / 既定 `defaults.plain("width", 0.0)`
- `mix` — フィールド `mix` / 型 `FloatParam` / 既定 `defaults.plain("mix", 0.50)`

## `gate` — 表示名 `fandango - gate`

```
VST3_CLASS_ID = [0x9e, 0x97, 0x28, 0x2d, 0x87, 0x4a, 0x4b, 0x87, 0x8a, 0xc9, 0xf4, 0xfe, 0x0d, 0x0b, 0x67, 0x42]
```

パラメータ:

- `bypass` — フィールド `bypass` / 型 `BoolParam` / 既定 `defaults.plain_bool("bypass", false)`
- `step_len` — フィールド `step_length` / 型 `EnumParam<StepLengthEnum>` / 既定 `StepLengthEnum::Sixteenth`
- `sync` — フィールド `sync_mode` / 型 `EnumParam<SyncModeEnum>` / 既定 `SyncModeEnum::Bar`
  （2026-08-29 追加）
- `duty` — フィールド `duty` / 型 `FloatParam` / 既定 `defaults.plain("duty", 0.60)`
- `edge` — フィールド `edge` / 型 `FloatParam` / 既定 `defaults.plain("edge", 0.35)`
- `shape` — フィールド `shape` / 型 `FloatParam` / 既定 `defaults.plain("shape", 0.0)`
- `depth` — フィールド `depth` / 型 `FloatParam` / 既定 `defaults.plain("depth", 0.95)`

`#[persist]` キー（パラメータ一覧には出ないが保存形式の一部。**変更＝保存状態の互換が壊れる**）:

- `"editor-state"` — 全 12 crate 共通
- `"pattern"` — パターン本体。`_` `|` `-` の 3 グリフによる文字列（`PatternCode::to_text` の正規形）
- `"pattern-tail"` — `-` で削ったセルの控え（2026-08-29 追加）。同じ 3 グリフ。
  **補助データであり正本ではない。** 本体と食い違ったら常に本体を優先して控えを捨てる

gate は `MIDI_INPUT = MidiConfig::Basic`（2026-08-29。sync = note のため）。
VST3 のイベント入力バスが 1 本増えており、ホストによってはインストゥルメント扱いになる。

## `drawl` — 表示名 `fandango - drawl`

```
VST3_CLASS_ID = [0xcb, 0x27, 0xb0, 0xdf, 0x08, 0xb0, 0x46, 0x30, 0x97, 0x12, 0xc5, 0x67, 0xa4, 0xc2, 0x8e, 0x00]
```

パラメータ:

- `bypass` — フィールド `bypass` / 型 `BoolParam` / 既定 `defaults.plain_bool("bypass", false)`
- `vowel` — フィールド `vowel` / 型 `FloatParam` / 既定 `defaults.plain("vowel", crate::dsp::VOWEL_A)`
- `jitter` — フィールド `jitter` / 型 `FloatParam` / 既定 `defaults.plain("jitter", 0.2)`
- `mix` — フィールド `mix` / 型 `FloatParam` / 既定 `defaults.plain("mix", 1.0)`

## `trip-delay` — 表示名 `fandango - trip delay`

```
VST3_CLASS_ID = [0xca, 0x80, 0xa6, 0x96, 0x04, 0xe7, 0x46, 0xbf, 0xae, 0xf0, 0x2b, 0x79, 0xb5, 0x29, 0x9c, 0xcd]
```

パラメータ:

- `bypass` — フィールド `bypass` / 型 `BoolParam` / 既定 `defaults.plain_bool("bypass", false)`
- `level` — フィールド `level` / 型 `FloatParam` / 既定 `defaults.plain("level", 0.5)`
- `feedback` — フィールド `feedback` / 型 `FloatParam` / 既定 `defaults.plain("feedback", 0.30)`

## `velvet` — 表示名 `fandango - velvet`

```
VST3_CLASS_ID = [0x0e, 0x2c, 0xcd, 0x68, 0xf9, 0x5a, 0x4a, 0x28, 0xb7, 0xd8, 0xd2, 0x7e, 0xa2, 0x43, 0xb4, 0x7a]
```

パラメータ:

- `bypass` — フィールド `bypass` / 型 `BoolParam` / 既定 `defaults.plain_bool("bypass", false)`
- `width` — フィールド `width` / 型 `FloatParam` / 既定 `defaults.plain("width", 0.35)`
- `length` — フィールド `length` / 型 `EnumParam<LengthMs>` / 既定 `LengthMs::Ms30`
- `color` — フィールド `color` / 型 `FloatParam` / 既定 `defaults.plain("color", 0.0)`

## `rotate` — 表示名 `fandango - rotate`

```
VST3_CLASS_ID = [0xc7, 0x24, 0x2b, 0xe5, 0xd2, 0xcb, 0x48, 0xe5, 0x9b, 0xae, 0x80, 0x16, 0x06, 0xa0, 0x97, 0x28]
```

パラメータ:

- `bypass` — フィールド `bypass` / 型 `BoolParam` / 既定 `defaults.plain_bool("bypass", false)`
- `stages` — フィールド `stages` / 型 `EnumParam<Stages>` / 既定 `Stages::Four`
- `freq` — フィールド `freq` / 型 `FloatParam` / 既定 `defaults.plain("freq", 700.0)`

## `crush` — 表示名 `fandango - crush`

```
VST3_CLASS_ID = [0xee, 0xfc, 0x1d, 0xe5, 0x75, 0xe1, 0x43, 0x51, 0xb0, 0x50, 0xbc, 0x76, 0x7d, 0x3e, 0xe2, 0x94]
```

パラメータ:

- `bypass` — フィールド `bypass` / 型 `BoolParam` / 既定 `defaults.plain_bool("bypass", false)`
- `bits` — フィールド `bits` / 型 `FloatParam` / 既定 `defaults.plain("bits", 8.0)`
- `rate` — フィールド `rate` / 型 `FloatParam` / 既定 `defaults.plain("rate", 0.0)`
  - **値は `rate` そのものではなく `log2(rate)`（オクターブ数 0.0..=5.0）**。
    実係数は `rate_from_octaves(p) = 2^p` で、`p = 0` ⇔ `rate = 1`（恒等点）、
    `p = 5` ⇔ `rate = 32`（従来の上限）。表示文字列は実係数を出す。
  - 2026-08-27 に `IntParam`（1..=32、既定 1）から変更。**公開前の変更**。
    連続値化により rate を掃引できるようにするため。
- `mix` — フィールド `mix` / 型 `FloatParam` / 既定 `defaults.plain("mix", 1.0)`

## `tilt` — 表示名 `fandango - tilt`

```
VST3_CLASS_ID = [0x3d, 0x8a, 0x6e, 0x6c, 0xd5, 0x01, 0x4b, 0xf9, 0x89, 0xbc, 0xad, 0x34, 0x58, 0x4e, 0xb9, 0x88]
```

パラメータ:

- `bypass` — フィールド `bypass` / 型 `BoolParam` / 既定 `defaults.plain_bool("bypass", false)`
- `gain` — フィールド `gain` / 型 `FloatParam` / 既定 `defaults.plain("gain", 0.0)`
- `pivot` — フィールド `pivot` / 型 `FloatParam` / 既定 `defaults.plain("pivot", 700.0)`
- `slope` — フィールド `slope` / 型 `FloatParam` / 既定 `defaults.plain("slope", 0.7)`

## `nice-bulk` — 表示名 `fandango - nice bulk`

```
VST3_CLASS_ID = [0x8c, 0x89, 0x43, 0xfc, 0xd6, 0x39, 0x46, 0xec, 0xb3, 0x58, 0x2e, 0x59, 0xeb, 0x0a, 0x0b, 0x72]
```

パラメータ:

- `bypass` — フィールド `bypass` / 型 `BoolParam` / 既定 `defaults.plain_bool("bypass", false)`
- `bass_amount` — フィールド `bass_amount` / 型 `FloatParam` / 既定 `defaults.plain("bass_amount", 0.0)`
- `hi_amount` — フィールド `hi_amount` / 型 `FloatParam` / 既定 `defaults.plain("hi_amount", 0.0)`

## `nimbus` — 表示名 `fandango - nimbus`

```
VST3_CLASS_ID = [0x03, 0xf1, 0x7a, 0x3a, 0x96, 0x2e, 0x4e, 0x99, 0xbd, 0xba, 0x50, 0xd6, 0xd0, 0x32, 0xf4, 0xc8]
```

パラメータ:

- `bypass` — フィールド `bypass` / 型 `BoolParam` / 既定 `defaults.plain_bool("bypass", false)`
- `distance` — フィールド `distance` / 型 `FloatParam` / 既定 `defaults.plain("distance", 0.5)`
- `mix` — フィールド `mix` / 型 `FloatParam` / 既定 `defaults.plain("mix", 1.0)`

## `finalist` — 表示名 `fandango - finalist`

```
VST3_CLASS_ID = [0x6f, 0x2c, 0x91, 0xd4, 0x5b, 0x83, 0x47, 0x1a, 0x8e, 0xc7, 0x3d, 0x40, 0xa9, 0x16, 0xbb, 0x52]
```

パラメータ:

- `bypass` — フィールド `bypass` / 型 `BoolParam` / 既定 `defaults.plain_bool("bypass", false)`
- `input` — フィールド `input_db` / 型 `FloatParam` / 既定 `defaults.plain("input", 0.0)`
- `drive` — フィールド `drive` / 型 `FloatParam` / 既定 `defaults.plain("drive", 0.0)`
- `ceiling` — フィールド `ceiling_db` / 型 `FloatParam` / 既定 `defaults.plain("ceiling", -1.0)`
- `release` — フィールド `release_ms` / 型 `FloatParam` / 既定 `defaults.plain("release", 80.0)`

