//! Removal of page furniture: running heads and feet, page numbers, margin
//! line numbers and the arXiv margin stamp.
//!
//! Furniture is recognised by repetition across pages (the same text, with
//! digits ignored, in the head or foot band of several pages), by shape
//! (page numbers, columns of margin line numbers) and by position (outside
//! the block of running text). Removed lines are not thrown away: they are
//! returned per page so that the position-preserving renderers can still
//! draw them.
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

use super::page::PageData;
use crate::model::Line;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// A line that is nothing but a page number: arabic or roman, optionally
/// "page N", "N of M" or "N/M", with up to three decorating non-word
/// characters on each side ("- 3 -", "[iv]").
static PAGE_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\W{0,3}(page\s+)?(\d{1,4}|[ivxlc]{1,7})(\s*(of|/)\s*\d{1,4})?\W{0,3}$").unwrap()
});
/// An arXiv identifier as printed in the margin stamp, old style
/// (`hep-th/9901001`, `math.AG/0101001`) or new style (`1706.03762`), with
/// an optional version. Group 1 is the identifier, group 2 the version.
static ARXIV_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"arXiv:\s*([a-z\-]+(?:\.[A-Z]{2})?/\d{7}|\d{4}\.\d{4,5})(v\d+)?").unwrap());

/// Tuning parameters of furniture removal. Part of [`LayoutConfig`](crate::config::LayoutConfig).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FurnitureConfig {
    /// A line whose bottom edge is above this fraction of the page height
    /// is in the head band, where running heads are looked for.
    pub head_band_ratio: f32,
    /// A line whose top edge is below this fraction of the page height is
    /// in the foot band, where running feet and page numbers are looked for.
    pub foot_band_ratio: f32,
    /// Shortest normalised text (in bytes, see [`normalize_furniture`]) of
    /// a line that is counted as a possible running head. Shorter texts
    /// (a lone page number, "#") repeat by chance.
    pub repeat_min_text_bytes: usize,
    /// Documents with at most this many pages use
    /// [`short_doc_min_repeat_count`](Self::short_doc_min_repeat_count).
    pub short_doc_max_pages_count: usize,
    /// How many pages a head/foot line must appear on to be a running head
    /// in a short document.
    pub short_doc_min_repeat_count: usize,
    /// How many pages a head/foot line must appear on to be a running head
    /// in a longer document. Three rules out a title repeated on the next
    /// page by chance.
    pub min_repeat_count: usize,
    /// Lines with at least this many glyphs (and not repeated) make up the
    /// block of running text whose extent separates content from margins.
    pub body_line_min_glyphs_count: usize,
    /// Longest digit-only line that can be a margin line number.
    pub line_number_max_glyphs_count: usize,
    /// How far (in points) a margin line number may reach into the text
    /// block and still count as outside it.
    pub line_number_margin_pt: f32,
    /// Width (in points) of the bins in which margin line numbers are
    /// grouped by right edge; numbers of one column are right-aligned to
    /// within this.
    pub line_number_bin_pt: f32,
    /// Fewest numbers sharing a right edge for them to be a column of margin
    /// line numbers (review copies number every line, so there are many).
    pub line_number_min_count: usize,
    /// How far (in points) a candidate may overlap the top or bottom of the
    /// text block and still count as outside it.
    pub body_edge_tolerance_pt: f32,
    /// Longest line (in glyphs) that is taken for a page number.
    pub page_number_max_glyphs_count: usize,
    /// Baseline distance (in points) below which another line counts as on
    /// the same row as a candidate, and so does not crowd it.
    pub crowded_min_distance_pt: f32,
    /// Baseline distance (in font sizes of the larger line) below which a
    /// non-candidate line crowds a candidate, making it content: a normal
    /// line pitch is about 1.2 font sizes.
    pub crowded_max_distance_em: f32,
}

impl Default for FurnitureConfig {
    fn default() -> Self {
        Self {
            head_band_ratio: 0.11,
            foot_band_ratio: 0.89,
            repeat_min_text_bytes: 3,
            short_doc_max_pages_count: 5,
            short_doc_min_repeat_count: 2,
            min_repeat_count: 3,
            body_line_min_glyphs_count: 25,
            line_number_max_glyphs_count: 4,
            line_number_margin_pt: 2.0,
            line_number_bin_pt: 3.0,
            line_number_min_count: 8,
            body_edge_tolerance_pt: 1.0,
            page_number_max_glyphs_count: 12,
            crowded_min_distance_pt: 2.0,
            crowded_max_distance_em: 1.45,
        }
    }
}

/// Result of the furniture pass.
#[derive(Debug, Default)]
pub struct Furniture {
    /// arXiv identifier (with version) read from the margin stamp, if any.
    pub arxiv_id: Option<String>,
    /// Full text of the arXiv margin stamp, if any.
    pub arxiv_stamp: Option<String>,
    /// The removed lines of each page (they are still drawn by the
    /// position-preserving renderers).
    pub lines: Vec<Vec<Line>>,
}

