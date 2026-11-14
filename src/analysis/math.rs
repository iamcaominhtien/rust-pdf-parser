//! Displayed formulas rebuilt as a tree of math structure ([`MathNode`]).
//!
//! An equation is shown as a picture of its region of the page (see
//! [`crops`](super::crops)). Its glyphs and the rules drawn among them
//! (fraction bars, the bars of radicals) are enough to recover most of the
//! structure TeX laid out, which the reflowing renderers can write as
//! MathML: text that can be searched, copied, zoomed and read aloud.
//!
//! The structure comes from the geometry alone. Inside every group of
//! items (the whole formula, then each numerator, denominator, radicand,
//! script and limit), in this order:
//!
//! 1. pieces: the stacked glyphs of a tall bracket or radical sign become
//!    one glyph;
//! 2. accents: an accent glyph right above something (`Ê`, `L̃`);
//! 3. radicals: a radical sign and the bar that starts at its top right
//!    corner, with everything under the bar;
//! 4. fractions: a bar with the line right above and the line right below
//!    it (inner fractions first, so that each is one item of the line it
//!    sits on);
//! 5. limits: the small line right above or below a big operator (∑, ∏) or
//!    an operator name (lim, max, maximize);
//! 6. stacks: lines stacked after a tall opening delimiter (`cases`) or
//!    between two tall delimiters (a matrix), as a table;
//! 7. rows: the lines of a multi-line display, aligned on a relation;
//! 8. in each row, left to right: the tokens on the main baseline, and the
//!    smaller raised and lowered items as their scripts.
//!
//! A formula with anything these steps cannot explain (a rule left over,
//! glyphs stacked without a bar, a glyph without a Unicode value) gets no
//! tree and stays a picture.

use crate::analysis::page::PageData;
use crate::backend::FontInfo;
use crate::common::geom::Rect;
use crate::model::{BlockKind, Document, Line, MathNode};

/// Big operators whose limits TeX sets above and below them in displays.
const BIG_OPERATORS: &[&str] = &["∑", "∏", "∐", "⋃", "⋂", "⨁", "⨂", "⨀", "⋁", "⋀", "⨄", "⨆"];
/// Relations a multi-line display is aligned on.
const RELATIONS: &[&str] = &[
    "=", "≤", "≥", "<", ">", "≈", "≠", "≡", "∝", "∼", "≃", "≅", "⇒", "⇔", "≪", "≫", "≜", "≐",
];
/// Delimiters, which keep the height they have in the PDF.
const DELIMITERS: &[&str] = &["(", ")", "[", "]", "{", "}", "|", "‖", "⟨", "⟩", "⌊", "⌋", "⌈", "⌉"];
/// Pieces of tall symbols that are drawn as a stack of glyphs (the top, the
/// middle extensions and the bottom of a tall bracket or radical), and the
/// symbol they make.
const PIECES: &[(&str, &str)] = &[
    ("√", "√"),
    ("|", "|"),
    ("∣", "|"),
    ("‖", "‖"),
    ("∥", "‖"),
    ("⎛", "("),
    ("⎜", "("),
    ("⎝", "("),
    ("⎞", ")"),
    ("⎟", ")"),
    ("⎠", ")"),
    ("⎡", "["),
    ("⎢", "["),
    ("⎣", "["),
    ("⎤", "]"),
    ("⎥", "]"),
    ("⎦", "]"),
    ("⎧", "{"),
    ("⎨", "{"),
    ("⎩", "{"),
    ("⎫", "}"),
    ("⎬", "}"),
    ("⎭", "}"),
];
/// Accents drawn as glyphs of their own above their base, and the
/// character MathML draws for each.
const ACCENT_GLYPHS: &[(&str, &str)] = &[
    ("ˆ", "^"),
    ("^", "^"),
    ("˜", "~"),
    ("~", "~"),
    ("¯", "¯"),
    ("˙", "˙"),
    ("¨", "¨"),
    ("ˇ", "ˇ"),
    ("˘", "˘"),
    ("´", "´"),
    ("`", "`"),
    ("˚", "˚"),
];
/// Punctuation, which MathML follows by a thin space.
const PUNCTUATION: &[&str] = &[",", ";", ".", ":"];
/// Opening delimiters.
const OPENERS: &[&str] = &["(", "[", "{", "⟨", "⌊", "⌈"];
/// Closing delimiters.
const CLOSERS: &[&str] = &[")", "]", "}", "⟩", "⌋", "⌉"];
/// Symbols that are identifiers rather than operators.
const IDENT_SYMBOLS: &[char] = &['∞', '∂', '∇', '∅', 'ℏ', 'ℓ', '℘'];
/// Combining accents (as folded into the glyph text by the line builder)
/// and the spacing character MathML draws for each.
const ACCENTS: &[(char, &str)] = &[
    ('\u{300}', "`"),
    ('\u{301}', "´"),
    ('\u{302}', "^"),
    ('\u{303}', "~"),
    ('\u{304}', "¯"),
    ('\u{305}', "‾"),
    ('\u{306}', "˘"),
    ('\u{307}', "˙"),
    ('\u{308}', "¨"),
    ('\u{30A}', "˚"),
    ('\u{30C}', "ˇ"),
    ('\u{20D7}', "→"),
];
/// Distance, in em, of the math axis (the middle of fraction bars and of
/// the minus sign) above the baseline in Computer Modern.
const AXIS_HEIGHT_EM: f32 = 0.25;

/// Thresholds of the rebuilding of formulas. Part of
/// [`LayoutConfig`](crate::config::LayoutConfig).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MathConfig {
    /// A path thinner than this (points) and wider than
    /// [`MathConfig::rule_min_width_pt`] is a rule: a fraction bar or the
    /// bar of a radical.
    pub rule_max_thickness_pt: f32,
    /// Narrowest rule, in points.
    pub rule_min_width_pt: f32,
    /// How far (points) the parts of a stack may reach past the ends of its
    /// bar, and how far the bar of a radical may start from the corner of
    /// the radical sign.
    pub bar_tolerance_pt: f32,
    /// Largest vertical gap (em of the formula's font size) between a bar,
    /// an accent or a big operator and the nearest part stacked on it.
    pub stack_max_gap_em: f32,
    /// An item is a script (or a limit) when its font size is less than
    /// this ratio of the font size of its row.
    pub script_max_size_ratio: f32,
    /// A glyph whose baseline is within this distance (em) of the main
    /// baseline of its row sits on it.
    pub baseline_tolerance_em: f32,
    /// Baselines are rounded to multiples of this (points) when the most
    /// common one of a row is chosen, and when letters are grouped into
    /// words.
    pub baseline_step_pt: f32,
    /// Two items overlap horizontally (one above the other) when they share
    /// more than this many points.
    pub overlap_min_pt: f32,
    /// Letters closer than this (em) form one word (a function name).
    pub word_max_gap_em: f32,
    /// A gap wider than this (em) between two items that are not operators
    /// becomes a space (`\quad`, the space before "where").
    pub space_min_em: f32,
    /// Pieces of a tall symbol (bracket, bar, radical sign) stacked closer
    /// than this (em) make one symbol.
    pub piece_max_gap_em: f32,
    /// A small item is not a limit when more than this ratio of its height
    /// is level with a glyph of full size: it is a script of another line.
    pub limit_level_overlap_ratio: f32,
    /// Two limit pieces on one line are joined when closer than this (em).
    pub limit_join_gap_em: f32,
    /// A delimiter taller than this (em) keeps its height
    /// (`\left(`, `\bigl(`).
    pub tall_delimiter_min_height_em: f32,
    /// Columns of a stack (a matrix, `cases`) are cut where every line has
    /// a gap wider than this (em).
    pub column_min_gap_em: f32,
    /// Relations of different rows whose left edges are this close (points)
    /// are aligned.
    pub align_tolerance_pt: f32,
    /// A row of a multi-line display whose font is smaller than this ratio
    /// of the largest row is a stray limit or script that belongs to
    /// another equation: the formula stays a picture.
    pub row_min_size_ratio: f32,
}

impl Default for MathConfig {
    fn default() -> Self {
        Self {
            rule_max_thickness_pt: 1.2,
            rule_min_width_pt: 2.0,
            bar_tolerance_pt: 1.5,
            stack_max_gap_em: 0.7,
            script_max_size_ratio: 0.85,
            baseline_tolerance_em: 0.08,
            baseline_step_pt: 0.5,
            overlap_min_pt: 0.5,
            word_max_gap_em: 0.15,
            space_min_em: 0.6,
            piece_max_gap_em: 0.1,
            limit_level_overlap_ratio: 0.5,
            limit_join_gap_em: 0.3,
            tall_delimiter_min_height_em: 1.3,
            column_min_gap_em: 0.8,
            align_tolerance_pt: 2.0,
            row_min_size_ratio: 0.8,
        }
    }
}

