//! Gumroad 商品ページ (`--description`) 向け HTML の組版。
//!
//! 受理される要素は `plans/archive/copy_source_v01.md` §2 の実測に従う。**`<img>` は通らない
//! ので出さない** (同 不変条件 5)。画像はカバー機能で別途載せる。文字列は必ず
//! [`markdown::escape`] / [`markdown::to_html`] を通す (同 不変条件 4)。
//!
//! `docs/` への相対リンクを含む `prose.more` は、Gumroad 上では解決できないため
//! 出力しない。

use crate::markdown::{escape, to_html};
use crate::{Copy, Result};

const STYLE: &str = "<style>\
.fandango .ja{opacity:.7;font-size:.9em}\
.fandango table{width:100%;border-collapse:collapse}\
.fandango td,.fandango th{text-align:left;vertical-align:top;padding:.35em .6em}\
</style>";

fn stacked(en: &str, ja: &str) -> String {
    format!(
        "{}<br><span class=\"ja\" lang=\"ja\">{}</span>",
        escape(en),
        escape(ja)
    )
}

pub fn render(c: &Copy) -> Result<String> {
    let mut s = String::new();
    s.push_str(STYLE);
    s.push_str("\n<section class=\"fandango\">\n");

    // タグラインは共有ヘッドで英日を並べる (README と同じ方針)。
    s.push_str(&format!(
        "<blockquote><p><strong>{}</strong><br><strong lang=\"ja\">{}</strong></p></blockquote>
",
        escape(&c.tagline.en),
        escape(&c.tagline.ja)
    ));

    // デモ動画。`<img>` は通らないが `<iframe>` は通る (計画書 §2 の実測)。
    s.push_str(&format!(
        "<div style=\"position:relative;padding-bottom:56.25%;height:0\"><iframe src=\"https://www.youtube-nocookie.com/embed/{}\" title=\"{} demo\" style=\"position:absolute;top:0;left:0;width:100%;height:100%;border:0\" allowfullscreen loading=\"lazy\"></iframe></div>
",
        escape(&c.meta.youtube_id),
        escape(&c.meta.name)
    ));

    s.push_str(&to_html(&c.prose.lead.en)?);

    // --- 共有ブロック (言語分割より上)。導線と収録内容を設計思想より先に出す。
    s.push_str(&h2(&format!("{} / {}", c.headings.get.en, c.headings.get.ja)));
    s.push_str(&to_html(&c.prose.get.en)?);
    s.push_str("<div lang=\"ja\">
");
    s.push_str(&to_html(&c.prose.get.ja)?);
    s.push_str("</div>
");

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

    // 日本語全文。畳まない (§5)。タグラインと導線は共有ヘッドへ移したので繰り返さない。
    s.push_str("<hr>
");
    s.push_str("<div lang=\"ja\">
");
    s.push_str(&to_html(&c.prose.lead.ja)?);
    s.push_str(&h2(&c.headings.what_is.ja));
    s.push_str(&to_html(&c.prose.what_is.ja)?);
    s.push_str(&to_html(&c.prose.promises.ja)?);
    s.push_str(&to_html(&c.prose.tradeoff.ja)?);
    s.push_str("</div>
");

    s.push_str("<hr>
");
    s.push_str(&h2(&c.headings.disclaimer.en));
    s.push_str(&to_html(&c.prose.disclaimer.en)?);
    s.push_str("</section>\n");

    debug_assert!(!s.contains("<img"), "不変条件 5: img を出さない");
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

    s.push_str("<table>\n<thead><tr><th>Plugin</th><th>Type</th><th></th></tr></thead>\n<tbody>\n");
    for p in &c.plugins {
        s.push_str(&format!(
            "<tr><td><code>:{}</code></td><td>{}</td><td>{}</td></tr>\n",
            escape(&p.id),
            stacked(&p.kind.en, &p.kind.ja),
            stacked(&p.line.en, &p.line.ja),
        ));
    }
    s.push_str("</tbody>\n</table>\n");
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
