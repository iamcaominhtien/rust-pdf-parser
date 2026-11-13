//! Cross-reference links ("Figure 3", `[12]`, "Eq. (2)", URLs) in running text.
//!
//! Runs on the finished spans of every block: a reference becomes a link
//! when the anchor it points to exists in the document (see the anchors set
//! by [`build_document`](super::build_document)); URLs always do.
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

use crate::model::*;
use regex::{Captures, Match, Regex};
use std::collections::HashSet;
use std::sync::LazyLock;

/// A linkable reference, one alternative per kind: a URL (`url`), a
/// bracketed citation list (`cite`, `[2, 7–9]`), a figure, table,
/// algorithm, section or appendix (`obj` with its number `num`), or an
/// equation (`eq`, numbered `eqa` in parentheses or bare `eqb`).
pub(crate) static XREF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?P<url>https?://[^\s<>]+)",
        r"|\[(?P<cite>\d{1,3}(?:\s*[,;–-]\s*\d{1,3})*)\]",
        r"|(?P<obj>(?:Figures?|Figs?\.|Tables?|Algorithms?|Sections?|Sec\.|Appendix)\s*~?\s*(?P<num>(?:\d{1,3}|[A-Z])(?:\.\d{1,2})*))",
        r"|(?P<eq>(?:Eqs?\.|Eqn\.|Equations?)\s*~?\s*(?:\((?P<eqa>\d{1,3}(?:\.\d{1,2})*[a-z]?)\)|(?P<eqb>\d{1,3}(?:\.\d{1,2})*)))",
    ))
    .unwrap()
});

/// One number of a citation list; each is linked separately.
static CITE_NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());

/// Splits the spans at cross-references and links them to their targets.
///
/// `anchors` are the anchors present in the document; references to
/// missing anchors, and to `own` (the anchor of the block itself, e.g. a
/// reference entry citing its own label), stay plain text. Spans that are
/// already links, superscripts or subscripts are passed through unchanged.
/// The concatenated text of the result is the same as that of `spans`.
pub(crate) fn link_spans(spans: Vec<Span>, anchors: &HashSet<String>, own: Option<&str>) -> Vec<Span> {
    let mut out = Vec::with_capacity(spans.len());
    for span in spans {
        if span.href.is_some() || span.style & (SUP | SUB) != 0 {
            out.push(span);
            continue;
        }
        link_span(&span, anchors, own, &mut out);
    }
    out
}

/// Appends the pieces of one plain span to `out`, with the references in
/// it linked.
fn link_span(span: &Span, anchors: &HashSet<String>, own: Option<&str>, out: &mut Vec<Span>) {
    let text = span.text.as_str();
    let mut last = 0;
    for captures in XREF.captures_iter(text) {
        if let Some(url) = captures.name("url") {
            last = push_url(span, last, url, out);
        } else if let Some(cite) = captures.name("cite") {
            last = push_citation(span, last, cite, anchors, own, out);
        } else if let Some(num) = captures.name("num").or(captures.name("eqa")).or(captures.name("eqb")) {
            let whole = captures.get(0).unwrap();
            let target = format!("{}-{}", object_prefix(&captures, whole), num.as_str());
            if anchors.contains(&target) && own != Some(target.as_str()) {
                push_piece(out, &text[last..whole.start()], None, span);
                push_piece(out, whole.as_str(), Some(format!("#{target}")), span);
                last = whole.end();
            }
        }
    }
    push_piece(out, &text[last..], None, span);
}

/// Links a URL, without trailing punctuation. `last` is the end of the text
/// already written; returns the new end.
fn push_url(span: &Span, last: usize, url: Match<'_>, out: &mut Vec<Span>) -> usize {
    let text = span.text.as_str();
    let trimmed = url.as_str().trim_end_matches(['.', ',', ';', ')', ']', ':']);
    let end = url.start() + trimmed.len();
    push_piece(out, &text[last..url.start()], None, span);
    push_piece(out, &text[url.start()..end], Some(trimmed.to_string()), span);
    end
}

/// Links every number of a citation list that has a reference entry.
/// `last` is the end of the text already written; returns the new end (the
/// end of the list, before the closing bracket).
fn push_citation(
    span: &Span,
    last: usize,
    cite: Match<'_>,
    anchors: &HashSet<String>,
    own: Option<&str>,
    out: &mut Vec<Span>,
) -> usize {
    let text = span.text.as_str();
    // Link every number separately.
    push_piece(out, &text[last..cite.start()], None, span);
    let mut pos = cite.start();
    for number in CITE_NUMBER.find_iter(cite.as_str()) {
        let (start, end) = (cite.start() + number.start(), cite.start() + number.end());
        push_piece(out, &text[pos..start], None, span);
        let target = format!("ref-{}", number.as_str());
        let href = (anchors.contains(&target) && own != Some(target.as_str())).then(|| format!("#{target}"));
        push_piece(out, &text[start..end], href, span);
        pos = end;
    }
    push_piece(out, &text[pos..cite.end()], None, span);
    cite.end()
}

/// Anchor prefix of a figure, table, algorithm, section or equation
/// reference (the anchor naming of `build_document`).
fn object_prefix(captures: &Captures<'_>, whole: Match<'_>) -> &'static str {
    let word = whole.as_str().to_ascii_lowercase();
    if captures.name("eq").is_some() {
        "eq"
    } else if word.starts_with("fig") {
        "fig"
    } else if word.starts_with("tab") {
        "tab"
    } else if word.starts_with("alg") {
        "alg"
    } else {
        "sec"
    }
}

/// Appends `piece` with the style and colour of `like` and the link `href`;
/// empty pieces are skipped.
fn push_piece(out: &mut Vec<Span>, piece: &str, href: Option<String>, like: &Span) {
    if !piece.is_empty() {
        out.push(Span {
            text: piece.to_string(),
            style: like.style,
            href,
            color: like.color,
        });
    }
}

#[cfg(test)]
#[allow(clippy::min_ident_chars, clippy::missing_docs_in_private_items)]
mod tests {
    use super::*;

    use std::collections::HashSet;

    #[test]
    fn cross_references_are_linked() {
        let anchors: HashSet<String> = ["ref-2", "fig-3", "sec-4.1"].iter().map(|s| s.to_string()).collect();
        let spans = vec![Span::plain(
            "As in [2, 7] and Figure 3 (see Section 4.1), cf. https://example.org/x.",
        )];
        let eq_anchors: HashSet<String> = ["eq-3".to_string()].into_iter().collect();
        let eq = link_spans(vec![Span::plain("by Eq. (3) and")], &eq_anchors, None);
        assert!(
            eq.iter()
                .any(|s| s.text == "Eq. (3)" && s.href.as_deref() == Some("#eq-3"))
        );
        let out = link_spans(spans, &anchors, None);
        let links: Vec<(&str, &str)> = out
            .iter()
            .filter_map(|s| s.href.as_deref().map(|h| (s.text.as_str(), h)))
            .collect();
        assert_eq!(
            links,
            [
                ("2", "#ref-2"),
                ("Figure 3", "#fig-3"),
                ("Section 4.1", "#sec-4.1"),
                ("https://example.org/x", "https://example.org/x")
            ]
        );
        // The text itself is unchanged.
        let text: String = out.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            text,
            "As in [2, 7] and Figure 3 (see Section 4.1), cf. https://example.org/x."
        );
    }
}
