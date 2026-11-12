//! HTML for text: escaping, styled spans, captions and small attributes.

use crate::model::*;
use regex::Regex;
use std::sync::LazyLock;

pub(crate) fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn spans_html(spans: &[Span]) -> String {
    let mut out = String::new();
    for s in spans {
        let mut open = String::new();
        let mut close = String::new();
        let mut tag = |name: &str| {
            open.push_str(&format!("<{name}>"));
            close.insert_str(0, &format!("</{name}>"));
        };
        if s.style & MONO != 0 {
            tag("code");
        }
        if s.style & BOLD != 0 {
            tag("strong");
        }
        if s.style & ITALIC != 0 {
            tag("em");
        }
        if s.style & SUP != 0 {
            tag("sup");
        }
        if s.style & SUB != 0 {
            tag("sub");
        }
        let body = format!("{open}{}{close}", esc(&s.text));
        match &s.href {
            Some(h) if h.starts_with('#') => out.push_str(&format!("<a class=\"xref\" href=\"{}\">{body}</a>", esc(h))),
            Some(h) => out.push_str(&format!(
                "<a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">{body}</a>",
                esc(h)
            )),
            None => out.push_str(&body),
        }
    }
    out
}

pub(crate) fn plain(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| s.text.as_str())
        .collect::<String>()
        .trim()
        .to_string()
}

pub(crate) static CAP_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:Supplementary\s+|Extended\s+Data\s+)?(?:Figure|Fig\.?|FIGURE|FIG\.?|Table|TABLE|Tab\.|Algorithm|ALGORITHM|Listing|Scheme|Chart|Plate)\s*\S+?\s*[:.|—–-]").unwrap()
});

/// Caption HTML with the label ("Figure 3:") emphasised.
pub(crate) fn caption_html(spans: &[Span]) -> String {
    if let Some(first) = spans.first()
        && let Some(m) = CAP_LABEL.find(&first.text)
    {
        let mut rest = spans.to_vec();
        let label = first.text[..m.end()].to_string();
        rest[0].text = first.text[m.end()..].to_string();
        return format!("<span class=\"cap-label\">{}</span>{}", esc(&label), spans_html(&rest));
    }
    spans_html(spans)
}

pub(crate) fn float_class(kind: crate::model::FloatKind) -> &'static str {
    use crate::model::FloatKind::*;
    match kind {
        Figure => "fig",
        Table => "tab",
        Algorithm => "alg",
        Graphic => "gfx",
    }
}

pub(crate) fn id_attr(b: &Block) -> String {
    match &b.anchor {
        Some(a) => format!(" id=\"{}\"", esc(a)),
        None => String::new(),
    }
}

pub(crate) fn doc_title(doc: &Document) -> String {
    doc.meta.title.clone().unwrap_or_else(|| "Untitled paper".to_string())
}

/// Alt text for a figure: its label plus any text found inside it.
pub(crate) fn figure_alt(b: &Block, label: &Option<String>) -> String {
    let mut alt = label.clone().unwrap_or_else(|| "Figure".into());
    if let Some(inner) = b.rows.first() {
        let t = plain(inner);
        if !t.is_empty() {
            alt.push_str(": ");
            alt.extend(t.chars().take(240));
        }
    }
    alt
}
