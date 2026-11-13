//! Code in the friendly mode: listings (monospaced text set as a block, with
//! optional syntax colour) and algorithms rebuilt as text from the text
//! layer of their float.

mod highlight;

use super::*;
use crate::model::{Line, MONO, Span};
use highlight::{Carry, Lang, detect, highlight_line};

/// Width of a monospaced character in em, for fonts whose glyph widths
/// cannot be measured (Courier and most code fonts).
const MONO_ADVANCE_EM: f32 = 0.6;
/// A gap of more than this many line pitches between two lines of code is
/// an empty line of the listing.
const BLANK_LINE_MIN_PITCHES: f32 = 1.6;
/// Line pitch assumed for a listing of one line, in em of its font size.
const LONE_LINE_PITCH_EM: f32 = 1.2;
/// Listings with at least this many lines get line numbers.
const NUMBERED_MIN_LINES: usize = 5;
/// Fewest lines of an algorithm rebuilt as text.
const ALGORITHM_MIN_LINES: usize = 3;
/// Lines whose x positions differ by less than this (points) are at the
/// same indentation level.
const INDENT_TOLERANCE_PT: f32 = 2.0;
/// Deepest indentation class of an algorithm line (`in3`).
const ALGORITHM_MAX_INDENT: usize = 3;
/// A line of an algorithm with at most this many characters (a radical
/// sign drawn apart) is merged into the line beside it.
const ORPHAN_MAX_CHARS: usize = 2;
/// Words that start the first line of an algorithm written as pseudocode.
const ALGORITHM_OPENERS: [&str; 12] = [
    "Require",
    "Ensure",
    "Input",
    "Output",
    "Initialize",
    "Initialise",
    "procedure",
    "function",
    "for",
    "while",
    "repeat",
    "Data",
];
/// Words of the unnumbered header lines of an algorithm (inputs, outputs).
const ALGORITHM_HEADERS: [&str; 5] = ["Require", "Ensure", "Input", "Output", "Data"];

/// Whether `block` is a paragraph set entirely in a monospaced font (a
/// piece of code), apart from spaces.
pub(crate) fn is_code_paragraph(block: &Block) -> bool {
    matches!(block.kind, BlockKind::Paragraph { in_abstract: false })
        && block.spans.iter().any(|span| !span.text.trim().is_empty())
        && block
            .spans
            .iter()
            .all(|span| span.style & MONO != 0 || span.text.trim().is_empty())
        && block.hlines.len() == block.lines.len()
}

/// The lines of code of consecutive code paragraphs, with their
/// indentation rebuilt from the positions of the lines. An empty line is
/// kept where the gap between two paragraphs is more than
/// [`BLANK_LINE_MIN_PITCHES`] line pitches.
pub(crate) fn listing_lines(blocks: &[&Block]) -> Vec<String> {
    let base = blocks
        .iter()
        .flat_map(|block| block.lines.iter())
        .map(|line| line.bbox.x0)
        .fold(f32::INFINITY, f32::min);
    let pitch = line_pitch(blocks);
    let mut lines = Vec::new();
    let mut previous: Option<&Line> = None;
    for block in blocks {
        for (line, hline) in block.lines.iter().zip(&block.hlines) {
            if let Some(prev) = previous
                && prev.page == line.page
                && line.baseline - prev.baseline > BLANK_LINE_MIN_PITCHES * pitch
            {
                lines.push(String::new());
            }
            let mut text: String = hline.spans.iter().map(|span| span.text.as_str()).collect();
            if text.ends_with(' ') {
                text.pop();
            }
            if hline.soft_hyphen {
                text.push('-');
            }
            let indent = ((line.bbox.x0 - base) / char_width(line)).round().max(0.0) as usize;
            lines.push(format!("{}{}", " ".repeat(indent), text.trim_start()));
            previous = Some(line);
        }
    }
    lines
}

