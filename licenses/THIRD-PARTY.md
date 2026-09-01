# fandango — 同梱依存のライセンス

配布する VST3 バイナリは **GPL-3.0-or-later**。`nih_export_vst3!` が vst3-sys (GPL-3) に
依存するため、バイナリ全体が GPL-3 になる。ソース公開は必須。

crate 以外の資産（フォント等）は `licenses/NON-CARGO.md` に手で記録している。

本ファイルは実測由来:

```
cargo license --avoid-dev-deps --filter-platform x86_64-pc-windows-msvc --json
```

- 生成日: 2026-08-22 / 対象プラットフォーム: `x86_64-pc-windows-msvc` / 第三者パッケージ 139 件
- ライセンス本文は `licenses/texts/<hash>.txt`（各 crate 同梱の LICENSE ファイルを内容ハッシュで重複排除）

---

## ライセンス種別ごとの内訳

### (Apache-2.0 OR MIT) AND OFL-1.1 AND Ubuntu-font-1.0 (1)

epaint_default_fonts 0.31.1

### (Apache-2.0 OR MIT) AND Unicode-3.0 (1)

unicode-ident 1.0.24

### 0BSD OR Apache-2.0 OR MIT (1)

adler2 2.0.1

### Apache-2.0 (9)

ab_glyph 0.2.32, ab_glyph_rasterizer 0.1.10, gl_generator 0.14.0, glutin 0.32.3, glutin_egl_sys 0.7.1, glutin_wgl_sys 0.6.1, khronos_api 3.1.0, owned_ttf_parser 0.25.1, winit 0.30.13

### Apache-2.0 AND MIT (1)

dpi 0.1.2

### Apache-2.0 OR BSD-2-Clause OR MIT (1)

zerocopy 0.8.52

### Apache-2.0 OR BSD-3-Clause (2)

moxcms 0.8.1, pxfm 0.1.29

### Apache-2.0 OR BlueOak-1.0.0 OR MIT (1)

anymap3 1.1.0

### Apache-2.0 OR MIT (84)

ahash 0.8.12, anyhow 1.0.103, atomic_refcell 0.1.14, autocfg 1.5.1, backtrace 0.3.76, baseview 0.1.0, bitflags 1.3.2, bitflags 2.13.0, cfg-if 1.0.4, chrono 0.4.45, clap-sys 0.5.0, copypasta 0.10.2, crc32fast 1.5.0, crossbeam 0.8.4, crossbeam-channel 0.5.15, crossbeam-deque 0.8.6, crossbeam-epoch 0.9.18, crossbeam-queue 0.3.12, crossbeam-utils 0.8.21, deranged 0.5.8, ecolor 0.31.1, egui 0.31.1, egui_glow 0.31.1, emath 0.31.1, epaint 0.31.1, equivalent 1.0.2, fdeflate 0.3.7, flate2 1.1.9, getrandom 0.2.17, getrandom 0.3.4, hashbrown 0.17.1, image 0.25.10, indexmap 2.14.0, itoa 1.0.18, keyboard-types 0.6.2, lock_api 0.4.14, log 0.4.33, midi-consts 0.1.0, nohash-hasher 0.2.0, num-conv 0.2.2, num-traits 0.2.19, once_cell 1.21.4, parking_lot 0.12.5, parking_lot_core 0.9.12, pin-project-lite 0.2.17, png 0.18.1, powerfmt 0.2.0, proc-macro2 1.0.106, profiling 1.0.18, quote 1.0.46, rustc-demangle 0.1.27, scopeguard 1.2.0, serde 1.0.228, serde_core 1.0.228, serde_derive 1.0.228, serde_json 1.0.150, serde_spanned 1.1.1, smallvec 1.15.2, smol_str 0.2.2, syn 1.0.109, syn 2.0.118, thiserror 2.0.18, thiserror-impl 2.0.18, time 0.3.53, time-core 0.1.9, time-macros 0.2.31, toml 0.9.12+spec-1.1.0, toml_datetime 0.7.5+spec-1.1.0, toml_parser 1.1.2+spec-1.1.0, toml_writer 1.1.2+spec-1.1.0, ttf-parser 0.25.1, unicode-segmentation 1.13.3, uuid 0.8.2, version_check 0.9.5, widestring 1.2.1, winapi 0.3.9, windows 0.44.0, windows-link 0.2.1, windows-sys 0.52.0, windows-sys 0.61.2, windows-targets 0.42.2, windows-targets 0.52.6, windows_x86_64_msvc 0.42.2, windows_x86_64_msvc 0.52.6

