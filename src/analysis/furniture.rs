//! Removal of page furniture: running heads and feet, page numbers, margin
//! line numbers and the arXiv margin stamp.

use super::page::PageData;
use crate::model::Line;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

static PAGE_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\W{0,3}(page\s+)?(\d{1,4}|[ivxlc]{1,7})(\s*(of|/)\s*\d{1,4})?\W{0,3}$").unwrap()
});
static ARXIV_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"arXiv:\s*([a-z\-]+(?:\.[A-Z]{2})?/\d{7}|\d{4}\.\d{4,5})(v\d+)?").unwrap());

fn normalize_furniture(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            if !out.ends_with('#') {
                out.push('#');
            }
        } else if !c.is_whitespace() {
            out.extend(c.to_lowercase());
        }
    }
    out
}

/// Result of the furniture pass.
#[derive(Debug, Default)]
pub struct Furniture {
    pub arxiv_id: Option<String>,
    pub arxiv_stamp: Option<String>,
    /// The removed lines of each page (they are still drawn by the
    /// position-preserving renderers).
    pub lines: Vec<Vec<Line>>,
}

/// Removes running headers/footers, page numbers, margin line numbers and the
/// arXiv margin stamp from the pages.
pub fn remove_furniture(pages: &mut [PageData]) -> Furniture {
    let mut out = Furniture::default();

    // arXiv stamp: rotated text in the left margin of the first page.
    for p in pages.iter_mut() {
        for r in &p.rotated {
            let compact: String = r.text.chars().filter(|c| !c.is_whitespace()).collect();
            if compact.starts_with("arXiv:") && out.arxiv_stamp.is_none() {
                out.arxiv_stamp = Some(r.text.clone());
                if let Some(c) = ARXIV_ID.captures(&compact) {
                    out.arxiv_id = Some(format!("{}{}", &c[1], c.get(2).map(|m| m.as_str()).unwrap_or("")));
                }
            }
        }
    }

    let n_pages = pages.len();
    // Repeated header / footer lines.
    let mut counts: HashMap<(String, i32), usize> = HashMap::new();
    let band = |p: &PageData, l: &Line| -> Option<i32> {
        if l.bbox.y1 < 0.11 * p.height {
            Some(0)
        } else if l.bbox.y0 > 0.89 * p.height {
            Some(1)
        } else {
            None
        }
    };
    for p in pages.iter() {
        let mut seen = std::collections::HashSet::new();
        for l in &p.lines {
            if let Some(b) = band(p, l) {
                let key = (normalize_furniture(&l.text()), b);
                if key.0.len() >= 3 && seen.insert(key.clone()) {
                    *counts.entry(key).or_default() += 1;
                }
            }
        }
    }
    let min_repeat = if n_pages <= 5 { 2 } else { 3 };

    for p in pages.iter_mut() {
        let h = p.height;
        let w = p.width;
        // Extent of the main text: lines of body size.
        let is_repeated = |l: &Line| {
            band(p, l)
                .is_some_and(|b| counts.get(&(normalize_furniture(&l.text()), b)).copied().unwrap_or(0) >= min_repeat)
        };
        let body: Vec<&Line> = p
            .lines
            .iter()
            .filter(|l| l.char_count() >= 25 && !is_repeated(l))
            .collect();
        let (body_top, body_bottom) = if body.is_empty() {
            (0.0, h)
        } else {
            (
                body.iter().map(|l| l.bbox.y0).fold(f32::INFINITY, f32::min),
                body.iter().map(|l| l.bbox.y1).fold(f32::NEG_INFINITY, f32::max),
            )
        };
        let (body_left, body_right) = if body.is_empty() {
            (0.0, w)
        } else {
            (
                body.iter().map(|l| l.bbox.x0).fold(f32::INFINITY, f32::min),
                body.iter().map(|l| l.bbox.x1).fold(f32::NEG_INFINITY, f32::max),
            )
        };

        // Margin line numbers (review mode): many short numeric lines sharing
        // a right edge outside the text block.
        let mut numeric: HashMap<i32, Vec<usize>> = HashMap::new();
        for (i, l) in p.lines.iter().enumerate() {
            if l.char_count() <= 4
                && l.glyphs.iter().all(|g| g.text.chars().all(|c| c.is_ascii_digit()))
                && (l.bbox.x1 < body_left + 2.0 || l.bbox.x0 > body_right - 2.0)
            {
                numeric.entry((l.bbox.x1 / 3.0).round() as i32).or_default().push(i);
            }
        }
        let mut drop: Vec<bool> = vec![false; p.lines.len()];
        for idxs in numeric.values() {
            if idxs.len() >= 8 {
                for &i in idxs {
                    drop[i] = true;
                }
            }
        }

        // Candidates: repeated lines and page numbers in the margins.
        let candidate: Vec<bool> = p
            .lines
            .iter()
            .map(|l| {
                let Some(b) = band(p, l) else { return false };
                let text = l.text();
                let repeated = counts.get(&(normalize_furniture(&text), b)).copied().unwrap_or(0) >= min_repeat;
                let outside = l.bbox.y1 <= body_top + 1.0 || l.bbox.y0 >= body_bottom - 1.0;
                let is_page_no = PAGE_NUMBER.is_match(text.trim()) && l.char_count() <= 12;
                (repeated || is_page_no) && outside
            })
            .collect();
        for (i, l) in p.lines.iter().enumerate() {
            if !candidate[i] {
                continue;
            }
            // Running heads stand apart from the content. A repeated line
            // with other text right above or below it is content itself
            // (for instance the header row of a table at the top of a page).
            let crowded = p.lines.iter().enumerate().any(|(j, o)| {
                let d = (o.baseline - l.baseline).abs();
                !candidate[j] && d > 2.0 && d < 1.45 * l.size.max(o.size)
            });
            if !crowded {
                drop[i] = true;
            }
        }
        let mut kept = Vec::with_capacity(p.lines.len());
        let mut removed = Vec::new();
        for (i, l) in std::mem::take(&mut p.lines).into_iter().enumerate() {
            if drop[i] {
                removed.push(l);
            } else {
                kept.push(l);
            }
        }
        p.lines = kept;
        out.lines.push(removed);
    }
    out
}
