//! The fonts of the `mathabx` package (`matha`, `mathb`, `mathx`), which
//! PDFs give the Unicode values of a text encoding: `(` comes out as `p`,
//! `=` as `“`. Their glyphs are read from the package's own encoding
//! (`mathabx.dcl`) instead.

use serde::{Deserialize, Serialize};

/// One of the three fonts of `mathabx`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MathAbx {
    /// Binary operators, relations, small delimiters, arrows.
    Matha,
    /// More operators and relations, corners, astronomical symbols.
    Mathb,
    /// Big delimiters and operators, radical signs, brace pieces.
    Mathx,
}

impl MathAbx {
    /// The font of `mathabx` whose PostScript name (lower case) is `lower`:
    /// `matha10`, `tex-mathx10`, ...
    pub fn from_name(lower: &str) -> Option<Self> {
        let name = lower.strip_prefix("tex-").unwrap_or(lower);
        let rest = name.strip_prefix("math")?;
        let mut chars = rest.chars();
        let font = match chars.next()? {
            'a' => Self::Matha,
            'b' => Self::Mathb,
            'x' => Self::Mathx,
            _ => return None,
        };
        let size = chars.as_str();
        (!size.is_empty() && size.chars().all(|ch| ch.is_ascii_digit())).then_some(font)
    }

    /// The text of the glyph of character code `code`, or `None` for the
    /// pieces of tall symbols and the symbols without a Unicode value.
    pub fn text(self, code: u32) -> Option<&'static str> {
        let table = match self {
            Self::Matha => &MATHA,
            Self::Mathb => &MATHB,
            Self::Mathx => &MATHX,
        };
        table.get(usize::try_from(code).ok()?).copied().flatten()
    }
}

/// The tables, generated from `mathabx.dcl` (TeX names mapped to Unicode).
#[rustfmt::skip]
mod tables {
    /// Code -> text of the `matha` font of mathabx.
    pub(super) const MATHA: [Option<&str>; 256] = [
        Some("+"), Some("−"), Some("×"), Some("÷"), Some("·"), Some("∘"), Some("∗"), Some("⁎"),
        Some("±"), Some("∓"), Some("⋉"), Some("⋊"), Some("⋄"), Some("∙"), Some("⋆"), Some("✶"),
        Some("="), Some("≡"), Some("∼"), Some("≈"), Some("≃"), Some("≅"), Some("≍"), Some("∣"),
        Some("≠"), Some("≢"), Some("≁"), Some("≉"), Some("≄"), Some("≇"), Some("≭"), Some("∤"),
        Some("¬"), Some("≪"), Some("≫"), Some("#"), Some("⊢"), Some("⊣"), Some("⊬"), None,
        Some("⊨"), Some("⫤"), Some("⊭"), None, Some("⊩"), Some("⫣"), Some("⊮"), None,
        Some("°"), Some("′"), Some("″"), Some("‴"), Some("⁗"), Some("♭"), Some("♮"), Some("♯"),
        Some("∞"), Some("∝"), Some("†"), Some("‡"), Some("∑"), Some("∏"), Some("⨿"), Some("√"),
        Some("∀"), Some("∁"), Some("∂"), None, Some("∃"), Some("∄"), Some("Ⅎ"), Some("⅁"),
        Some("∅"), Some("⌀"), Some("⊤"), Some("⊥"), None, None, Some("⋏"), Some("⋎"),
        Some("∈"), Some("∋"), Some("∉"), Some("∌"), Some("∉"), Some("∌"), None, None,
        Some("∩"), Some("∪"), Some("⊎"), Some("⊓"), Some("⊔"), None, Some("∧"), Some("∨"),
        Some("⊕"), Some("⊖"), Some("⊗"), Some("⨸"), Some("⊙"), Some("⊚"), Some("⊛"), None,
        None, None, None, None, None, Some("⊘"), Some("⦸"), None,
        Some("("), Some(")"), Some("["), Some("]"), Some("{"), Some("}"), Some("⟦"), Some("⟧"),
        Some("⟨"), Some("⟩"), Some("∖"), Some("/"), Some("|"), Some("‖"), Some("⦀"), Some("\u{338}"),
        Some("⊂"), Some("⊃"), Some("⊄"), Some("⊅"), Some("⊆"), Some("⊇"), Some("⊈"), Some("⊉"),
        Some("⊊"), Some("⊋"), Some("⊊"), Some("⊋"), Some("⫅"), Some("⫆"), Some("⫅̸"), Some("⫆̸"),
        Some("⫋"), Some("⫌"), Some("⫋"), Some("⫌"), Some("⋐"), Some("⋑"), None, None,
        Some("◁"), Some("▷"), Some("⋪"), Some("⋫"), Some("⊴"), Some("⊵"), Some("⋬"), Some("⋭"),
        Some("<"), Some(">"), Some("≮"), Some("≯"), Some("≤"), Some("≥"), Some("≰"), Some("≱"),
        Some("⩽"), Some("⩾"), None, None, Some("⪇"), Some("⪈"), Some("≦"), Some("≧"),
        Some("≦̸"), Some("≧̸"), Some("≨"), Some("≩"), None, None, Some("⪕"), Some("⪖"),
        None, None, Some("≶"), Some("≷"), Some("⋚"), Some("⋛"), Some("⪋"), Some("⪌"),
        Some("≲"), Some("≳"), Some("≴"), Some("≵"), Some("⋦"), Some("⋧"), Some("⪅"), Some("⪆"),
        None, None, Some("⪉"), Some("⪊"), Some("⋖"), Some("⋗"), Some("⋘"), Some("⋙"),
        Some("←"), Some("→"), Some("↑"), Some("↓"), Some("↖"), Some("↗"), Some("↙"), Some("↘"),
        Some("↔"), Some("↕"), Some("↚"), Some("↛"), Some("↮"), None, None, None,
        Some("↼"), Some("⇀"), Some("↽"), Some("⇁"), Some("↿"), Some("⇃"), Some("↾"), Some("⇂"),
        Some("⇋"), Some("⇌"), None, None, None, None, None, None,
        Some("⇐"), Some("⇒"), Some("⇑"), Some("⇓"), Some("⇔"), Some("⇕"), Some("⇍"), Some("⇏"),
        Some("⇎"), None, None, None, None, None, None, None,
    ];
    
