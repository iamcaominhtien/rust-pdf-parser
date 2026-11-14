//! The TeX fonts formulas are set in, which the templates are drawn from:
//! their files and what their glyphs (named as in the fonts) stand for.

use super::templates::{Kind, font};

/// How the glyphs of a TeX font are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// `cmr`: upright letters, digits, capital Greek, punctuation, accents.
    Roman,
    /// `cmmi`: italic letters and Greek, a few symbols.
    MathItalic,
    /// `cmsy`: operators, relations, delimiters, calligraphic capitals.
    Symbol,
    /// `cmex`: big operators and delimiters.
    Extension,
    /// `cmbx`: bold letters and digits.
    Bold,
    /// `cmmib`: bold italic letters and Greek.
    BoldItalic,
    /// `msbm`, `dsrom`: blackboard capitals.
    DoubleStruck,
    /// `msam`: more relations.
    Ams,
    /// `eufm`: Fraktur letters.
    Fraktur,
    /// `rsfs`: script capitals.
    Script,
}

/// A TeX font: its Type 1 file, how its glyphs are read and its size
/// against the text (1 for 10 pt, 0.7 for scripts, 0.5 for scripts of
/// scripts).
pub struct TexFont {
    /// Its name (`cmmi10`), for the tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub name: &'static str,
    /// The font file.
    pub data: &'static [u8],
    /// How its glyphs are read.
    pub role: Role,
    /// Its size against the text.
    pub level: f32,
}

/// Declares the embedded TeX fonts.
macro_rules! tex_fonts {
    ($($file:literal, $role:ident, $level:literal;)*) => {
        &[$(TexFont { name: $file, data: include_bytes!(concat!("fonts/", $file, ".pfb")), role: Role::$role, level: $level },)*]
    };
}

/// The TeX fonts (AMS Type 1 versions of Computer Modern and the AMS
/// fonts, `dsfont` and `rsfs`; see `fonts/README.md`).
pub static TEX_FONTS: &[TexFont] = tex_fonts![
    "cmr10", Roman, 1.0; "cmr7", Roman, 0.7; "cmr5", Roman, 0.5;
    "cmmi10", MathItalic, 1.0; "cmmi7", MathItalic, 0.7; "cmmi5", MathItalic, 0.5;
    "cmsy10", Symbol, 1.0; "cmsy7", Symbol, 0.7; "cmsy5", Symbol, 0.5;
    "cmex10", Extension, 1.0; "cmex7", Extension, 0.7;
    "cmbx10", Bold, 1.0; "cmbx7", Bold, 0.7;
    "cmmib10", BoldItalic, 1.0; "cmmib7", BoldItalic, 0.7;
    "msbm10", DoubleStruck, 1.0; "msbm7", DoubleStruck, 0.7; "msbm5", DoubleStruck, 0.5;
    "dsrom10", DoubleStruck, 1.0;
    "msam10", Ams, 1.0;
    "eufm10", Fraktur, 1.0; "eufm7", Fraktur, 0.7; "eufm5", Fraktur, 0.5;
    "rsfs10", Script, 1.0; "rsfs7", Script, 0.7;
];

/// The font the glyphs of a role are given (see [`font`]).
pub fn output_font(role: Role) -> u16 {
    match role {
        Role::Roman => font::ROMAN,
        Role::MathItalic => font::ITALIC,
        Role::Symbol | Role::Ams => font::SYMBOL,
        Role::Extension => font::EXTENSION,
        Role::Bold => font::BOLD,
        Role::BoldItalic => font::BOLD_ITALIC,
        Role::DoubleStruck => font::DOUBLE_STRUCK,
        Role::Fraktur => font::FRAKTUR,
        Role::Script => font::SCRIPT,
    }
}

