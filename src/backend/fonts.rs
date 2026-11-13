//! Font identification.
//!
//! The interpreter hands us glyphs but not the PDF font they came from. We
//! need the font name (bold / italic / math / monospace are all inferred from
//! it), so we load every font dictionary reachable from a page ourselves and
//! match glyphs back to fonts through the glyph identifier, which is a hash of
//! the font program's contents and the glyph id.
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

use hayro_font::{Font, FontCache, FontGlyph, GlyphId};
use hayro_interpret::InterpreterSettings;
use hayro_syntax::object::{Array, Dict, Name, Number, Stream};
use hayro_syntax::page::{Page, Resources};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Index of the placeholder font in every font table: glyphs whose font is
/// unknown (Type 3 glyphs, glyphs no candidate font produces) refer to it.
pub const UNKNOWN_FONT: u16 = 0;

/// Length of the subset tag that PDF producers put in front of the names of
/// embedded subsets (`ABCDEF+CMR10`).
const SUBSET_PREFIX_LEN: usize = 6;
/// Font descriptor flag (bit 1): all glyphs have the same width.
const FLAG_FIXED_PITCH: u32 = 1;
/// Font descriptor flag (bit 2): the glyphs have serifs.
const FLAG_SERIF: u32 = 2;
/// Font descriptor flag (bit 7): the glyphs are italic.
const FLAG_ITALIC: u32 = 1 << 6;
/// Font descriptor flag (bit 19): draw bold even at small sizes, set for
/// bold fonts.
const FLAG_FORCE_BOLD: u32 = 1 << 18;
/// A font descriptor weight at or above this is bold (600 is "semibold" on
/// the CSS / OpenType scale).
const BOLD_MIN_WEIGHT: f32 = 600.0;
/// A font whose italic angle exceeds this (in degrees, either direction) is
/// italic; real italics lean by about 10 to 15 degrees.
const ITALIC_MIN_ANGLE_DEG: f32 = 0.5;
/// How deep to follow form XObjects nested in form XObjects when collecting
/// the fonts of a page (deeper nesting does not occur in real papers, and the
/// limit guards against pathological files).
const MAX_FORM_DEPTH: u32 = 6;