/// Alphabet of a letter or digit, from its font.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Alphabet {
    /// The usual letters.
    Plain,
    /// Bold upright (`\mathbf`).
    Bold,
    /// Bold italic (`\boldsymbol`).
    BoldItalic,
    /// Calligraphic or script capitals (`\mathcal`).
    Script,
    /// Blackboard bold (`\mathbb`).
    DoubleStruck,
    /// Fraktur (`\mathfrak`).
    Fraktur,
}

/// A glyph of the formula.
#[derive(Debug, Clone)]
struct Glyph {
    /// The text it stands for (possibly with a combining accent).
    text: String,
    /// Baseline.
    baseline: f32,
    /// Font size in points.
    size: f32,
    /// Ink height in em of the font size.
    height_em: f32,
    /// Set in italics.
    italic: bool,
    /// Set in the text font (not a math font): words are text.
    text_font: bool,
    /// Alphabet of the font.
    alphabet: Alphabet,
    /// The line builder saw a space before the glyph (in a formula, an
    /// explicit space: `\,`, `\quad`, the space around text).
    space_before: bool,
}

/// What an item of a formula is.
#[derive(Debug, Clone)]
enum Content {
    /// One glyph.
    Glyph(Glyph),
    /// A horizontal rule.
    Rule,
    /// A part already rebuilt (a fraction, a radical, an operator with its
    /// limits).
    Built(MathNode),
}

/// A piece of a formula with its place on the page.
#[derive(Debug, Clone)]
struct Item {
    /// What it is.
    content: Content,
    /// Its ink.
    ink: Rect,
    /// Left edge of its horizontal extent (the advance box of a glyph).
    left: f32,
    /// Right edge of its horizontal extent.
    right: f32,
    /// Largest font size in it, in points (0 for rules).
    size: f32,
    /// The baseline it sits on: that of a glyph, of the base of an accent
    /// or of an operator with limits; `None` for rules and for parts
    /// centred on the math axis (fractions, radicals).
    baseline: Option<f32>,
}

impl Item {
    /// The glyph, if the item is one.
    fn glyph(&self) -> Option<&Glyph> {
        match &self.content {
            Content::Glyph(glyph) => Some(glyph),
            _ => None,
        }
    }

    /// A rebuilt part covering `parts`, sitting on `baseline`.
    fn built(node: MathNode, parts: &[&Item], baseline: Option<f32>) -> Item {
        let ink = parts.iter().fold(Rect::EMPTY, |acc, item| acc.union(&item.ink));
        Item {
            content: Content::Built(node),
            ink,
            left: ink.x0,
            right: ink.x1,
            size: parts.iter().map(|item| item.size).fold(0.0, f32::max),
            baseline,
        }
    }

    /// The baseline of the largest of `items` that has one.
    fn baseline_of(items: &[&Item]) -> Option<f32> {
        items
            .iter()
            .filter(|item| item.baseline.is_some())
            .max_by(|a, b| a.size.total_cmp(&b.size))
            .and_then(|item| item.baseline)
    }
}

/// Where an item of a row sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// On the main baseline (or a rebuilt part centred on it).
    Base,
    /// A superscript.
    Raised,
    /// A subscript.
    Lowered,
}

/// What an item of a row starts or ends with, for the spaces around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edge {
    /// An operator, which MathML spaces itself.
    Operator,
    /// Punctuation (which MathML follows by a thin space), a delimiter or
    /// a slash: only a wide gap next to it is a space of its own.
    Punctuation,
    /// Anything else.
    Plain,
}

impl Edge {
    /// The edge made by a glyph of text `text`.
    fn of(text: &str) -> Edge {
        // Delimiters and the slash are written without space around them:
        // like punctuation, only a wide gap next to them is a space.
        if PUNCTUATION.contains(&text) || DELIMITERS.contains(&text) || text == "/" {
            Edge::Punctuation
        } else if token_kind(text) == TokenKind::Op {
            Edge::Operator
        } else {
            Edge::Plain
        }
    }
}

/// What kind of token a glyph makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    /// A letter or a symbol used as an identifier.
    Letter,
    /// A digit.
    Digit,
    /// Anything else: an operator, a relation, a delimiter, punctuation.
    Op,
}

/// Rebuilds the formulas of the parts of a formula, with the thresholds
/// and the font size of the whole formula.
struct Parser<'a> {
    /// Thresholds.
    cfg: &'a MathConfig,
    /// Font size of the formula (its largest glyphs), in points.
    size: f32,
}

/// Gives every equation of `doc` whose structure can be rebuilt its
/// [`MathNode`] tree (in [`Block::math`](crate::model::Block::math)); the
/// others keep `None` and stay pictures.
pub fn build_math(doc: &mut Document, pages: &[PageData], fonts: &[FontInfo], cfg: &MathConfig) {
    for block in &mut doc.blocks {
        let BlockKind::Equation { image, .. } = &block.kind else {
            continue;
        };
        let rules = page_rules(&pages[block.page], &image.rect, cfg);
        block.math = formula(&block.lines, &rules, fonts, cfg);
    }
}

/// The tree of the formula made of `lines` and `rules`, or `None` when some
/// part of it cannot be explained.
fn formula(lines: &[Line], rules: &[Rect], fonts: &[FontInfo], cfg: &MathConfig) -> Option<MathNode> {
    let mut items = Vec::new();
    for line in lines {
        for glyph in &line.glyphs {
            // A glyph without a Unicode value, or from the private use area,
            // cannot be written as text.
            if glyph.text.is_empty() || glyph.text.chars().any(|ch| ch == '\u{FFFD}' || is_private(ch)) {
                return None;
            }
            let font = fonts.get(glyph.font as usize)?;
            let (italic, text_font, alphabet) = font_style(font);
            items.push(Item {
                content: Content::Glyph(Glyph {
                    // The micro sign of text fonts is the Greek letter mu.
                    text: glyph.text.replace('µ', "μ"),
                    baseline: glyph.y,
                    size: glyph.size,
                    height_em: glyph.ink.height() / glyph.size.max(1.0),
                    italic,
                    text_font,
                    alphabet,
                    space_before: glyph.space_before,
                }),
                ink: glyph.ink,
                left: glyph.x0,
                right: glyph.x1,
                size: glyph.size,
                baseline: Some(glyph.y),
            });
        }
    }
    let size = items.iter().map(|item| item.size).fold(0.0, f32::max);
    if items.is_empty() || size <= 0.0 {
        return None;
    }
    items.extend(rules.iter().map(|&ink| Item {
        content: Content::Rule,
        ink,
        left: ink.x0,
        right: ink.x1,
        size: 0.0,
        baseline: None,
    }));
    Parser { cfg, size }.group(items)
}

/// The rules (thin horizontal paths) of `page` inside `region`.
fn page_rules(page: &PageData, region: &Rect, cfg: &MathConfig) -> Vec<Rect> {
    page.paths
        .iter()
        .filter(|path| !path.is_white())
        .map(|path| path.bbox)
        .filter(|bbox| bbox.height() <= cfg.rule_max_thickness_pt && bbox.width() >= cfg.rule_min_width_pt)
        .filter(|bbox| region.contains(bbox))
        .collect()
}

/// Italic shape, text font and alphabet of the glyphs of `font`.
fn font_style(font: &FontInfo) -> (bool, bool, Alphabet) {
    let lower = font.name.to_ascii_lowercase();
    let alphabet = if lower.starts_with("msbm") || lower.contains("bbold") || lower.contains("dsrom") {
        Alphabet::DoubleStruck
    } else if lower.starts_with("euf") {
        Alphabet::Fraktur
    } else if lower.starts_with("cmsy")
        || lower.starts_with("cmbsy")
        || lower.contains("rsfs")
        || lower.starts_with("eus")
    {
        Alphabet::Script
    } else if font.bold && font.italic {
        Alphabet::BoldItalic
    } else if font.bold {
        Alphabet::Bold
    } else {
        Alphabet::Plain
    };
    let tex_font = lower.starts_with("cm") || lower.starts_with("lm") || lower.starts_with("ms");
    (font.italic, !font.math && !tex_font && !font.mono, alphabet)
}

/// A character of the private use area (unmapped symbols of some fonts).
fn is_private(ch: char) -> bool {
    ('\u{E000}'..='\u{F8FF}').contains(&ch)
}