/// The margin band of a page in which a line lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Band {
    /// Top of the page (running heads).
    Head,
    /// Bottom of the page (running feet, page numbers).
    Foot,
}

/// Key under which head/foot lines are counted across pages: the
/// normalised text and the band it was found in.
type RepeatKey = (String, Band);

/// How many pages each head/foot line occurs on, and how many it takes to
/// count as a running head.
struct RepeatCounts {
    /// Number of pages on which each key occurs (at most once per page).
    counts: HashMap<RepeatKey, usize>,
    /// Number of pages from which on a key is a running head or foot.
    min_repeat: usize,
}

impl RepeatCounts {
    /// Whether the line with text `text` in band `band` is repeated on
    /// enough pages to be a running head or foot.
    fn is_repeated(&self, text: &str, band: Band) -> bool {
        self.counts
            .get(&(normalize_furniture(text), band))
            .copied()
            .unwrap_or(0)
            >= self.min_repeat
    }
}

/// Extent of the running text of a page.
struct BodyExtent {
    /// Top of the highest body line.
    top: f32,
    /// Bottom of the lowest body line.
    bottom: f32,
    /// Left edge of the leftmost body line.
    left: f32,
    /// Right edge of the rightmost body line.
    right: f32,
}

/// Removes running headers/footers, page numbers, margin line numbers and the
/// arXiv margin stamp from the pages.
///
/// The removed lines of each page are returned in [`Furniture::lines`] (one
/// entry per page, in page order), together with the arXiv stamp and the
/// identifier read from it.
pub fn remove_furniture(pages: &mut [PageData], cfg: &FurnitureConfig) -> Furniture {
    let mut out = Furniture::default();
    find_arxiv_stamp(pages, &mut out);

    let n_pages = pages.len();
    let repeats = RepeatCounts {
        counts: count_head_foot_lines(pages, cfg),
        min_repeat: if n_pages <= cfg.short_doc_max_pages_count {
            cfg.short_doc_min_repeat_count
        } else {
            cfg.min_repeat_count
        },
    };
    for page in pages.iter_mut() {
        let removed = remove_page_furniture(page, &repeats, cfg);
        out.lines.push(removed);
    }
    out
}

