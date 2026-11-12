//! Assembles the structured [`Document`] from the per-page layout analysis:
//! front matter, headings, lists, references, paragraph merging across
//! columns and pages, de-hyphenation and cross-reference links.

use crate::analysis::Line;
use crate::analysis::blocks::{EquationBlock, LIST_LABEL, Loc, RawBlock, TextBlock, build_blocks};
use crate::analysis::floats::{Float, FloatKind, detect_floats, join_caption_labels, table_caption_votes};
use crate::analysis::lines::split_at_gaps;
use crate::analysis::order::order_page;
use crate::analysis::page::{DocStats, PageData, compute_stats, remove_furniture};
use crate::analysis::tables::{Grid, extract_grid};
use crate::backend::{FontInfo, RawPage};
use crate::common::geom::{Rect, median};
use crate::model::*;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

static HEADING_NUM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<num>\d{1,2}(?:\.\d{1,2}){0,3})\.?\s+(?P<rest>\S.*)$").unwrap());
static HEADING_APP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<word>Appendix\s+|APPENDIX\s+)?(?P<num>[A-Z](?:\.\d{1,2}){0,3})[.:]?\s+(?P<rest>\S.*)$").unwrap()
});
static HEADING_ROMAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<num>[IVX]{1,5})\.\s+(?P<rest>[A-Z].*)$").unwrap());
static HEADING_LETTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<num>[A-Z])\.\s+(?P<rest>[A-Z].*)$").unwrap());
static ABSTRACT_INLINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?i:abstract)\s*[—–:.\-]+\s*").unwrap());
static REF_HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:\d+\.?\s+|[ivx]+\.\s+)?(references|bibliography|references and notes|literature cited|works cited)$",
    )
    .unwrap()
});
static REF_LABEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\[(?P<a>[^\]]{1,24})\]|(?P<b>\d{1,3})\.(?:\s|$))").unwrap());
static KNOWN_HEADINGS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(abstract|references|bibliography|acknowledge?ments?|appendix|appendices|supplementary materials?|conclusions?|introduction|related work|contents|keywords|index terms)$").unwrap()
});

#[derive(Debug, Clone, PartialEq)]
enum Class {
    Unknown,
    Title,
    Authors,
    Note,
    Heading { level: u8, number: Option<String> },
    Paragraph { in_abstract: bool },
    ListItem { bullet: bool },
    Reference,
}

enum NodeKind {
    Text(TextBlock),
    Equation(EquationBlock),
    Float(Float),
    Footnote(TextBlock),
}

struct Node {
    kind: NodeKind,
    loc: Loc,
    class: Class,
    /// Leaf of the last line (differs from `loc` after merging).
    tail: (usize, usize),
    leaf_left: f32,
    leaf_right: f32,
    /// Number of leading characters to drop from the text (inline labels).
    strip_prefix: usize,
}

impl Node {
    fn text_block(&self) -> Option<&TextBlock> {
        match &self.kind {
            NodeKind::Text(t) => Some(t),
            _ => None,
        }
    }
}

fn block_text(lines: &[Line]) -> String {
    lines.iter().map(|l| l.text()).collect::<Vec<_>>().join(" ")
}

fn block_size(lines: &[Line]) -> f32 {
    let sizes: Vec<f32> = lines.iter().map(|l| l.size).collect();
    median(&sizes, 10.0)
}

fn frac(lines: &[Line], fonts: &[FontInfo], pred: impl Fn(&FontInfo) -> bool + Copy) -> f32 {
    let n: usize = lines.iter().map(|l| l.glyphs.len()).sum();
    if n == 0 {
        return 0.0;
    }
    let k: f32 = lines
        .iter()
        .map(|l| l.font_frac(fonts, pred) * l.glyphs.len() as f32)
        .sum();
    k / n as f32
}

fn upper_ratio(s: &str) -> f32 {
    let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() < 4 {
        return 0.0;
    }
    letters.iter().filter(|c| c.is_uppercase()).count() as f32 / letters.len() as f32
}

/// Small caps are set as capitals of two sizes.
fn small_caps(lines: &[Line]) -> bool {
    let mut sizes: Vec<f32> = Vec::new();
    let mut letters = 0;
    for g in lines.iter().flat_map(|l| l.glyphs.iter()) {
        if g.text.chars().all(|c| c.is_alphabetic()) {
            letters += 1;
            if !g.text.chars().all(|c| c.is_uppercase()) {
                return false;
            }
            if !sizes.iter().any(|s| (s - g.size).abs() < 0.2) {
                sizes.push(g.size);
            }
        }
    }
    letters >= 4 && sizes.len() >= 2
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').chars().take(48).collect()
}

// ---------------------------------------------------------------------------
// Text assembly
// ---------------------------------------------------------------------------

fn tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_alphabetic() || c == '-'))
        .filter(|t| !t.is_empty())
}

fn build_vocab<'a>(blocks: impl Iterator<Item = &'a [Line]>) -> HashSet<String> {
    let mut vocab = HashSet::new();
    for lines in blocks {
        let mut prev_hyphen = false;
        for l in lines {
            let text = l.text();
            let ends_hyphen = text.ends_with('-');
            let toks: Vec<&str> = tokens(&text).collect();
            let n = toks.len();
            for (i, t) in toks.into_iter().enumerate() {
                if (i == 0 && prev_hyphen) || (i + 1 == n && ends_hyphen) {
                    continue;
                }
                let t = t.trim_matches('-').to_lowercase();
                if t.is_empty() {
                    continue;
                }
                for part in t.split('-') {
                    if !part.is_empty() {
                        vocab.insert(part.to_string());
                    }
                }
                vocab.insert(t);
            }
            prev_hyphen = ends_hyphen;
        }
    }
    vocab
}

/// Decides whether a line-end hyphen is a soft hyphen (to be removed).
fn remove_hyphen(left: &str, right: &str, vocab: &HashSet<String>) -> bool {
    let trailing: String = left
        .chars()
        .rev()
        .skip(1)
        .take_while(|c| c.is_alphabetic())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let leading: String = right.chars().take_while(|c| c.is_alphabetic()).collect();
    if trailing.is_empty() || leading.is_empty() {
        return false;
    }
    if !leading.chars().next().unwrap().is_lowercase() {
        return false;
    }
    // "state-of-\nthe-art": the left part is itself a hyphenated compound.
    let before: Vec<char> = left.chars().rev().skip(1 + trailing.chars().count()).take(1).collect();
    if before.first() == Some(&'-') {
        return false;
    }
    let (l, r) = (trailing.to_lowercase(), leading.to_lowercase());
    if vocab.contains(&format!("{l}{r}")) {
        return true;
    }
    if vocab.contains(&format!("{l}-{r}")) {
        return false;
    }
    if l.chars().count() >= 4 && vocab.contains(&l) && vocab.contains(&r) {
        return false;
    }
    true
}

struct TextBuilder<'a> {
    fonts: &'a [FontInfo],
    vocab: &'a HashSet<String>,
}

struct Built {
    /// The running text of the block.
    spans: Vec<Span>,
    line_texts: Vec<String>,
    hlines: Vec<HLine>,
}