impl Parser<'_> {
    /// `em` of the formula's font size, in points.
    fn em(&self, em: f32) -> f32 {
        em * self.size
    }

    /// The tree of a group of items: its tall symbols, accents, radicals,
    /// fractions and limits, then its rows (see the module documentation).
    fn group(&self, items: Vec<Item>) -> Option<MathNode> {
        if items.is_empty() {
            return Some(MathNode::Row(Vec::new()));
        }
        let items = self.pieces(items);
        let items = self.accents(items)?;
        let items = self.radicals(items)?;
        let items = self.fractions(items)?;
        let items = self.limits(items)?;
        let items = self.stacks(items)?;
        let mut rows = self.rows(items);
        if rows.len() == 1 {
            self.row(rows.pop()?)
        } else {
            self.table(rows)
        }
    }

    /// Joins the pieces of tall symbols (stacked glyphs that make one
    /// bracket, bar or radical sign) into one glyph.
    fn pieces(&self, mut items: Vec<Item>) -> Vec<Item> {
        let symbol = |item: &Item| {
            let glyph = item.glyph()?;
            PIECES
                .iter()
                .find(|(piece, _)| *piece == glyph.text)
                .map(|(_, symbol)| *symbol)
        };
        let gap = self.em(self.cfg.piece_max_gap_em);
        loop {
            let pair = (0..items.len()).find_map(|a| {
                let first = symbol(&items[a])?;
                (0..items.len())
                    .find(|&b| {
                        let (top, bottom) = (&items[a].ink, &items[b].ink);
                        b != a
                            && symbol(&items[b]) == Some(first)
                            && top.cy() < bottom.cy()
                            && bottom.y0 - top.y1 <= gap
                            && top.x_overlap(bottom) > 0.0
                    })
                    .map(|b| (a, b, first))
            });
            let Some((a, b, symbol)) = pair else {
                return items;
            };
            let lower = items.swap_remove(a.max(b));
            let upper = &mut items[a.min(b)];
            upper.ink = upper.ink.union(&lower.ink);
            upper.left = upper.left.min(lower.left);
            upper.right = upper.right.max(lower.right);
            if let Content::Glyph(glyph) = &mut upper.content {
                glyph.text = symbol.to_string();
                glyph.height_em = upper.ink.height() / glyph.size.max(1.0);
            }
        }
    }

    /// Replaces every accent glyph that sits right above something (`Ê`,
    /// a tilde over `AB`) and that something by one accented node.
    fn accents(&self, mut items: Vec<Item>) -> Option<Vec<Item>> {
        loop {
            let found = (0..items.len()).find_map(|idx| {
                let glyph = items[idx].glyph()?;
                let (_, symbol) = ACCENT_GLYPHS.iter().find(|(accent, _)| *accent == glyph.text)?;
                let ink = items[idx].ink;
                let tol = self.cfg.bar_tolerance_pt;
                let base = self.stacked(
                    &items,
                    |item| item.ink.cx() >= ink.x0 - tol && item.ink.cx() <= ink.x1 + tol,
                    idx,
                    false,
                );
                (!base.is_empty()).then_some((idx, *symbol, base))
            });
            let Some((accent, symbol, base)) = found else {
                return Some(items);
            };
            let (taken, rest) = split_off(items, |idx| idx == accent || base.contains(&idx));
            let base_items: Vec<Item> = taken
                .iter()
                .filter(|(idx, _)| base.contains(idx))
                .map(|(_, item)| item.clone())
                .collect();
            let baseline = Item::baseline_of(&base_items.iter().collect::<Vec<_>>());
            let node = MathNode::UnderOver {
                base: Box::new(self.group(base_items)?),
                under: None,
                over: Some(Box::new(operator(symbol))),
                accent: true,
            };
            let parts: Vec<&Item> = taken.iter().map(|(_, item)| item).collect();
            items = rest;
            items.push(Item::built(node, &parts, baseline));
        }
    }

    /// Replaces every radical sign of `items`, its bar and what is under
    /// the bar by one rebuilt radical. The radical with the widest bar goes
    /// first: nested radicals are under its bar. Returns `None` for a
    /// radical sign without a bar.
    fn radicals(&self, mut items: Vec<Item>) -> Option<Vec<Item>> {
        loop {
            let mut widest: Option<(usize, usize)> = None;
            for idx in 0..items.len() {
                if items[idx].glyph().is_none_or(|glyph| glyph.text != "√") {
                    continue;
                }
                // A radical sign without a bar is not understood.
                let bar = self.radical_bar(&items, idx)?;
                if widest.is_none_or(|(_, best)| items[bar].ink.width() > items[best].ink.width()) {
                    widest = Some((idx, bar));
                }
            }
            let Some((sign, bar)) = widest else {
                return Some(items);
            };
            items = self.take_radical(items, sign, bar)?;
        }
    }

    /// The rule that starts at the top right corner of the radical sign
    /// `sign` (the nearest one).
    fn radical_bar(&self, items: &[Item], sign: usize) -> Option<usize> {
        let tol = self.cfg.bar_tolerance_pt;
        let ink = items[sign].ink;
        (0..items.len())
            .filter(|&idx| matches!(items[idx].content, Content::Rule))
            .filter(|&idx| {
                let bar = items[idx].ink;
                (bar.y0 - ink.y0).abs() <= tol && bar.x0 >= ink.cx() && bar.x0 <= ink.x1 + tol
            })
            .min_by(|&a, &b| items[a].ink.x0.total_cmp(&items[b].ink.x0))
    }

    /// Builds the radical of sign `sign` and bar `bar`: the radicand is
    /// what lies under the bar, the index what sits small in the crook of
    /// the sign.
    fn take_radical(&self, items: Vec<Item>, sign: usize, bar: usize) -> Option<Vec<Item>> {
        let tol = self.cfg.bar_tolerance_pt;
        let sign_ink = items[sign].ink;
        let bar_ink = items[bar].ink;
        let sign_size = items[sign].size;
        let mut radicand = Vec::new();
        let mut index = Vec::new();
        let mut rest = Vec::new();
        let mut parts = Vec::new();
        for (idx, item) in items.into_iter().enumerate() {
            let ink = item.ink;
            if idx == sign || idx == bar {
                parts.push(item);
            } else if ink.x0 >= bar_ink.x0 - tol
                && ink.x1 <= bar_ink.x1 + tol
                && ink.y0 >= bar_ink.y0 - tol
                && ink.cy() <= sign_ink.y1 + tol
            {
                radicand.push(item);
            } else if ink.x1 <= bar_ink.x0 + tol
                && ink.cx() >= sign_ink.x0 - tol
                && ink.y1 <= sign_ink.cy()
                && ink.y1 > sign_ink.y0
                && item.size < sign_size * self.cfg.script_max_size_ratio
            {
                index.push(item);
            } else {
                rest.push(item);
            }
        }
        if radicand.is_empty() {
            return None;
        }
        let all: Vec<&Item> = parts.iter().chain(&radicand).chain(&index).collect();
        let base = Box::new(self.group(radicand.clone())?);
        let node = if index.is_empty() {
            MathNode::Sqrt(base)
        } else {
            MathNode::Root {
                base,
                index: Box::new(self.group(index.clone())?),
            }
        };
        rest.push(Item::built(node, &all, None));
        Some(rest)
    }

    /// Replaces every bar of `items` and what is stacked right above and
    /// below it by one fraction (or an overline or underline when only one
    /// side has something). The narrowest bar goes first: a fraction nested
    /// in a numerator is then one item on the line of that numerator. Returns `None` for a bar with nothing on either
    /// side.
    fn fractions(&self, mut items: Vec<Item>) -> Option<Vec<Item>> {
        loop {
            let bar = (0..items.len())
                .filter(|&idx| matches!(items[idx].content, Content::Rule))
                .min_by(|&a, &b| items[a].ink.width().total_cmp(&items[b].ink.width()));
            let Some(bar) = bar else {
                return Some(items);
            };
            let bar_ink = items[bar].ink;
            let above = self.stacked(
                &items,
                |item| {
                    item.ink.x0 >= bar_ink.x0 - self.cfg.bar_tolerance_pt
                        && item.ink.x1 <= bar_ink.x1 + self.cfg.bar_tolerance_pt
                },
                bar,
                true,
            );
            let below = self.stacked(
                &items,
                |item| {
                    item.ink.x0 >= bar_ink.x0 - self.cfg.bar_tolerance_pt
                        && item.ink.x1 <= bar_ink.x1 + self.cfg.bar_tolerance_pt
                },
                bar,
                false,
            );
            if above.is_empty() && below.is_empty() {
                return None;
            }
            let (taken, rest) = split_off(items, |idx| idx == bar || above.contains(&idx) || below.contains(&idx));
            let num: Vec<Item> = taken
                .iter()
                .filter(|(idx, _)| above.contains(idx))
                .map(|(_, item)| item.clone())
                .collect();
            let den: Vec<Item> = taken
                .iter()
                .filter(|(idx, _)| below.contains(idx))
                .map(|(_, item)| item.clone())
                .collect();
            let node = match (num.is_empty(), den.is_empty()) {
                (false, false) => MathNode::Frac {
                    num: Box::new(self.group(num)?),
                    den: Box::new(self.group(den)?),
                },
                // Something only above the bar: the bar underlines it.
                (false, true) => MathNode::UnderOver {
                    base: Box::new(self.group(num)?),
                    under: Some(Box::new(operator("_"))),
                    over: None,
                    accent: false,
                },
                // Something only below the bar: the bar is over it.
                _ => MathNode::UnderOver {
                    base: Box::new(self.group(den)?),
                    under: None,
                    over: Some(Box::new(operator("‾"))),
                    accent: true,
                },
            };
            let parts: Vec<&Item> = taken.iter().map(|(_, item)| item).collect();
            items = rest;
            items.push(Item::built(node, &parts, None));
        }
    }

    /// The items stacked right above (`above`) or below the item `anchor`:
    /// among those that `fits` (horizontally), the nearest one within
    /// [`MathConfig::stack_max_gap_em`] of the anchor and every item on the
    /// same line as it (overlapping it vertically, directly or through
    /// another such item). Parts stacked further away belong to something
    /// else (the line above in a multi-line display).
    fn stacked(&self, items: &[Item], fits: impl Fn(&Item) -> bool, anchor: usize, above: bool) -> Vec<usize> {
        let edge = items[anchor].ink;
        let tol = self.cfg.overlap_min_pt;
        let candidates: Vec<usize> = (0..items.len())
            .filter(|&idx| idx != anchor && fits(&items[idx]))
            .filter(|&idx| {
                let ink = items[idx].ink;
                if above {
                    ink.y1 <= edge.y0 + tol
                } else {
                    ink.y0 >= edge.y1 - tol
                }
            })
            .collect();
        let distance = |idx: usize| {
            let ink = items[idx].ink;
            if above { edge.y0 - ink.y1 } else { ink.y0 - edge.y1 }
        };
        let nearest = candidates
            .iter()
            .copied()
            .min_by(|&a, &b| distance(a).total_cmp(&distance(b)));
        let Some(nearest) = nearest.filter(|&idx| distance(idx) <= self.em(self.cfg.stack_max_gap_em)) else {
            return Vec::new();
        };
        let mut taken = vec![nearest];
        let mut band = items[nearest].ink;
        while let Some(next) = candidates
            .iter()
            .copied()
            .find(|idx| !taken.contains(idx) && items[*idx].ink.y_overlap(&band) > 0.0)
        {
            band = band.union(&items[next].ink);
            taken.push(next);
        }
        taken
    }

    /// Replaces every big operator (∑, lim, ...) with limits stacked above
    /// or below it by one rebuilt operator with its limits.
    fn limits(&self, mut items: Vec<Item>) -> Option<Vec<Item>> {
        let mut done: Vec<usize> = Vec::new();
        loop {
            let mut found = None;
            for op in big_operators(&items, self.em(self.cfg.word_max_gap_em), self.cfg.baseline_step_pt) {
                if done.contains(&op[0]) {
                    continue;
                }
                let under = self.limit(&items, &op, false);
                let over = self.limit(&items, &op, true);
                if under.is_empty() && over.is_empty() {
                    done.push(op[0]);
                    continue;
                }
                found = Some((op, under, over));
                break;
            }
            let Some((op, under, over)) = found else {
                return Some(items);
            };
            let base = self.operator_name(&op.iter().map(|&idx| &items[idx]).collect::<Vec<_>>());
            let (taken, rest) = split_off(items, |idx| {
                op.contains(&idx) || under.contains(&idx) || over.contains(&idx)
            });
            let pick = |set: &[usize]| -> Vec<Item> {
                taken
                    .iter()
                    .filter(|(idx, _)| set.contains(idx))
                    .map(|(_, item)| item.clone())
                    .collect()
            };
            let limit = |set: &[usize]| -> Option<Option<Box<MathNode>>> {
                if set.is_empty() {
                    Some(None)
                } else {
                    Some(Some(Box::new(self.group(pick(set))?)))
                }
            };
            let node = MathNode::UnderOver {
                base: Box::new(base),
                under: limit(&under)?,
                over: limit(&over)?,
                accent: false,
            };
            let parts: Vec<&Item> = taken.iter().map(|(_, item)| item).collect();
            let op_items: Vec<&Item> = taken
                .iter()
                .filter(|(idx, _)| op.contains(idx))
                .map(|(_, item)| item)
                .collect();
            // Indices of the remaining items changed: start over.
            done.clear();
            items = rest;
            items.push(Item::built(node, &parts, Item::baseline_of(&op_items)));
            // Operators without limits are found again but stay as they are.
        }
    }

    /// The node of the operator made of the glyph items `parts` (one big
    /// operator, or the letters of an operator name).
    fn operator_name(&self, parts: &[&Item]) -> MathNode {
        let text: String = parts
            .iter()
            .filter_map(|item| item.glyph())
            .map(|glyph| glyph.text.as_str())
            .collect();
        if parts.len() == 1 {
            operator(&text)
        } else {
            MathNode::Ident { text, upright: false }
        }
    }

    /// The limit below (or above, `over`) the operator made of the items
    /// `op`: script-sized items stacked right under it, with the items on
    /// the same line next to them.
    fn limit(&self, items: &[Item], op: &[usize], over: bool) -> Vec<usize> {
        let op_ink = op.iter().fold(Rect::EMPTY, |acc, &idx| acc.union(&items[idx].ink));
        let op_size = op.iter().map(|&idx| items[idx].size).fold(0.0, f32::max);
        let tol = self.cfg.overlap_min_pt;
        let gap = self.em(self.cfg.stack_max_gap_em);
        // A limit is small, and on a line of its own: a small item level
        // with glyphs of full size is a script of another line.
        let full_size: Vec<Rect> = items
            .iter()
            .filter_map(|item| {
                let glyph = item.glyph()?;
                let full = item.size >= op_size * self.cfg.script_max_size_ratio
                    && glyph.height_em <= self.cfg.tall_delimiter_min_height_em;
                full.then_some(item.ink)
            })
            .collect();
        let small = |item: &Item| {
            item.size < op_size * self.cfg.script_max_size_ratio
                && !full_size
                    .iter()
                    .any(|ink| ink.y_overlap(&item.ink) > self.cfg.limit_level_overlap_ratio * item.ink.height())
        };
        let beyond = |item: &Item| {
            if over {
                item.ink.y1 <= op_ink.y0 + tol
            } else {
                item.ink.y0 >= op_ink.y1 - tol
            }
        };
        let near = |item: &Item| {
            if over {
                op_ink.y0 - item.ink.y1 <= gap
            } else {
                item.ink.y0 - op_ink.y1 <= gap
            }
        };
        // The nearest piece right above or below the operator starts the
        // limit; pieces further away belong to another line.
        let distance = |idx: usize| {
            let ink = items[idx].ink;
            if over { op_ink.y0 - ink.y1 } else { ink.y0 - op_ink.y1 }
        };
        let nearest = (0..items.len())
            .filter(|idx| !op.contains(idx))
            .filter(|&idx| {
                let item = &items[idx];
                small(item) && beyond(item) && near(item) && item.ink.x_overlap(&op_ink) > 0.0
            })
            .min_by(|&a, &b| distance(a).total_cmp(&distance(b)));
        let Some(nearest) = nearest else {
            return Vec::new();
        };
        let mut taken = vec![nearest];
        // Extend along the line of the limit.
        let join = self.em(self.cfg.limit_join_gap_em);
        loop {
            let line = taken.iter().fold(Rect::EMPTY, |acc, &idx| acc.union(&items[idx].ink));
            let next = (0..items.len()).find(|idx| {
                let item = &items[*idx];
                !op.contains(idx)
                    && !taken.contains(idx)
                    && small(item)
                    && beyond(item)
                    && item.ink.y_overlap(&line) > 0.0
                    && item.ink.x0 <= line.x1 + join
                    && item.ink.x1 >= line.x0 - join
            });
            match next {
                Some(idx) => taken.push(idx),
                None => return taken,
            }
        }
    }

    /// Replaces every tall opening delimiter with lines stacked after it
    /// (`cases`), or with lines stacked between it and a tall closing
    /// delimiter of the same height (a matrix, a binomial), by the
    /// delimiters around a table of those lines.
    fn stacks(&self, mut items: Vec<Item>) -> Option<Vec<Item>> {
        let mut seen: Vec<Rect> = Vec::new();
        loop {
            let open =
                (0..items.len()).find(|&idx| self.is_tall(&items[idx], OPENERS) && !seen.contains(&items[idx].ink));
            let Some(open) = open else {
                return Some(items);
            };
            seen.push(items[open].ink);
            let ink = items[open].ink;
            let tol = self.cfg.bar_tolerance_pt;
            let close = (0..items.len())
                .filter(|&idx| self.is_tall(&items[idx], CLOSERS) && items[idx].left > items[open].left)
                .filter(|&idx| (items[idx].ink.y0 - ink.y0).abs() <= tol && (items[idx].ink.y1 - ink.y1).abs() <= tol)
                .min_by(|&a, &b| items[a].left.total_cmp(&items[b].left));
            let end = close.map_or(f32::INFINITY, |idx| items[idx].left + tol);
            let inside: Vec<usize> = (0..items.len())
                .filter(|&idx| idx != open && Some(idx) != close)
                .filter(|&idx| {
                    let item = &items[idx];
                    item.left >= items[open].right - tol
                        && item.right <= end
                        && item.ink.y0 >= ink.y0 - tol
                        && item.ink.y1 <= ink.y1 + tol
                })
                .collect();
            let lines = self.rows(inside.iter().map(|&idx| items[idx].clone()).collect());
            if lines.len() < 2 {
                continue;
            }
            let table = self.stack_table(lines, close.is_none())?;
            let delimiter = |idx: usize| {
                let glyph = items[idx].glyph()?;
                Some(MathNode::Operator {
                    text: glyph.text.clone(),
                    height_em: Some(glyph.height_em * glyph.size / self.size),
                })
            };
            let mut nodes = vec![delimiter(open)?, table];
            if let Some(close) = close {
                nodes.push(delimiter(close)?);
            }
            let (taken, rest) = split_off(items, |idx| idx == open || Some(idx) == close || inside.contains(&idx));
            let parts: Vec<&Item> = taken.iter().map(|(_, item)| item).collect();
            let built = Item::built(MathNode::Row(nodes), &parts, None);
            items = rest;
            items.push(built);
        }
    }

    /// Whether `item` is a delimiter of `set` drawn taller than the text.
    fn is_tall(&self, item: &Item, set: &[&str]) -> bool {
        item.glyph().is_some_and(|glyph| {
            set.contains(&glyph.text.as_str()) && glyph.height_em > self.cfg.tall_delimiter_min_height_em
        })
    }

    /// The table of the stacked `lines` of a matrix or of `cases`
    /// (`cases`: its cells flush left): columns are cut where every line
    /// has a gap wider than [`MathConfig::column_min_gap_em`].
    fn stack_table(&self, lines: Vec<Vec<Item>>, cases: bool) -> Option<MathNode> {
        let mut spans: Vec<(f32, f32)> = lines.iter().flatten().map(|item| (item.left, item.right)).collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let gap = self.em(self.cfg.column_min_gap_em);
        let mut columns: Vec<(f32, f32)> = Vec::new();
        for (left, right) in spans {
            match columns.last_mut() {
                Some(column) if left < column.1 + gap => column.1 = column.1.max(right),
                _ => columns.push((left, right)),
            }
        }
        let mut rows = Vec::new();
        for line in lines {
            let mut cells = vec![Vec::new(); columns.len()];
            for item in line {
                let col = columns.iter().position(|&(_, right)| item.left <= right).unwrap_or(0);
                cells[col].push(item);
            }
            rows.push(
                cells
                    .into_iter()
                    .map(|cell| self.group(cell))
                    .collect::<Option<Vec<_>>>()?,
            );
        }
        let align = if cases { "left" } else { "center" };
        Some(MathNode::Table {
            rows,
            align: vec![align; columns.len()],
        })
    }

    /// Splits `items` into the lines of a multi-line display: groups whose
    /// vertical extents do not overlap, top to bottom.
    fn rows(&self, mut items: Vec<Item>) -> Vec<Vec<Item>> {
        items.sort_by(|a, b| a.ink.y0.total_cmp(&b.ink.y0));
        let mut rows: Vec<Vec<Item>> = Vec::new();
        let mut bottom = f32::NEG_INFINITY;
        for item in items {
            if rows.is_empty() || item.ink.y0 > bottom - self.cfg.overlap_min_pt {
                rows.push(Vec::new());
            }
            bottom = bottom.max(item.ink.y1);
            if let Some(row) = rows.last_mut() {
                row.push(item);
            }
        }
        rows
    }

    /// A multi-line display: one table row per line, split into two
    /// columns at the relation the lines are aligned on (when at least two
    /// lines share one), otherwise in one column.
    fn table(&self, rows: Vec<Vec<Item>>) -> Option<MathNode> {
        let sizes: Vec<f32> = rows
            .iter()
            .map(|row| row.iter().map(|item| item.size).fold(0.0, f32::max))
            .collect();
        let largest = sizes.iter().copied().fold(0.0, f32::max);
        if sizes.iter().any(|&size| size < largest * self.cfg.row_min_size_ratio) {
            return None;
        }
        let relations: Vec<Vec<f32>> = rows.iter().map(|row| self.relation_edges(row)).collect();
        let tol = self.cfg.align_tolerance_pt;
        let rows_near = |x: f32| {
            relations
                .iter()
                .filter(|edges| edges.iter().any(|&edge| (edge - x).abs() <= tol))
                .count()
        };
        let align_x = relations
            .iter()
            .flatten()
            .copied()
            .map(|x| (rows_near(x), x))
            .max_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)))
            .filter(|&(count, _)| count >= 2)
            .map(|(_, x)| x);
        let mut cells = Vec::new();
        match align_x {
            Some(x) => {
                for (row, edges) in rows.into_iter().zip(&relations) {
                    let split = edges
                        .iter()
                        .copied()
                        .find(|edge| (edge - x).abs() <= tol)
                        .unwrap_or_else(|| {
                            let start = row.iter().map(|item| item.left).fold(f32::INFINITY, f32::min);
                            if start >= x - tol {
                                f32::NEG_INFINITY
                            } else {
                                f32::INFINITY
                            }
                        });
                    let (left, right): (Vec<Item>, Vec<Item>) =
                        row.into_iter().partition(|item| item.left < split - tol);
                    cells.push(vec![self.row(left)?, self.row(right)?]);
                }
                Some(MathNode::Table {
                    rows: cells,
                    align: vec!["right", "left"],
                })
            }
            None => {
                for row in rows {
                    cells.push(vec![self.row(row)?]);
                }
                Some(MathNode::Table {
                    rows: cells,
                    align: vec!["center"],
                })
            }
        }
    }

    /// Left edges of the relations of a row that are set at the size of
    /// the row.
    fn relation_edges(&self, row: &[Item]) -> Vec<f32> {
        let size = row.iter().map(|item| item.size).fold(0.0, f32::max);
        row.iter()
            .filter(|item| item.size >= size * self.cfg.script_max_size_ratio)
            .filter(|item| {
                item.glyph()
                    .is_some_and(|glyph| RELATIONS.contains(&glyph.text.as_str()))
            })
            .map(|item| item.left)
            .collect()
    }

    /// One line of a formula, left to right: tokens on the main baseline
    /// and rebuilt parts, each followed by its scripts. Returns `None` when
    /// a rule is left over or two items are stacked without a reason.
    fn row(&self, mut items: Vec<Item>) -> Option<MathNode> {
        if items.is_empty() {
            return Some(MathNode::Row(Vec::new()));
        }
        if items.iter().any(|item| matches!(item.content, Content::Rule)) {
            return None;
        }
        items.sort_by(|a, b| a.left.total_cmp(&b.left));
        let size = items.iter().map(|item| item.size).fold(0.0, f32::max);
        let places = self.places(&items, size);
        self.check_stacks(&items, &places)?;
        let mut out: Vec<MathNode> = Vec::new();
        let mut pending: Vec<&Glyph> = Vec::new();
        // Right edge of what is written so far, and what it ended with.
        let mut last: Option<(f32, Edge)> = None;
        let mut idx = 0;
        while idx < items.len() {
            let item = &items[idx];
            if places[idx] == Place::Base {
                let glyph = item.glyph();
                let edge = glyph.map_or(Edge::Plain, |glyph| Edge::of(&glyph.text));
                let spaced = glyph.is_some_and(|glyph| glyph.space_before);
                if let Some((right, last_edge)) = last {
                    let gap = item.left - right;
                    let wide = gap > self.cfg.space_min_em * size;
                    // Spaces around operators are MathML's to make, and so
                    // is the thin space after punctuation.
                    let wanted = match (last_edge, edge) {
                        (Edge::Operator, _) | (_, Edge::Operator) => false,
                        (Edge::Punctuation, _) | (_, Edge::Punctuation) => wide,
                        _ => wide || (spaced && gap > 0.0),
                    };
                    if wanted {
                        flush(&mut pending, &mut out, self);
                        out.push(MathNode::Space(gap / size));
                    }
                }
                match &item.content {
                    Content::Glyph(glyph) => pending.push(glyph),
                    Content::Built(node) => {
                        flush(&mut pending, &mut out, self);
                        out.push(node.clone());
                    }
                    Content::Rule => return None,
                }
                let right = last.map_or(item.right, |(right, _)| right.max(item.right));
                last = Some((right, edge));
                idx += 1;
                continue;
            }
            let start = idx;
            while idx < items.len() && places[idx] != Place::Base {
                idx += 1;
            }
            let pick = |place: Place| -> Vec<Item> {
                (start..idx)
                    .filter(|&k| places[k] == place)
                    .map(|k| items[k].clone())
                    .collect()
            };
            let (sup, sub) = (pick(Place::Raised), pick(Place::Lowered));
            flush(&mut pending, &mut out, self);
            let base = script_base(&mut out);
            let script = |part: Vec<Item>| -> Option<Option<Box<MathNode>>> {
                if part.is_empty() {
                    Some(None)
                } else {
                    Some(Some(Box::new(self.group(part)?)))
                }
            };
            out.push(MathNode::Scripts {
                base: Box::new(base),
                sub: script(sub)?,
                sup: script(sup)?,
            });
            let right = items[start..idx]
                .iter()
                .map(|item| item.right)
                .fold(f32::NEG_INFINITY, f32::max);
            last = Some((last.map_or(right, |(prev, _)| prev.max(right)), Edge::Plain));
        }
        flush(&mut pending, &mut out, self);
        Some(MathNode::row(out))
    }

    /// The baseline most glyphs of the row's font size sit on (delimiters,
    /// big operators and radical signs left out). A row of rebuilt parts
    /// only takes the baseline below the math axis through their middle.
    fn main_baseline(&self, items: &[Item], size: f32) -> f32 {
        let mut votes: Vec<(f32, usize)> = Vec::new();
        for item in items {
            let Some(baseline) = item.baseline else { continue };
            let tall = item
                .glyph()
                .is_some_and(|glyph| glyph.height_em > self.cfg.tall_delimiter_min_height_em);
            if item.size < size * self.cfg.script_max_size_ratio || tall {
                continue;
            }
            let key = (baseline / self.cfg.baseline_step_pt).round() * self.cfg.baseline_step_pt;
            match votes.iter_mut().find(|(baseline, _)| *baseline == key) {
                Some(vote) => vote.1 += 1,
                None => votes.push((key, 1)),
            }
        }
        if let Some((baseline, _)) = votes.iter().max_by_key(|(_, count)| *count) {
            return *baseline;
        }
        let built = items
            .iter()
            .find(|item| matches!(item.content, Content::Built(_)))
            .unwrap_or(&items[0]);
        built.ink.cy() + AXIS_HEIGHT_EM * size
    }

    /// Where `item` sits in a row of font size `size` and main baseline
    /// `baseline`: smaller items above or below the baseline are scripts.
    fn place(&self, item: &Item, size: f32, baseline: f32) -> Place {
        if item.size >= size * self.cfg.script_max_size_ratio {
            return Place::Base;
        }
        match item.baseline {
            Some(own) => {
                let tol = self.cfg.baseline_tolerance_em * size;
                if own < baseline - tol {
                    Place::Raised
                } else if own > baseline + tol {
                    Place::Lowered
                } else {
                    Place::Base
                }
            }
            None => {
                if item.ink.cy() < baseline - AXIS_HEIGHT_EM * size {
                    Place::Raised
                } else {
                    Place::Lowered
                }
            }
        }
    }

    /// Where each item of a row (sorted left to right, font size `size`)
    /// sits: on the main baseline, or raised or lowered as a script.
    fn places(&self, items: &[Item], size: f32) -> Vec<Place> {
        let baseline = self.main_baseline(items, size);
        let mut places: Vec<Place> = items.iter().map(|item| self.place(item, size, baseline)).collect();
        self.join_scripts(items, &mut places, size);
        places
    }

    /// A small item that happens to sit on the main baseline right after a
    /// script belongs to the scripts: the prime of `s′` in the subscript of
    /// `𝔼_{s′∼ε}` is raised from the subscript just up to the baseline. It
    /// joins the script of the run (superscripts or subscripts) whose
    /// baseline is nearest its own.
    fn join_scripts(&self, items: &[Item], places: &mut [Place], size: f32) {
        for idx in 1..items.len() {
            let small = items[idx].size < size * self.cfg.script_max_size_ratio;
            let gap = items[idx].left - items[idx - 1].right;
            if places[idx] != Place::Base
                || places[idx - 1] == Place::Base
                || !small
                || gap >= self.cfg.word_max_gap_em * size
            {
                continue;
            }
            let own = items[idx].baseline.unwrap_or(items[idx].ink.y1);
            let run = (0..idx).rev().take_while(|&k| places[k] != Place::Base);
            let nearest = run.min_by(|&a, &b| {
                let off = |k: usize| (items[k].baseline.unwrap_or(items[k].ink.y1) - own).abs();
                off(a).total_cmp(&off(b))
            });
            places[idx] = nearest.map_or(places[idx - 1], |k| places[k]);
        }
    }

    /// `None` when two items on the baseline are stacked one above the
    /// other without a bar or a big operator to explain it (a matrix, a
    /// binomial, a misread line).
    fn check_stacks(&self, items: &[Item], places: &[Place]) -> Option<()> {
        let mut reach: Option<(f32, Rect)> = None;
        for (item, place) in items.iter().zip(places) {
            if *place != Place::Base {
                continue;
            }
            if let Some((right, ink)) = reach
                && item.left < right - self.cfg.overlap_min_pt
                && item.ink.y_overlap(&ink) <= 0.0
            {
                return None;
            }
            reach = match reach {
                Some((right, ink)) if right >= item.right => Some((right, ink)),
                _ => Some((item.right, item.ink)),
            };
        }
        Some(())
    }

    /// The tokens of consecutive glyphs on the baseline: numbers, words
    /// (function names, text), single-letter identifiers and operators.
    fn tokens(&self, glyphs: &[&Glyph], out: &mut Vec<MathNode>) {
        let mut idx = 0;
        while idx < glyphs.len() {
            let glyph = glyphs[idx];
            if let Some(node) = accented(glyph) {
                out.push(node);
                idx += 1;
                continue;
            }
            let kind = token_kind(&glyph.text);
            let end = match kind {
                TokenKind::Digit => run_end(glyphs, idx, |prev, next| {
                    token_kind(&next.text) == TokenKind::Digit || (next.text == "." && prev.text != ".")
                }),
                TokenKind::Letter if !glyph.italic => run_end(glyphs, idx, |prev, next| {
                    token_kind(&next.text) == TokenKind::Letter
                        && !next.italic
                        && next.text_font == prev.text_font
                        && next.alphabet == prev.alphabet
                        && accented(next).is_none()
                }),
                _ => idx + 1,
            };
            // A number does not end with its decimal point.
            let end = if kind == TokenKind::Digit && end - idx > 1 && glyphs[end - 1].text == "." {
                end - 1
            } else {
                end
            };
            let text: String = glyphs[idx..end]
                .iter()
                .map(|glyph| styled(&glyph.text, glyph.alphabet))
                .collect();
            match kind {
                TokenKind::Digit => out.push(MathNode::Number(text)),
                TokenKind::Letter if end - idx > 1 && glyph.text_font => out.push(MathNode::Text(text)),
                TokenKind::Letter => out.push(MathNode::Ident {
                    upright: !glyph.italic && end - idx == 1,
                    text,
                }),
                TokenKind::Op => {
                    let (text, end) = ellipsis(glyphs, idx)
                        .or_else(|| compound(glyphs, idx))
                        .unwrap_or((glyph.text.clone(), end));
                    let height_em = (DELIMITERS.contains(&text.as_str())
                        && glyph.height_em > self.cfg.tall_delimiter_min_height_em)
                        .then_some(glyph.height_em * glyph.size / self.size);
                    out.push(MathNode::Operator { text, height_em });
                    idx = end;
                    continue;
                }
            }
            idx = end;
        }
    }
}