/// Greek letters by glyph name.
const GREEK: &[(&str, &str)] = &[
    ("alpha", "α"),
    ("beta", "β"),
    ("gamma", "γ"),
    ("delta", "δ"),
    ("epsilon", "ϵ"),
    ("epsilon1", "ε"),
    ("zeta", "ζ"),
    ("eta", "η"),
    ("theta", "θ"),
    ("theta1", "ϑ"),
    ("iota", "ι"),
    ("kappa", "κ"),
    ("lambda", "λ"),
    ("mu", "μ"),
    ("nu", "ν"),
    ("xi", "ξ"),
    ("pi", "π"),
    ("pi1", "ϖ"),
    ("rho", "ρ"),
    ("rho1", "ϱ"),
    ("sigma", "σ"),
    ("sigma1", "ς"),
    ("tau", "τ"),
    ("upsilon", "υ"),
    ("phi", "ϕ"),
    ("phi1", "φ"),
    ("chi", "χ"),
    ("psi", "ψ"),
    ("omega", "ω"),
    ("Gamma", "Γ"),
    ("Delta", "Δ"),
    ("Theta", "Θ"),
    ("Lambda", "Λ"),
    ("Xi", "Ξ"),
    ("Pi", "Π"),
    ("Sigma", "Σ"),
    ("Upsilon", "Υ"),
    ("Phi", "Φ"),
    ("Psi", "Ψ"),
    ("Omega", "Ω"),
];

/// Digits by glyph name.
const DIGITS: [&str; 10] = [
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
];