impl TextBuilder<'_> {
    /// Styled text of a single line, exactly as it appears.
    fn line_spans(&self, line: &Line) -> Vec<Span> {
        let mut spans: Vec<Span> = Vec::new();
        for (gi, g) in line.glyphs.iter().enumerate() {
            let style = line.style_of(g, self.fonts);
            let space = g.space_before && gi > 0;
            match spans.last_mut() {
                Some(s) if s.style == style && s.color == g.color => {
                    if space {
                        s.text.push(' ');
                    }
                    s.text.push_str(&g.text);
                }
                Some(s) => {
                    if space {
                        s.text.push(' ');
                    }
                    spans.push(Span {
                        text: g.text.clone(),
                        style,
                        href: None,
                        color: g.color,
                    });
                }
                None => spans.push(Span {
                    text: g.text.clone(),
                    style,
                    href: None,
                    color: g.color,
                }),
            }
        }
        spans
    }

    /// Converts lines into styled spans, joining the lines into running text.
    /// `skip` glyphs are dropped from the start of the running text and
    /// `mask` style bits are cleared in it (e.g. bold inside headings). The
    /// per-line output keeps everything.
    fn build(&self, lines: &[Line], skip: usize, mask: u8) -> Built {
        let texts: Vec<String> = lines.iter().map(|l| l.text()).collect();
        let mut hlines: Vec<HLine> = Vec::with_capacity(lines.len());
        let mut line_texts: Vec<String> = Vec::with_capacity(lines.len());
        for (li, line) in lines.iter().enumerate() {
            let mut ls = self.line_spans(line);
            let mut soft_hyphen = false;
            if li + 1 < lines.len() {
                let lt = &texts[li];
                if lt.ends_with('-') && lt.chars().count() > 1 {
                    if remove_hyphen(lt, &texts[li + 1], self.vocab) {
                        if let Some(last) = ls.last_mut() {
                            last.text.pop();
                            if last.text.is_empty() {
                                ls.pop();
                            }
                        }
                        soft_hyphen = true;
                    }
                } else if !lt.ends_with('/')
                    && !lt.is_empty()
                    && let Some(last) = ls.last_mut()
                {
                    last.text.push(' ');
                }
            }
            line_texts.push(ls.iter().map(|s| s.text.as_str()).collect());
            hlines.push(HLine { spans: ls, soft_hyphen });
        }
        let mut spans: Vec<Span> = Vec::new();
        for s in hlines.iter().flat_map(|h| h.spans.iter()) {
            let style = s.style & !mask;
            match spans.last_mut() {
                Some(p) if p.style == style && p.color == s.color => p.text.push_str(&s.text),
                _ => spans.push(Span {
                    text: s.text.clone(),
                    style,
                    href: None,
                    color: s.color,
                }),
            }
        }
        if skip > 0 && !lines.is_empty() {
            let n: usize = lines[0].glyphs.iter().take(skip).map(|g| g.text.chars().count()).sum();
            strip_chars(&mut spans, n);
        }
        Built {
            spans,
            line_texts,
            hlines,
        }
    }

    /// Positioned glyph runs of a displayed formula.
    fn eq_runs(&self, lines: &[Line]) -> Vec<EqRun> {
        let mut runs: Vec<EqRun> = Vec::new();
        for line in lines {
            // (run, font, right edge)
            let mut cur: Option<(EqRun, u16, f32)> = None;
            for g in &line.glyphs {
                // No Unicode value: the glyph stays part of the page picture.
                if g.text.contains('\u{FFFD}') {
                    if let Some((r, _, _)) = cur.take() {
                        runs.push(r);
                    }
                    continue;
                }
                let style = line.style_of(g, self.fonts) & !(SUP | SUB);
                let axis = g.oy - 0.25 * g.size;
                let odd =
                    g.ink.height() > 1.3 * g.size || (g.ink.cy() - axis).abs() > 0.6 * g.size || g.text == "\u{221A}";
                if odd {
                    if let Some((r, _, _)) = cur.take() {
                        runs.push(r);
                    }
                    runs.push(EqRun {
                        text: g.text.clone(),
                        x: g.ox,
                        y: g.oy,
                        width: (g.x1 - g.x0).max(0.1),
                        size: g.size,
                        style,
                        color: g.color,
                        ink: Some(g.ink),
                    });
                    continue;
                }
                let joins = cur.as_ref().is_some_and(|(r, font, end)| {
                    *font == g.font
                        && (r.size - g.size).abs() < 0.1
                        && (r.y - g.oy).abs() < 0.2
                        && r.color == g.color
                        && g.x0 - end < 0.6 * g.size
                        && g.x0 >= r.x
                });
                if joins {
                    let (r, _, end) = cur.as_mut().unwrap();
                    if g.space_before {
                        r.text.push(' ');
                    }
                    r.text.push_str(&g.text);
                    *end = g.x1;
                    r.width = g.x1 - r.x;
                } else {
                    if let Some((r, _, _)) = cur.take() {
                        runs.push(r);
                    }
                    cur = Some((
                        EqRun {
                            text: g.text.clone(),
                            x: g.x0,
                            y: g.oy,
                            width: (g.x1 - g.x0).max(0.1),
                            size: g.size,
                            style,
                            color: g.color,
                            ink: None,
                        },
                        g.font,
                        g.x1,
                    ));
                }
            }
            if let Some((r, _, _)) = cur.take() {
                runs.push(r);
            }
        }
        runs
    }

    fn table(&self, grid: Grid) -> Table {
        let rows = grid
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| {
                        let mut spans: Vec<Span> = Vec::new();
                        for (i, l) in cell.lines.iter().enumerate() {
                            if i > 0
                                && let Some(last) = spans.last_mut()
                            {
                                last.text.push(' ');
                            }
                            spans.extend(self.line_spans(l));
                        }
                        TableCell {
                            col: cell.col,
                            colspan: cell.colspan,
                            rowspan: cell.rowspan,
                            spans,
                        }
                    })
                    .collect()
            })
            .collect();
        Table {
            n_cols: grid.n_cols,
            header_rows: grid.header_rows,
            col_align: grid
                .col_align
                .iter()
                .map(|a| match a {
                    Align::Left => "left",
                    Align::Center => "center",
                    Align::Right => "right",
                })
                .collect(),
            rows,
            rule_above: grid.rule_above,
            rule_bottom: grid.rule_bottom,
            rule_left: grid.rule_left,
            rule_right: grid.rule_right,
            geom: TableGeom {
                bbox: grid.bbox,
                col_bounds: grid.col_bounds,
                col_text: grid.col_text,
                row_bounds: grid.row_bounds,
                row_baselines: grid.row_baselines,
                size: grid.size,
            },
        }
    }
}

/// Removes the first `n` characters from a span list.
fn strip_chars(spans: &mut Vec<Span>, mut n: usize) {
    while n > 0 && !spans.is_empty() {
        let len = spans[0].text.chars().count();
        if len <= n {
            n -= len;
            spans.remove(0);
        } else {
            spans[0].text = spans[0].text.chars().skip(n).collect();
            n = 0;
        }
    }
    if let Some(s) = spans.first_mut() {
        s.text = s.text.trim_start().to_string();
    }
}