/// Takes the base of the scripts that follow `out`: its last node, or,
/// when that is a tall closing delimiter (`\right)^2`), everything from the
/// matching tall opening delimiter on, so that the scripts go to the whole
/// group (and the delimiters stretch to it only).
fn script_base(out: &mut Vec<MathNode>) -> MathNode {
    let tall = |node: &MathNode, set: &[&str]| matches!(node, MathNode::Operator { text, height_em: Some(_) } if set.contains(&text.as_str()));
    if out.last().is_some_and(|node| tall(node, CLOSERS)) {
        let mut depth = 0;
        for start in (0..out.len()).rev() {
            if tall(&out[start], CLOSERS) {
                depth += 1;
            } else if tall(&out[start], OPENERS) {
                depth -= 1;
                if depth == 0 {
                    return MathNode::Row(out.split_off(start));
                }
            }
        }
    }
    out.pop().unwrap_or(MathNode::Row(Vec::new()))
}

/// Writes the pending glyphs as tokens and clears them.
fn flush(pending: &mut Vec<&Glyph>, out: &mut Vec<MathNode>, parser: &Parser<'_>) {
    if !pending.is_empty() {
        parser.tokens(pending, out);
        pending.clear();
    }
}

/// The end (exclusive) of the run of glyphs starting at `start` in which
/// every glyph `continues` the one before it.
fn run_end(glyphs: &[&Glyph], start: usize, continues: impl Fn(&Glyph, &Glyph) -> bool) -> usize {
    let mut end = start + 1;
    while end < glyphs.len() && continues(glyphs[end - 1], glyphs[end]) {
        end += 1;
    }
    end
}

