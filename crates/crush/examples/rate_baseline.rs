//! 現行 crush の音を SHA-256 で固定する基準ハーネス。
//!
//! **目的**: オーバーサンプリング / BLEP へ踏み込むと「整数 `rate` の音まで変わりうる」
//! という懸念に対し、**等価かどうかを耳の印象ではなくハッシュの一致/不一致にする**。
//!
//! `rate = 1..32` × `bits = 1 / 4 / 8 / 16` の 128 組を現行実装で走らせ、
//! `bits` ごとに 32 レートぶんを連結した SHA-256 を取る。
//! 期待値は `EXPECTED` に埋め込んであり、**不一致ならどの `rate` が動いたかまで報告する**。
//!
//! 実行: cargo run -p fandango-crush --example rate_baseline --release
//! 一致すれば exit 0、不一致なら exit 1。

use fandango_crush::dsp::CrushChannel;

// ---- SHA-256 (FIPS 180-4)。新しい依存を足さないため自前。 ----
// **`sha256_self_test` が既知テストベクタで検証する。** これを外さないこと。
// ハッシュ関数が誤っていると、基準値が静かに無意味になる (不一致を検出できない)。

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

struct Sha256 {
    h: [u32; 8],
    buf: [u8; 64],
    len_buf: usize,
    total: u64,
}

impl Sha256 {
    fn new() -> Self {
        Self {
            h: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            buf: [0; 64],
            len_buf: 0,
            total: 0,
        }
    }

    fn compress(&mut self) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                self.buf[i * 4], self.buf[i * 4 + 1], self.buf[i * 4 + 2], self.buf[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = self.h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v[7] = v[6]; v[6] = v[5]; v[5] = v[4];
            v[4] = v[3].wrapping_add(t1);
            v[3] = v[2]; v[2] = v[1]; v[1] = v[0];
            v[0] = t1.wrapping_add(t2);
        }
        for i in 0..8 {
            self.h[i] = self.h[i].wrapping_add(v[i]);
        }
    }

    fn update(&mut self, data: &[u8]) {
        self.total += data.len() as u64;
        for &b in data {
            self.buf[self.len_buf] = b;
            self.len_buf += 1;
            if self.len_buf == 64 {
                self.compress();
                self.len_buf = 0;
            }
        }
    }

    fn finalize(mut self) -> String {
        let bits = self.total * 8;
        self.update(&[0x80]);
        while self.len_buf != 56 {
            self.update(&[0x00]);
        }
        // update が total を進めてしまうので長さは退避済みの値を使う。
        self.buf[56..64].copy_from_slice(&bits.to_be_bytes());
        self.len_buf = 64;
        self.compress();
        self.h.iter().map(|w| format!("{w:08x}")).collect()
    }
}

fn sha256_self_test() {
    let mut h = Sha256::new();
    h.update(b"abc");
    let got = h.finalize();
    const WANT: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert_eq!(got, WANT, "SHA-256 実装が誤っている。基準値が無意味になるので続行しない");
}

// ---- ここから基準の定義 ----

/// 基準は 48 kHz で取る。`DcBlocker` が sr 依存なので **sr を変えたら基準も別物**。
const SR: f32 = 48_000.0;
const N: usize = 48_000;

/// 基準入力。**この式を変えたら基準ハッシュは全部無効になる。**
/// 正弦だけだと S&H の掴む位相が偏るので、非整数比の倍音と広帯域成分を混ぜる。
fn baseline_input() -> Vec<f32> {
    let mut rng = 0x2545_F491_u32;
    (0..=N)
        .map(|i| {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            let noise = (rng as i32) as f32 / 2_147_483_647.0;
            let t = i as f32 / SR;
            let tau = 2.0 * std::f32::consts::PI;
            0.45 * (tau * 220.0 * t).sin()
                + 0.20 * (tau * 1237.0 * t).sin()
                + 0.10 * (tau * 4519.0 * t).sin()
                + 0.05 * noise
        })
        .collect()
}