// ---------------------------------------------------------------------------
// Cross references
// ---------------------------------------------------------------------------

static XREF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?P<url>https?://[^\s<>]+)",
        r"|\[(?P<cite>\d{1,3}(?:\s*[,;–-]\s*\d{1,3})*)\]",
        r"|(?P<obj>(?:Figures?|Figs?\.|Tables?|Algorithms?|Sections?|Sec\.|Appendix)\s*~?\s*(?P<num>(?:\d{1,3}|[A-Z])(?:\.\d{1,2})*))",
        r"|(?P<eq>(?:Eqs?\.|Eqn\.|Equations?)\s*~?\s*(?:\((?P<eqa>\d{1,3}(?:\.\d{1,2})*[a-z]?)\)|(?P<eqb>\d{1,3}(?:\.\d{1,2})*)))",
    ))
    .unwrap()
});

fn link_spans(spans: Vec<Span>, anchors: &HashSet<String>, own: Option<&str>) -> Vec<Span> {
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

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

/// Options of the layout analysis.
#[derive(Debug, Clone, Copy)]
pub struct AnalyzeOptions {
    /// Reconstruct the row/column structure of tables.
    pub tables: bool,
    /// Worker threads for reading the pages (0 = one per CPU).
    pub jobs: usize,
}

impl Default for AnalyzeOptions {
    fn default() -> Self {
        Self { tables: true, jobs: 0 }
    }
}

pub struct Analysis {
    pub doc: Document,
    /// Lines that belong to no block (running heads, page numbers, equation
    /// numbers), per page. The position-preserving renderers draw them.
    pub loose: Vec<Vec<PLine>>,
    /// Rotated text, per page.
    pub rotated: Vec<Vec<RotatedRun>>,
}

pub fn analyze(raw_pages: Vec<RawPage>, fonts: &[FontInfo], mut meta: Meta, opts: AnalyzeOptions) -> Analysis {
    let mut pages: Vec<PageData> = raw_pages.into_iter().map(PageData::from_raw).collect();
    let stats = compute_stats(&pages);
    let furniture = remove_furniture(&mut pages);
    meta.arxiv_id = meta.arxiv_id.or(furniture.arxiv_id);
    meta.arxiv_stamp = furniture.arxiv_stamp.clone();

    // --- per-page layout ---
    let mut nodes: Vec<Node> = Vec::new();
    for page in pages.iter_mut() {
        join_caption_labels(page);
    }
    // Convention for table captions (above or below the table).
    let (mut votes_above, mut votes_below) = (0, 0);
    for page in pages.iter() {
        let (a, b) = table_caption_votes(page, fonts);
        votes_above += a;
        votes_below += b;
    }
    let table_caption_above = if votes_above > 2 * votes_below {
        Some(true)
    } else if votes_below > 2 * votes_above {
        Some(false)
    } else {
        None
    };

    for page in pages.iter_mut() {
        let floats = detect_floats(page, &stats, fonts, table_caption_above);

        let leaves = order_page(&page.lines, &floats, &stats);
        let leaf_bounds: Vec<(f32, f32)> = leaves.iter().map(|l| (l.bbox.x0, l.bbox.x1)).collect();
        for rb in build_blocks(page, &leaves, &stats, fonts) {
            let (kind, loc) = match rb {
                RawBlock::Text(t) => {
                    let loc = t.loc;
                    (NodeKind::Text(t), loc)
                }
                RawBlock::Footnote(t) => {
                    let loc = t.loc;
                    (NodeKind::Footnote(t), loc)
                }
                RawBlock::Equation(e) => {
                    let loc = e.loc;
                    (NodeKind::Equation(e), loc)
                }
                RawBlock::Float { idx, loc } => (NodeKind::Float(floats[idx].clone()), loc),
            };
            let (leaf_left, leaf_right) = leaf_bounds[loc.leaf];
            nodes.push(Node {
                kind,
                loc,
                class: Class::Unknown,
                tail: (loc.page, loc.leaf),
                leaf_left,
                leaf_right,
                strip_prefix: 0,
            });
        }
    }

    classify_front_matter(&mut nodes, &pages, &stats, fonts);
    classify_body(&mut nodes, &stats, fonts);
    split_references(&mut nodes);
    merge_paragraphs(&mut nodes, &stats);

    let rotated = pages
        .iter()
        .map(|p| {
            p.rotated
                .iter()
                .filter(|r| !r.text.contains('\u{FFFD}') && !r.text.trim().is_empty())
                .map(|r| {
                    let f = &fonts[r.font as usize];
                    let mut style = 0;
                    if f.bold {
                        style |= BOLD;
                    }
                    if f.italic {
                        style |= ITALIC;
                    }
                    if f.mono {
                        style |= MONO;
                    }
                    if f.sans {
                        style |= SANS;
                    }
                    RotatedRun {
                        text: r.text.clone(),
                        x: r.x,
                        y: r.y,
                        angle: r.angle,
                        size: r.size,
                        length: r.length,
                        style,
                        color: r.color,
                        origins: r.origins.clone(),
                    }
                })
                .collect()
        })
        .collect();
    let (doc, loose) = build_document(nodes, &pages, &stats, fonts, meta, furniture.lines, opts);
    Analysis { doc, loose, rotated }
}

fn classify_front_matter(nodes: &mut Vec<Node>, pages: &[PageData], stats: &DocStats, fonts: &[FontInfo]) {
    let Some(first_page) = nodes.iter().find(|n| n.text_block().is_some()).map(|n| n.loc.page) else {
        return;
    };
    let page_h = pages[first_page].height;
    let on_first = |n: &Node| n.loc.page == first_page;

    // Title: the largest text in the upper part of the first page.
    let mut tsize = 0.0f32;
    for n in nodes.iter().filter(|n| on_first(n)) {
        if let Some(t) = n.text_block()
            && t.bbox.y0 < 0.65 * page_h
            && block_text(&t.lines).chars().count() >= 4
        {
            tsize = tsize.max(block_size(&t.lines));
        }
    }
    let mut title_range: Option<(usize, usize)> = None;
    if tsize >= stats.body_size + 1.4 {
        for (i, n) in nodes.iter().enumerate() {
            if !on_first(n) {
                continue;
            }
            let is_title = n
                .text_block()
                .is_some_and(|t| block_size(&t.lines) >= tsize - 0.6 && t.bbox.y0 < 0.65 * page_h);
            match (&mut title_range, is_title) {
                (None, true) => title_range = Some((i, i)),
                (Some((_, e)), true) if *e + 1 == i => *e = i,
                _ => {}
            }
        }
    }
    let Some((ts, te)) = title_range else { return };
    for n in nodes[..ts].iter_mut().filter(|n| n.loc.page == first_page) {
        if n.text_block().is_some() {
            n.class = Class::Note;
        }
    }
    for n in &mut nodes[ts..=te] {
        n.class = Class::Title;
    }

    // Authors: everything up to the abstract or the first section.
    let mut stopped_by_prose = false;
    let mut end = te + 1;
    while end < nodes.len() && nodes[end].loc.page == first_page {
        let n = &nodes[end];
        if let Some(t) = n.text_block() {
            let text = block_text(&t.lines);
            let trimmed = text.trim();
            let is_abstract = trimmed.eq_ignore_ascii_case("abstract") || ABSTRACT_INLINE.is_match(trimmed);
            let bold = frac(&t.lines, fonts, |f| f.bold) >= 0.6;
            let is_section = (HEADING_NUM.is_match(trimmed) || HEADING_ROMAN.is_match(trimmed))
                && (bold || block_size(&t.lines) > stats.body_size + 0.5 || upper_ratio(trimmed) > 0.8)
                && trimmed.chars().count() < 80;
            // Running prose: several justified lines.
            let full = t
                .lines
                .iter()
                .filter(|l| {
                    l.char_count() >= 40
                        && (l.bbox.x1 - t.right).abs() < 1.5
                        && (l.bbox.x0 - t.left).abs() < 2.0 * l.size
                })
                .count();
            let prose = t.lines.len() >= 4 && full >= 3;
            if is_abstract || is_section || prose {
                stopped_by_prose = prose && !is_abstract && !is_section;
                break;
            }
        }
        end += 1;
    }
    // Author blocks are laid out as a grid of small centred groups, which
    // the column analysis cannot order. Re-cluster their lines into one
    // "card" per group and order the cards row by row.
    // An abstract without a heading (journal layouts): the prose that ended
    // the author block, up to the first heading.
    if stopped_by_prose {
        let next_heading = nodes[end..].iter().position(|n| {
            n.loc.page > first_page + 1
                || n.text_block().is_some_and(|t| {
                    let text = block_text(&t.lines);
                    let text = text.trim();
                    text.chars().count() < 80
                        && (HEADING_NUM.is_match(text) || HEADING_ROMAN.is_match(text) || KNOWN_HEADINGS.is_match(text))
                })
        });
        if let Some(k) = next_heading {
            let texts = nodes[end..end + k].iter().filter(|n| n.text_block().is_some()).count();
            if nodes[end + k].loc.page <= first_page + 1 && texts <= 3 {
                for n in nodes[end..end + k].iter_mut().filter(|n| n.text_block().is_some()) {
                    n.class = Class::Paragraph { in_abstract: true };
                }
            }
        }
    }

    // Besides the nodes before the stop, anything that lies between the
    // title and the stop geometrically belongs to the author block (the
    // column analysis may have ordered a second author column later).
    let title_bottom = nodes[ts..=te]
        .iter()
        .filter_map(|n| n.text_block())
        .map(|t| t.bbox.y1)
        .fold(0.0, f32::max);
    let stop_y = nodes
        .get(end)
        .filter(|n| n.loc.page == first_page)
        .and_then(|n| n.text_block())
        .map(|t| t.bbox.y0)
        .unwrap_or(f32::INFINITY);
    let author_idx: Vec<usize> = (te + 1..nodes.len())
        .filter(|&i| {
            let n = &nodes[i];
            let Some(t) = n.text_block() else { return false };
            n.loc.page == first_page
                && n.class == Class::Unknown
                && (i < end || (stop_y.is_finite() && t.bbox.y1 <= stop_y - 1.0 && t.bbox.y0 >= title_bottom - 2.0))
        })
        .collect();
    if author_idx.is_empty() {
        return;
    }
    let author_zones: HashSet<usize> = author_idx.iter().map(|&i| nodes[i].loc.zone).collect();
    let mut lines: Vec<Line> = Vec::new();
    for &i in &author_idx {
        lines.extend(nodes[i].text_block().unwrap().lines.iter().cloned());
    }
    lines.sort_by(|a, b| a.baseline.total_cmp(&b.baseline).then(a.bbox.x0.total_cmp(&b.bbox.x0)));
    let mut parent: Vec<usize> = (0..lines.len()).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        p[i] = r;
        r
    }
    for i in 0..lines.len() {
        for j in i + 1..lines.len() {
            let (a, b) = (&lines[i], &lines[j]);
            let gap = b.bbox.y0 - a.bbox.y1;
            if gap > 0.9 * a.size.max(b.size) {
                break;
            }
            let same_row = (a.baseline - b.baseline).abs() < 2.0;
            if !same_row && a.bbox.x_overlap(&b.bbox) >= 0.5 * a.bbox.width().min(b.bbox.width()) {
                let (ra, rb) = (find(&mut parent, i), find(&mut parent, j));
                if ra != rb {
                    parent[rb] = ra;
                }
            }
        }
    }
    let mut cards: Vec<(Rect, Vec<Line>)> = Vec::new();
    let mut card_of: HashMap<usize, usize> = HashMap::new();
    for (i, line) in lines.iter().enumerate() {
        let r = find(&mut parent, i);
        let ci = *card_of.entry(r).or_insert_with(|| {
            cards.push((Rect::EMPTY, Vec::new()));
            cards.len() - 1
        });
        cards[ci].0 = cards[ci].0.union(&line.bbox);
        cards[ci].1.push(line.clone());
    }
    // Rows of cards.
    cards.sort_by(|a, b| a.0.y0.total_cmp(&b.0.y0));
    let mut ordered: Vec<(Rect, Vec<Line>)> = Vec::new();
    let mut row: Vec<(Rect, Vec<Line>)> = Vec::new();
    let mut row_box = Rect::EMPTY;
    for c in cards {
        if !row.is_empty() && row_box.y_overlap(&c.0) < 0.4 * row_box.height().min(c.0.height()) {
            row.sort_by(|a, b| a.0.x0.total_cmp(&b.0.x0));
            ordered.append(&mut row);
            row_box = Rect::EMPTY;
        }
        row_box = row_box.union(&c.0);
        row.push(c);
    }
    row.sort_by(|a, b| a.0.x0.total_cmp(&b.0.x0));
    ordered.append(&mut row);

    let template_loc = Loc {
        zone_cols: 1,
        col: 0,
        zone: usize::MAX / 2,
        ..nodes[author_idx[0]].loc
    };
    let (leaf_left, leaf_right) = (nodes[author_idx[0]].leaf_left, nodes[author_idx[0]].leaf_right);
    let new_nodes: Vec<Node> = ordered
        .into_iter()
        .map(|(bbox, lines)| Node {
            kind: NodeKind::Text(TextBlock {
                lines,
                loc: template_loc,
                left: bbox.x0,
                right: bbox.x1,
                centered: true,
                gap_before: f32::INFINITY,
                bbox,
            }),
            loc: template_loc,
            class: Class::Authors,
            tail: (template_loc.page, template_loc.leaf),
            leaf_left,
            leaf_right,
            strip_prefix: 0,
        })
        .collect();
    // Replace the old author nodes (keeping floats etc. in between).
    let first = author_idx[0];
    let drop: HashSet<usize> = author_idx.iter().copied().collect();
    let mut i = 0;
    nodes.retain(|_| {
        let keep = !drop.contains(&i);
        i += 1;
        keep
    });
    nodes.splice(first..first, new_nodes);

    // Whatever else shared a zone with the author grid (typically the
    // "Abstract" heading) is not really part of a multi-column zone.
    for z in author_zones {
        let others: Vec<usize> = nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.loc.page == first_page && n.loc.zone == z && n.class != Class::Authors)
            .map(|(i, _)| i)
            .collect();
        if others.len() <= 3 {
            for i in others {
                nodes[i].loc.zone_cols = 1;
                nodes[i].loc.col = 0;
            }
        }
    }
}