/// Name fragments (lower case) of math fonts: the TeX math families
/// (Computer Modern, AMS, Latin Modern, txfonts, pxfonts, newtx, MathTime,
/// ...), symbol and extension fonts, and the generic "math" / "symbol".
const MATH_PATTERNS: &[&str] = &[
    "cmmi", "cmsy", "cmex", "cmbsy", "msam", "msbm", "eufm", "eufb", "eusm", "eurm", "rsfs", "math", "mtmi", "mtsy",
    "mtex", "txsy", "txmi", "txex", "pxsy", "pxmi", "pxex", "ntxmi", "ntxsy", "ntxex", "stmary", "wasy", "esint",
    "bbm", "bbold", "dsrom", "dsss", "mnsymbol", "lasy", "cmbrmi", "lmmi", "lmsy", "lmex", "fplmbi", "zplm", "zptm",
    "zeur", "zeus", "symbol", "mdbch", "md-",
];
/// Name prefixes (lower case) of LaTeX's picture-mode fonts (`line10`,
/// `lcircle10`), which draw diagram pieces and count as math.
const MATH_PREFIXES: &[&str] = &["line", "lcircle"];
/// Name fragments (lower case) of bold TeX fonts, whose names say "bx" or
/// "b" instead of "bold".
const TEX_BOLD_PATTERNS: &[&str] = &[
    "cmbx", "cmssbx", "cmssdc", "sfbx", "sfsx", "ecbx", "ecsx", "cmbsy", "cmmib", "sfxc", "ectb", "cmtb",
];
/// Prefix of the Computer Modern bold fonts named `cmb` plus the design
/// size (`cmb10`).
const TEX_BOLD_SIZE_PREFIX: &str = "cmb";
/// Name fragments (lower case) that mean bold in conventional font names.
const NAMED_BOLD_PATTERNS: &[&str] = &["bold", "black", "heavy", "demi", "-bd", "extrab", "ultra"];
/// Abbreviation of "medium" used for the bold weight of some URW fonts
/// (`NimbusRomNo9L-Medi`); the full word "medium" is a regular weight.
const MEDIUM_BOLD_ABBREVIATION: &str = "medi";
/// The full word that [`MEDIUM_BOLD_ABBREVIATION`] must not be part of.
const MEDIUM_WORD: &str = "medium";
/// Name endings (lower case) that mean bold (`Times-B`, `ptmb8t.b`).
const BOLD_SUFFIXES: &[&str] = &["-b", ".b"];
/// Name fragments (lower case) of italic and slanted TeX fonts.
const TEX_ITALIC_PATTERNS: &[&str] = &[
    "cmti", "cmsl", "cmbxti", "cmbxsl", "cmssi", "cmitt", "sfti", "sfsl", "sfbi", "ecti", "ecsl", "ecbi", "cmu",
    "sfsi", "sfso",
];
/// Name fragments (lower case) that mean italic in conventional font names.
const NAMED_ITALIC_PATTERNS: &[&str] = &["ital", "obli", "slant", "-it", "kursiv", "inclined"];
/// Name endings (lower case) that mean italic (`Times-I`, `-BI`, ...).
const ITALIC_SUFFIXES: &[&str] = &["-i", "-ri", "-bi", ".i"];
/// Name fragments (lower case) of the math-italic alphabets, the only math
/// fonts that are typeset in italics.
const MATH_ITALIC_PATTERNS: &[&str] = &["cmmi", "cmmib", "ntxmi", "txmi", "pxmi", "mtmi", "lmmi"];
/// Name fragments (lower case) of monospaced fonts.
const MONO_PATTERNS: &[&str] = &[
    "mono",
    "courier",
    "cmtt",
    "cmitt",
    "typewriter",
    "consol",
    "inconsolata",
    "sftt",
    "ectt",
    "txtt",
    "menlo",
    "sourcecode",
    "nimbusmon",
    "lmtt",
    "cmvtt",
    "dejavusansm",
    "zi4",
    "fira",
    "cascadia",
];
/// Name prefix (lower case) of the embedded "Mathematical Pi" font of
/// APS/Elsevier typesetting, which has an anonymous name.
const MATH_PI_PREFIX: &str = "advp4c4e74";
/// Name fragments (lower case) of the "Mathematical Pi 1" font.
const MATH_PI_PATTERNS: &[&str] = &["mathematicalpi-one", "mathpi-one"];
/// Name fragments (lower case) of sans-serif fonts.
const SANS_PATTERNS: &[&str] = &[
    "helvet",
    "arial",
    "sans",
    "cmss",
    "nimbussan",
    "verdana",
    "calibri",
    "roboto",
    "lato",
    "lmss",
    "tahoma",
    "futura",
    "avenir",
    "gotham",
    "inter-",
    "segoe",
    "frutiger",
    "univers",
    "phv",
    "sfss",
];

/// What the analysis needs to know about a font, inferred from its name and
/// font descriptor.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FontInfo {
    /// PostScript name without the subset prefix (`ABCDEF+`).
    pub name: String,
    /// Bold weight.
    pub bold: bool,
    /// Italic or slanted (false for math symbol fonts, which report an
    /// italic angle but are not typeset as italics).
    pub italic: bool,
    /// Monospaced (typewriter) font.
    pub mono: bool,
    /// The font is a dedicated math font (symbols, math italic, extensions).
    pub math: bool,
    /// The font descriptor says the font has serifs.
    pub serif: bool,
    /// Adobe "Mathematical Pi 1" style font whose glyphs are encoded on
    /// Latin-1 letters (used by APS/Elsevier typesetting).
    pub math_pi: bool,
    /// Sans-serif text font (never set for math or monospaced fonts).
    pub sans: bool,
}