/// 1 組ぶんの出力バイト列 (f32 のビットパターン、リトルエンディアン)。
///
/// **経路 F (区間平均) の因果版は出力が 1 サンプル遅れる**
/// 整数 `rate` での「現行と bit 一致」は
/// **1 サンプルシフトして一致**という意味なので、ここで位置を揃える:
/// 入力を 1 サンプル余分に (`N+1` 個) 流し、出力の先頭 1 個を捨てて `N` 個を採る。
/// これにより基準ハッシュ 4 本は実装変更をまたいで**そのまま一致するはず**であり、
/// ハッシュを更新せずに恒等性を検定できる。
fn render_bytes(x: &[f32], bits: f32, rate: f32) -> Vec<u8> {
    let mut ch = CrushChannel::new();
    ch.set_sample_rate(SR);
    let mut out = Vec::with_capacity(N * 4);
    for (i, &v) in x.iter().enumerate() {
        let y = ch.process(v, bits, rate, 1.0);
        if i > 0 {
            out.extend_from_slice(&y.to_le_bytes());
        }
    }
    out
}

fn hash_for_bits(x: &[f32], bits: f32) -> String {
    let mut h = Sha256::new();
    for rate in 1..=32u32 {
        h.update(&render_bytes(x, bits, rate as f32));
    }
    h.finalize()
}

/// **2026-08-27 の現行実装 (commit f442b1d 時点) で取得した基準値。**
/// `bits` ごとに `rate = 1..32` を連結した SHA-256。
/// 音を変える変更を入れたときだけ、意図的に更新する。
///
/// **`bits = 1` を採らない理由**: 1 bit では `Δ = 2/(2^1-1) = 2` なので
/// `m = floor(x/Δ + 1/2)` は `|x| < 1` で常に 0 になり、**出力が恒等的に無音**。
/// 基準入力のピークは 0.8 なので、`bits = 1` の行は全 `rate` で同一ハッシュになり
/// **何も検出しない**（実測で確認済み）。これは crush の欠陥ではなく mid-tread 1 bit の
/// 性質。代わりに `bits = 2`（`Δ = 2/3`、量子化段が実際に動く最小値）を使う。
const EXPECTED: [(f32, &str); 4] = [
    (2.0, "d76a9d2df2adc209bc3d613c6d2bf485524a422db4b8debefbd083d6a0d83d0c"),
    (4.0, "641ad9b87c0acfbcd067eb6bb512565632a3c860a72e203c6b148d58bb9ad610"),
    (8.0, "9a644343038c9ea000cd281a4ac3794ab5517777add4445bbf7407ce25272884"),
    (16.0, "89452bed960ab8eaae4ab45170beee9b60957e44690bd3f88e115cbcff366db0"),
];