/// Splits reference blocks that contain several labelled entries.
fn split_references(nodes: &mut Vec<Node>) {
    let mut i = 0;
    while i < nodes.len() {
        if nodes[i].class != Class::Reference {
            i += 1;
            continue;
        }
        let Some(t) = nodes[i].text_block() else {
            i += 1;
            continue;
        };
        let first_x0 = t.lines[0].bbox.x0;
        let em = t.lines[0].size;
        if !REF_LABEL.is_match(t.lines[0].text().trim()) {
            i += 1;
            continue;
        }
        let cut = t
            .lines
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, l)| l.bbox.x0 <= first_x0 + 0.6 * em && REF_LABEL.is_match(l.text().trim()));
        let Some((k, _)) = cut else {
            i += 1;
            continue;
        };
        let NodeKind::Text(t) = &mut nodes[i].kind else {
            unreachable!()
        };
        let rest_lines = t.lines.split_off(k);
        let mut rest = t.clone();
        rest.lines = rest_lines;
        let node = Node {
            kind: NodeKind::Text(rest),
            loc: nodes[i].loc,
            class: Class::Reference,
            tail: nodes[i].tail,
            leaf_left: nodes[i].leaf_left,
            leaf_right: nodes[i].leaf_right,
            strip_prefix: 0,
        };
        nodes.insert(i + 1, node);
        i += 1;
    }
}