/// Three dots in a row as one ellipsis, with the index after them.
fn ellipsis(glyphs: &[&Glyph], start: usize) -> Option<(String, usize)> {
    let dots = glyphs.get(start..start + 3)?;
    let text = &dots[0].text;
    let symbol = match text.as_str() {
        "." => "…",
        "·" | "⋅" => "⋯",
        _ => return None,
    };
    dots.iter()
        .all(|glyph| &glyph.text == text)
        .then(|| (symbol.to_string(), start + 3))
}

/// Two operator glyphs that make one operator (`:=`, `=:`), with the
/// index after them.
fn compound(glyphs: &[&Glyph], start: usize) -> Option<(String, usize)> {
    let pair = glyphs.get(start..start + 2)?;
    let text = format!("{}{}", pair[0].text, pair[1].text);
    (matches!(text.as_str(), ":=" | "=:") && !pair[1].space_before).then_some((text, start + 2))
}

/// The accent node of a glyph whose text carries a combining accent
/// (`m̂`), or `None`.
fn accented(glyph: &Glyph) -> Option<MathNode> {
    let mut base = String::new();
    let mut accent = None;
    for ch in glyph.text.chars() {
        match ACCENTS.iter().find(|(mark, _)| *mark == ch) {
            Some((_, symbol)) => accent = Some(*symbol),
            None => base.push(ch),
        }
    }
    let accent = accent?;
    let base_node = match token_kind(&base) {
        TokenKind::Digit => MathNode::Number(base),
        TokenKind::Letter => MathNode::Ident {
            upright: !glyph.italic && base.chars().count() == 1,
            text: styled(&base, glyph.alphabet),
        },
        TokenKind::Op => operator(&base),
    };
    Some(MathNode::UnderOver {
        base: Box::new(base_node),
        under: None,
        over: Some(Box::new(operator(accent))),
        accent: true,
    })
}

