//! ビルド時刻を環境変数 `BUILD_TIMESTAMP` に注入。
//! DAW 側で表示中の DLL が最新か目視確認するため、editor 右上に埋込む。

fn main() {
    let now = chrono::Local::now();
    println!("cargo:rustc-env=BUILD_TIMESTAMP={}", now.format("%Y%m%d-%H%M"));
    // rerun-if-changed を意図的に emit しない = 全ソース変更で build.rs 再実行。
    // 何も編集せず build しても再実行はされないが、その場合 DLL 自体も再生成されない
    // = timestamp が古いまま = 「DAW が古い DLL 使ってる」の証拠にできる。
}
