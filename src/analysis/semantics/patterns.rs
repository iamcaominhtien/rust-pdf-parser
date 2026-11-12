//! Regular expressions shared by the classification stages.

use regex::Regex;
use std::sync::LazyLock;

pub(crate) static HEADING_NUM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<num>\d{1,2}(?:\.\d{1,2}){0,3})\.?\s+(?P<rest>\S.*)$").unwrap());
pub(crate) static HEADING_APP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<word>Appendix\s+|APPENDIX\s+)?(?P<num>[A-Z](?:\.\d{1,2}){0,3})[.:]?\s+(?P<rest>\S.*)$").unwrap()
});
pub(crate) static HEADING_ROMAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<num>[IVX]{1,5})\.\s+(?P<rest>[A-Z].*)$").unwrap());
pub(crate) static HEADING_LETTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<num>[A-Z])\.\s+(?P<rest>[A-Z].*)$").unwrap());
pub(crate) static ABSTRACT_INLINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?i:abstract)\s*[—–:.\-]+\s*").unwrap());
pub(crate) static REF_HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:\d+\.?\s+|[ivx]+\.\s+)?(references|bibliography|references and notes|literature cited|works cited)$",
    )
    .unwrap()
});
pub(crate) static REF_LABEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\[(?P<a>[^\]]{1,24})\]|(?P<b>\d{1,3})\.(?:\s|$))").unwrap());
pub(crate) static KNOWN_HEADINGS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(abstract|references|bibliography|acknowledge?ments?|appendix|appendices|supplementary materials?|conclusions?|introduction|related work|contents|keywords|index terms)$").unwrap()
});

#[cfg(test)]
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