/// An operator of normal size.
fn operator(text: &str) -> MathNode {
    MathNode::Operator {
        text: text.to_string(),
        height_em: None,
    }
}

/// The kind of token the glyph text `text` makes.
fn token_kind(text: &str) -> TokenKind {
    match text.chars().next() {
        Some(ch) if ch.is_ascii_digit() => TokenKind::Digit,
        Some(ch) if ch.is_alphabetic() || IDENT_SYMBOLS.contains(&ch) => TokenKind::Letter,
        _ => TokenKind::Op,
    }
}

/// The big operators of `items`, as lists of item indices: a single big
/// operator glyph, or the letters of a word on one baseline (`lim`, `max`,
/// `maximize`: an operator name, which takes limits below it), left to right.
/// Letters closer than `word_gap` (points) whose baselines round to the same
/// multiple of `step` (points) form one word.
fn big_operators(items: &[Item], word_gap: f32, step: f32) -> Vec<Vec<usize>> {
    let mut ops: Vec<Vec<usize>> = (0..items.len())
        .filter(|&idx| {
            items[idx]
                .glyph()
                .is_some_and(|glyph| BIG_OPERATORS.contains(&glyph.text.as_str()))
        })
        .map(|idx| vec![idx])
        .collect();
    let baseline = |idx: usize| items[idx].glyph().map_or(0.0, |glyph| (glyph.baseline / step).round());
    let mut letters: Vec<usize> = (0..items.len())
        .filter(|&idx| {
            items[idx]
                .glyph()
                .is_some_and(|glyph| !glyph.italic && token_kind(&glyph.text) == TokenKind::Letter)
        })
        .collect();
    letters.sort_by(|&a, &b| {
        baseline(a)
            .total_cmp(&baseline(b))
            .then(items[a].left.total_cmp(&items[b].left))
    });
    let mut run: Vec<usize> = Vec::new();
    for idx in letters {
        let continues = run.last().is_some_and(|&prev| {
            baseline(prev) == baseline(idx) && (items[idx].left - items[prev].right).abs() < word_gap
        });
        if !continues {
            if run.len() > 1 {
                ops.push(std::mem::take(&mut run));
            }
            run.clear();
        }
        run.push(idx);
    }
    if run.len() > 1 {
        ops.push(run);
    }
    ops
}