    /// Code -> text of the `mathb` font of mathabx.
    pub(super) const MATHB: [Option<&str>; 256] = [
        Some("∔"), Some("∸"), None, Some("⋇"), None, Some("□"), None, None,
        None, None, None, None, Some("◆"), None, Some("★"), None,
        None, None, Some("≑"), Some("≓"), Some("≒"), Some("≔"), Some("≕"), Some("≏"),
        None, Some("≎"), Some("≗"), Some("≖"), Some("≜"), Some("≙"), None, None,
        Some("≬"), Some("⌣"), Some("⌢"), None, Some("⋋"), Some("⋌"), Some("⋔"), Some("⋈"),
        None, None, None, None, Some("⊪"), None, None, None,
        None, None, None, None, None, None, Some("∴"), Some("∵"),
        None, None, None, None, None, Some("∠"), Some("∡"), Some("∢"),
        None, None, None, None, None, None, None, None,
        None, None, None, None, None, None, None, None,
        None, None, None, None, None, None, None, None,
        Some("⊼"), Some("⊻"), Some("⩞"), None, Some("⋒"), Some("⋓"), None, None,
        Some("⊞"), Some("⊟"), Some("⊠"), None, Some("⊡"), None, None, None,
        None, None, None, None, None, None, None, None,
        Some("⟮"), Some("⟯"), Some("⌈"), Some("⌉"), Some("⌊"), Some("⌋"), None, None,
        Some("⌜"), Some("⌝"), Some("⌞"), Some("⌟"), None, None, Some("|"), None,
        Some("⊏"), Some("⊐"), None, None, Some("⊑"), Some("⊒"), Some("⋢"), Some("⋣"),
        Some("⋤"), Some("⋥"), None, None, None, None, None, None,
        None, None, None, None, None, None, None, None,
        Some("▵"), Some("▿"), Some("◃"), Some("▹"), Some("▴"), Some("▾"), Some("◂"), Some("▸"),
        Some("≺"), Some("≻"), Some("⊀"), Some("⊁"), Some("≼"), Some("≽"), Some("⋠"), Some("⋡"),
        Some("⪯"), Some("⪰"), None, None, Some("⪱"), Some("⪲"), None, None,
        None, None, None, None, None, None, Some("⋞"), Some("⋟"),
        None, None, None, None, None, None, None, None,
        Some("≾"), Some("≿"), None, None, Some("⋨"), Some("⋩"), Some("⪷"), Some("⪸"),
        None, None, Some("⪹"), Some("⪺"), None, None, None, None,
        Some("⇇"), Some("⇉"), Some("⇈"), Some("⇊"), Some("⇆"), Some("⇄"), None, None,
        None, None, None, None, None, None, None, None,
        None, None, None, None, Some("╱"), Some("╲"), None, None,
        Some("↰"), Some("↱"), None, None, Some("↫"), Some("↬"), None, None,
        Some("↶"), Some("↷"), None, None, None, None, Some("↺"), Some("↻"),
        Some("⇜"), Some("⇝"), Some("↭"), None, None, None, None, None,
    ];
    
