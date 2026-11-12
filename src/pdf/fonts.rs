//! Font identification.
//!
//! The interpreter hands us glyphs but not the PDF font they came from. We
//! need the font name (bold / italic / math / monospace are all inferred from
//! it), so we load every font dictionary reachable from a page ourselves and
//! match glyphs back to fonts through the glyph identifier, which is a hash of
//! the font program's contents and the glyph id.

use hayro_font::{Font, FontCache, FontGlyph, GlyphId};
use hayro_interpret::InterpreterSettings;
use hayro_syntax::object::{Array, Dict, Name, Number, Stream};
use hayro_syntax::page::{Page, Resources};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FontInfo {
    /// PostScript name without the subset prefix (`ABCDEF+`).
    pub name: String,
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
    /// The font is a dedicated math font (symbols, math italic, extensions).
    pub math: bool,
    pub serif: bool,
    /// Adobe "Mathematical Pi 1" style font whose glyphs are encoded on
    /// Latin-1 letters (used by APS/Elsevier typesetting).
    pub math_pi: bool,
    pub sans: bool,
}

impl FontInfo {
    pub fn from_name(raw: &str, flags: u32, italic_angle: f32, weight: f32) -> Self {
        let name = match raw.split_once('+') {
            Some((prefix, rest)) if prefix.len() == 6 => rest.to_string(),
            _ => raw.to_string(),
        };
        let l = name.to_ascii_lowercase();
        let has = |pats: &[&str]| pats.iter().any(|p| l.contains(p));

        let math = has(&[
            "cmmi", "cmsy", "cmex", "cmbsy", "msam", "msbm", "eufm", "eufb", "eusm", "eurm", "rsfs", "math", "mtmi",
            "mtsy", "mtex", "txsy", "txmi", "txex", "pxsy", "pxmi", "pxex", "ntxmi", "ntxsy", "ntxex", "stmary",
            "wasy", "esint", "bbm", "bbold", "dsrom", "dsss", "mnsymbol", "lasy", "cmbrmi", "lmmi", "lmsy", "lmex",
            "fplmbi", "zplm", "zptm", "zeur", "zeus", "symbol", "mdbch", "md-",
        ]) || l.starts_with("line")
            || l.starts_with("lcircle");

        let tex_bold = has(&[
            "cmbx", "cmssbx", "cmssdc", "sfbx", "sfsx", "ecbx", "ecsx", "cmbsy", "cmmib", "sfxc", "ectb", "cmtb",
        ]) || (l.starts_with("cmb") && l[3..].chars().next().is_some_and(|c| c.is_ascii_digit()));
        let named_bold = has(&["bold", "black", "heavy", "demi", "-bd", "extrab", "ultra"])
            || (l.contains("medi") && !l.contains("medium"))
            || l.ends_with("-b")
            || l.ends_with(".b");
        let bold = tex_bold || named_bold || weight >= 600.0 || flags & (1 << 18) != 0;

        let tex_italic = has(&[
            "cmti", "cmsl", "cmbxti", "cmbxsl", "cmssi", "cmitt", "sfti", "sfsl", "sfbi", "ecti", "ecsl", "ecbi",
            "cmu", "sfsi", "sfso",
        ]);
        let named_italic = has(&["ital", "obli", "slant", "-it", "kursiv", "inclined"])
            || l.ends_with("-i")
            || l.ends_with("-ri")
            || l.ends_with("-bi")
            || l.ends_with(".i");
        let italic = tex_italic
            || named_italic
            || italic_angle.abs() > 0.5
            || flags & (1 << 6) != 0
            || has(&["cmmi", "cmmib", "ntxmi", "txmi", "pxmi", "mtmi", "lmmi"]);

        let mono = flags & 1 != 0
            || has(&[
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
            ]);

        let serif = flags & 2 != 0;
        // Symbol fonts report an italic angle, but only the math-italic
        // alphabets should be typeset in italics.
        let math_italic = has(&["cmmi", "cmmib", "ntxmi", "txmi", "pxmi", "mtmi", "lmmi"]);
        let italic = if math { math_italic } else { italic };

        let math_pi = l.starts_with("advp4c4e74") || l.contains("mathematicalpi-one") || l.contains("mathpi-one");
        let sans = !math
            && !mono
            && has(&[
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
            ]);
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

struct Candidate<'a> {
    font: Font<'a>,
    idx: u16,
}

/// Document-wide registry of the fonts that were seen on any page.
pub struct FontRegistry<'a> {
    cache: FontCache<'a>,
    settings: InterpreterSettings,
    pub infos: Vec<FontInfo>,
    by_name: HashMap<String, u16>,
    /// Fonts already loaded, keyed by the font dictionary's cache key.
    loaded: HashMap<u128, Option<(u16, Font<'a>)>>,
    /// Glyph identifier -> font index.
    glyph_cache: HashMap<u128, u16>,
    /// Glyph identifier -> outline bounding box in glyph units.
    outline_cache: HashMap<u128, Option<kurbo::Rect>>,
    candidates: Vec<Candidate<'a>>,
}

pub const UNKNOWN_FONT: u16 = 0;

impl<'a> FontRegistry<'a> {
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