/// Symbols by glyph name, with their kind.
const SYMBOLS: &[(&str, &str, Kind)] = &[
    // cmr and cmmi
    ("exclam", "!", Kind::Symbol),
    ("percent", "%", Kind::Symbol),
    ("quoteright", "'", Kind::Symbol),
    ("parenleft", "(", Kind::Delimiter),
    ("parenright", ")", Kind::Delimiter),
    ("plus", "+", Kind::Symbol),
    ("comma", ",", Kind::Symbol),
    ("period", ".", Kind::Symbol),
    ("slash", "/", Kind::Symbol),
    ("colon", ":", Kind::Symbol),
    ("semicolon", ";", Kind::Symbol),
    ("equal", "=", Kind::Symbol),
    ("question", "?", Kind::Symbol),
    ("bracketleft", "[", Kind::Delimiter),
    ("bracketright", "]", Kind::Delimiter),
    ("less", "<", Kind::Symbol),
    ("greater", ">", Kind::Symbol),
    ("star", "⋆", Kind::Symbol),
    ("partialdiff", "∂", Kind::Letter),
    ("lscript", "ℓ", Kind::Letter),
    ("dotlessi", "ı", Kind::Letter),
    ("dotlessj", "ȷ", Kind::Letter),
    ("weierstrass", "℘", Kind::Letter),
    ("circumflex", "ˆ", Kind::Accent),
    ("tilde", "˜", Kind::Accent),
    ("macron", "¯", Kind::Accent),
    ("dotaccent", "˙", Kind::Accent),
    ("dieresis", "¨", Kind::Accent),
    ("caron", "ˇ", Kind::Accent),
    ("breve", "˘", Kind::Accent),
    ("acute", "´", Kind::Accent),
    ("grave", "`", Kind::Accent),
    ("ring", "˚", Kind::Accent),
    ("vector", "⃗", Kind::Accent),
    // cmsy
    ("minus", "−", Kind::Symbol),
    ("periodcentered", "·", Kind::Symbol),
    ("multiply", "×", Kind::Symbol),
    ("asteriskmath", "∗", Kind::Symbol),
    ("divide", "÷", Kind::Symbol),
    ("diamondmath", "⋄", Kind::Symbol),
    ("plusminus", "±", Kind::Symbol),
    ("minusplus", "∓", Kind::Symbol),
    ("circleplus", "⊕", Kind::Symbol),
    ("circleminus", "⊖", Kind::Symbol),
    ("circlemultiply", "⊗", Kind::Symbol),
    ("circledot", "⊙", Kind::Symbol),
    ("openbullet", "∘", Kind::Symbol),
    ("bullet", "∙", Kind::Symbol),
    ("equivasymptotic", "≍", Kind::Symbol),
    ("equivalence", "≡", Kind::Symbol),
    ("reflexsubset", "⊆", Kind::Symbol),
    ("reflexsuperset", "⊇", Kind::Symbol),
    ("lessequal", "≤", Kind::Symbol),
    ("greaterequal", "≥", Kind::Symbol),
    ("precedesequal", "⪯", Kind::Symbol),
    ("followsequal", "⪰", Kind::Symbol),
    ("similar", "∼", Kind::Symbol),
    ("approxequal", "≈", Kind::Symbol),
    ("propersubset", "⊂", Kind::Symbol),
    ("propersuperset", "⊃", Kind::Symbol),
    ("lessmuch", "≪", Kind::Symbol),
    ("greatermuch", "≫", Kind::Symbol),
    ("precedes", "≺", Kind::Symbol),
    ("follows", "≻", Kind::Symbol),
    ("arrowleft", "←", Kind::Symbol),
    ("arrowright", "→", Kind::Symbol),
    ("arrowup", "↑", Kind::Symbol),
    ("arrowdown", "↓", Kind::Symbol),
    ("arrowboth", "↔", Kind::Symbol),
    ("similarequal", "≃", Kind::Symbol),
    ("arrowdblleft", "⇐", Kind::Symbol),
    ("arrowdblright", "⇒", Kind::Symbol),
    ("arrowdblboth", "⇔", Kind::Symbol),
    ("proportional", "∝", Kind::Symbol),
    ("prime", "′", Kind::Symbol),
    ("infinity", "∞", Kind::Letter),
    ("element", "∈", Kind::Symbol),
    ("owner", "∋", Kind::Symbol),
    ("triangle", "△", Kind::Symbol),
    ("negationslash", "\u{338}", Kind::Symbol),
    ("mapsto", "↦", Kind::Symbol),
    ("universal", "∀", Kind::Symbol),
    ("existential", "∃", Kind::Symbol),
    ("logicalnot", "¬", Kind::Symbol),
    ("emptyset", "∅", Kind::Letter),
    ("latticetop", "⊤", Kind::Symbol),
    ("perpendicular", "⊥", Kind::Symbol),
    ("aleph", "ℵ", Kind::Letter),
    ("union", "∪", Kind::Symbol),
    ("intersection", "∩", Kind::Symbol),
    ("unionmulti", "⊎", Kind::Symbol),
    ("logicaland", "∧", Kind::Symbol),
    ("logicalor", "∨", Kind::Symbol),
    ("turnstileleft", "⊢", Kind::Symbol),
    ("turnstileright", "⊣", Kind::Symbol),
    ("floorleft", "⌊", Kind::Delimiter),
    ("floorright", "⌋", Kind::Delimiter),
    ("ceilingleft", "⌈", Kind::Delimiter),
    ("ceilingright", "⌉", Kind::Delimiter),
    ("braceleft", "{", Kind::Delimiter),
    ("braceright", "}", Kind::Delimiter),
    ("angbracketleft", "⟨", Kind::Delimiter),
    ("angbracketright", "⟩", Kind::Delimiter),
    ("bar", "|", Kind::Delimiter),
    ("bardbl", "‖", Kind::Delimiter),
    ("backslash", "∖", Kind::Symbol),
    ("radical", "√", Kind::Delimiter),
    ("nabla", "∇", Kind::Letter),
    ("dagger", "†", Kind::Symbol),
    ("daggerdbl", "‡", Kind::Symbol),
    // msam
    ("lessorequalslant", "⩽", Kind::Symbol),
    ("greaterorequalslant", "⩾", Kind::Symbol),
    ("lessorsimilar", "≲", Kind::Symbol),
    ("greaterorsimilar", "≳", Kind::Symbol),
    ("therefore", "∴", Kind::Symbol),
    ("because", "∵", Kind::Symbol),
    ("square", "□", Kind::Symbol),
    ("defines", "≜", Kind::Symbol),
    ("lessdblequal", "≦", Kind::Symbol),
    ("greaterdblequal", "≧", Kind::Symbol),
];