/// Reduces a line's text to the form in which running heads are compared:
/// whitespace removed, lower case, and every run of digits replaced by one
/// `#` (so that "Page 3" and "Page 14" match).
fn normalize_furniture(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            if !out.ends_with('#') {
                out.push('#');
            }
        } else if !ch.is_whitespace() {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

/// Finds the arXiv stamp (rotated text in the left margin, usually of the
/// first page) and stores the first one found, and the identifier read from
/// it, in `out`.
fn find_arxiv_stamp(pages: &[PageData], out: &mut Furniture) {
    for page in pages.iter() {
        for rotated in &page.rotated {
            let compact: String = rotated.text.chars().filter(|ch| !ch.is_whitespace()).collect();
            if compact.starts_with("arXiv:") && out.arxiv_stamp.is_none() {
                out.arxiv_stamp = Some(rotated.text.clone());
                if let Some(caps) = ARXIV_ID.captures(&compact) {
                    out.arxiv_id = Some(format!(
                        "{}{}",
                        &caps[1],
                        caps.get(2).map(|version| version.as_str()).unwrap_or("")
                    ));
                }
            }
        }
    }
}

/// The margin band `line` lies in, or `None` when it is in the middle of the
/// page.
fn band(page: &PageData, line: &Line, cfg: &FurnitureConfig) -> Option<Band> {
    if line.bbox.y1 < cfg.head_band_ratio * page.height {
        Some(Band::Head)
    } else if line.bbox.y0 > cfg.foot_band_ratio * page.height {
        Some(Band::Foot)
    } else {
        None
    }
}

/// Counts, for every normalised head/foot text, on how many pages it occurs.
fn count_head_foot_lines(pages: &[PageData], cfg: &FurnitureConfig) -> HashMap<RepeatKey, usize> {
    let mut counts: HashMap<RepeatKey, usize> = HashMap::new();
    for page in pages.iter() {
        let mut seen = HashSet::new();
        for line in &page.lines {
            if let Some(line_band) = band(page, line, cfg) {
                let key = (normalize_furniture(&line.text()), line_band);
                if key.0.len() >= cfg.repeat_min_text_bytes && seen.insert(key.clone()) {
                    *counts.entry(key).or_default() += 1;
                }
            }
        }
    }
    counts
}

/// Removes the furniture lines of one page and returns them in their
/// original order.
fn remove_page_furniture(page: &mut PageData, repeats: &RepeatCounts, cfg: &FurnitureConfig) -> Vec<Line> {
    let body = body_extent(page, repeats, cfg);
    let mut drop = margin_line_numbers(page, &body, cfg);
    mark_running_heads(page, &body, repeats, cfg, &mut drop);

    let mut kept = Vec::with_capacity(page.lines.len());
    let mut removed = Vec::new();
    for (i, line) in std::mem::take(&mut page.lines).into_iter().enumerate() {
        if drop[i] {
            removed.push(line);
        } else {
            kept.push(line);
        }
    }
    page.lines = kept;
    removed
}

/// Extent of the main text: the long lines that are not running heads. When
/// there are none, the whole page.
fn body_extent(page: &PageData, repeats: &RepeatCounts, cfg: &FurnitureConfig) -> BodyExtent {
    let is_repeated =
        |line: &Line| band(page, line, cfg).is_some_and(|line_band| repeats.is_repeated(&line.text(), line_band));
    let body: Vec<&Line> = page
        .lines
        .iter()
        .filter(|line| line.char_count() >= cfg.body_line_min_glyphs_count && !is_repeated(line))
        .collect();
    let (top, bottom) = if body.is_empty() {
        (0.0, page.height)
    } else {
        (
            body.iter().map(|line| line.bbox.y0).fold(f32::INFINITY, f32::min),
            body.iter().map(|line| line.bbox.y1).fold(f32::NEG_INFINITY, f32::max),
        )
    };
    let (left, right) = if body.is_empty() {
        (0.0, page.width)
    } else {
        (
            body.iter().map(|line| line.bbox.x0).fold(f32::INFINITY, f32::min),
            body.iter().map(|line| line.bbox.x1).fold(f32::NEG_INFINITY, f32::max),
        )
    };
    BodyExtent {
        top,
        bottom,
        left,
        right,
    }
}

/// Margin line numbers (review mode): many short numeric lines sharing a
/// right edge outside the text block. Returns one flag per line of the page,
/// set for the line numbers.
fn margin_line_numbers(page: &PageData, body: &BodyExtent, cfg: &FurnitureConfig) -> Vec<bool> {
    let mut numeric: HashMap<i32, Vec<usize>> = HashMap::new();
    for (i, line) in page.lines.iter().enumerate() {
        if line.char_count() <= cfg.line_number_max_glyphs_count
            && line
                .glyphs
                .iter()
                .all(|glyph| glyph.text.chars().all(|ch| ch.is_ascii_digit()))
            && (line.bbox.x1 < body.left + cfg.line_number_margin_pt
                || line.bbox.x0 > body.right - cfg.line_number_margin_pt)
        {
            numeric
                .entry((line.bbox.x1 / cfg.line_number_bin_pt).round() as i32)
                .or_default()
                .push(i);
        }
    }
    let mut drop: Vec<bool> = vec![false; page.lines.len()];
    for idxs in numeric.values() {
        if idxs.len() >= cfg.line_number_min_count {
            for &i in idxs {
                drop[i] = true;
            }
        }
    }
    drop
}

/// Sets the `drop` flag of the running heads, running feet and page numbers
/// of the page: lines in a margin band, above or below the text block, that
/// are repeated or look like a page number, and that stand apart from other
/// text.
fn mark_running_heads(
    page: &PageData,
    body: &BodyExtent,
    repeats: &RepeatCounts,
    cfg: &FurnitureConfig,
    drop: &mut [bool],
) {
    let candidate: Vec<bool> = page
        .lines
        .iter()
        .map(|line| is_furniture_candidate(page, line, body, repeats, cfg))
        .collect();
    for (i, line) in page.lines.iter().enumerate() {
        if !candidate[i] {
            continue;
        }
        // Running heads stand apart from the content. A repeated line
        // with other text right above or below it is content itself
        // (for instance the header row of a table at the top of a page).
        let crowded = page.lines.iter().enumerate().any(|(j, other)| {
            let distance = (other.baseline - line.baseline).abs();
            !candidate[j]
                && distance > cfg.crowded_min_distance_pt
                && distance < cfg.crowded_max_distance_em * line.size.max(other.size)
        });
        if !crowded {
            drop[i] = true;
        }
    }
}

/// Whether `line` is a candidate for removal: a repeated line or a page
/// number in a margin band, outside the text block.
fn is_furniture_candidate(
    page: &PageData,
    line: &Line,
    body: &BodyExtent,
    repeats: &RepeatCounts,
    cfg: &FurnitureConfig,
) -> bool {
    let Some(line_band) = band(page, line, cfg) else {
        return false;
    };
    let text = line.text();
    let repeated = repeats.is_repeated(&text, line_band);
    let outside = line.bbox.y1 <= body.top + cfg.body_edge_tolerance_pt
        || line.bbox.y0 >= body.bottom - cfg.body_edge_tolerance_pt;
    let is_page_no = PAGE_NUMBER.is_match(text.trim()) && line.char_count() <= cfg.page_number_max_glyphs_count;
    (repeated || is_page_no) && outside
}
