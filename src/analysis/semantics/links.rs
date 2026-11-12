//! Cross-reference links ("Figure 3", "[12]", "Eq. (2)", URLs) in running text.

use crate::model::*;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

pub(crate) static XREF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?P<url>https?://[^\s<>]+)",
        r"|\[(?P<cite>\d{1,3}(?:\s*[,;–-]\s*\d{1,3})*)\]",
        r"|(?P<obj>(?:Figures?|Figs?\.|Tables?|Algorithms?|Sections?|Sec\.|Appendix)\s*~?\s*(?P<num>(?:\d{1,3}|[A-Z])(?:\.\d{1,2})*))",
        r"|(?P<eq>(?:Eqs?\.|Eqn\.|Equations?)\s*~?\s*(?:\((?P<eqa>\d{1,3}(?:\.\d{1,2})*[a-z]?)\)|(?P<eqb>\d{1,3}(?:\.\d{1,2})*)))",
    ))
    .unwrap()
});

pub(crate) fn link_spans(spans: Vec<Span>, anchors: &HashSet<String>, own: Option<&str>) -> Vec<Span> {
    let mut out = Vec::with_capacity(spans.len());
    for s in spans {
        if s.href.is_some() || s.style & (SUP | SUB) != 0 {
            out.push(s);
            continue;
        }
        let mut last = 0;
        let text = s.text.clone();
        let push = |t: &str, href: Option<String>, out: &mut Vec<Span>| {
            if !t.is_empty() {
                out.push(Span {
                    text: t.to_string(),
                    style: s.style,
                    href,
                    color: s.color,
                });
            }
        };
        for c in XREF.captures_iter(&text) {
            let m = c.get(0).unwrap();
            if let Some(u) = c.name("url") {
                let url = u.as_str().trim_end_matches(['.', ',', ';', ')', ']', ':']);
                let end = u.start() + url.len();
                push(&text[last..u.start()], None, &mut out);
                push(&text[u.start()..end], Some(url.to_string()), &mut out);
                last = end;
            } else if let Some(cite) = c.name("cite") {
                // Link every number separately.
                push(&text[last..cite.start()], None, &mut out);
                let mut pos = cite.start();
                static NUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());
                for n in NUM.find_iter(cite.as_str()) {
                    let (a, b) = (cite.start() + n.start(), cite.start() + n.end());
                    push(&text[pos..a], None, &mut out);
                    let target = format!("ref-{}", n.as_str());
                    let href =
                        (anchors.contains(&target) && own != Some(target.as_str())).then(|| format!("#{target}"));
                    push(&text[a..b], href, &mut out);
                    pos = b;
                }
                push(&text[pos..cite.end()], None, &mut out);
                last = cite.end();
            } else if let Some(num) = c.name("num").or(c.name("eqa")).or(c.name("eqb")) {
                let word = m.as_str().to_ascii_lowercase();
                let prefix = if c.name("eq").is_some() {
                    "eq"
                } else if word.starts_with("fig") {
                    "fig"
                } else if word.starts_with("tab") {
                    "tab"
                } else if word.starts_with("alg") {
                    "alg"
                } else {
                    "sec"
                };
                let target = format!("{prefix}-{}", num.as_str());
                if anchors.contains(&target) && own != Some(target.as_str()) {
                    push(&text[last..m.start()], None, &mut out);
                    push(m.as_str(), Some(format!("#{target}")), &mut out);
                    last = m.end();
                }
            }
        }
        push(&text[last..], None, &mut out);
    }
    out
}

#[cfg(test)]
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
