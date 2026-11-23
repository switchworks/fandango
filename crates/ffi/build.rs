//! Patch shared/nice-bulk sources so they compile inside this crate without
//! pulling nih_plug via fandango-shared.

use std::env;
use std::fs;
use std::path::PathBuf;

/// `include!` cannot host `//!` module docs — demote to plain comments.
fn demote_inner_docs(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("//!") {
            let indent = &line[..line.len() - t.len()];
            out.push_str(indent);
            out.push_str("//");
            out.push_str(rest);
        } else if t.starts_with("/*!") {
            out.push_str("// (doc block stripped)");
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());

    let lr4_src = manifest.join("../shared/src/lr4.rs");
    let lr4 = fs::read_to_string(&lr4_src).expect("read lr4.rs");
    // include! is inside crate::shared_dsp where Biquad is already `pub use`d.
    let lr4 = demote_inner_docs(&lr4).replace("use crate::Biquad;\n", "");
    fs::write(out.join("lr4_patched.rs"), lr4).expect("write lr4_patched.rs");
    println!("cargo:rerun-if-changed={}", lr4_src.display());

    let nice_src = manifest.join("../nice-bulk/src/dsp.rs");
    let nice = fs::read_to_string(&nice_src).expect("read nice-bulk dsp.rs");
    // `use` 行だけでなく完全修飾の出現も置く。`#[cfg(test)]` 側は import を経由せず
    // `fandango_shared::Lr4Crossover` と直接書くため、`use` 行だけの置換では test
    // ターゲットが解決に失敗する。
    let nice = demote_inner_docs(&nice).replace(
        "fandango_shared::Lr4Crossover",
        "crate::shared_dsp::Lr4Crossover",
    );
    fs::write(out.join("nice_bulk_dsp_patched.rs"), nice).expect("write nice_bulk patch");
    println!("cargo:rerun-if-changed={}", nice_src.display());

    let biquad = manifest.join("../shared/src/biquad.rs");
    println!("cargo:rerun-if-changed={}", biquad.display());
}