### Apache-2.0 OR MIT OR Zlib (10)

atomic_float 0.1.0, bytemuck 1.25.0, bytemuck_derive 1.10.2, cursor-icon 1.2.0, glow 0.16.0, miniz_oxide 0.8.9, raw-window-handle 0.5.2, raw-window-handle 0.6.2, zune-core 0.5.1, zune-jpeg 0.5.15

### BSL-1.0 (2)

clipboard-win 5.4.1, error-code 3.3.2

### GPL-3.0 (1)

vst3-sys 0.1.0

### ISC (5)

libloading 0.8.9, nih_log 0.3.1, nih_plug 0.0.0, nih_plug_derive 0.1.0, nih_plug_egui 0.0.0

### MIT (16)

atty 0.2.14, cfg_aliases 0.2.1, egui-baseview 0.5.0, glutin-winit 0.5.0, memoffset 0.9.1, open 5.3.6, simd-adler32 0.3.9, tracing 0.1.44, tracing-core 0.1.36, vst3-com 0.1.0, vst3-com-macros 0.2.0, vst3-com-macros-support 0.2.0, winnow 0.7.15, winnow 1.0.3, xml-rs 0.8.29, zmij 1.0.21

### MIT OR Unlicense (4)

byteorder-lite 0.1.0, memchr 2.8.2, termcolor 1.4.1, winapi-util 0.1.11

---

## パッケージ → 同梱されている本文

