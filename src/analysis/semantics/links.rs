//! Cross-reference links ("Figure 3", `[12]`, "Eq. (2)", URLs) in running text.
//!
//! Runs on the finished spans of every block: a reference becomes a link
//! when the anchor it points to exists in the document (see the anchors set
//! by [`build_document`](super::build_document)); URLs always do.

use crate::model::*;
use regex::{Captures, Match, Regex};
use std::collections::{HashMap, HashSet};
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

/// One run of the numbering of a kind of target (references, sections,
/// figures, ...): the anchors `base`, `base-2`, ... of one generation.
#[derive(Debug, Clone, Copy)]
struct TargetSet {
    /// Generation: 1 for the plain anchors, k for those ending in `-k`.
    generation: usize,
    /// Index of the first block holding an anchor of the set.
    start: usize,
    /// Index of the last block holding an anchor of the set.
    end: usize,
    /// Index of the first block whose label starts a part (an abstract, or
    /// sections 1 or 2, A or B), if the set has one: where its part of the
    /// document begins.
    restart: Option<usize>,
}

/// What a generation of anchors covers while it is collected: first block,
/// last block, anchor count, part start block, and whether the numbering
/// restarts (label 1 or A).
type SetSpan = (usize, usize, usize, Option<usize>, bool);

/// The anchors a mention can link to, with the document position of each
/// numbering, so that a mention picks the right one when a document has
/// several sets of numbering (a report followed by a system card, each with
/// its own `[1]`, Section 1, Figure 1, ...).
///
/// The anchors of a repeated label are `base`, `base-2`, `base-3` (see
/// `unique_anchor` in `assemble.rs`), so the k-th occurrence of a label
/// belongs to generation k. The generations of one kind of anchor (`ref`,
/// `sec`, `fig`, ...) are *sets* with a first and a last block. Which set a
/// mention at block `at` refers to:
///
/// - When the sections are numbered more than once, the set of the part the
///   mention is in (see [`LinkTargets::part_at`]), for every kind.
/// - Otherwise, citations: the first set that has not ended before the
///   mention (a bibliography comes after the text citing it); sections,
///   figures, tables, algorithms and equations: the last set that starts at
///   or before the mention (the first set if there is none).
///
/// A generation above 1 counts as a set only when it holds at least
/// `AssembleConfig::min_target_set_count` anchors or restarts the numbering
/// (it holds label 1 or A), so that one stray duplicate label does not
/// redirect all the mentions after it. When the chosen set lacks the label,
/// nothing is linked: a mention never reaches into another part.
#[derive(Debug, Default)]
pub(crate) struct LinkTargets {
    /// Every anchor of the document.
    anchors: HashSet<String>,
    /// The sets of each anchor kind, in generation order.
    sets: HashMap<String, Vec<TargetSet>>,
}

impl LinkTargets {
    /// Builds the targets from `(block index, anchor)` pairs; generations
    /// above 1 need `min_set_count` anchors, or label 1 or A, to count as a
    /// set.
    pub(crate) fn new<'a>(anchors: impl IntoIterator<Item = (usize, &'a str)>, min_set_count: usize) -> Self {
        let mut targets = Self::default();
        let mut spans: HashMap<(String, usize), SetSpan> = HashMap::new();
        for (block, anchor) in anchors {
            targets.anchors.insert(anchor.to_string());
            let Some((kind, rest)) = anchor.split_once('-') else {
                continue;
            };
            let generation = match rest.rsplit_once('-') {
                Some((_, tail)) if !tail.is_empty() && tail.bytes().all(|byte| byte.is_ascii_digit()) => {
                    tail.parse().unwrap_or(1).max(1)
                }
                _ => 1,
            };
            let label = rest.rsplit_once('-').map_or(rest, |(label, _)| label);
            let entry = spans
                .entry((kind.to_string(), generation))
                .or_insert((block, block, 0, None, false));
            entry.0 = entry.0.min(block);
            entry.1 = entry.1.max(block);
            entry.2 += 1;
            // A part begins with its abstract or its first sections; it
            // may lose the number of its first heading, and its second still
            // marks where it begins.
            if matches!(label, "abstract" | "1" | "2" | "A" | "B") {
                entry.3 = Some(entry.3.map_or(block, |first: usize| first.min(block)));
            }
            entry.4 |= matches!(label, "1" | "A");
        }
        for ((kind, generation), (start, end, count, restart, restarts)) in spans {
            if generation == 1 || restarts || count >= min_set_count {
                targets.sets.entry(kind).or_default().push(TargetSet {
                    generation,
                    start,
                    end,
                    restart,
                });
            }
        }
        for sets in targets.sets.values_mut() {
            sets.sort_by_key(|set| set.generation);
        }
        targets
    }

    /// The part of the document block `at` is in, when the sections are
    /// numbered more than once: the generation of the last section set
    /// whose numbering restarts (its section 1 or A) at or before it. A
    /// report's appendices, after its bibliography, still belong to the
    /// report and cite its list.
    fn part_at(&self, at: usize) -> Option<usize> {
        let sections = self.sets.get("sec").filter(|sets| sets.len() > 1)?;
        sections
            .iter()
            .rev()
            .find(|set| set.generation == 1 || set.restart.is_some_and(|restart| restart <= at))
            .map(|set| set.generation)
    }

    /// The anchor of the target of kind `kind` (`ref`, `sec`, ...) with
    /// `label` that a mention in block `at` refers to, if it exists.
    fn resolve(&self, kind: &str, label: &str, at: usize) -> Option<String> {
        let sets = self.sets.get(kind)?;
        let in_part = self
            .part_at(at)
            .and_then(|part| sets.iter().find(|set| set.generation == part));
        let chosen = in_part.or_else(|| {
            if kind == "ref" {
                sets.iter().find(|set| set.end >= at).or(sets.last())
            } else {
                sets.iter().rev().find(|set| set.start <= at).or(sets.first())
            }
        })?;
        let anchor = if chosen.generation == 1 {
            format!("{kind}-{label}")
        } else {
            format!("{kind}-{label}-{}", chosen.generation)
        };
        self.anchors.contains(&anchor).then_some(anchor)
    }
}