impl FontInfo {
    /// Describes a font from its PostScript name `raw` and the `Flags`,
    /// `ItalicAngle` and `FontWeight` entries of its font descriptor (0 when
    /// absent).
    ///
    /// The style is inferred from the name first (TeX font naming and the
    /// usual "Bold" / "Italic" suffixes), with the descriptor as a fallback.
    pub fn from_name(raw: &str, flags: u32, italic_angle: f32, weight: f32) -> Self {
        let name = match raw.split_once('+') {
            Some((prefix, rest)) if prefix.len() == SUBSET_PREFIX_LEN => rest.to_string(),
            _ => raw.to_string(),
        };
        let lower = name.to_ascii_lowercase();
        let math = is_math_name(&lower);
        let bold = is_bold(&lower, flags, weight);
        let italic = is_italic(&lower, flags, italic_angle);
        let mono = flags & FLAG_FIXED_PITCH != 0 || contains_any(&lower, MONO_PATTERNS);
        let serif = flags & FLAG_SERIF != 0;
        // Symbol fonts report an italic angle, but only the math-italic
        // alphabets should be typeset in italics.
        let math_italic = contains_any(&lower, MATH_ITALIC_PATTERNS);
        let italic = if math { math_italic } else { italic };

        let math_pi = lower.starts_with(MATH_PI_PREFIX) || contains_any(&lower, MATH_PI_PATTERNS);
        let sans = !math && !mono && contains_any(&lower, SANS_PATTERNS);
        Self {
            sans,
            name,
            bold: bold && !math_pi,
            italic: italic && !math_pi,
            mono,
            math: math || math_pi,
            serif,
            math_pi,
        }
    }
}

/// A font of the current page that a glyph may come from.
struct Candidate<'a> {
    /// The loaded font program.
    font: Font<'a>,
    /// Index of the font in the registry's table.
    idx: u16,
}

/// Document-wide registry of the fonts that were seen on any page.
pub struct FontRegistry<'a> {
    /// Cache of loaded font programs.
    cache: FontCache<'a>,
    /// Font and CMap resolvers used to load fonts.
    settings: InterpreterSettings,
    /// Font table: glyphs refer to fonts by index into it. Entry
    /// [`UNKNOWN_FONT`] is the placeholder for unidentified glyphs.
    pub infos: Vec<FontInfo>,
    /// Font name -> index into `infos` (fonts are deduplicated by name).
    by_name: HashMap<String, u16>,
    /// Fonts already loaded, keyed by the font dictionary's cache key.
    loaded: HashMap<u128, Option<(u16, Font<'a>)>>,
    /// Glyph identifier -> font index.
    glyph_cache: HashMap<u128, u16>,
    /// Glyph identifier -> outline bounding box in glyph units.
    outline_cache: HashMap<u128, Option<kurbo::Rect>>,
    /// The fonts of the current page, the only ones searched by
    /// [`Self::identify`].
    candidates: Vec<Candidate<'a>>,
}

impl<'a> FontRegistry<'a> {
    /// An empty registry whose table holds only the "unknown" placeholder
    /// font at index [`UNKNOWN_FONT`].
    pub fn new() -> Self {
        Self {
            cache: FontCache::new(),
            settings: InterpreterSettings::default(),
            infos: vec![FontInfo {
                name: "unknown".into(),
                ..Default::default()
            }],
            by_name: HashMap::new(),
            loaded: HashMap::new(),
            glyph_cache: HashMap::new(),
            outline_cache: HashMap::new(),
            candidates: Vec::new(),
        }
    }

    /// Makes the fonts of `page` (including those of nested form XObjects)
    /// the active candidate set for [`Self::identify`].
    pub fn begin_page(&mut self, page: &Page<'a>) {
        self.candidates.clear();
        let mut seen_forms = HashSet::new();
        let resources = page.resources().clone();
        self.collect_fonts(&resources, &mut seen_forms, 0);
    }

    /// Bounding box of the glyph outline in glyph space (1000 units per em),
    /// or `None` for blank glyphs.
    pub fn outline_bbox(&mut self, glyph: &hayro_font::OutlineGlyph) -> Option<kurbo::Rect> {
        use kurbo::Shape;
        let key = glyph.identifier().cache_key();
        *self.outline_cache.entry(key).or_insert_with(|| {
            let outline = glyph.outline();
            if outline.elements().is_empty() {
                return None;
            }
            let bounds = outline.bounding_box();
            (bounds.width() > 0.0 && bounds.height() > 0.0 && bounds.width().is_finite() && bounds.height().is_finite())
                .then_some(bounds)
        })
    }

    /// Finds the font an outline glyph belongs to: the first candidate font
    /// of the current page that produces the same glyph. Returns
    /// [`UNKNOWN_FONT`] when none does. Results are cached per glyph.
    pub fn identify(&mut self, glyph: &hayro_font::OutlineGlyph) -> u16 {
        let key = glyph.identifier().cache_key();
        if let Some(&idx) = self.glyph_cache.get(&key) {
            return idx;
        }
        let gid: GlyphId = glyph.glyph_id();
        let code = glyph.char_code();
        let mut found = UNKNOWN_FONT;
        for candidate in &self.candidates {
            if let FontGlyph::Outline(outline) = candidate.font.glyph(gid, code)
                && outline.identifier().cache_key() == key
            {
                found = candidate.idx;
                break;
            }
        }
        self.glyph_cache.insert(key, found);
        found
    }

