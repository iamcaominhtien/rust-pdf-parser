//! Classification of body blocks: headings, paragraphs, list items, references.

use super::patterns::*;
use super::*;
use crate::analysis::blocks::LIST_LABEL;
use crate::analysis::page::DocStats;
use crate::backend::FontInfo;
use crate::common::geom::median;

pub(crate) fn classify_body(nodes: &mut [Node], stats: &DocStats, fonts: &[FontInfo], _cfg: &SemanticsConfig) {
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
