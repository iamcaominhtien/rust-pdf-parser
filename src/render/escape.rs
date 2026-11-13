//! HTML for text: escaping, styled spans, captions and small attributes.
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

use crate::model::*;
use regex::Regex;
use std::sync::LazyLock;

/// Extra bytes reserved when escaping a string, so that a few entities fit
/// without the output growing again.
const ESCAPE_SLACK_BYTES: usize = 8;

/// Maximum number of characters of the text inside a figure that are added
/// to its alt text (enough to identify it, short enough for screen readers).
const FIGURE_ALT_MAX_CHARS: usize = 240;

/// The label at the start of a caption ("Figure 3:", "Table 2.",
/// "Extended Data Fig. 1 |"), emphasised in the HTML.
pub(crate) static CAP_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:Supplementary\s+|Extended\s+Data\s+)?(?:Figure|Fig\.?|FIGURE|FIG\.?|Table|TABLE|Tab\.|Algorithm|ALGORITHM|Listing|Scheme|Chart|Plate)\s*\S+?\s*[:.|—–-]").unwrap()
});

/// Escapes `text` for use in HTML text and in double-quoted attribute
/// values (`&`, `<`, `>` and `"`).
pub(crate) fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + ESCAPE_SLACK_BYTES);
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

/// Styled spans as semantic HTML (`<code>`, `<strong>`, `<em>`, `<sup>`,
/// `<sub>`), with links: internal ones (`#anchor`) get the `xref` class,
/// external ones open in a new tab.
pub(crate) fn spans_html(spans: &[Span]) -> String {
    let mut out = String::new();
    for span in spans {
        let mut open = String::new();
        let mut close = String::new();
        let mut tag = |name: &str| {
            open.push_str(&format!("<{name}>"));
            close.insert_str(0, &format!("</{name}>"));
        };
        if span.style & MONO != 0 {
            tag("code");
        }
        if span.style & BOLD != 0 {
            tag("strong");
        }
        if span.style & ITALIC != 0 {
            tag("em");
        }
        if span.style & SUP != 0 {
            tag("sup");
        }
        if span.style & SUB != 0 {
            tag("sub");
        }
        let body = format!("{open}{}{close}", esc(&span.text));
        match &span.href {
            Some(href) if href.starts_with('#') => {
                out.push_str(&format!("<a class=\"xref\" href=\"{}\">{body}</a>", esc(href)))
            }
            Some(href) => out.push_str(&format!(
                "<a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">{body}</a>",
                esc(href)
            )),
            None => out.push_str(&body),
        }
    }
    out
}

/// The text of `spans` without styling, trimmed.
pub(crate) fn plain(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>()
        .trim()
        .to_string()
}

/// Caption HTML with the label ("Figure 3:") emphasised.
pub(crate) fn caption_html(spans: &[Span]) -> String {
    if let Some(first) = spans.first()
        && let Some(label_match) = CAP_LABEL.find(&first.text)
    {
        let mut rest = spans.to_vec();
        let label = first.text[..label_match.end()].to_string();
        rest[0].text = first.text[label_match.end()..].to_string();
        return format!("<span class=\"cap-label\">{}</span>{}", esc(&label), spans_html(&rest));
    }
    spans_html(spans)
}

/// The CSS class of a float of the given kind.
pub(crate) fn float_class(kind: crate::model::FloatKind) -> &'static str {
    use crate::model::FloatKind::*;
    match kind {
        Figure => "fig",
        Table => "tab",
        Algorithm => "alg",
        Graphic => "gfx",
    }
}

/// The ` id="..."` attribute of a block that is the target of a link, or an
/// empty string when nothing links to it.
pub(crate) fn id_attr(block: &Block) -> String {
    match &block.anchor {
        Some(anchor) => format!(" id=\"{}\"", esc(anchor)),
        None => String::new(),
    }
}

/// The title of the document, or "Untitled paper" when none was found.
pub(crate) fn doc_title(doc: &Document) -> String {
    doc.meta.title.clone().unwrap_or_else(|| "Untitled paper".to_string())
}

/// Alt text for a figure: its label plus any text found inside it (at most
/// [`FIGURE_ALT_MAX_CHARS`] characters of it).
pub(crate) fn figure_alt(block: &Block, label: &Option<String>) -> String {
    let mut alt = label.clone().unwrap_or_else(|| "Figure".into());
    if let Some(inner) = block.rows.first() {
        let text = plain(inner);
        if !text.is_empty() {
            alt.push_str(": ");
            alt.extend(text.chars().take(FIGURE_ALT_MAX_CHARS));
        }
    }
    alt
}