/// Splits the spans at cross-references and links them to their targets.
///
/// `targets` are the anchors present in the document and `at` is the index
/// of the block the spans belong to (see [`LinkTargets`]); references to
/// missing anchors, and to `own` (the anchor of the block itself, e.g. a
/// reference entry citing its own label), stay plain text. Spans that are
/// already links, superscripts or subscripts are passed through unchanged.
/// A reference is found in the concatenated text of a run of adjacent plain
/// spans, so that `[1]` set as "[", a coloured "1" and "]" is still linked;
/// every span keeps its style and colour, and the link goes on the spans
/// (or parts of spans) the reference covers. The concatenated text of the
/// result is the same as that of `spans`.
pub(crate) fn link_spans(spans: Vec<Span>, targets: &LinkTargets, at: usize, own: Option<&str>) -> Vec<Span> {
    let mut out = Vec::with_capacity(spans.len());
    let mut run: Vec<Span> = Vec::new();
    for span in spans {
        if span.href.is_some() || span.style & (SUP | SUB) != 0 {
            link_run(std::mem::take(&mut run), targets, at, own, &mut out);
            out.push(span);
        } else {
            run.push(span);
        }
    }
    link_run(run, targets, at, own, &mut out);
    out
}

/// Appends the spans of one run of adjacent plain spans to `out`, with the
/// references in their concatenated text linked.
fn link_run(run: Vec<Span>, targets: &LinkTargets, at: usize, own: Option<&str>, out: &mut Vec<Span>) {
    let text: String = run.iter().map(|span| span.text.as_str()).collect();
    let links = find_links(&text, targets, at, own);
    if links.is_empty() {
        out.extend(run);
        return;
    }
    let mut offset = 0;
    for span in &run {
        let (start, end) = (offset, offset + span.text.len());
        offset = end;
        let mut cursor = start;
        for (link_start, link_end, href) in links.iter().filter(|link| link.0 < end && link.1 > start) {
            let (from, to) = ((*link_start).max(start), (*link_end).min(end));
            push_piece(out, &span.text[cursor - start..from - start], None, span);
            push_piece(out, &span.text[from - start..to - start], Some(href.clone()), span);
            cursor = to;
        }
        push_piece(out, &span.text[cursor - start..], None, span);
    }
}

