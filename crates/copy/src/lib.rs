//! `copy.toml` と `copy/*.md` のパース。
//!
//! コピーの単一情報源 (`plans/copy_source_v01.md` 不変条件 1)。README と Gumroad の
//! 商品説明はここから生成する。欠損キー・未知 crate 名・不明な言語コードは既定値で
//! 埋めずにエラーで落とす (同 不変条件 6)。

pub mod markdown;
pub mod render_html;
pub mod render_md;

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// 本スキーマのバージョン。`copy.toml` の `schema_version` がこれと違えば落とす。
pub const SCHEMA_VERSION: u32 = 1;

/// 日英そろって必要な文言。`ja` / `en` 以外のキーはエラーになる。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bilingual {
    pub ja: String,
    pub en: String,
}

/// 英語 1 回だけ置く節 (動作環境・インストール・ライセンス・Disclaimer)。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnOnly {
    pub en: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    pub name: String,
    pub author: String,
}

/// 散文。`copy.toml` 上はファイル名で、[`Copy::load`] が本文へ差し替える。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prose {
    pub lead: Bilingual,
    pub what_is: Bilingual,
    pub promises: Bilingual,
    pub tradeoff: Bilingual,
    pub get: Bilingual,
    pub more: Bilingual,
    pub requirements: EnOnly,
    pub install: EnOnly,
    pub license: EnOnly,
    pub disclaimer: EnOnly,
}

impl Prose {
    fn parts_mut(&mut self) -> Vec<&mut String> {
        vec![
            &mut self.lead.ja,
            &mut self.lead.en,
            &mut self.what_is.ja,
            &mut self.what_is.en,
            &mut self.promises.ja,
            &mut self.promises.en,
            &mut self.tradeoff.ja,
            &mut self.tradeoff.en,
            &mut self.get.ja,
            &mut self.get.en,
            &mut self.more.ja,
            &mut self.more.en,
            &mut self.requirements.en,
            &mut self.install.en,
            &mut self.license.en,
            &mut self.disclaimer.en,
        ]
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Headings {
    pub what_is: Bilingual,
    pub plugins: Bilingual,
    pub get: Bilingual,
    pub more: Bilingual,
    pub requirements: EnOnly,
    pub install: EnOnly,
    pub license: EnOnly,
    pub disclaimer: EnOnly,
}

/// 表の直前に置く導線ブロック。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Starters {
    pub heading: Bilingual,
    /// 掲載順。値は [`Plugin::id`] と一致しなければエラー。
    pub order: Vec<String>,
    pub note: BTreeMap<String, Bilingual>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plugin {
    /// crate 名。`crates/shots` の `Target::name()` と同じ綴り。
    pub id: String,
    /// GUI 右上のラベル。
    pub label: String,
    pub kind: Bilingual,
    pub line: Bilingual,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Copy {
    pub schema_version: u32,
    pub meta: Meta,
    pub tagline: Bilingual,
    pub prose: Prose,
    pub headings: Headings,
    pub starters: Starters,
    #[serde(rename = "plugin")]
    pub plugins: Vec<Plugin>,
}

#[derive(Debug)]
pub struct CopyError(String);

impl fmt::Display for CopyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for CopyError {}

pub(crate) fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(Box::new(CopyError(msg.into())))
}

impl Copy {
    /// リポジトリルートを渡すと `copy.toml` と `copy/*.md` を読む。
    pub fn load(root: &Path) -> Result<Copy> {
        let toml_path = root.join("copy.toml");
        let text = std::fs::read_to_string(&toml_path)
            .map_err(|e| CopyError(format!("{}: {e}", toml_path.display())))?;
        let mut copy: Copy = toml::from_str(&text)
            .map_err(|e| CopyError(format!("{}: {e}", toml_path.display())))?;
        copy.resolve_prose(&root.join("copy"))?;
        copy.validate()?;
        Ok(copy)
    }

    /// 散文フィールドのファイル名を、その中身へ差し替える。
    fn resolve_prose(&mut self, dir: &Path) -> Result<()> {
        for slot in self.prose.parts_mut() {
            let path: PathBuf = dir.join(&*slot);
            let body = std::fs::read_to_string(&path)
                .map_err(|e| CopyError(format!("{}: {e}", path.display())))?;
            if body.trim().is_empty() {
                return err(format!("{}: 中身が空", path.display()));
            }
            *slot = body.trim_end().to_string();
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return err(format!(
                "schema_version {} は未対応 (対応は {SCHEMA_VERSION})",
                self.schema_version
            ));
        }
        if self.plugins.is_empty() {
            return err("[[plugin]] が 1 件も無い");
        }
        let mut seen: Vec<&str> = Vec::new();
        for p in &self.plugins {
            if seen.contains(&p.id.as_str()) {
                return err(format!("plugin id が重複: {}", p.id));
            }
            seen.push(&p.id);
        }
        for id in &self.starters.order {
            if !seen.contains(&id.as_str()) {
                return err(format!("starters.order の未知 crate 名: {id}"));
            }
        }
        for id in self.starters.note.keys() {
            if !self.starters.order.contains(id) {
                return err(format!("starters.note.{id} が starters.order に無い"));
            }
        }
        for id in &self.starters.order {
            if !self.starters.note.contains_key(id) {
                return err(format!("starters.note.{id} が無い"));
            }
        }
        Ok(())
    }

    pub fn ids(&self) -> Vec<&str> {
        self.plugins.iter().map(|p| p.id.as_str()).collect()
    }
}

/// リポジトリルート (`copy.toml` が置かれた場所)。
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/copy の 2 つ上がリポジトリルート")
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded() -> Copy {
        Copy::load(&repo_root()).expect("copy.toml が読めること")
    }

    #[test]
    fn parses_repo_copy_toml() {
        let c = loaded();
        assert_eq!(c.schema_version, SCHEMA_VERSION);
        assert_eq!(c.plugins.len(), 12);
        assert_eq!(c.plugins[0].id, "spread");
    }

    #[test]
    fn prose_is_replaced_by_file_body() {
        let c = loaded();
        assert!(!c.prose.lead.ja.ends_with(".md"));
        assert!(c.prose.install.en.contains("VST3"));
    }

    #[test]
    fn rejects_unknown_starter_id() {
        let mut c = loaded();
        c.starters.order.push("nope".into());
        let e = c.validate().unwrap_err().to_string();
        assert!(e.contains("nope"), "{e}");
    }

    #[test]
    fn rejects_missing_starter_note() {
        let mut c = loaded();
        c.starters.note.remove("finalist");
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_unknown_language_code() {
        let e = toml::from_str::<Bilingual>("ja = \"あ\"\nen = \"a\"\nfr = \"a\"\n")
            .unwrap_err()
            .to_string();
        assert!(e.contains("fr"), "{e}");
    }

    #[test]
    fn rejects_missing_prose_file() {
        let mut c = loaded();
        c.prose.lead.ja = "does-not-exist.md".into();
        assert!(c.resolve_prose(&repo_root().join("copy")).is_err());
    }
}