    /// Index of the font named like `info` in the table, adding `info` to the
    /// table if no font of that name is there yet.
    fn intern(&mut self, info: FontInfo) -> u16 {
        if let Some(&idx) = self.by_name.get(&info.name) {
            return idx;
        }
        let idx = self.infos.len() as u16;
        self.by_name.insert(info.name.clone(), idx);
        self.infos.push(info);
        idx
    }

    /// Adds the fonts of `resources` to the candidates, then those of the
    /// form XObjects it uses, recursively up to [`MAX_FORM_DEPTH`] levels.
    /// `seen` holds the object ids of the forms already visited, so a form
    /// used several times (or recursively) is visited once.
    fn collect_fonts(&mut self, resources: &Resources<'a>, seen: &mut HashSet<(u32, i32)>, depth: u32) {
        let keys: Vec<Name<'a>> = resources.fonts.keys().collect();
        for key in keys {
            if let Some(dict) = resources.fonts.get::<Dict<'a>>(&key) {
                self.add_font(&dict);
            }
        }
        if depth >= MAX_FORM_DEPTH {
            return;
        }
        let keys: Vec<Name<'a>> = resources.x_objects.keys().collect();
        for key in keys {
            let Some(stream) = resources.x_objects.get::<Stream<'a>>(&key) else {
                continue;
            };
            let id = stream.obj_id();
            if !seen.insert((id.obj_number, id.gen_number)) {
                continue;
            }
            let dict = stream.dict();
            let is_form = dict
                .get::<Name<'_>>("Subtype")
                .is_some_and(|subtype| subtype.as_str() == "Form");
            if !is_form {
                continue;
            }
            if let Some(res) = dict.get::<Dict<'a>>("Resources") {
                let res = Resources::new(res);
                self.collect_fonts(&res, seen, depth + 1);
            }
        }
    }

    /// Makes the font of `dict` a candidate of the current page, loading and
    /// describing it the first time it is seen. Fonts that cannot be loaded
    /// are remembered as such and never become candidates.
    fn add_font(&mut self, dict: &Dict<'a>) {
        use hayro_interpret::CacheKey;
        let key = dict.cache_key();
        if let Some(entry) = self.loaded.get(&key) {
            if let Some((idx, font)) = entry
                && !self.candidates.iter().any(|candidate| candidate.idx == *idx)
            {
                self.candidates.push(Candidate {
                    font: font.clone(),
                    idx: *idx,
                });
            }
            return;
        }

        let info = describe(dict);
        let font = self
            .cache
            .font(dict, &self.settings.font_resolver, &self.settings.cmap_resolver);
        let entry = font.map(|font| {
            let idx = self.intern(info);
            (idx, font)
        });
        if let Some((idx, font)) = &entry {
            self.candidates.push(Candidate {
                font: font.clone(),
                idx: *idx,
            });
        }
        self.loaded.insert(key, entry);
    }
}

impl Default for FontRegistry<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// Describes the font of a font dictionary.
///
/// The name is the `BaseFont`, else that of the first descendant font
/// (Type 0 fonts), else the descriptor's `FontName`, else the font subtype,
/// else "unnamed". The descriptor is the font's own or its descendant's.
fn describe(dict: &Dict<'_>) -> FontInfo {
    let mut name = dict
        .get::<Name<'_>>("BaseFont")
        .map(|base_font| base_font.as_str().to_string())
        .unwrap_or_default();

    let mut descriptor = dict.get::<Dict<'_>>("FontDescriptor");
    if descriptor.is_none()
        && let Some(descendants) = dict.get::<Array<'_>>("DescendantFonts")
        && let Some(descendant) = descendants.iter::<Dict<'_>>().next()
    {
        descriptor = descendant.get::<Dict<'_>>("FontDescriptor");
        if name.is_empty() {
            name = descendant
                .get::<Name<'_>>("BaseFont")
                .map(|base_font| base_font.as_str().to_string())
                .unwrap_or_default();
        }
    }

    let num = |entries: &Dict<'_>, key: &str| -> Option<f32> {
        entries.get::<Number>(key).map(|number| number.as_f64() as f32)
    };
    let (mut flags, mut angle, mut weight) = (0u32, 0.0f32, 0.0f32);
    if let Some(desc) = &descriptor {
        flags = num(desc, "Flags").unwrap_or(0.0) as u32;
        angle = num(desc, "ItalicAngle").unwrap_or(0.0);
        weight = num(desc, "FontWeight").unwrap_or(0.0);
        if name.is_empty() {
            name = desc
                .get::<Name<'_>>("FontName")
                .map(|font_name| font_name.as_str().to_string())
                .unwrap_or_default();
        }
    }
    if name.is_empty() {
        name = dict
            .get::<Name<'_>>("Subtype")
            .map(|subtype| subtype.as_str().to_string())
            .unwrap_or_else(|| "unnamed".into());
    }
    FontInfo::from_name(&name, flags, angle, weight)
}

