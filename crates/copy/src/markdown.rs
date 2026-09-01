//! `copy/*.md` の断片を HTML へ起こす最小変換。
//!
//! 断片は自分たちで書いたものだけなので、対応する記法を段落・箇条書き・
//! コードフェンス・`**強調**`・`` `コード` ``・`[文字](URL)` に限る。**未対応の
//! 記法を見つけたら既定の扱いへ落とさずエラーにする** (`plans/copy_source_v01.md`
//! 不変条件 6)。エスケープは必ず先に済ませてから記法を置換する (同 不変条件 4)。

use crate::{err, Result};

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// 断片 1 本を HTML へ。
pub fn to_html(md: &str) -> Result<String> {
    let mut out = String::new();
    let mut lines = md.lines().peekable();

    while let Some(line) = lines.next() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with("```") {
            let mut body = String::new();
            let mut closed = false;
            for l in lines.by_ref() {
                if l.starts_with("```") {
                    closed = true;
                    break;
                }
                body.push_str(&escape(l));
                body.push('\n');
            }
            if !closed {
                return err("コードフェンスが閉じていない");
            }
            out.push_str(&format!("<pre><code>{}</code></pre>\n", body.trim_end()));
            continue;
        }
        if let Some(item) = list_item(line) {
            out.push_str("<ul>\n");
            out.push_str(&format!("<li>{}</li>\n", inline(item)?));
            while let Some(next) = lines.peek() {
                // 箇条書きの継続行 (2 スペース以上のインデント) は直前の項目へ足す。
                if let Some(item) = list_item(next) {
                    out.push_str(&format!("<li>{}</li>\n", inline(item)?));
                } else if next.starts_with("  ") && !next.trim().is_empty() {
                    let cont = inline(next.trim())?;
                    let at = out.rfind("</li>").expect("直前に <li> がある");
                    out.insert_str(at, &format!(" {cont}"));
                } else {
                    break;
                }
                lines.next();
            }
            out.push_str("</ul>\n");
            continue;
        }
        check_unsupported(line)?;

        // 段落。断片の行内改行は読みやすさのための折返しなので空白へ潰す。
        // ただし `**強調**` で終わる行は小見出しとして書かれているので改行を残す。
        let mut para = inline(line)?;
        let mut prev_is_heading = line.trim_end().ends_with("**");
        while let Some(next) = lines.peek() {
            if next.trim().is_empty() || list_item(next).is_some() || next.starts_with("```") {
                break;
            }
            check_unsupported(next)?;
            para.push_str(if prev_is_heading { "<br>\n" } else { " " });
            prev_is_heading = next.trim_end().ends_with("**");
            para.push_str(&inline(next.trim())?);
            lines.next();
        }
        out.push_str(&format!("<p>{para}</p>\n"));
    }
    Ok(out)
}

fn list_item(line: &str) -> Option<&str> {
    line.strip_prefix("- ")
}

fn check_unsupported(line: &str) -> Result<()> {
    let t = line.trim_start();
    let bad = ["#", ">", "|", "* ", "+ ", "---"];
    if bad.iter().any(|p| t.starts_with(p)) || t.starts_with(|c: char| c.is_ascii_digit()) && t.contains(". ")
    {
        return err(format!("未対応の Markdown 記法: {line}"));
    }
    Ok(())
}

/// インライン記法。エスケープを先に済ませてから置換する。
fn inline(s: &str) -> Result<String> {
    let mut out = String::new();
    let mut rest = escape(s);

    // [文字](URL)
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](") else {
            break;
        };
        let close = open + close;
        let Some(end) = rest[close..].find(')') else {
            return err(format!("リンクが閉じていない: {s}"));
        };
        let end = close + end;
        let text = rest[open + 1..close].to_string();
        let url = rest[close + 2..end].to_string();
        out.push_str(&emphasis(&rest[..open]));
        out.push_str(&format!("<a href=\"{url}\">{}</a>", emphasis(&text)));
        rest = rest[end + 1..].to_string();
    }
    out.push_str(&emphasis(&rest));
    Ok(out)
}

fn emphasis(s: &str) -> String {
    wrap(&wrap(s, "**", "strong"), "`", "code")
}

/// `delim` で囲まれた区間を `tag` で包む。閉じ側が無ければそのまま残す。
fn wrap(s: &str, delim: &str, tag: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(open) = rest.find(delim) {
        let after = &rest[open + delim.len()..];
        let Some(close) = after.find(delim) else {
            break;
        };
        out.push_str(&rest[..open]);
        out.push_str(&format!("<{tag}>{}</{tag}>", &after[..close]));
        rest = &after[close + delim.len()..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_before_markup() {
        assert_eq!(
            to_html("a <b> & \"c\"").unwrap(),
            "<p>a &lt;b&gt; &amp; &quot;c&quot;</p>\n"
        );
    }

    #[test]
    fn wraps_soft_line_breaks_but_keeps_heading_break() {
        assert_eq!(to_html("a\nb").unwrap(), "<p>a b</p>\n");
        assert_eq!(
            to_html("**h**\nb").unwrap(),
            "<p><strong>h</strong><br>\nb</p>\n"
        );
    }

    #[test]
    fn strong_code_link() {
        assert_eq!(
            to_html("**a** `b` [c](https://x/y)").unwrap(),
            "<p><strong>a</strong> <code>b</code> <a href=\"https://x/y\">c</a></p>\n"
        );
    }

    #[test]
    fn list_with_continuation() {
        assert_eq!(
            to_html("- a\n  b\n- c\n").unwrap(),
            "<ul>\n<li>a b</li>\n<li>c</li>\n</ul>\n"
        );
    }

    #[test]
    fn fence() {
        assert_eq!(
            to_html("```\nC:\\x\n```\n").unwrap(),
            "<pre><code>C:\\x</code></pre>\n"
        );
    }

    #[test]
    fn rejects_unsupported_syntax() {
        assert!(to_html("# heading").is_err());
        assert!(to_html("> quote").is_err());
        assert!(to_html("```\nunclosed").is_err());
    }

    #[test]
    fn every_fragment_converts() {
        let c = crate::Copy::load(&crate::repo_root()).unwrap();
        for f in [
            &c.prose.lead.ja,
            &c.prose.lead.en,
            &c.prose.promises.ja,
            &c.prose.install.en,
            &c.prose.more.ja,
            &c.prose.get.ja,
        ] {
            to_html(f).unwrap_or_else(|e| panic!("{e}: {f}"));
        }
    }
}