fn classify_body(nodes: &mut [Node], stats: &DocStats, fonts: &[FontInfo]) {
    // Pass 1: heading candidates.
    struct Cand {
        idx: usize,
        level: Option<u8>,
        number: Option<String>,
        size: f32,
    }
    let mut cands: Vec<Cand> = Vec::new();
    let mut seen_refs = false;
    for (i, n) in nodes.iter().enumerate() {
        if n.class != Class::Unknown {
            continue;
        }
        let Some(t) = n.text_block() else { continue };
        let text = block_text(&t.lines);
        let text = text.trim();
        let nchars = text.chars().count();
        if t.lines.len() > 3 || !(2..=160).contains(&nchars) {
            continue;
        }
        let size = block_size(&t.lines);
        let bold = frac(&t.lines, fonts, |f| f.bold) >= 0.9;
        let italic = frac(&t.lines, fonts, |f| f.italic) >= 0.7;
        let caps = upper_ratio(text) >= 0.8 || small_caps(&t.lines);
        let strong = bold || size >= stats.body_size + 0.7;
        let weak = italic || caps;
        let known = KNOWN_HEADINGS.is_match(text.trim_end_matches(['.', ':']));
        let words = text.split_whitespace().count();

        let (mut level, mut number): (Option<u8>, Option<String>) = (None, None);
        let mut numbered = false;
        if let Some(c) = HEADING_NUM.captures(text) {
            let rest = &c["rest"];
            // "2 layers" is not a heading; a heading title starts with a
            // capital letter (or is all caps).
            if rest.chars().next().is_some_and(|ch| ch.is_uppercase()) && (strong || weak) {
                numbered = true;
                number = Some(c["num"].to_string());
                level = Some(1 + c["num"].matches('.').count() as u8);
            }
        } else if let Some(c) = HEADING_ROMAN.captures(text).filter(|_| caps || strong) {
            numbered = true;
            number = Some(c["num"].to_string());
            level = Some(1);
        } else if let Some(c) = HEADING_APP
            .captures(text)
            .filter(|c| (seen_refs || c.name("word").is_some() || c["num"].contains('.')) && strong)
        {
            numbered = true;
            number = Some(c["num"].to_string());
            level = Some(1 + c["num"].matches('.').count() as u8);
        } else if let Some(c) = HEADING_LETTER
            .captures(text)
            .filter(|_| italic && !strong && words <= 12)
        {
            numbered = true;
            number = Some(c["num"].to_string());
            level = Some(2);
        }

        let ends_sentence = text.ends_with('.') && words > 6;
        let last_full = t.lines.len() > 1 && {
            let l = t.lines.last().unwrap();
            l.bbox.x1 > t.right - 2.0 && t.lines.len() >= 3
        };
        let list_like = LIST_LABEL.is_match(text) && !numbered;
        let is_heading = numbered
            || (known && (strong || caps) && words <= 4)
            || (strong
                && t.lines.len() <= 2
                && nchars <= 120
                && !ends_sentence
                && !last_full
                && !list_like
                && words <= 16
                && text.chars().any(|c| c.is_alphabetic()));
        if !is_heading {
            continue;
        }
        if REF_HEADING.is_match(text) {
            seen_refs = true;
        }
        cands.push(Cand {
            idx: i,
            level,
            number,
            size,
        });
    }

    // Level-1 size, to rank unnumbered headings.
    let h1_sizes: Vec<f32> = cands.iter().filter(|c| c.level == Some(1)).map(|c| c.size).collect();
    let h1_size = if h1_sizes.is_empty() {
        stats.body_size + 1.5
    } else {
        median(&h1_sizes, stats.body_size + 1.5)
    };
    let h2_sizes: Vec<f32> = cands.iter().filter(|c| c.level == Some(2)).map(|c| c.size).collect();
    let h2_size = median(&h2_sizes, stats.body_size);
    for c in cands {
        let text = block_text(&nodes[c.idx].text_block().unwrap().lines);
        let known = KNOWN_HEADINGS.is_match(text.trim().trim_end_matches(['.', ':']));
        let level = c.level.unwrap_or({
            if known || c.size >= h1_size - 0.3 {
                1
            } else if c.size >= h2_size - 0.3 && c.size > stats.body_size + 0.3 {
                2
            } else {
                3
            }
        });
        nodes[c.idx].class = Class::Heading {
            level: level.min(5),
            number: c.number,
        };
    }

    // Pass 2: paragraphs, abstract, references, lists.
    let mut in_abstract = false;
    let mut in_refs = false;
    let mut abstract_page = 0;
    for n in nodes.iter_mut() {
        let NodeKind::Text(t) = &n.kind else { continue };
        let text = block_text(&t.lines);
        let trimmed = text.trim();
        match &n.class {
            Class::Heading { .. } => {
                in_abstract = trimmed.trim_end_matches(['.', ':']).eq_ignore_ascii_case("abstract");
                abstract_page = n.loc.page;
                in_refs = REF_HEADING.is_match(trimmed);
                continue;
            }
            Class::Unknown => {}
            _ => continue,
        }
        if in_abstract && n.loc.page > abstract_page + 1 {
            in_abstract = false;
        }
        if let Some(m) = ABSTRACT_INLINE.find(trimmed)
            && n.loc.page <= 1
        {
            n.strip_prefix = trimmed[..m.end()].chars().count();
            in_abstract = true;
            abstract_page = n.loc.page;
            n.class = Class::Paragraph { in_abstract: true };
            continue;
        }
        if in_refs {
            n.class = Class::Reference;
            continue;
        }
        if let Some(m) = LIST_LABEL.find(trimmed) {
            let label = m.as_str().trim();
            let bullet = label.chars().count() == 1 && !label.chars().next().unwrap().is_alphanumeric();
            if !label.starts_with('[') {
                n.class = Class::ListItem { bullet };
                continue;
            }
        }
        n.class = Class::Paragraph { in_abstract };
    }
}