    fn intern(&mut self, info: FontInfo) -> u16 {
        if let Some(&i) = self.by_name.get(&info.name) {
            return i;
        }
        let idx = self.infos.len() as u16;
        self.by_name.insert(info.name.clone(), idx);
        self.infos.push(info);
        idx
    }

    /// Makes the fonts of `page` (including those of nested form XObjects)
    /// the active candidate set for [`Self::identify`].
    pub fn begin_page(&mut self, page: &Page<'a>) {
        self.candidates.clear();
        let mut seen_forms = HashSet::new();
        let resources = page.resources().clone();
        self.collect(&resources, &mut seen_forms, 0);
    }

    fn collect(&mut self, resources: &Resources<'a>, seen: &mut HashSet<(u32, i32)>, depth: u32) {
        let keys: Vec<Name<'a>> = resources.fonts.keys().collect();
        for key in keys {
            if let Some(dict) = resources.fonts.get::<Dict<'a>>(&key) {
                self.add_font(&dict);
            }
        }
        if depth >= 6 {
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
            let is_form = dict.get::<Name<'_>>("Subtype").is_some_and(|n| n.as_str() == "Form");
            if !is_form {
                continue;
            }
            if let Some(res) = dict.get::<Dict<'a>>("Resources") {
                let res = Resources::new(res);
                self.collect(&res, seen, depth + 1);
            }
        }
    }

    fn add_font(&mut self, dict: &Dict<'a>) {
        use hayro_interpret::CacheKey;
        let key = dict.cache_key();
        if let Some(entry) = self.loaded.get(&key) {
            if let Some((idx, font)) = entry
                && !self.candidates.iter().any(|c| c.idx == *idx)
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
            let b = outline.bounding_box();
            (b.width() > 0.0 && b.height() > 0.0 && b.width().is_finite() && b.height().is_finite()).then_some(b)
        })
    }

    /// Finds the font an outline glyph belongs to.
    pub fn identify(&mut self, glyph: &hayro_font::OutlineGlyph) -> u16 {
        let key = glyph.identifier().cache_key();
        if let Some(&idx) = self.glyph_cache.get(&key) {
            return idx;
        }
        let gid: GlyphId = glyph.glyph_id();
        let code = glyph.char_code();
        let mut found = UNKNOWN_FONT;
        for cand in &self.candidates {
            if let FontGlyph::Outline(g) = cand.font.glyph(gid, code)
                && g.identifier().cache_key() == key
            {
                found = cand.idx;
                break;
            }
        }
        self.glyph_cache.insert(key, found);
        found
    }
}

impl Default for FontRegistry<'_> {
    fn default() -> Self {
        Self::new()
    }
}

fn describe(dict: &Dict<'_>) -> FontInfo {
    let mut name = dict
        .get::<Name<'_>>("BaseFont")
        .map(|n| n.as_str().to_string())
        .unwrap_or_default();

    let mut descriptor = dict.get::<Dict<'_>>("FontDescriptor");
    if descriptor.is_none()
        && let Some(desc) = dict.get::<Array<'_>>("DescendantFonts")
        && let Some(d) = desc.iter::<Dict<'_>>().next()
    {
        descriptor = d.get::<Dict<'_>>("FontDescriptor");
        if name.is_empty() {
            name = d
                .get::<Name<'_>>("BaseFont")
                .map(|n| n.as_str().to_string())
                .unwrap_or_default();
        }
    }

    let num = |d: &Dict<'_>, key: &str| -> Option<f32> { d.get::<Number>(key).map(|n| n.as_f64() as f32) };
    let (mut flags, mut angle, mut weight) = (0u32, 0.0f32, 0.0f32);
    if let Some(d) = &descriptor {
        flags = num(d, "Flags").unwrap_or(0.0) as u32;
        angle = num(d, "ItalicAngle").unwrap_or(0.0);
        weight = num(d, "FontWeight").unwrap_or(0.0);
        if name.is_empty() {
            name = d
                .get::<Name<'_>>("FontName")
                .map(|n| n.as_str().to_string())
                .unwrap_or_default();
        }
    }
    if name.is_empty() {
        name = dict
            .get::<Name<'_>>("Subtype")
            .map(|n| n.as_str().to_string())
            .unwrap_or_else(|| "unnamed".into());
    }
    FontInfo::from_name(&name, flags, angle, weight)
}

#[cfg(test)]
mod tests {
    use super::FontInfo;

    #[test]
    fn font_names() {
        let f = FontInfo::from_name("ABCDEF+CMBX10", 0, 0.0, 0.0);
        assert_eq!(f.name, "CMBX10");
        assert!(f.bold && !f.italic && !f.math);

        let f = FontInfo::from_name("NimbusRomNo9L-ReguItal", 0, 0.0, 0.0);
        assert!(f.italic && !f.bold);

        let f = FontInfo::from_name("NimbusRomNo9L-Medi", 0, 0.0, 0.0);
        assert!(f.bold);

        // Math italic is italic, the symbol font is not (despite its angle).
        assert!(FontInfo::from_name("CMMI10", 0, -14.0, 0.0).italic);
        let sy = FontInfo::from_name("CMSY10", 0, -14.0, 0.0);
        assert!(sy.math && !sy.italic);

        assert!(FontInfo::from_name("SFTT1000", 0, 0.0, 0.0).mono);
        assert!(!FontInfo::from_name("Helvetica", 0, 0.0, 0.0).bold);
    }
}