/// Splits `items` into those whose index `take`s (with their index) and
/// the others, both in their order.
fn split_off(items: Vec<Item>, take: impl Fn(usize) -> bool) -> (Vec<(usize, Item)>, Vec<Item>) {
    let mut taken = Vec::new();
    let mut rest = Vec::new();
    for (idx, item) in items.into_iter().enumerate() {
        if take(idx) {
            taken.push((idx, item));
        } else {
            rest.push(item);
        }
    }
    (taken, rest)
}

/// `text` written in the Unicode mathematical alphanumerics of `alphabet`
/// (letters and digits that have no such form are kept).
fn styled(text: &str, alphabet: Alphabet) -> String {
    text.chars().map(|ch| styled_char(ch, alphabet)).collect()
}

/// One character of [`styled`].
fn styled_char(ch: char, alphabet: Alphabet) -> char {
    // Letters that have a form outside the block of mathematical
    // alphanumerics.
    let exception = match (alphabet, ch) {
        (Alphabet::Script, 'B') => Some('ℬ'),
        (Alphabet::Script, 'E') => Some('ℰ'),
        (Alphabet::Script, 'F') => Some('ℱ'),
        (Alphabet::Script, 'H') => Some('ℋ'),
        (Alphabet::Script, 'I') => Some('ℐ'),
        (Alphabet::Script, 'L') => Some('ℒ'),
        (Alphabet::Script, 'M') => Some('ℳ'),
        (Alphabet::Script, 'R') => Some('ℛ'),
        (Alphabet::DoubleStruck, 'C') => Some('ℂ'),
        (Alphabet::DoubleStruck, 'H') => Some('ℍ'),
        (Alphabet::DoubleStruck, 'N') => Some('ℕ'),
        (Alphabet::DoubleStruck, 'P') => Some('ℙ'),
        (Alphabet::DoubleStruck, 'Q') => Some('ℚ'),
        (Alphabet::DoubleStruck, 'R') => Some('ℝ'),
        (Alphabet::DoubleStruck, 'Z') => Some('ℤ'),
        (Alphabet::Fraktur, 'C') => Some('ℭ'),
        (Alphabet::Fraktur, 'H') => Some('ℌ'),
        (Alphabet::Fraktur, 'I') => Some('ℑ'),
        (Alphabet::Fraktur, 'R') => Some('ℜ'),
        (Alphabet::Fraktur, 'Z') => Some('ℨ'),
        _ => None,
    };
    if let Some(mapped) = exception {
        return mapped;
    }
    // First code point of the capitals, of the small letters and of the
    // digits of each alphabet.
    let (upper, lower, digits) = match alphabet {
        Alphabet::Plain => return ch,
        Alphabet::Bold => (0x1D400, Some(0x1D41A), Some(0x1D7CE)),
        Alphabet::BoldItalic => (0x1D468, Some(0x1D482), Some(0x1D7CE)),
        // Symbol fonts have calligraphic capitals only.
        Alphabet::Script => (0x1D49C, None, None),
        Alphabet::DoubleStruck => (0x1D538, Some(0x1D552), Some(0x1D7D8)),
        Alphabet::Fraktur => (0x1D504, Some(0x1D51E), None),
    };
    let base = match ch {
        'A'..='Z' => Some(upper + (ch as u32 - 'A' as u32)),
        'a'..='z' => lower.map(|first| first + (ch as u32 - 'a' as u32)),
        '0'..='9' => digits.map(|first| first + (ch as u32 - '0' as u32)),
        _ => None,
    };
    base.and_then(char::from_u32).unwrap_or(ch)
}

#[cfg(test)]
#[allow(clippy::min_ident_chars, clippy::missing_docs_in_private_items)]
mod tests {
    use super::*;
    use crate::model::LineGlyph;

    /// Font 0: math italic; font 1: roman; font 2: symbols; font 3:
    /// extension (big operators).
    fn fonts() -> Vec<FontInfo> {
        let font = |name: &str, italic: bool, math: bool| FontInfo {
            name: name.into(),
            italic,
            math,
            ..Default::default()
        };
        vec![
            font("CMMI10", true, true),
            font("CMR10", false, false),
            font("CMSY10", false, true),
            font("CMEX10", false, true),
        ]
    }

    /// A glyph of `size` at `x` on `baseline`, with the ink of a letter
    /// (or `ink` when given).
    fn glyph(text: &str, font: u16, x: f32, baseline: f32, size: f32, ink: Option<Rect>) -> LineGlyph {
        let x1 = x + 0.5 * size;
        LineGlyph {
            text: text.into(),
            x0: x,
            x1,
            y: baseline,
            size,
            font,
            script: 0,
            space_before: false,
            ink: ink.unwrap_or(Rect::new(x, baseline - 0.7 * size, x1, baseline)),
            ox: x,
            oy: baseline,
            extra: None,
            color: [0; 3],
        }
    }