/// The usual distance between consecutive baselines of code lines (the
/// smallest one on a page), or 1.2 times the font size when there is a
/// single line.
fn line_pitch(blocks: &[&Block]) -> f32 {
    let lines: Vec<&Line> = blocks.iter().flat_map(|block| block.lines.iter()).collect();
    lines
        .windows(2)
        .filter(|pair| pair[0].page == pair[1].page)
        .map(|pair| pair[1].baseline - pair[0].baseline)
        .filter(|pitch| *pitch > 0.0)
        .fold(f32::INFINITY, f32::min)
        .min(
            lines
                .first()
                .map_or(f32::INFINITY, |line| LONE_LINE_PITCH_EM * line.size),
        )
}

/// The lines of a float whose text is all monospaced (a listing set as a
/// float), with their indentation, or `None` when some of its text is not
/// monospaced.
pub(crate) fn float_listing_lines(block: &Block) -> Option<Vec<String>> {
    let spans = &block.inner_spans;
    let mono = spans
        .iter()
        .flatten()
        .all(|span| span.style & MONO != 0 || span.text.trim().is_empty());
    if block.inner_lines.len() < 2 || !mono {
        return None;
    }
    let base = block
        .inner_lines
        .iter()
        .map(|line| line.bbox.x0)
        .fold(f32::INFINITY, f32::min);
    Some(
        block
            .inner_lines
            .iter()
            .zip(spans)
            .map(|(line, line_spans)| {
                let text: String = line_spans.iter().map(|span| span.text.as_str()).collect();
                let indent = ((line.bbox.x0 - base) / char_width(line)).round().max(0.0) as usize;
                format!("{}{}", " ".repeat(indent), text.trim())
            })
            .collect(),
    )
}

/// The advance of one character of a monospaced line, in points.
fn char_width(line: &Line) -> f32 {
    let mut widths: Vec<f32> = line
        .glyphs
        .iter()
        .filter(|glyph| glyph.text.chars().count() == 1)
        .map(|glyph| glyph.x1 - glyph.x0)
        .filter(|width| *width > 0.0)
        .collect();
    widths.sort_by(f32::total_cmp);
    widths
        .get(widths.len() / 2)
        .copied()
        .unwrap_or(MONO_ADVANCE_EM * line.size.max(1.0))
}

/// A code listing: an optional caption, the Colour and Copy buttons, and
/// the lines in a `<pre class="listing">`, coloured when the language was
/// recognised with confidence (otherwise only strings, comments and
/// numbers are marked, and shown plain unless the reader asks for colour).
pub(crate) fn listing_html(lines: &[String], caption: &str, id: &str) -> String {
    let code = lines.join("\n");
    let (lang, confident) = detect(&code);
    let lang = if confident { lang } else { Lang::Generic };
    let mut carry = Carry::None;
    let body: String = lines
        .iter()
        .map(|line| format!("<span class=\"l\">{}</span>", highlight_line(line, lang, &mut carry)))
        .collect();
    let mut classes = String::from("listing");
    if lines.len() >= NUMBERED_MIN_LINES {
        classes.push_str(" numbered");
    }
    if confident {
        classes.push_str(" hl");
    }
    let caption = if caption.is_empty() {
        String::new()
    } else {
        format!("<figcaption>{caption}</figcaption>")
    };
    format!(
        "<figure class=\"code\"{id}><div class=\"code-head\">{caption}<div class=\"code-tools\">\
<button class=\"btn code-hl\" type=\"button\" aria-pressed=\"{confident}\" aria-label=\"Colour code\">Colour</button>\
<button class=\"btn code-copy\" type=\"button\" aria-label=\"Copy code\"><span class=\"cp\">Copy</span><span class=\"ok\">Copied</span></button>\
</div></div><div class=\"code-box\"><pre class=\"{classes}\" tabindex=\"0\" data-lang=\"{}\" translate=\"no\"><code>{body}</code></pre></div></figure>\n",
        lang_name(lang)
    )
}

/// The name of a language for the `data-lang` attribute.
fn lang_name(lang: Lang) -> &'static str {
    match lang {
        Lang::Python => "python",
        Lang::CFamily => "c",
        Lang::JavaScript => "javascript",
        Lang::Go => "go",
        Lang::Rust => "rust",
        Lang::Generic => "text",
    }
}

/// One line of an algorithm float.
struct AlgoLine {
    /// Styled text of the line.
    spans: Vec<Span>,
    /// The source line (position and glyphs).
    line: Line,
}

