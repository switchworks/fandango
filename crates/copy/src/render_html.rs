//! Gumroad 商品ページ (`--description`) 向け HTML の組版。
//!
//! 受理される要素は `plans/archive/copy_source_v01.md` §2 の実測に従う。**`<img>` は通らない
//! ので出さない** (同 不変条件 5)。
//!
//! **保存されることと描画されることは別**。2026-09-01 の実機実測 (公開ページの DOM) で、
//! `table` は行ごと、`iframe` は要素ごと落ち、`section` は本文全体が描画されなくなった。
//! 描画されるのは `p` `h2` `h3` `ul` `ol` `li` `blockquote` `pre` `code` `strong` `em`
//! `a[href]` `br` `hr` のみ。ここではその範囲だけを出す。動画はカバー機能 (`--preview-video`) で載せる。画像はカバー機能で別途載せる。文字列は必ず
//! [`markdown::escape`] / [`markdown::to_html`] を通す (同 不変条件 4)。
//!
//! `docs/` への相対リンクを含む `prose.more` は、Gumroad 上では解決できないため
//! 出力しない。

use crate::markdown::{escape, to_html};
use crate::{Copy, Result};

fn stacked(en: &str, ja: &str) -> String {
    format!(
        "{}<br>{}",
        escape(en),
        escape(ja)
    )
}

pub fn render(c: &Copy) -> Result<String> {
    let mut s = String::new();

    // タグラインは共有ヘッドで英日を並べる (README と同じ方針)。
    s.push_str(&format!(
        "<blockquote><p><strong>{}</strong><br><strong lang=\"ja\">{}</strong></p></blockquote>
",
        escape(&c.tagline.en),
        escape(&c.tagline.ja)
    ));


    s.push_str(&to_html(&c.prose.lead.en)?);

    // --- 共有ブロック (言語分割より上)。収録内容を設計思想より先に出す。
    //
    // `prose.get` は Gumroad では出さない。3 項目のうち Gumroad 自身への
    // リンクが今いるページの自己参照になり、価格と配布形式の説明も購入 UI と
    // 重複する。残る 2 項目も別経路がある: デモ動画はカバー枠で再生でき、
    // GitHub は License 節からリンクされる。README では 3 項目とも出す。
    s.push_str("<hr>
");
    s.push_str(&plugins_section(c));

    // --- ここから英語全文 ---
    s.push_str("<hr>
");
    s.push_str(&h2(&c.headings.what_is.en));
    s.push_str(&to_html(&c.prose.what_is.en)?);
    s.push_str(&to_html(&c.prose.promises.en)?);
    s.push_str(&to_html(&c.prose.tradeoff.en)?);

    s.push_str(&h2(&c.headings.requirements.en));
    s.push_str(&to_html(&c.prose.requirements.en)?);
    s.push_str(&h2(&c.headings.install.en));
    s.push_str(&to_html(&c.prose.install.en)?);
    s.push_str(&h2(&c.headings.license.en));
    s.push_str(&to_html(&c.prose.license.en)?);

    s.push_str(&h2(&c.headings.fb2k.en));
    s.push_str(&to_html(&c.prose.fb2k.en)?);

    // 日本語全文。畳まない (§5)。タグラインと導線は共有ヘッドへ移したので繰り返さない。
    s.push_str("<hr>
");
    s.push_str(&to_html(&c.prose.lead.ja)?);
    s.push_str(&h2(&c.headings.what_is.ja));
    s.push_str(&to_html(&c.prose.what_is.ja)?);
    s.push_str(&to_html(&c.prose.promises.ja)?);
    s.push_str(&to_html(&c.prose.tradeoff.ja)?);

    s.push_str("<hr>
");
    s.push_str(&h2(&c.headings.disclaimer.en));
    s.push_str(&to_html(&c.prose.disclaimer.en)?);

    debug_assert!(!s.contains("<img"), "不変条件 5: img を出さない");
    debug_assert!(!s.contains("<table"), "描画されない: table を出さない");
    debug_assert!(!s.contains("<section"), "描画を全滅させる: section を出さない");
    debug_assert!(!s.contains("<iframe"), "描画されない: iframe を出さない");
    Ok(s)
}

fn h2(text: &str) -> String {
    format!("<h2>{}</h2>\n", escape(text))
}

fn plugins_section(c: &Copy) -> String {
    let mut s = h2(&format!(
        "{} / {}",
        c.headings.plugins.en, c.headings.plugins.ja
    ));
    s.push_str(&format!(
        "<p><strong>{}</strong><br><span class=\"ja\" lang=\"ja\">{}</span></p>\n",
        escape(&c.starters.heading.en),
        escape(&c.starters.heading.ja)
    ));
    s.push_str("<ul>\n");
    for id in &c.starters.order {
        let note = &c.starters.note[id];
        s.push_str(&format!(
            "<li><code>:{}</code> — {}</li>\n",
            escape(id),
            stacked(&note.en, &note.ja)
        ));
    }
    s.push_str("</ul>\n");

    s.push_str(&format!(
        "<p><strong>{}</strong><br><span class=\"ja\" lang=\"ja\">{}</span></p>
",
        escape(&c.headings.plugins_all.en),
        escape(&c.headings.plugins_all.ja)
    ));
    s.push_str("<ul>
");
    for p in &c.plugins {
        s.push_str(&format!(
            "<li><code>:{}</code> — {} — {}<br>{} — {}</li>
",
            escape(&p.id),
            escape(&p.kind.en),
            escape(&p.line.en),
            escape(&p.kind.ja),
            escape(&p.line.ja),
        ));
    }
    s.push_str("</ul>
");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html() -> String {
        render(&Copy::load(&crate::repo_root()).unwrap()).unwrap()
    }

    #[test]
    fn has_no_image() {
        let h = html();
        assert!(!h.contains("<img"));
        assert!(!h.contains("background-image"));
        assert!(!h.contains("<picture"));
    }

    /// `prose` に項目を足して `Prose::parts_mut()` への追加を忘れると、本文の
    /// 代わりにファイル名がそのまま出る (2026-09-01 に `fb2k` で踏んだ:
    /// `<p>fb2k.en.md</p>`)。`parts_mut()` を使って検査すると同じ列挙を見に
    /// 行くだけで漏れを検出できないため、`copy/` の実ファイル名を独立に
    /// 読んで突き合わせる。
    #[test]
    fn no_prose_filename_leaks_into_output() {
        let h = html();
        let dir = crate::repo_root().join("copy");
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).expect("copy/ が読めること") {
            let name = entry.expect("dir entry").file_name();
            let name = name.to_string_lossy().into_owned();
            if !name.ends_with(".md") {
                continue;
            }
            assert!(
                !h.contains(&format!(">{name}<")),
                "本文へ置換されていない: {name}"
            );
            checked += 1;
        }
        assert!(checked > 0, "copy/ に .md が 1 つも無い");
    }

    #[test]
    fn covers_all_twelve() {
        let h = html();
        let c = Copy::load(&crate::repo_root()).unwrap();
        for p in &c.plugins {
            assert!(h.contains(&format!("<code>:{}</code>", p.id)), "{}", p.id);
            assert!(h.contains(&escape(&p.line.en)), "{}", p.id);
            assert!(h.contains(&escape(&p.line.ja)), "{}", p.id);
        }
    }

    #[test]
    fn escapes_install_path() {
        assert!(html().contains(r"C:\Program Files"));
    }
}