/// The links in `text`: byte ranges and their targets, in order and without
/// overlap.
fn find_links(text: &str, targets: &LinkTargets, at: usize, own: Option<&str>) -> Vec<(usize, usize, String)> {
    let mut links = Vec::new();
    let linkable = |anchor: Option<String>| anchor.filter(|anchor| own != Some(anchor.as_str()));
    for captures in XREF.captures_iter(text) {
        if let Some(url) = captures.name("url") {
            let trimmed = url.as_str().trim_end_matches(['.', ',', ';', ')', ']', ':']);
            links.push((url.start(), url.start() + trimmed.len(), trimmed.to_string()));
        } else if let Some(cite) = captures.name("cite") {
            // "[0, 1]" is an interval, not a citation: references count
            // from 1.
            if CITE_NUMBER
                .find_iter(cite.as_str())
                .any(|number| number.as_str().starts_with('0'))
            {
                continue;
            }
            for number in CITE_NUMBER.find_iter(cite.as_str()) {
                if let Some(anchor) = linkable(targets.resolve("ref", number.as_str(), at)) {
                    let start = cite.start() + number.start();
                    links.push((start, cite.start() + number.end(), format!("#{anchor}")));
                }
            }
        } else if let Some(num) = captures.name("num").or(captures.name("eqa")).or(captures.name("eqb")) {
            let whole = captures.get(0).unwrap();
            // A label letter that starts a word ("Section Exam") is no label.
            let word_continues = num.as_str().chars().all(char::is_alphabetic)
                && text[num.end()..].chars().next().is_some_and(char::is_alphabetic);
            if word_continues {
                continue;
            }
            let kind = object_prefix(&captures, whole);
            if let Some(anchor) = linkable(targets.resolve(kind, num.as_str(), at)) {
                links.push((whole.start(), whole.end(), format!("#{anchor}")));
            }
        }
    }
    links
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

    /// Targets whose anchors all sit in block 0.
    fn flat(anchors: &[&str]) -> LinkTargets {
        LinkTargets::new(anchors.iter().map(|anchor| (0, *anchor)), 2)
    }

    /// The `(text, href)` pairs of the linked spans.
    fn links(spans: &[Span]) -> Vec<(&str, &str)> {
        spans
            .iter()
            .filter_map(|span| span.href.as_deref().map(|href| (span.text.as_str(), href)))
            .collect()
    }

    #[test]
    fn cross_references_are_linked() {
        let targets = flat(&["ref-2", "fig-3", "sec-4.1"]);
        let spans = vec![Span::plain(
            "As in [2, 7] and Figure 3 (see Section 4.1), cf. https://example.org/x.",
        )];
        let eq = link_spans(vec![Span::plain("by Eq. (3) and")], &flat(&["eq-3"]), 0, None);
        assert_eq!(links(&eq), [("Eq. (3)", "#eq-3")]);
        let out = link_spans(spans, &targets, 0, None);
        assert_eq!(
            links(&out),
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

    #[test]
    fn citation_split_into_coloured_spans_is_linked() {
        let coloured = Span {
            color: [0, 0, 255],
            style: BOLD,
            ..Span::plain("1")
        };
        let spans = vec![
            Span::plain("see ["),
            coloured.clone(),
            Span::plain("] and ["),
            Span {
                text: "2, 3".into(),
                ..coloured.clone()
            },
            Span::plain("]."),
        ];
        let out = link_spans(spans, &flat(&["ref-1", "ref-3"]), 0, None);
        assert_eq!(links(&out), [("1", "#ref-1"), ("3", "#ref-3")]);
        let text: String = out.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "see [1] and [2, 3].");
        // The coloured number keeps its colour and style.
        let one = out.iter().find(|s| s.text == "1").unwrap();
        assert_eq!((one.color, one.style), (coloured.color, coloured.style));
    }

    #[test]
    fn interval_is_not_a_citation() {
        let spans = vec![Span::plain("a score in [0, 1] as in [1, 2]")];
        let out = link_spans(spans, &flat(&["ref-1", "ref-2"]), 0, None);
        assert_eq!(links(&out), [("1", "#ref-1"), ("2", "#ref-2")]);
    }

    #[test]
    fn label_letter_inside_a_word_is_not_a_reference() {
        let targets = flat(&["sec-E"]);
        let out = link_spans(
            vec![Span::plain("USNCO Local Section Exam, Section E.")],
            &targets,
            0,
            None,
        );
        assert_eq!(links(&out), [("Section E", "#sec-E")]);
    }

    #[test]
    fn mentions_reach_the_numbering_of_their_own_part() {
        // Main report: sections at blocks 0-1, references at 5-6. System
        // card: sections at 10-11, references at 15-16.
        let anchors = [
            (0, "sec-1"),
            (1, "sec-2"),
            (5, "ref-1"),
            (6, "ref-2"),
            (10, "sec-1-2"),
            (11, "sec-2-2"),
            (15, "ref-1-2"),
            (16, "ref-2-2"),
        ];
        let targets = LinkTargets::new(anchors, 2);
        let href = |at: usize, text: &str| {
            let out = link_spans(vec![Span::plain(text)], &targets, at, None);
            links(&out).first().map(|link| link.1.to_string())
        };
        assert_eq!(href(3, "[1]").as_deref(), Some("#ref-1"));
        assert_eq!(href(5, "[2]").as_deref(), Some("#ref-2"));
        assert_eq!(href(12, "[1]").as_deref(), Some("#ref-1-2"));
        assert_eq!(href(3, "Section 2").as_deref(), Some("#sec-2"));
        assert_eq!(href(12, "Section 2").as_deref(), Some("#sec-2-2"));
        // A label the part lacks is not linked into another part.
        assert_eq!(href(12, "Section 3"), None);
    }

    #[test]
    fn a_stray_duplicate_does_not_start_a_new_set() {
        let targets = LinkTargets::new([(0, "fig-1"), (1, "fig-2"), (50, "fig-2-2")], 2);
        let out = link_spans(vec![Span::plain("Figure 1")], &targets, 60, None);
        assert_eq!(links(&out), [("Figure 1", "#fig-1")]);
        // A lone label 1 does restart the numbering.
        let targets = LinkTargets::new([(0, "ref-1"), (1, "ref-2"), (5, "ref-1-2")], 2);
        let out = link_spans(vec![Span::plain("[1] [2]")], &targets, 9, None);
        assert_eq!(links(&out), [("1", "#ref-1-2")]);
    }
}