/// The algorithm of a float rebuilt as text (`<div class="algo">`), when
/// its text layer reads as pseudocode: at least [`ALGORITHM_MIN_LINES`]
/// lines, the first one starting with one of [`ALGORITHM_OPENERS`] or a
/// line number. `None` otherwise (the float keeps its picture).
pub(crate) fn algorithm_html(block: &Block) -> Option<String> {
    let mut lines: Vec<AlgoLine> = block
        .inner_lines
        .iter()
        .zip(&block.inner_spans)
        .map(|(line, spans)| AlgoLine {
            spans: spans.clone(),
            line: line.clone(),
        })
        .collect();
    merge_orphans(&mut lines);
    lines.retain(|algo| !plain(&algo.spans).is_empty());
    if lines.len() < ALGORITHM_MIN_LINES || !opens_algorithm(&plain(&lines[0].spans)) {
        return None;
    }
    let levels = indent_levels(&lines);
    let mut out = String::from("<div class=\"algo\">");
    let mut number = 0;
    for (algo, level) in lines.iter().zip(levels) {
        let text = plain(&algo.spans);
        let header = ALGORITHM_HEADERS.iter().any(|word| text.starts_with(word));
        let (statement, comment) = split_comment(&algo.spans, &text);
        let num = if header {
            String::new()
        } else {
            number += 1;
            number.to_string()
        };
        let cmt = comment
            .map(|spans| format!("<span class=\"cmt\">{}</span>", spans_html(&spans)))
            .unwrap_or_default();
        out.push_str(&format!(
            "<div class=\"ln{}\"><i>{num}</i><div class=\"st in{level}\"><span>{}</span>{cmt}</div></div>",
            if header { " req" } else { "" },
            spans_html(&statement)
        ));
    }
    out.push_str("</div>");
    Some(out)
}

/// Whether the first line of a float's text opens an algorithm.
fn opens_algorithm(first: &str) -> bool {
    let word: String = first.chars().take_while(|ch| ch.is_alphanumeric()).collect();
    ALGORITHM_OPENERS.contains(&word.as_str()) || first.split_once([':', '.']).is_some_and(|(num, _)| num == "1")
}

/// Merges lines of one or two characters (a radical sign set apart from
/// its line) into the line with the nearest baseline, at the position of
/// their glyphs.
fn merge_orphans(lines: &mut Vec<AlgoLine>) {
    let mut idx = 0;
    while idx < lines.len() {
        let text = plain(&lines[idx].spans);
        let orphan = !text.is_empty() && text.chars().count() <= ORPHAN_MAX_CHARS && lines.len() > 1;
        let host = orphan
            .then(|| nearest_line(lines, idx))
            .flatten()
            .filter(|&host| (lines[host].line.baseline - lines[idx].line.baseline).abs() < lines[idx].line.size);
        match host {
            Some(host) => {
                let x0 = lines[idx].line.bbox.x0;
                let at = char_offset_at(&lines[host].line, x0);
                insert_text(&mut lines[host].spans, at, &text);
                lines.remove(idx);
            }
            None => idx += 1,
        }
    }
}

/// The other line whose baseline is closest to that of `lines[idx]`.
fn nearest_line(lines: &[AlgoLine], idx: usize) -> Option<usize> {
    let baseline = lines[idx].line.baseline;
    (0..lines.len()).filter(|&other| other != idx).min_by(|&a, &b| {
        let da = (lines[a].line.baseline - baseline).abs();
        let db = (lines[b].line.baseline - baseline).abs();
        da.total_cmp(&db)
    })
}

/// The character offset in the text of `line` (glyphs joined with a space
/// where a glyph has `space_before`) of the first glyph that starts at or
/// right of `x`.
fn char_offset_at(line: &Line, x: f32) -> usize {
    let mut offset = 0;
    for (glyph_idx, glyph) in line.glyphs.iter().enumerate() {
        if glyph.space_before && glyph_idx > 0 {
            offset += 1;
        }
        if glyph.x0 >= x {
            return offset;
        }
        offset += glyph.text.chars().count();
    }
    offset
}