fn first_alpha_lower(s: &str) -> bool {
    s.chars().find(|c| c.is_alphabetic()).is_some_and(|c| c.is_lowercase())
}

fn ends_terminal(s: &str) -> bool {
    let t = s.trim_end().trim_end_matches([')', '”', '’', '"', '\'', ']']);
    t.ends_with(['.', '?', '!', ':'])
}

fn merge_paragraphs(nodes: &mut Vec<Node>, stats: &DocStats) {
    // Does this document indent the first line of a paragraph?
    let (mut indented, mut total) = (0, 0);
    for n in nodes.iter() {
        if let (Some(t), Class::Paragraph { .. }) = (n.text_block(), &n.class)
            && t.lines.len() >= 3
            && (block_size(&t.lines) - stats.body_size).abs() < 0.6
        {
            total += 1;
            let rest = t.lines[1..].iter().map(|l| l.bbox.x0).fold(f32::INFINITY, f32::min);
            if t.lines[0].bbox.x0 > rest + 0.8 * stats.body_size {
                indented += 1;
            }
        }
    }
    let uses_indent = total >= 4 && indented * 100 >= total * 35;
    let refs_labeled = nodes
        .iter()
        .filter(|n| n.class == Class::Reference)
        .filter_map(|n| n.text_block())
        .filter(|t| REF_LABEL.is_match(block_text(&t.lines).trim()))
        .count()
        >= 3;

    let flow = |n: &Node| matches!(n.kind, NodeKind::Text(_) | NodeKind::Equation(_));
    let mut i = 0;
    while i < nodes.len() {
        let mergeable = matches!(
            nodes[i].class,
            Class::Paragraph { .. } | Class::Reference | Class::ListItem { .. }
        ) && nodes[i].text_block().is_some();
        if !mergeable {
            i += 1;
            continue;
        }
        let tail = nodes[i].tail;
        // `i` must be the last flow block of its (tail) leaf.
        let mut j = i + 1;
        let mut last_in_leaf = true;
        while j < nodes.len() && (nodes[j].loc.page, nodes[j].loc.leaf) == tail {
            if flow(&nodes[j]) {
                last_in_leaf = false;
                break;
            }
            j += 1;
        }
        if !last_in_leaf {
            i += 1;
            continue;
        }
        // Next flow block, which must be the first one of its leaf.
        while j < nodes.len() && !flow(&nodes[j]) {
            j += 1;
        }
        if j >= nodes.len() || nodes[j].loc.page > tail.0 + 1 {
            i += 1;
            continue;
        }
        let jleaf = (nodes[j].loc.page, nodes[j].loc.leaf);
        let first_in_leaf = !nodes[..j]
            .iter()
            .rev()
            .take_while(|n| (n.loc.page, n.loc.leaf) == jleaf)
            .any(flow);
        let compatible = match (&nodes[i].class, &nodes[j].class) {
            (Class::Paragraph { in_abstract: a }, Class::Paragraph { in_abstract: b }) => a == b,
            (Class::Reference, Class::Reference) => true,
            (Class::ListItem { .. }, Class::Paragraph { .. }) => true,
            // "... question answering, and" / "(4) a degenerate ...": an
            // enumeration inside a sentence, not a list.
            (Class::Paragraph { .. }, Class::ListItem { bullet: false }) => true,
            _ => false,
        };
        if !first_in_leaf || !compatible || jleaf == tail {
            i += 1;
            continue;
        }
        let decision = {
            let (p1, p2) = (nodes[i].text_block().unwrap(), nodes[j].text_block().unwrap());
            let (a, b) = (p1.lines.last().unwrap(), &p2.lines[0]);
            let em = b.size;
            let a_text = a.text();
            let b_text = b.text();
            let right_edge = if p1.lines.len() >= 2 {
                p1.right
            } else {
                nodes[i].leaf_right
            };
            let hyph = a_text.ends_with('-');
            let short_a = a.bbox.x1 + 0.25 * em + b.first_word_width() < right_edge - 1.0;
            let indent_b = if p2.lines.len() >= 2 {
                let rest = p2.lines[1..].iter().map(|l| l.bbox.x0).fold(f32::INFINITY, f32::min);
                b.bbox.x0 > rest + 0.6 * em
            } else {
                b.bbox.x0 > nodes[j].leaf_left + 0.6 * em
            };
            let label_b = LIST_LABEL.is_match(b_text.trim());
            if (a.size - b.size).abs() > 0.4 {
                false
            } else if nodes[i].class == Class::Reference {
                let labeled = REF_LABEL.is_match(b_text.trim());
                !labeled && (refs_labeled || b.bbox.x0 > nodes[j].leaf_left + 0.6 * em)
            } else if label_b
                && (matches!(nodes[j].class, Class::ListItem { bullet: true }) || ends_terminal(&a_text) || short_a)
            {
                false
            } else if hyph {
                true
            } else if short_a || indent_b {
                false
            } else if first_alpha_lower(&b_text) || !ends_terminal(&a_text) {
                true
            } else {
                uses_indent
            }
        };
        if !decision {
            i += 1;
            continue;
        }
        let removed = nodes.remove(j);
        let NodeKind::Text(t2) = removed.kind else {
            unreachable!()
        };
        let NodeKind::Text(t1) = &mut nodes[i].kind else {
            unreachable!()
        };
        t1.lines.extend(t2.lines);
        nodes[i].tail = removed.tail;
        nodes[i].leaf_right = removed.leaf_right;
        // Stay on `i`: the merged paragraph may continue further.
    }

    // Footnotes that continue on the next column or page: an unmarked
    // footnote directly following (in footnote order) an unfinished one.
    let marked = |t: &TextBlock| {
        t.lines[0].starts_with_superscript()
            || t.lines[0]
                .glyphs
                .first()
                .is_some_and(|g| matches!(g.text.as_str(), "∗" | "†" | "‡" | "§" | "¶" | "*"))
    };
    let mut last_fn: Option<usize> = None;
    let mut i = 0;
    while i < nodes.len() {
        let NodeKind::Footnote(t) = &nodes[i].kind else {
            i += 1;
            continue;
        };
        if let Some(p) = last_fn {
            let NodeKind::Footnote(prev) = &nodes[p].kind else {
                unreachable!()
            };
            let prev_text = prev.lines.last().unwrap().text();
            let different_leaf = (nodes[p].loc.page, nodes[p].loc.leaf) != (nodes[i].loc.page, nodes[i].loc.leaf);
            if different_leaf
                && !marked(t)
                && marked(prev)
                && (prev_text.ends_with('-') || !ends_terminal(&prev_text))
                && (prev.lines[0].size - t.lines[0].size).abs() < 0.4
            {
                let removed = nodes.remove(i);
                let NodeKind::Footnote(t2) = removed.kind else {
                    unreachable!()
                };
                let NodeKind::Footnote(t1) = &mut nodes[p].kind else {
                    unreachable!()
                };
                t1.lines.extend(t2.lines);
                continue;
            }
        }
        last_fn = Some(i);
        i += 1;
    }
}