    /// Code -> text of the `mathx` font of mathabx.
    pub(super) const MATHX: [Option<&str>; 256] = [
        Some("("), Some("("), Some("("), Some("("), Some("("), Some("("), Some("("), Some("|"),
        Some(")"), Some(")"), Some(")"), Some(")"), Some(")"), Some(")"), Some(")"), Some("‖"),
        Some("["), Some("["), Some("["), Some("["), Some("["), Some("["), Some("["), Some("⦀"),
        Some("]"), Some("]"), Some("]"), Some("]"), Some("]"), Some("]"), Some("]"), Some("|"),
        Some("{"), Some("{"), Some("{"), Some("{"), Some("{"), Some("{"), Some("{"), Some("{"),
        Some("}"), Some("}"), Some("}"), Some("}"), Some("}"), Some("}"), Some("}"), Some("}"),
        Some("⟦"), Some("⟦"), Some("⟦"), Some("⟦"), Some("⟦"), Some("⟦"), Some("⟦"), None,
        Some("⟧"), Some("⟧"), Some("⟧"), Some("⟧"), Some("⟧"), Some("⟧"), Some("⟧"), None,
        Some("∑"), Some("∏"), Some("∐"), Some("∁"), None, None, None, None,
        Some("\\"), Some("\\"), None, Some("\\"), Some("/"), Some("/"), Some("/"), Some("/"),
        Some("⌈"), Some("⌈"), Some("⌈"), Some("⌈"), Some("⌉"), Some("⌉"), Some("⌉"), Some("⌉"),
        Some("⌊"), Some("⌊"), Some("⌊"), Some("⌊"), Some("⌋"), Some("⌋"), Some("⌋"), Some("⌋"),
        Some("√"), Some("√"), Some("√"), Some("√"), Some("√"), Some("√"), Some("√"), Some("√"),
        None, None, None, None, None, None, None, None,
        None, None, None, None, None, None, Some("⟮"), Some("⟯"),
        None, None, None, None, None, None, None, None,
        None, None, None, None, None, None, None, None,
        None, None, None, None, None, None, None, None,
        Some("+"), Some("⨉"), None, Some("⋂"), Some("⋃"), Some("⨄"), Some("⨅"), Some("⨆"),
        None, Some("⋀"), Some("⋁"), None, None, Some("↑"), Some("↓"), Some("↕"),
        None, None, None, None, None, None, None, None,
        None, None, None, None, None, Some("⇑"), Some("⇓"), Some("⇕"),
        Some("∑"), Some("∏"), Some("∐"), Some("∫"), Some("∬"), Some("∭"), Some("∮"), Some("∯"),
        Some("∑"), Some("∏"), Some("∐"), Some("∫"), Some("∬"), Some("∭"), Some("∮"), Some("∯"),
        Some("⨁"), None, Some("⨂"), None, Some("⨀"), None, None, None,
        None, None, None, None, None, None, None, None,
        None, None, None, None, None, None, None, None,
        None, None, None, None, None, None, None, None,
        None, None, None, None, None, None, None, None,
        None, None, None, None, None, None, None, None,
        None, None, None, None, None, None, None, None,
        None, None, None, None, None, None, None, None,
    ];
}
use tables::{MATHA, MATHB, MATHX};

#[cfg(test)]
#[allow(clippy::missing_docs_in_private_items)]
mod tests {
    use super::*;

    #[test]
    fn names_and_codes() {
        assert_eq!(MathAbx::from_name("tex-matha10"), Some(MathAbx::Matha));
        assert_eq!(MathAbx::from_name("mathx7"), Some(MathAbx::Mathx));
        assert_eq!(MathAbx::from_name("mathematicalpi-one"), None);
        assert_eq!(MathAbx::Matha.text(0x70), Some("("));
        assert_eq!(MathAbx::Matha.text(0x10), Some("="));
        assert_eq!(MathAbx::Matha.text(0xD1), Some("→"));
        assert_eq!(MathAbx::Mathx.text(0xB8), Some("∑"));
        assert_eq!(MathAbx::Mathx.text(0x6F), None);
    }
}