- `ab_glyph 0.2.32` — Apache-2.0 — [a6a5418b4d67](texts/a6a5418b4d67.txt)
- `ab_glyph_rasterizer 0.1.10` — Apache-2.0 — [a6a5418b4d67](texts/a6a5418b4d67.txt)
- `adler2 2.0.1` — 0BSD OR Apache-2.0 OR MIT — [3aedaafe8ea8](texts/3aedaafe8ea8.txt), [1d47c63586fe](texts/1d47c63586fe.txt), [ce3a2603094e](texts/ce3a2603094e.txt)
- `ahash 0.8.12` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [2646b6d24532](texts/2646b6d24532.txt)
- `anyhow 1.0.103` — Apache-2.0 OR MIT — [6e5c4711bcae](texts/6e5c4711bcae.txt), [ce3a2603094e](texts/ce3a2603094e.txt)
- `anymap3 1.1.0` — Apache-2.0 OR BlueOak-1.0.0 OR MIT — [bdd81e5cfb81](texts/bdd81e5cfb81.txt)
- `atomic_float 0.1.0` — Apache-2.0 OR MIT OR Zlib — [4d9def672833](texts/4d9def672833.txt), [5392ec630c5a](texts/5392ec630c5a.txt), [36b788aea124](texts/36b788aea124.txt)
- `atomic_refcell 0.1.14` — Apache-2.0 OR MIT — [7df059597099](texts/7df059597099.txt), [c953175cd513](texts/c953175cd513.txt)
- `atty 0.2.14` — MIT — [3acad00f27f8](texts/3acad00f27f8.txt)
- `autocfg 1.5.1` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [e6d32072ef5f](texts/e6d32072ef5f.txt)
- `backtrace 0.3.76` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [3b042d3d9719](texts/3b042d3d9719.txt)
- `baseview 0.1.0` — Apache-2.0 OR MIT — [ed19a80b922d](texts/ed19a80b922d.txt), [ce3a2603094e](texts/ce3a2603094e.txt)
- `bitflags 1.3.2` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [9f3c36d2b7d3](texts/9f3c36d2b7d3.txt)
- `bitflags 2.13.0` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [9f3c36d2b7d3](texts/9f3c36d2b7d3.txt)
- `bytemuck 1.25.0` — Apache-2.0 OR MIT OR Zlib — [9e132ef44ef2](texts/9e132ef44ef2.txt), [c0fdcaf865e7](texts/c0fdcaf865e7.txt), [9a00b14a5648](texts/9a00b14a5648.txt)
- `bytemuck_derive 1.10.2` — Apache-2.0 OR MIT OR Zlib — [9e132ef44ef2](texts/9e132ef44ef2.txt), [c0fdcaf865e7](texts/c0fdcaf865e7.txt), [9a00b14a5648](texts/9a00b14a5648.txt)
- `byteorder-lite 0.1.0` — MIT OR Unlicense — [4c8990add918](texts/4c8990add918.txt), [ff007ce11f3f](texts/ff007ce11f3f.txt)
- `cfg-if 1.0.4` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [3b042d3d9719](texts/3b042d3d9719.txt)
- `cfg_aliases 0.2.1` — MIT — [39806df76979](texts/39806df76979.txt), [955a53991c96](texts/955a53991c96.txt)
- `chrono 0.4.45` — Apache-2.0 OR MIT — [efa441c17f27](texts/efa441c17f27.txt)
- `clap-sys 0.5.0` — Apache-2.0 OR MIT — [2b8b815229aa](texts/2b8b815229aa.txt), [ce3a2603094e](texts/ce3a2603094e.txt)
- `clipboard-win 5.4.1` — BSL-1.0 — **本文未同梱**（crate にライセンスファイルが同梱されていない）
- `copypasta 0.10.2` — Apache-2.0 OR MIT — [3d9ca858ae04](texts/3d9ca858ae04.txt), [1a6dae1a4d8c](texts/1a6dae1a4d8c.txt)
- `crc32fast 1.5.0` — Apache-2.0 OR MIT — [669a1e53b9dd](texts/669a1e53b9dd.txt), [8f178d4cc556](texts/8f178d4cc556.txt)
- `crossbeam 0.8.4` — Apache-2.0 OR MIT — [d7e0212195bd](texts/d7e0212195bd.txt), [144111aa0f14](texts/144111aa0f14.txt)
- `crossbeam-channel 0.5.15` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [144111aa0f14](texts/144111aa0f14.txt), [ca40acd247bc](texts/ca40acd247bc.txt)
- `crossbeam-deque 0.8.6` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [144111aa0f14](texts/144111aa0f14.txt)
- `crossbeam-epoch 0.9.18` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [144111aa0f14](texts/144111aa0f14.txt)
- `crossbeam-queue 0.3.12` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [144111aa0f14](texts/144111aa0f14.txt)
- `crossbeam-utils 0.8.21` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [144111aa0f14](texts/144111aa0f14.txt)
- `cursor-icon 1.2.0` — Apache-2.0 OR MIT OR Zlib — [920116928172](texts/920116928172.txt), [7cb149c01d3f](texts/7cb149c01d3f.txt), [e600d9789215](texts/e600d9789215.txt)
- `deranged 0.5.8` — Apache-2.0 OR MIT — [75b9500512bb](texts/75b9500512bb.txt), [03431277c040](texts/03431277c040.txt)
- `dpi 0.1.2` — Apache-2.0 AND MIT — [3d9ca858ae04](texts/3d9ca858ae04.txt), [10a23e8db0bf](texts/10a23e8db0bf.txt)
- `ecolor 0.31.1` — Apache-2.0 OR MIT — **本文未同梱**（crate にライセンスファイルが同梱されていない）
- `egui 0.31.1` — Apache-2.0 OR MIT — **本文未同梱**（crate にライセンスファイルが同梱されていない）
- `egui-baseview 0.5.0` — MIT — [9a712190a2ba](texts/9a712190a2ba.txt)
- `egui_glow 0.31.1` — Apache-2.0 OR MIT — **本文未同梱**（crate にライセンスファイルが同梱されていない）
- `emath 0.31.1` — Apache-2.0 OR MIT — **本文未同梱**（crate にライセンスファイルが同梱されていない）
- `epaint 0.31.1` — Apache-2.0 OR MIT — **本文未同梱**（crate にライセンスファイルが同梱されていない）
- `epaint_default_fonts 0.31.1` — (Apache-2.0 OR MIT) AND OFL-1.1 AND Ubuntu-font-1.0 — **本文未同梱**（crate にライセンスファイルが同梱されていない）
- `equivalent 1.0.2` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [c371f0a7cbb2](texts/c371f0a7cbb2.txt)
- `error-code 3.3.2` — BSL-1.0 — [3cba29011be2](texts/3cba29011be2.txt)
- `fdeflate 0.3.7` — Apache-2.0 OR MIT — [598f87f072f6](texts/598f87f072f6.txt), [391aa8dcafb1](texts/391aa8dcafb1.txt)
- `flate2 1.1.9` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [67d0c65de2bc](texts/67d0c65de2bc.txt)
- `getrandom 0.2.17` — Apache-2.0 OR MIT — [e9b475b5dccf](texts/e9b475b5dccf.txt), [16c35c8abc90](texts/16c35c8abc90.txt)
- `getrandom 0.3.4` — Apache-2.0 OR MIT — [e9b475b5dccf](texts/e9b475b5dccf.txt), [16c35c8abc90](texts/16c35c8abc90.txt)
- `gl_generator 0.14.0` — Apache-2.0 — **本文未同梱**（crate にライセンスファイルが同梱されていない）
- `glow 0.16.0` — Apache-2.0 OR MIT OR Zlib — [a2a5f21be575](texts/a2a5f21be575.txt), [0222e667f35c](texts/0222e667f35c.txt), [2f245547849a](texts/2f245547849a.txt)
- `glutin 0.32.3` — Apache-2.0 — [7e6c1147aab6](texts/7e6c1147aab6.txt)
- `glutin-winit 0.5.0` — MIT — [ca4874c94248](texts/ca4874c94248.txt)
- `glutin_egl_sys 0.7.1` — Apache-2.0 — [7e6c1147aab6](texts/7e6c1147aab6.txt)
- `glutin_wgl_sys 0.6.1` — Apache-2.0 — [7e6c1147aab6](texts/7e6c1147aab6.txt)
- `hashbrown 0.17.1` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [c9c1c33aee59](texts/c9c1c33aee59.txt)
- `image 0.25.10` — Apache-2.0 OR MIT — [598f87f072f6](texts/598f87f072f6.txt), [391aa8dcafb1](texts/391aa8dcafb1.txt)
- `indexmap 2.14.0` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [7e5936a6fa3c](texts/7e5936a6fa3c.txt)
- `itoa 1.0.18` — Apache-2.0 OR MIT — [6e5c4711bcae](texts/6e5c4711bcae.txt), [ce3a2603094e](texts/ce3a2603094e.txt)
- `keyboard-types 0.6.2` — Apache-2.0 OR MIT — [b95542c0be3b](texts/b95542c0be3b.txt), [6435f78d75fe](texts/6435f78d75fe.txt)
- `khronos_api 3.1.0` — Apache-2.0 — **本文未同梱**（crate にライセンスファイルが同梱されていない）
- `libloading 0.8.9` — ISC — [4ad37fc99fec](texts/4ad37fc99fec.txt)
- `lock_api 0.4.14` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [9a2b6b4ad55e](texts/9a2b6b4ad55e.txt)
- `log 0.4.33` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [9f3c36d2b7d3](texts/9f3c36d2b7d3.txt)
- `memchr 2.8.2` — MIT OR Unlicense — [dd445710e6e4](texts/dd445710e6e4.txt), [4c8990add918](texts/4c8990add918.txt), [ff007ce11f3f](texts/ff007ce11f3f.txt)
- `memoffset 0.9.1` — MIT — [02bf11a87b9b](texts/02bf11a87b9b.txt)
- `midi-consts 0.1.0` — Apache-2.0 OR MIT — [7492aeed0586](texts/7492aeed0586.txt), [143566b8d705](texts/143566b8d705.txt)
- `miniz_oxide 0.8.9` — Apache-2.0 OR MIT OR Zlib — [e15c19df6e3d](texts/e15c19df6e3d.txt), [598f87f072f6](texts/598f87f072f6.txt), [8d2f4567f70d](texts/8d2f4567f70d.txt), [bfc8f9133673](texts/bfc8f9133673.txt)
- `moxcms 0.8.1` — Apache-2.0 OR BSD-3-Clause — [04d7efeea5b0](texts/04d7efeea5b0.txt), [78cdf47f7229](texts/78cdf47f7229.txt)
- `nih_log 0.3.1` — ISC — [5f726b93375c](texts/5f726b93375c.txt)
- `nih_plug 0.0.0` — ISC — [0b283b462c07](texts/0b283b462c07.txt)
- `nih_plug_derive 0.1.0` — ISC — **本文未同梱**（crate にライセンスファイルが同梱されていない）
- `nih_plug_egui 0.0.0` — ISC — **本文未同梱**（crate にライセンスファイルが同梱されていない）
- `nohash-hasher 0.2.0` — Apache-2.0 OR MIT — [2b8b815229aa](texts/2b8b815229aa.txt), [9c09d6dfeace](texts/9c09d6dfeace.txt)
- `num-conv 0.2.2` — Apache-2.0 OR MIT — [598f87f072f6](texts/598f87f072f6.txt), [fb9bac179848](texts/fb9bac179848.txt)
- `num-traits 0.2.19` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [9f3c36d2b7d3](texts/9f3c36d2b7d3.txt)
- `once_cell 1.21.4` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [ce3a2603094e](texts/ce3a2603094e.txt)
- `open 5.3.6` — MIT — [018be976a675](texts/018be976a675.txt)
- `owned_ttf_parser 0.25.1` — Apache-2.0 — [a6a5418b4d67](texts/a6a5418b4d67.txt)
- `parking_lot 0.12.5` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [9a2b6b4ad55e](texts/9a2b6b4ad55e.txt)
- `parking_lot_core 0.9.12` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [9a2b6b4ad55e](texts/9a2b6b4ad55e.txt)
- `pin-project-lite 0.2.17` — Apache-2.0 OR MIT — [598f87f072f6](texts/598f87f072f6.txt), [ce3a2603094e](texts/ce3a2603094e.txt)
- `png 0.18.1` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [f218d448a993](texts/f218d448a993.txt)
- `powerfmt 0.2.0` — Apache-2.0 OR MIT — [2ef5dbf41519](texts/2ef5dbf41519.txt), [5641ab41a628](texts/5641ab41a628.txt)
- `proc-macro2 1.0.106` — Apache-2.0 OR MIT — [6e5c4711bcae](texts/6e5c4711bcae.txt), [ce3a2603094e](texts/ce3a2603094e.txt)
- `profiling 1.0.18` — Apache-2.0 OR MIT — **本文未同梱**（crate にライセンスファイルが同梱されていない）
- `pxfm 0.1.29` — Apache-2.0 OR BSD-3-Clause — [04d7efeea5b0](texts/04d7efeea5b0.txt), [78cdf47f7229](texts/78cdf47f7229.txt)
- `quote 1.0.46` — Apache-2.0 OR MIT — [6e5c4711bcae](texts/6e5c4711bcae.txt), [ce3a2603094e](texts/ce3a2603094e.txt)
- `raw-window-handle 0.5.2` — Apache-2.0 OR MIT OR Zlib — [598f87f072f6](texts/598f87f072f6.txt), [e05a8cb7867c](texts/e05a8cb7867c.txt), [c15ed8b9c1ae](texts/c15ed8b9c1ae.txt)
- `raw-window-handle 0.6.2` — Apache-2.0 OR MIT OR Zlib — [598f87f072f6](texts/598f87f072f6.txt), [e05a8cb7867c](texts/e05a8cb7867c.txt), [c15ed8b9c1ae](texts/c15ed8b9c1ae.txt)
- `rustc-demangle 0.1.27` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [3b042d3d9719](texts/3b042d3d9719.txt)
- `scopeguard 1.2.0` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [f498d95a4888](texts/f498d95a4888.txt)
- `serde 1.0.228` — Apache-2.0 OR MIT — [6e5c4711bcae](texts/6e5c4711bcae.txt), [ce3a2603094e](texts/ce3a2603094e.txt)
- `serde_core 1.0.228` — Apache-2.0 OR MIT — [6e5c4711bcae](texts/6e5c4711bcae.txt), [ce3a2603094e](texts/ce3a2603094e.txt)
- `serde_derive 1.0.228` — Apache-2.0 OR MIT — [6e5c4711bcae](texts/6e5c4711bcae.txt), [ce3a2603094e](texts/ce3a2603094e.txt)
- `serde_json 1.0.150` — Apache-2.0 OR MIT — [6e5c4711bcae](texts/6e5c4711bcae.txt), [ce3a2603094e](texts/ce3a2603094e.txt)
- `serde_spanned 1.1.1` — Apache-2.0 OR MIT — [669a1e53b9dd](texts/669a1e53b9dd.txt), [4dbe8833d018](texts/4dbe8833d018.txt)
- `simd-adler32 0.3.9` — MIT — [469d0508b20c](texts/469d0508b20c.txt)
- `smallvec 1.15.2` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [c61640f6c218](texts/c61640f6c218.txt)
- `smol_str 0.2.2` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [ce3a2603094e](texts/ce3a2603094e.txt)
- `syn 1.0.109` — Apache-2.0 OR MIT — [6e5c4711bcae](texts/6e5c4711bcae.txt), [ce3a2603094e](texts/ce3a2603094e.txt)
- `syn 2.0.118` — Apache-2.0 OR MIT — [6e5c4711bcae](texts/6e5c4711bcae.txt), [ce3a2603094e](texts/ce3a2603094e.txt)
- `termcolor 1.4.1` — MIT OR Unlicense — [dd445710e6e4](texts/dd445710e6e4.txt), [4c8990add918](texts/4c8990add918.txt), [ff007ce11f3f](texts/ff007ce11f3f.txt)
- `thiserror 2.0.18` — Apache-2.0 OR MIT — [6e5c4711bcae](texts/6e5c4711bcae.txt), [ce3a2603094e](texts/ce3a2603094e.txt)
- `thiserror-impl 2.0.18` — Apache-2.0 OR MIT — [6e5c4711bcae](texts/6e5c4711bcae.txt), [ce3a2603094e](texts/ce3a2603094e.txt)
- `time 0.3.53` — Apache-2.0 OR MIT — [598f87f072f6](texts/598f87f072f6.txt), [c0943116e05a](texts/c0943116e05a.txt)
- `time-core 0.1.9` — Apache-2.0 OR MIT — [598f87f072f6](texts/598f87f072f6.txt), [c0943116e05a](texts/c0943116e05a.txt)
- `time-macros 0.2.31` — Apache-2.0 OR MIT — [598f87f072f6](texts/598f87f072f6.txt), [c0943116e05a](texts/c0943116e05a.txt)
- `toml 0.9.12+spec-1.1.0` — Apache-2.0 OR MIT — [669a1e53b9dd](texts/669a1e53b9dd.txt), [4dbe8833d018](texts/4dbe8833d018.txt)
- `toml_datetime 0.7.5+spec-1.1.0` — Apache-2.0 OR MIT — [669a1e53b9dd](texts/669a1e53b9dd.txt), [4dbe8833d018](texts/4dbe8833d018.txt)
- `toml_parser 1.1.2+spec-1.1.0` — Apache-2.0 OR MIT — [669a1e53b9dd](texts/669a1e53b9dd.txt), [4dbe8833d018](texts/4dbe8833d018.txt)
- `toml_writer 1.1.2+spec-1.1.0` — Apache-2.0 OR MIT — [669a1e53b9dd](texts/669a1e53b9dd.txt), [4dbe8833d018](texts/4dbe8833d018.txt)
- `tracing 0.1.44` — MIT — [3c8e7847ca19](texts/3c8e7847ca19.txt)
- `tracing-core 0.1.36` — MIT — [3c8e7847ca19](texts/3c8e7847ca19.txt)
- `ttf-parser 0.25.1` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [84adb31eb194](texts/84adb31eb194.txt)
- `unicode-ident 1.0.24` — (Apache-2.0 OR MIT) AND Unicode-3.0 — [6e5c4711bcae](texts/6e5c4711bcae.txt), [ce3a2603094e](texts/ce3a2603094e.txt), [30281f9e52a6](texts/30281f9e52a6.txt)
- `unicode-segmentation 1.13.3` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [60c3522081bf](texts/60c3522081bf.txt)
- `uuid 0.8.2` — Apache-2.0 OR MIT — [aca374a3362a](texts/aca374a3362a.txt), [0eb133022d74](texts/0eb133022d74.txt)
- `version_check 0.9.5` — Apache-2.0 OR MIT — [5798832c3166](texts/5798832c3166.txt), [cfcb552ef0af](texts/cfcb552ef0af.txt)
- `vst3-com 0.1.0` — MIT — **本文未同梱**（crate にライセンスファイルが同梱されていない）
- `vst3-com-macros 0.2.0` — MIT — **本文未同梱**（crate にライセンスファイルが同梱されていない）
- `vst3-com-macros-support 0.2.0` — MIT — **本文未同梱**（crate にライセンスファイルが同梱されていない）
- `vst3-sys 0.1.0` — GPL-3.0 — [4725bf184ff7](texts/4725bf184ff7.txt)
- `widestring 1.2.1` — Apache-2.0 OR MIT — [a6a5418b4d67](texts/a6a5418b4d67.txt), [218fc8c15534](texts/218fc8c15534.txt)
- `winapi 0.3.9` — Apache-2.0 OR MIT — [92170cdc034b](texts/92170cdc034b.txt), [2243f7a86daa](texts/2243f7a86daa.txt)
- `winapi-util 0.1.11` — MIT OR Unlicense — [dd445710e6e4](texts/dd445710e6e4.txt), [2ad1215c12bd](texts/2ad1215c12bd.txt), [ff007ce11f3f](texts/ff007ce11f3f.txt)
- `windows 0.44.0` — Apache-2.0 OR MIT — [a3b3a65335e7](texts/a3b3a65335e7.txt), [689ec0681815](texts/689ec0681815.txt)
- `windows-link 0.2.1` — Apache-2.0 OR MIT — [a3b3a65335e7](texts/a3b3a65335e7.txt), [689ec0681815](texts/689ec0681815.txt)
- `windows-sys 0.52.0` — Apache-2.0 OR MIT — [a3b3a65335e7](texts/a3b3a65335e7.txt), [689ec0681815](texts/689ec0681815.txt)
- `windows-sys 0.61.2` — Apache-2.0 OR MIT — [a3b3a65335e7](texts/a3b3a65335e7.txt), [689ec0681815](texts/689ec0681815.txt)
- `windows-targets 0.42.2` — Apache-2.0 OR MIT — [a3b3a65335e7](texts/a3b3a65335e7.txt), [689ec0681815](texts/689ec0681815.txt)
- `windows-targets 0.52.6` — Apache-2.0 OR MIT — [a3b3a65335e7](texts/a3b3a65335e7.txt), [689ec0681815](texts/689ec0681815.txt)
- `windows_x86_64_msvc 0.42.2` — Apache-2.0 OR MIT — [a3b3a65335e7](texts/a3b3a65335e7.txt), [689ec0681815](texts/689ec0681815.txt)
- `windows_x86_64_msvc 0.52.6` — Apache-2.0 OR MIT — [a3b3a65335e7](texts/a3b3a65335e7.txt), [689ec0681815](texts/689ec0681815.txt)
- `winit 0.30.13` — Apache-2.0 — [3d9ca858ae04](texts/3d9ca858ae04.txt)
- `winnow 0.7.15` — MIT — [c0955b5351b1](texts/c0955b5351b1.txt)
- `winnow 1.0.3` — MIT — [c0955b5351b1](texts/c0955b5351b1.txt)
- `xml-rs 0.8.29` — MIT — [8318ca66788e](texts/8318ca66788e.txt)
- `zerocopy 0.8.52` — Apache-2.0 OR BSD-2-Clause OR MIT — [ebbc0758a032](texts/ebbc0758a032.txt), [46aace8adc5b](texts/46aace8adc5b.txt), [493b3ee6eb0b](texts/493b3ee6eb0b.txt)
- `zmij 1.0.21` — MIT — [ce3a2603094e](texts/ce3a2603094e.txt)
- `zune-core 0.5.1` — Apache-2.0 OR MIT OR Zlib — [7df059597099](texts/7df059597099.txt), [76023cddb6be](texts/76023cddb6be.txt), [01a9e0b6721e](texts/01a9e0b6721e.txt)
- `zune-jpeg 0.5.15` — Apache-2.0 OR MIT OR Zlib — [7df059597099](texts/7df059597099.txt), [76023cddb6be](texts/76023cddb6be.txt), [01a9e0b6721e](texts/01a9e0b6721e.txt)

---

## 本文を取得できなかったパッケージ

SPDX 宣言は上の一覧のとおり。crate 自体にライセンスファイルが入っていないケース。

- `clipboard-win 5.4.1`
- `ecolor 0.31.1`
- `egui 0.31.1`
- `egui_glow 0.31.1`
- `emath 0.31.1`
- `epaint 0.31.1`
- `epaint_default_fonts 0.31.1`
- `gl_generator 0.14.0`
- `khronos_api 3.1.0`
- `nih_plug_derive 0.1.0`
- `nih_plug_egui 0.0.0`
- `profiling 1.0.18`
- `vst3-com 0.1.0`
- `vst3-com-macros 0.2.0`
- `vst3-com-macros-support 0.2.0`