fn new_block(id: usize, kind: BlockKind, loc: Loc, bbox: Rect) -> Block {
    Block {
        id,
        kind,
        page: loc.page,
        bbox,
        spans: Vec::new(),
        rows: Vec::new(),
        anchor: None,
        loc,
        lines: Vec::new(),
        line_texts: Vec::new(),
        hlines: Vec::new(),
        table: None,
        inner_lines: Vec::new(),
        inner_spans: Vec::new(),
        eq_runs: Vec::new(),
    }
}

fn is_footnote_symbol(s: &str) -> bool {
    matches!(s, "∗" | "†" | "‡" | "§" | "¶" | "*")
}

fn build_document(
    nodes: Vec<Node>,
    pages: &[PageData],
    stats: &DocStats,
    fonts: &[FontInfo],
    mut meta: Meta,
    furniture: Vec<Vec<Line>>,
    opts: AnalyzeOptions,
) -> (Document, Vec<Vec<PLine>>) {
    let vocab = build_vocab(nodes.iter().filter_map(|n| match &n.kind {
        NodeKind::Text(t) | NodeKind::Footnote(t) => Some(t.lines.as_slice()),
        NodeKind::Float(f) => Some(f.caption.as_slice()),
        _ => None,
    }));
    let tb = TextBuilder { fonts, vocab: &vocab };

    let mut loose: Vec<Vec<PLine>> = vec![Vec::new(); pages.len()];
    for (pi, lines) in furniture.into_iter().enumerate() {
        for line in lines {
            let spans = tb.line_spans(&line);
            loose[pi].push(PLine {
                line,
                spans,
                kind: "header",
            });
        }
    }

    let mut blocks: Vec<Block> = Vec::new();
    let mut anchors: HashSet<String> = HashSet::new();
    let unique = |base: String, anchors: &mut HashSet<String>| -> String {
        let mut a = base.clone();
        let mut k = 2;
        while !anchors.insert(a.clone()) {
            a = format!("{base}-{k}");
            k += 1;
        }
        a
    };
    let mut title_parts: Vec<String> = Vec::new();
    let mut eq_counter = 0;

    for n in nodes {
        let id = blocks.len();
        let loc = n.loc;
        match n.kind {
            NodeKind::Text(t) => {
                let bbox = t.lines.iter().fold(Rect::EMPTY, |a, l| a.union(&l.bbox));
                let mut rows = Vec::new();
                let mut anchor = None;
                let (kind, built) = match n.class {
                    Class::Title => {
                        let b = tb.build(&t.lines, 0, BOLD);
                        title_parts.push(b.spans.iter().map(|x| x.text.as_str()).collect());
                        (BlockKind::Title, b)
                    }
                    Class::Authors => {
                        // One row per baseline; side-by-side pieces are
                        // separated by an em space.
                        let mut last_baseline = f32::NEG_INFINITY;
                        for l in &t.lines {
                            let mut spans = tb.line_spans(l);
                            if (l.baseline - last_baseline).abs() < 2.0 && !rows.is_empty() {
                                let row: &mut Vec<Span> = rows.last_mut().unwrap();
                                row.push(Span::plain("\u{2003}"));
                                row.append(&mut spans);
                            } else {
                                rows.push(spans);
                            }
                            last_baseline = l.baseline;
                        }
                        (BlockKind::Authors, tb.build(&t.lines, 0, 0))
                    }
                    Class::Note => (BlockKind::Note, tb.build(&t.lines, 0, 0)),
                    Class::Heading { level, number } => {
                        let b = tb.build(&t.lines, 0, BOLD);
                        let text: String = b.spans.iter().map(|x| x.text.as_str()).collect();
                        let base = match &number {
                            Some(num) => format!("sec-{num}"),
                            None => format!("sec-{}", slug(&text)),
                        };
                        anchor = Some(unique(base, &mut anchors));
                        (BlockKind::Heading { level }, b)
                    }
                    Class::ListItem { bullet } => {
                        let b = tb.build(&t.lines, if bullet { 1 } else { 0 }, 0);
                        let marker = if bullet {
                            None
                        } else {
                            let text: String = b.spans.iter().map(|x| x.text.as_str()).collect();
                            LIST_LABEL.find(&text).map(|m| m.as_str().trim().to_string())
                        };
                        (BlockKind::ListItem { marker }, b)
                    }
                    Class::Reference => {
                        let b = tb.build(&t.lines, 0, 0);
                        let text: String = b.spans.iter().map(|x| x.text.as_str()).collect();
                        let label = REF_LABEL
                            .captures(text.trim())
                            .map(|c| c.name("a").or(c.name("b")).unwrap().as_str().to_string());
                        if let Some(l) = &label {
                            anchor = Some(unique(format!("ref-{}", slug(l)), &mut anchors));
                        }
                        (BlockKind::Reference { label }, b)
                    }
                    Class::Paragraph { in_abstract } => {
                        let mut b = tb.build(&t.lines, 0, 0);
                        if n.strip_prefix > 0 {
                            strip_chars(&mut b.spans, n.strip_prefix);
                        }
                        (BlockKind::Paragraph { in_abstract }, b)
                    }
                    Class::Unknown => (BlockKind::Paragraph { in_abstract: false }, tb.build(&t.lines, 0, 0)),
                };
                let mut block = new_block(id, kind, loc, bbox);
                block.spans = built.spans;
                block.line_texts = built.line_texts;
                block.hlines = built.hlines;
                block.rows = rows;
                block.anchor = anchor;
                block.lines = t.lines;
                blocks.push(block);
            }
            NodeKind::Footnote(t) => {
                let bbox = t.lines.iter().fold(Rect::EMPTY, |a, l| a.union(&l.bbox));
                let first = &t.lines[0];
                let mut skip = first.glyphs.iter().take_while(|g| g.script == 1).count();
                if skip == 0 && first.glyphs.first().is_some_and(|g| is_footnote_symbol(&g.text)) {
                    skip = 1;
                }
                let marker: String = first.glyphs[..skip].iter().map(|g| g.text.as_str()).collect();
                let built = tb.build(&t.lines, skip, 0);
                let anchor = (!marker.is_empty()).then(|| unique(format!("fn-{}", slug_marker(&marker)), &mut anchors));
                let mut block = new_block(id, BlockKind::Footnote { marker }, loc, bbox);
                block.spans = built.spans;
                block.line_texts = built.line_texts;
                block.hlines = built.hlines;
                block.anchor = anchor;
                block.lines = t.lines;
                blocks.push(block);
            }
            NodeKind::Equation(e) => {
                let mut lines = e.lines;
                lines.sort_by(|a, b| a.baseline.total_cmp(&b.baseline).then(a.bbox.x0.total_cmp(&b.bbox.x0)));
                let alt = lines.iter().map(|l| l.text()).collect::<Vec<_>>().join(" ");
                let rect = e.bbox.inflate(2.5, 2.5).intersect(&pages[loc.page].rect());
                let anchor = match &e.number {
                    Some(num) => Some(unique(format!("eq-{}", num.trim_matches(['(', ')'])), &mut anchors)),
                    None => {
                        eq_counter += 1;
                        Some(unique(format!("eq-u{eq_counter}"), &mut anchors))
                    }
                };
                if let Some(line) = e.number_line {
                    let spans = tb.line_spans(&line);
                    loose[loc.page].push(PLine {
                        line,
                        spans,
                        kind: "eqno",
                    });
                }
                let mut block = new_block(
                    id,
                    BlockKind::Equation {
                        image: ImageSpec {
                            page: loc.page,
                            rect,
                            transparent: true,
                        },
                        number: e.number,
                    },
                    loc,
                    rect,
                );
                block.spans = vec![Span::plain(alt)];
                block.line_texts = lines.iter().map(|l| l.text()).collect();
                block.eq_runs = tb.eq_runs(&lines);
                block.anchor = anchor;
                block.lines = lines;
                blocks.push(block);
            }
            NodeKind::Float(f) => {
                let built = tb.build(&f.caption, 0, 0);
                let anchor = f.label.as_ref().map(|label| {
                    let num = label.rsplit(' ').next().unwrap_or("");
                    let prefix = match f.kind {
                        FloatKind::Table => "tab",
                        FloatKind::Algorithm => "alg",
                        _ => "fig",
                    };
                    unique(format!("{prefix}-{num}"), &mut anchors)
                });
                let mut rows = Vec::new();
                if !f.inner_text.is_empty() {
                    rows.push(vec![Span::plain(f.inner_text.clone())]);
                }
                let tabular = f.kind == FloatKind::Table || (f.kind == FloatKind::Graphic && f.table_like);
                let table = match f.region {
                    Some(region) if opts.tables && tabular && !f.complex => {
                        // Cells can be closer together than the gap at which
                        // lines are broken; split more finely. Pieces that
                        // end up in the same column are joined again.
                        let pieces: Vec<Line> = f
                            .inner_lines
                            .iter()
                            .flat_map(|l| split_at_gaps(l, 0.55 * l.size))
                            .collect();
                        extract_grid(region, &pieces, &f.hrules, &f.vrules).map(|g| tb.table(g))
                    }
                    _ => None,
                };
                let mut block = new_block(
                    id,
                    BlockKind::Figure {
                        kind: f.kind,
                        image: f.region.map(|rect| ImageSpec {
                            page: f.page,
                            rect,
                            transparent: false,
                        }),
                        label: f.label.clone(),
                    },
                    loc,
                    f.bbox,
                );
                block.spans = built.spans;
                block.line_texts = built.line_texts;
                block.hlines = built.hlines;
                block.rows = rows;
                block.anchor = anchor;
                block.lines = f.caption;
                block.inner_spans = f.inner_lines.iter().map(|l| tb.line_spans(l)).collect();
                block.inner_lines = f.inner_lines;
                block.table = table;
                blocks.push(block);
            }
        }
    }

    // Footnote marks in the running text.
    let fn_markers: HashMap<String, String> = blocks
        .iter()
        .filter_map(|b| match (&b.kind, &b.anchor) {
            (BlockKind::Footnote { marker }, Some(a)) if marker.chars().all(|c| c.is_ascii_digit()) => {
                Some((marker.clone(), a.clone()))
            }
            _ => None,
        })
        .collect();

    for b in blocks.iter_mut() {
        let own = b.anchor.clone();
        let is_footnote = matches!(b.kind, BlockKind::Footnote { .. });
        let linkable = !matches!(
            b.kind,
            BlockKind::Equation { .. } | BlockKind::Title | BlockKind::Heading { .. }
        );
        if linkable {
            let spans = std::mem::take(&mut b.spans);
            b.spans = link_spans(spans, &anchors, own.as_deref());
            let rows = std::mem::take(&mut b.rows);
            b.rows = rows.into_iter().map(|r| link_spans(r, &anchors, None)).collect();
            for h in b.hlines.iter_mut() {
                let spans = std::mem::take(&mut h.spans);
                h.spans = link_spans(spans, &anchors, own.as_deref());
            }
            if let Some(t) = b.table.as_mut() {
                for cell in t.rows.iter_mut().flatten() {
                    let spans = std::mem::take(&mut cell.spans);
                    cell.spans = link_spans(spans, &anchors, None);
                }
            }
        }
        if !is_footnote {
            for i in 0..b.spans.len() {
                let s = &b.spans[i];
                if s.style & SUP != 0 && s.style & MATH == 0 && s.href.is_none() {
                    // A footnote mark follows a word or punctuation; a digit
                    // or a single letter before it means an exponent (x², c²).
                    let exponent = i == 0 || b.spans[i - 1].style & SUP != 0 || {
                        let prev = b.spans[i - 1].text.trim_end();
                        let run = prev.chars().rev().take_while(|c| c.is_alphabetic()).count();
                        match prev.chars().last() {
                            Some(c) if c.is_ascii_digit() => true,
                            Some(c) if c.is_alphabetic() => run < 2,
                            Some(_) => false,
                            None => true,
                        }
                    };
                    if let (Some(a), false) = (fn_markers.get(s.text.trim()), exponent) {
                        b.spans[i].href = Some(format!("#{a}"));
                    }
                }
            }
        }
    }

    if !title_parts.is_empty() {
        meta.title = Some(title_parts.join(" ").split_whitespace().collect::<Vec<_>>().join(" "));
    }
    let doc = Document {
        meta,
        pages: pages
            .iter()
            .map(|p| PageInfo {
                width: p.width,
                height: p.height,
            })
            .collect(),
        body_size: stats.body_size,
        leading: stats.leading,
        body_font: fonts
            .get(stats.body_font as usize)
            .map(|f| f.name.clone())
            .unwrap_or_default(),
        blocks,
    };
    (doc, loose)
}