/// True when the lower-case font name `lower` contains any of `patterns`.
fn contains_any(lower: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| lower.contains(pattern))
}

/// True when the lower-case font name `lower` names a math font.
fn is_math_name(lower: &str) -> bool {
    contains_any(lower, MATH_PATTERNS) || MATH_PREFIXES.iter().any(|prefix| lower.starts_with(prefix))
}

/// True when the font is bold: by its TeX name (`cmbx10`, `cmb10`), by a
/// conventional name ("Bold", "Medi", `-B`), by its descriptor weight, or by
/// the descriptor's force-bold flag.
fn is_bold(lower: &str, flags: u32, weight: f32) -> bool {
    let tex_bold = contains_any(lower, TEX_BOLD_PATTERNS)
        || (lower.starts_with(TEX_BOLD_SIZE_PREFIX)
            && lower[TEX_BOLD_SIZE_PREFIX.len()..]
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_digit()));
    let named_bold = contains_any(lower, NAMED_BOLD_PATTERNS)
        || (lower.contains(MEDIUM_BOLD_ABBREVIATION) && !lower.contains(MEDIUM_WORD))
        || BOLD_SUFFIXES.iter().any(|suffix| lower.ends_with(suffix));
    tex_bold || named_bold || weight >= BOLD_MIN_WEIGHT || flags & FLAG_FORCE_BOLD != 0
}

/// True when the font is italic or slanted: by its TeX name, by a
/// conventional name ("Italic", "Oblique", `-I`), by its descriptor's
/// italic angle or italic flag, or because it is a math-italic font.
fn is_italic(lower: &str, flags: u32, italic_angle: f32) -> bool {
    let tex_italic = contains_any(lower, TEX_ITALIC_PATTERNS);
    let named_italic =
        contains_any(lower, NAMED_ITALIC_PATTERNS) || ITALIC_SUFFIXES.iter().any(|suffix| lower.ends_with(suffix));
    tex_italic
        || named_italic
        || italic_angle.abs() > ITALIC_MIN_ANGLE_DEG
        || flags & FLAG_ITALIC != 0
        || contains_any(lower, MATH_ITALIC_PATTERNS)
}

#[cfg(test)]
mod tests {
    use super::FontInfo;

    /// Style inference from typical TeX and PostScript font names.
    #[test]
    fn font_names() {
        let font = FontInfo::from_name("ABCDEF+CMBX10", 0, 0.0, 0.0);
        assert_eq!(font.name, "CMBX10");
        assert!(font.bold && !font.italic && !font.math);

        let font = FontInfo::from_name("NimbusRomNo9L-ReguItal", 0, 0.0, 0.0);
        assert!(font.italic && !font.bold);

        let font = FontInfo::from_name("NimbusRomNo9L-Medi", 0, 0.0, 0.0);
        assert!(font.bold);

        // Math italic is italic, the symbol font is not (despite its angle).
        assert!(FontInfo::from_name("CMMI10", 0, -14.0, 0.0).italic);
        let sy = FontInfo::from_name("CMSY10", 0, -14.0, 0.0);
        assert!(sy.math && !sy.italic);

        assert!(FontInfo::from_name("SFTT1000", 0, 0.0, 0.0).mono);
        assert!(!FontInfo::from_name("Helvetica", 0, 0.0, 0.0).bold);
    }
}