/// `rate` 単位の期待値 (SHA-256 の先頭 16 桁 = 64 bit)。**どの `rate` が動いたかの特定用。**
/// 出荷判定の本体は `EXPECTED` の 256 bit 側で、こちらは所在を絞るための索引。
/// 行の順は `EXPECTED` と同じ (`bits` = 1 / 4 / 8 / 16)、列は `rate` = 1..32。
const EXPECTED_PER_RATE: [[&str; 32]; 4] = [
    [
        "bbb74d4f302a532c", "3fe8ec41c48969d8", "4b0cc54002501ccb", "87c7386f27350325",
        "43a6b68c7d0721ca", "6e5de34fb4026bd2", "7b16b77adf82089b", "d62d337566497f29",
        "ffe7299d44a79f86", "eff87402b5d95ba3", "c1b3b533abc0d233", "7b9bfb0c5d1b778a",
        "d0d40ef0efab4db2", "aac227747e55b8e3", "0220f994730b5631", "49cc334ef421b5b1",
        "abd59ccb9ebb675d", "1ee83c261803c9ae", "b91c6848c07cf523", "d740c6fb39d7002e",
        "ac0c1ae2cd5fcdec", "1b59948c97a4ab40", "33cec78f3326a34b", "b5898dc272fc0afb",
        "b55bba84fb102bc4", "f2ccbdfd8f6f0cea", "8bf0051606347149", "788df8c81753ca2a",
        "1bcc1488126cccc5", "759512b4cd8c9900", "b81f0d80f1a7f38a", "cd0e119f7b138c8e",
    ],
    [
        "fdb6f02f0886015c", "6e5a8e871029b9a5", "f069fc7c845b1fe9", "b60066901e300234",
        "81ce9c5e87fd9783", "80275719a21b5fcd", "d8a0f733e81f3f41", "90e56f74b88dca36",
        "388abddcf0d04179", "adec2a4a2096ef27", "7e0877459014e853", "8f016df0cc03cd16",
        "ee20ba6edc315e3c", "dc5a017c3371084d", "af12e76ce76bc26b", "00b8609e6c074535",
        "9e8867d003726497", "b7178281ef037ce7", "ca733102efc0cd5a", "189ef437f332a2c2",
        "dc5930ed720b9b18", "1d442d8685c58bac", "0bfffb4556fe4b5e", "b48e8a4097e89936",
        "889918ce23deadb4", "fd9599f6a9a40270", "9ebd4ec4ae7b071a", "3531636231642ffe",
        "b8c3f642c095d462", "48ad11cacd1afb7e", "0a1f6601f36cf5fa", "7fb55259d2905c47",
    ],
    [
        "d575c21f09f3988f", "57294caf822f97ff", "0a19c5092a837700", "809ba346652d5375",
        "9d1ccf096f8a309d", "325f8b19cced38c9", "21480a79fe9438e9", "88ea73e1095ead6e",
        "d63bcf1ba8969d2b", "d0dc07b24ecc6dc9", "256dc7b495353b08", "97eed32f3033266c",
        "8f024988aae7a832", "0a92a192f45f155d", "72820c3ae690ce5e", "f7b7474621b0c987",
        "b1e77d77b4b570e2", "cde6517df03483c7", "4da99793cce7d6a5", "b2b2f7c35d959c97",
        "08d26914baece5c8", "5c5ce001a46f1193", "787069086cd8976f", "252bda1d5dc0763a",
        "80f506e87d87c281", "83d3a8210a09c225", "6650dd0d1ecc142c", "acfb0fcf15505e30",
        "c12b476674bc2236", "e7474f0c0fc9b0de", "8e5bb81a83141f61", "91935fc5d4f6e6f0",
    ],
    [
        "fd318c72a8ea93c5", "e3903a1a7d0c1ad7", "f11671ab165e46a2", "609f17616c7ef322",
        "2dec7394a8670eb0", "0f2b383da91ab903", "5baebb735f04778a", "54338ae9678acf39",
        "2be13872e28856e3", "d8eb261573904b34", "8bf8c69b7084d3c0", "750894d84d4c829e",
        "31aaa9deaef3f306", "ba6793d3ee390b36", "ae0e415acb973b01", "4a9f9fdb17f5a555",
        "b605e9eaeb5e65ca", "191518de2850e2ee", "30b95fe751b6afc7", "4f3f65a6826ad7eb",
        "627b7f62cbfe8458", "e0719ff34e5b5286", "aba679d51c36d31d", "1558f6fbb0455541",
        "e70447ee67daa6d0", "b6bcf21ad1273205", "1e5a3d7ab7a62316", "54bda10f88f59450",
        "aa2d8762906130a1", "85105207f83911f2", "51c020f703461ef1", "12831d83367b9db1",
    ],
];

fn main() {
    sha256_self_test();
    let x = baseline_input();
    println!("crush 現行音の基準 (sr = {SR} Hz, n = {N}, mix = 1.0, rate = 1..32)");
    println!();

    let mut ng = Vec::new();
    for (bi, (bits, want)) in EXPECTED.iter().enumerate() {
        let got = hash_for_bits(&x, *bits);
        let mark = if got == *want {
            "一致"
        } else {
            ng.push(bi);
            "**不一致**"
        };
        println!("  bits={bits:<5} {got}  {mark}");
    }

    if !ng.is_empty() {
        println!("
不一致の内訳 (動いた rate だけを挙げる):");
        for bi in ng {
            let (bits, _) = EXPECTED[bi];
            let mut moved = Vec::new();
            for rate in 1..=32u32 {
                let mut h = Sha256::new();
                h.update(&render_bytes(&x, bits, rate as f32));
                let got = h.finalize();
                if got[..16] != *EXPECTED_PER_RATE[bi][rate as usize - 1] {
                    moved.push(rate.to_string());
                }
            }
            println!("  bits={bits}: rate {} が変化 ({} / 32)", moved.join(", "), moved.len());
        }
        std::process::exit(1);
    }
}