fn slug_marker(m: &str) -> String {
    m.chars()
        .map(|c| match c {
            '∗' | '*' => "star".to_string(),
            '†' => "dagger".to_string(),
            '‡' => "ddagger".to_string(),
            '§' => "sect".to_string(),
            '¶' => "para".to_string(),
            c if c.is_alphanumeric() => c.to_string(),
            _ => "x".to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab(words: &[&str]) -> HashSet<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn soft_hyphens_are_removed() {
        let v = vocab(&["multi", "head", "multi-head", "self", "attention", "information"]);
        // Unknown fragments: a plain word break.
        assert!(remove_hyphen("dot-product (multi-", "plicative) attention", &v));
        // The joined word is known.
        assert!(remove_hyphen("more in-", "formation is needed", &v));
        // Both halves are words of their own and the compound is not known.
        assert!(!remove_hyphen("layers of self-", "attention are used", &v));
        // A compound seen elsewhere in the document keeps its hyphen.
        assert!(!remove_hyphen("we use multi-", "head attention", &v));
        // Capitalised or non-alphabetic continuations keep the hyphen.
        assert!(!remove_hyphen("the English-", "German task", &v));
        assert!(!remove_hyphen("pages 10-", "20 of the", &v));
        // Part of a longer hyphenated compound.
        assert!(!remove_hyphen("a state-of-", "the-art model", &v));
    }

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
