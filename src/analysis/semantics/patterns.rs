//! Regular expressions shared by the classification stages.
//!
//! Section numbers are captured as `num` and the title after them as
//! `rest`, so that the stages can read the level from the number.
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

use regex::Regex;
use std::sync::LazyLock;

/// Arabic section number and title: "3", "3.2", "3.2.1." followed by the
/// title (up to four levels of at most two digits each).
pub(crate) static HEADING_NUM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<num>\d{1,2}(?:\.\d{1,2}){0,3})\.?\s+(?P<rest>\S.*)$").unwrap());
/// Appendix label and title: "Appendix A", "A.1", "B:" followed by the
/// title; `word` captures the word "Appendix" when present.
pub(crate) static HEADING_APP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<word>Appendix\s+|APPENDIX\s+)?(?P<num>[A-Z](?:\.\d{1,2}){0,3})[.:]?\s+(?P<rest>\S.*)$").unwrap()
});
/// Roman section number with a period and a capitalised title ("IV. EXPERIMENTS").
pub(crate) static HEADING_ROMAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<num>[IVX]{1,5})\.\s+(?P<rest>[A-Z].*)$").unwrap());
/// Letter with a period and a capitalised title ("A. Datasets"), the
/// subsection style of IEEE papers.
pub(crate) static HEADING_LETTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<num>[A-Z])\.\s+(?P<rest>[A-Z].*)$").unwrap());
/// An "Abstract" label run into the first paragraph ("Abstract—We …",
/// "ABSTRACT: …"), with the punctuation and spaces after it.
pub(crate) static ABSTRACT_INLINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?i:abstract)\s*[—–:.\-]+\s*").unwrap());
/// The heading of the bibliography, optionally numbered ("7 References",
/// "VI. BIBLIOGRAPHY"); the whole text must be the heading.
pub(crate) static REF_HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:\d+\.?\s+|[ivx]+\.\s+)?(references|bibliography|references and notes|literature cited|works cited)$",
    )
    .unwrap()
});
/// Label at the start of a bibliography entry: "[label]" (captured as `a`,
/// e.g. "[12]" or "[Vas17]") or "12." (captured as `b`).
pub(crate) static REF_LABEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\[(?P<a>[^\]]{1,24})\]|(?P<b>\d{1,3})\.(?:\s|$))").unwrap());
/// Unnumbered headings common to papers ("Introduction", "Acknowledgments",
/// "References"); such a text is a heading even without a number.
pub(crate) static KNOWN_HEADINGS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(abstract|references|bibliography|acknowledge?ments?|appendix|appendices|supplementary materials?|conclusions?|introduction|related work|contents|keywords|index terms)$").unwrap()
});

#[cfg(test)]
#[allow(clippy::missing_docs_in_private_items)]
mod tests {
    use super::*;

    #[test]
    fn heading_patterns() {
        assert!(HEADING_NUM.is_match("3.2.1 Scaled Dot-Product Attention"));
        assert!(HEADING_ROMAN.is_match("IV. EXPERIMENTS"));
        assert!(REF_HEADING.is_match("References"));
        assert!(REF_HEADING.is_match("7 References"));
        assert!(!REF_HEADING.is_match("References to prior work"));
        assert!(ABSTRACT_INLINE.is_match("Abstract—We propose"));
    }
}