/// Glyph name stems of `cmex` (without `big`, `text`...), with the symbol.
const EXTENSIBLE: &[(&str, &str, Kind)] = &[
    ("parenleft", "(", Kind::Delimiter),
    ("parenright", ")", Kind::Delimiter),
    ("bracketleft", "[", Kind::Delimiter),
    ("bracketright", "]", Kind::Delimiter),
    ("floorleft", "⌊", Kind::Delimiter),
    ("floorright", "⌋", Kind::Delimiter),
    ("ceilingleft", "⌈", Kind::Delimiter),
    ("ceilingright", "⌉", Kind::Delimiter),
    ("braceleft", "{", Kind::Delimiter),
    ("braceright", "}", Kind::Delimiter),
    ("angbracketleft", "⟨", Kind::Delimiter),
    ("angbracketright", "⟩", Kind::Delimiter),
    ("slash", "/", Kind::Delimiter),
    ("backslash", "\\", Kind::Delimiter),
    ("radical", "√", Kind::Delimiter),
    ("summation", "∑", Kind::BigOp),
    ("product", "∏", Kind::BigOp),
    ("coproduct", "∐", Kind::BigOp),
    ("integral", "∫", Kind::BigOp),
    ("contintegral", "∮", Kind::BigOp),
    ("union", "⋃", Kind::BigOp),
    ("intersection", "⋂", Kind::BigOp),
    ("unionmulti", "⨄", Kind::BigOp),
    ("unionsq", "⨆", Kind::BigOp),
    ("logicaland", "⋀", Kind::BigOp),
    ("logicalor", "⋁", Kind::BigOp),
    ("circleplus", "⨁", Kind::BigOp),
    ("circlemultiply", "⨂", Kind::BigOp),
    ("circledot", "⨀", Kind::BigOp),
];

/// Size suffixes of the glyph names of `cmex`.
const SIZES: &[&str] = &["text", "display", "bigg", "Bigg", "big", "Big"];

/// What the glyph `name` of a font of `role` stands for (its text and
/// kind), or `None` for glyphs that formulas do not use or that are pieces
/// of larger symbols.
pub fn glyph(role: Role, name: &str) -> Option<(String, Kind)> {
    let letter = name.len() == 1 && name.chars().all(|ch| ch.is_ascii_alphabetic());
    let digit = DIGITS.iter().position(|digit| *digit == name);
    let greek = GREEK.iter().find(|(known, _)| *known == name).map(|(_, text)| *text);
    let symbol = SYMBOLS.iter().find(|(known, ..)| *known == name);
    match role {
        Role::Roman | Role::Bold => {
            if letter {
                Some((name.to_string(), Kind::Letter))
            } else if let Some(digit) = digit {
                Some((digit.to_string(), Kind::Letter))
            } else if let Some(greek) = greek.filter(|_| name.starts_with(char::is_uppercase)) {
                Some((greek.to_string(), Kind::Letter))
            } else {
                symbol
                    .filter(|_| role == Role::Roman)
                    .map(|(_, text, kind)| (text.to_string(), *kind))
            }
        }
        Role::MathItalic | Role::BoldItalic => {
            if letter {
                Some((name.to_string(), Kind::Letter))
            } else if let Some(greek) = greek {
                Some((greek.to_string(), Kind::Letter))
            } else {
                symbol.map(|(_, text, kind)| (text.to_string(), *kind))
            }
        }
        Role::Symbol => {
            if letter && name.starts_with(char::is_uppercase) {
                Some((name.to_string(), Kind::Letter))
            } else {
                symbol.map(|(_, text, kind)| (text.to_string(), *kind))
            }
        }
        Role::Ams => symbol.map(|(_, text, kind)| (text.to_string(), *kind)),
        Role::Extension => {
            let stem = SIZES.iter().find_map(|size| name.strip_suffix(size)).unwrap_or(name);
            EXTENSIBLE
                .iter()
                .find(|(known, ..)| *known == stem)
                .map(|(_, text, kind)| (text.to_string(), *kind))
        }
        Role::DoubleStruck | Role::Script => {
            (letter && name.starts_with(char::is_uppercase)).then(|| (name.to_string(), Kind::Letter))
        }
        Role::Fraktur => letter.then(|| (name.to_string(), Kind::Letter)),
    }
}