    fn line(glyphs: Vec<LineGlyph>) -> Line {
        Line {
            page: 0,
            bbox: Rect::default(),
            ink: Rect::default(),
            baseline: glyphs[0].y,
            size: glyphs[0].size,
            font: glyphs[0].font,
            glyphs,
            fixed_pitch: false,
        }
    }

    fn rebuild(glyphs: Vec<LineGlyph>, rules: &[Rect]) -> Option<MathNode> {
        formula(&[line(glyphs)], rules, &fonts(), &MathConfig::default())
    }

    fn ident(text: &str) -> MathNode {
        MathNode::Ident {
            text: text.into(),
            upright: false,
        }
    }

    #[test]
    fn a_bar_with_lines_above_and_below_is_a_fraction() {
        let math = rebuild(
            vec![
                glyph("x", 0, 10.0, 100.0, 10.0, None),
                glyph("=", 1, 17.0, 100.0, 10.0, Some(Rect::new(17.0, 96.0, 23.0, 98.0))),
                glyph("a", 0, 30.0, 93.0, 10.0, None),
                glyph("b", 0, 30.0, 106.5, 10.0, None),
            ],
            &[Rect::new(29.0, 97.3, 36.0, 97.7)],
        );
        let expected = MathNode::Row(vec![
            ident("x"),
            operator("="),
            MathNode::Frac {
                num: Box::new(ident("a")),
                den: Box::new(ident("b")),
            },
        ]);
        assert_eq!(math, Some(expected));
    }

    #[test]
    fn small_raised_and_lowered_glyphs_are_scripts_of_one_base() {
        let math = rebuild(
            vec![
                glyph("x", 0, 10.0, 100.0, 10.0, None),
                glyph("i", 0, 15.0, 101.5, 7.0, None),
                glyph("2", 1, 15.0, 96.0, 7.0, None),
            ],
            &[],
        );
        let expected = MathNode::Scripts {
            base: Box::new(ident("x")),
            sub: Some(Box::new(ident("i"))),
            sup: Some(Box::new(MathNode::Number("2".into()))),
        };
        assert_eq!(math, Some(expected));
    }

    #[test]
    fn a_radical_holds_what_is_under_its_bar() {
        let sign = glyph("√", 2, 10.0, 100.0, 10.0, Some(Rect::new(10.0, 91.0, 18.0, 102.0)));
        let math = rebuild(
            vec![sign, glyph("x", 0, 18.5, 100.0, 10.0, None)],
            &[Rect::new(17.8, 91.0, 24.0, 91.4)],
        );
        assert_eq!(math, Some(MathNode::Sqrt(Box::new(ident("x")))));
    }

    #[test]
    fn a_big_operator_takes_the_small_lines_above_and_below_it() {
        let sum = glyph("∑", 3, 10.0, 100.0, 10.0, Some(Rect::new(10.0, 90.0, 20.0, 104.0)));
        let math = rebuild(
            vec![
                sum,
                glyph("i", 0, 13.0, 112.0, 7.0, None),
                glyph("n", 0, 13.0, 88.0, 7.0, None),
                glyph("x", 0, 22.0, 100.0, 10.0, None),
            ],
            &[],
        );
        let expected = MathNode::Row(vec![
            MathNode::UnderOver {
                base: Box::new(operator("∑")),
                under: Some(Box::new(ident("i"))),
                over: Some(Box::new(ident("n"))),
                accent: false,
            },
            ident("x"),
        ]);
        assert_eq!(math, Some(expected));
    }

    #[test]
    fn a_small_glyph_on_the_baseline_after_a_subscript_belongs_to_it() {
        let math = rebuild(
            vec![
                glyph("E", 1, 10.0, 100.0, 10.0, None),
                glyph("s", 0, 17.0, 101.5, 7.0, Some(Rect::new(17.0, 98.0, 20.0, 101.5))),
                glyph("′", 2, 20.5, 100.0, 5.0, Some(Rect::new(20.5, 96.5, 21.5, 99.0))),
            ],
            &[],
        );
        let expected = MathNode::Scripts {
            base: Box::new(MathNode::Ident {
                text: "E".into(),
                upright: true,
            }),
            sub: Some(Box::new(MathNode::Scripts {
                base: Box::new(ident("s")),
                sub: None,
                sup: Some(Box::new(operator("′"))),
            })),
            sup: None,
        };
        assert_eq!(math, Some(expected));
    }

    #[test]
    fn lines_after_a_tall_brace_are_a_table() {
        let brace = glyph("{", 3, 20.0, 100.0, 10.0, Some(Rect::new(20.0, 88.0, 25.0, 108.0)));
        let math = rebuild(
            vec![
                glyph("f", 0, 10.0, 100.0, 10.0, None),
                brace,
                glyph("a", 0, 27.0, 95.0, 10.0, None),
                glyph("p", 0, 45.0, 95.0, 10.0, None),
                glyph("b", 0, 27.0, 106.0, 10.0, None),
                glyph("q", 0, 45.0, 106.0, 10.0, None),
            ],
            &[],
        );
        let Some(MathNode::Row(nodes)) = math else {
            panic!("not a row: {math:?}");
        };
        assert_eq!(nodes[0], ident("f"));
        let MathNode::Row(stack) = &nodes[1] else {
            panic!("no stack: {:?}", nodes[1]);
        };
        assert!(matches!(&stack[0], MathNode::Operator { text, height_em: Some(_) } if text == "{"));
        let expected = MathNode::Table {
            rows: vec![vec![ident("a"), ident("p")], vec![ident("b"), ident("q")]],
            align: vec!["left", "left"],
        };
        assert_eq!(stack[1], expected);
    }

    #[test]
    fn upright_letters_form_a_word() {
        let math = rebuild(
            vec![
                glyph("l", 1, 10.0, 100.0, 10.0, None),
                glyph("o", 1, 15.0, 100.0, 10.0, None),
                glyph("g", 1, 20.0, 100.0, 10.0, None),
                glyph("1", 1, 27.0, 100.0, 10.0, None),
                glyph(".", 1, 32.0, 100.0, 10.0, None),
                glyph("5", 1, 37.0, 100.0, 10.0, None),
            ],
            &[],
        );
        let expected = MathNode::Row(vec![ident("log"), MathNode::Number("1.5".into())]);
        assert_eq!(math, Some(expected));
    }

    #[test]
    fn glyphs_stacked_without_a_bar_stay_a_picture() {
        let math = rebuild(
            vec![
                glyph("a", 0, 10.0, 94.0, 10.0, None),
                glyph("b", 0, 10.0, 106.0, 10.0, None),
                glyph("c", 0, 20.0, 100.0, 10.0, None),
            ],
            &[],
        );
        assert_eq!(math, None);
    }

    #[test]
    fn a_bar_with_nothing_above_is_an_overline() {
        let math = rebuild(
            vec![glyph("A", 0, 10.0, 100.0, 10.0, None)],
            &[Rect::new(10.0, 91.5, 15.5, 91.9)],
        );
        let expected = MathNode::UnderOver {
            base: Box::new(ident("A")),
            under: None,
            over: Some(Box::new(operator("‾"))),
            accent: true,
        };
        assert_eq!(math, Some(expected));
    }

    #[test]
    fn a_gap_after_a_parenthesis_is_kept() {
        let mut close = glyph(")", 1, 10.0, 100.0, 10.0, None);
        close.x1 = 14.0;
        let mut word = glyph("w", 1, 24.0, 100.0, 10.0, None);
        word.space_before = true;
        let math = rebuild(vec![close, word], &[]);
        let expected = MathNode::Row(vec![
            operator(")"),
            MathNode::Space(1.0),
            MathNode::Ident {
                text: "w".into(),
                upright: true,
            },
        ]);
        assert_eq!(math, Some(expected));
    }

    #[test]
    fn a_rule_that_explains_nothing_keeps_the_picture() {
        let math = rebuild(
            vec![glyph("x", 0, 10.0, 100.0, 10.0, None)],
            &[Rect::new(40.0, 80.0, 60.0, 80.4)],
        );
        assert_eq!(math, None);
    }

    #[test]
    fn a_glyph_without_unicode_keeps_the_picture() {
        let math = rebuild(vec![glyph("\u{FFFD}", 0, 10.0, 100.0, 10.0, None)], &[]);
        assert_eq!(math, None);
    }

    #[test]
    fn alphabets_map_to_mathematical_letters() {
        assert_eq!(styled("R", Alphabet::DoubleStruck), "ℝ");
        assert_eq!(styled("E", Alphabet::DoubleStruck), "𝔼");
        assert_eq!(styled("x1", Alphabet::Bold), "𝐱𝟏");
        assert_eq!(styled("L", Alphabet::Script), "ℒ");
        assert_eq!(styled("A", Alphabet::Script), "𝒜");
        assert_eq!(styled("g", Alphabet::Fraktur), "𝔤");
        assert_eq!(styled("α", Alphabet::Bold), "α");
    }
}