/// Inserts `text` into `spans` at character offset `at` (at the end when
/// the offset is past it), in the style of the span it falls in.
fn insert_text(spans: &mut [Span], at: usize, text: &str) {
    let mut start = 0;
    let count = spans.len();
    for (idx, span) in spans.iter_mut().enumerate() {
        let len = span.text.chars().count();
        if at <= start + len || idx + 1 == count {
            let byte = span
                .text
                .char_indices()
                .nth(at.saturating_sub(start))
                .map_or(span.text.len(), |(byte, _)| byte);
            span.text.insert_str(byte, text);
            return;
        }
        start += len;
    }
}

/// Splits `spans` at character offset `at`.
fn split_spans(spans: &[Span], at: usize) -> (Vec<Span>, Vec<Span>) {
    let (mut head, mut tail) = (Vec::new(), Vec::new());
    let mut start = 0;
    for span in spans {
        let len = span.text.chars().count();
        if start + len <= at {
            head.push(span.clone());
        } else if start >= at {
            tail.push(span.clone());
        } else {
            let byte = span
                .text
                .char_indices()
                .nth(at - start)
                .map_or(span.text.len(), |(byte, _)| byte);
            let mut first = span.clone();
            first.text.truncate(byte);
            let mut second = span.clone();
            second.text = span.text[byte..].to_string();
            head.push(first);
            tail.push(second);
        }
        start += len;
    }
    (head, tail)
}

/// Splits off a trailing comment: `▷ text`, `// text`, or a parenthesised
/// phrase starting with a capital letter after a space ("(Update
/// parameters)"). Returns the statement and the comment, if any.
fn split_comment(spans: &[Span], text: &str) -> (Vec<Span>, Option<Vec<Span>>) {
    let chars: Vec<char> = text.chars().collect();
    let marker = chars
        .iter()
        .position(|ch| *ch == '▷')
        .or_else(|| text.find(" // ").map(|byte| text[..byte].chars().count() + 1));
    let at = marker.or_else(|| trailing_parenthetical(&chars));
    match at {
        Some(at) if at > 0 => {
            // `plain` trims the start: account for leading spaces.
            let lead = spans.iter().map(|span| span.text.as_str()).collect::<String>();
            let skipped = lead.chars().take_while(|ch| ch.is_whitespace()).count();
            let (statement, comment) = split_spans(spans, at + skipped);
            (statement, Some(comment))
        }
        _ => (spans.to_vec(), None),
    }
}

/// The start of a final "(Capitalised words)" group preceded by a space.
fn trailing_parenthetical(chars: &[char]) -> Option<usize> {
    if chars.last() != Some(&')') {
        return None;
    }
    let mut depth = 0;
    for idx in (0..chars.len()).rev() {
        match chars[idx] {
            ')' => depth += 1,
            '(' => {
                depth -= 1;
                if depth == 0 {
                    let words = chars.get(idx + 1).is_some_and(|ch| ch.is_uppercase()) && chars[idx..].contains(&' ');
                    let spaced = idx > 0 && chars[idx - 1] == ' ';
                    return (words && spaced).then_some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

/// The indentation level of each line (0 to [`ALGORITHM_MAX_INDENT`]):
/// the rank of its left edge among the distinct left edges of the
/// statements; header lines are level 0.
fn indent_levels(lines: &[AlgoLine]) -> Vec<usize> {
    let is_header = |algo: &AlgoLine| {
        ALGORITHM_HEADERS
            .iter()
            .any(|word| plain(&algo.spans).starts_with(word))
    };
    let mut edges: Vec<f32> = lines
        .iter()
        .filter(|algo| !is_header(algo))
        .map(|algo| algo.line.bbox.x0)
        .collect();
    edges.sort_by(f32::total_cmp);
    edges.dedup_by(|a, b| (*a - *b).abs() < INDENT_TOLERANCE_PT);
    lines
        .iter()
        .map(|algo| {
            if is_header(algo) {
                return 0;
            }
            let x0 = algo.line.bbox.x0;
            edges
                .iter()
                .take_while(|edge| **edge < x0 - INDENT_TOLERANCE_PT)
                .count()
                .min(ALGORITHM_MAX_INDENT)
        })
        .collect()
}
