//! `copygen` — `copy.toml` から各面の文面を生成する。
//!
//! ```text
//! copygen readme  [--out README.md]
//! copygen gumroad [--out tmp/gumroad/description.html]
//! copygen all
//! copygen check
//! ```

use std::path::PathBuf;

use fandango_copy::{render_html, render_md, repo_root, Copy, Result};

fn main() {
    if let Err(e) = run() {
        eprintln!("copygen: {e}");
        std::process::exit(1);
    }
}

const README: &str = "README.md";
const GUMROAD: &str = "tmp/gumroad/description.html";

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    let out = out_flag(args.collect())?;

    let root = repo_root();
    let copy = Copy::load(&root)?;

    match cmd.as_str() {
        "readme" => {
            let path = out.unwrap_or_else(|| root.join(README));
            write(&path, &render_md::render(&copy))
        }
        "gumroad" => {
            let path = out.unwrap_or_else(|| root.join(GUMROAD));
            write(&path, &render_html::render(&copy)?)
        }
        "all" => {
            if out.is_some() {
                return Err("all は --out を取らない".into());
            }
            write(&root.join(README), &render_md::render(&copy))?;
            write(&root.join(GUMROAD), &render_html::render(&copy)?)
        }
        "check" => {
            if out.is_some() {
                return Err("check は --out を取らない".into());
            }
            check(&[
                (root.join(README), render_md::render(&copy)),
                (root.join(GUMROAD), render_html::render(&copy)?),
            ])
        }
        "" => Err("サブコマンドが無い (readme / gumroad / all / check)".into()),
        other => Err(format!("未知のサブコマンド: {other}").into()),
    }
}

fn out_flag(args: Vec<String>) -> Result<Option<PathBuf>> {
    match args.split_first() {
        None => Ok(None),
        Some((flag, rest)) if flag == "--out" && rest.len() == 1 => {
            Ok(Some(PathBuf::from(&rest[0])))
        }
        Some(_) => Err(format!("引数を解釈できない: {}", args.join(" ")).into()),
    }
}

/// 作業ツリーの改行は CRLF になりうる (`core.autocrlf=true`)。改行だけ揃える。
fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// 生成物が `copy.toml` と一致しているかを見る。差分があれば非ゼロ終了。
fn check(pairs: &[(PathBuf, String)]) -> Result<()> {
    let mut stale = Vec::new();
    for (path, expected) in pairs {
        let actual =
            std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if normalize(&actual) != normalize(expected) {
            stale.push(path.display().to_string());
        }
    }
    if !stale.is_empty() {
        return Err(format!("copy.toml と一致しない生成物: {}", stale.join(", ")).into());
    }
    println!("check ok ({} files)", pairs.len());
    Ok(())
}

fn write(path: &std::path::Path, body: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, body)?;
    println!("wrote {}", path.display());
    Ok(())
}
