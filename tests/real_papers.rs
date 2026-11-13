//! Regression tests on real arXiv papers.
//!
//! The PDFs are listed (with pinned versions and checksums) in
//! `tests/data/papers.txt`. Tests never download anything: papers that are
//! not in `tests/data` are skipped with a note, unless
//! `P2H_REQUIRE_ALL_PAPERS=1` is set (CI), in which case a missing paper is
//! an error. `scripts/fetch-test-pdfs.sh` downloads them once.
//!
//! The expected values are the results of the current code. Counts of things
//! that should not happen (broken paragraphs, glyphs left in page pictures)
//! are upper bounds and counts of things that should happen (tables rebuilt)
//! are lower bounds, so an improvement passes and a regression fails. When a
//! change improves a number, tighten the bound here. Set
//! `P2H_PRINT_METRICS=1` to print the measured values.
//!
//! On top of these bounds, every output is compared with a snapshot in
//! `tests/data/snapshots`: a fingerprint (sha256) of the HTML of each mode and
//! of the JSON, so that any change in the output, however small, is noticed.
//! A restructuring of the code must leave them unchanged. When a change of
//! output is intended, regenerate them with
//! `P2H_UPDATE_SNAPSHOTS=1 cargo test --test real_papers` and commit them; the
//! diff then shows which papers and modes changed. For papers whose licence
//! allows it the full JSON is kept too, so `git diff` shows what changed.

use paper2html::model::{BlockKind, FloatKind, Span};
use paper2html::{AnalyzeOptions, HtmlOptions, Mode, Paper};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

struct Expect {
    id: &'static str,
    /// Text the detected title must contain.
    title: &'static str,
    min_tables: usize,
    max_broken_paragraphs: usize,
    max_split_words: usize,
    max_glyphs_left: usize,
    max_hyphenated: usize,
}

#[derive(Debug)]
struct Metrics {
    title: String,
    tables: usize,
    broken_paragraphs: usize,
    split_words: usize,
    glyphs_left: usize,
    hyphenated: usize,
}

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("data")
}

/// Loads a test paper, or returns `None` (with a note) if it has not been
/// downloaded and downloads are not required.
fn load(id: &str) -> Option<Paper> {
    let path = data_dir().join(format!("{id}.pdf"));
    match std::fs::read(&path) {
        Ok(data) => Some(Paper::from_bytes(data).expect("valid PDF")),
        Err(_) if std::env::var("P2H_REQUIRE_ALL_PAPERS").is_ok_and(|v| v == "1") => {
            panic!("{} is missing: run scripts/fetch-test-pdfs.sh", path.display())
        }
        Err(_) => {
            eprintln!("skipping {id}: not downloaded (run scripts/fetch-test-pdfs.sh)");
            None
        }
    }
}

/// Visible characters of the text inside figures in a hard-95 page: the
/// HTML copy of glyphs that stay in the page pictures.
fn figure_text_chars(html: &str) -> usize {
    let tag = Regex::new(r"<[^>]*>").unwrap();
    html.split("data-kind=\"figure-text\"")
        .skip(1)
        .map(|rest| {
            let body = &rest[rest.find('>').map_or(0, |i| i + 1)..];
            let body = &body[..body.find("</div>").unwrap_or(body.len())];
            tag.replace_all(body, "").chars().filter(|c| !c.is_whitespace()).count()
        })
        .sum()
}

fn plain(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| s.text.as_str())
        .collect::<String>()
        .trim()
        .to_string()
}

/// Papers whose licence allows the full JSON output to be kept in the
/// repository (CC BY 4.0), so that a change shows up in `git diff`.
const FULL_JSON: &[&str] = &["1602.03837v1"];

/// Fingerprints of the outputs of one paper: (mode, sha256), plus the JSON.
struct Snapshot {
    fingerprints: Vec<(&'static str, String)>,
    json: String,
}

fn hex(digest: impl AsRef<[u8]>) -> String {
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// Fingerprint of a rendered mode: the HTML and every file it refers to,
/// except the pixels of images. Those can differ slightly between CPUs (the
/// image decoders and the rasteriser use SIMD code chosen at run time), so
/// only the size of a PNG and the vector content of an SVG count.
fn fingerprint(html: &str, files: &[(String, Vec<u8>)]) -> String {
    let embedded = regex::bytes::Regex::new(r"data:image/[a-z+]+;base64,[A-Za-z0-9+/=]*").unwrap();
    let mut h = Sha256::new();
    h.update(html.as_bytes());
    for (name, data) in files {
        h.update(name.as_bytes());
        if name.ends_with(".svg") {
            h.update(embedded.replace_all(data, &b"data:image"[..]));
        } else if data.starts_with(b"\x89PNG") && data.len() >= 24 {
            h.update(&data[16..24]); // width and height
        }
    }
    hex(h.finalize())
}

/// Compares the outputs of a paper with its stored snapshot, or stores them
/// when `P2H_UPDATE_SNAPSHOTS=1`.
fn check_snapshot(id: &str, snap: &Snapshot) {
    let dir = data_dir().join("snapshots");
    let file = dir.join(format!("{id}.txt"));
    let json_file = dir.join(format!("{id}.json"));
    let keep_json = FULL_JSON.contains(&id);
    let text: String = snap.fingerprints.iter().map(|(m, h)| format!("{m} {h}\n")).collect();

    if std::env::var("P2H_UPDATE_SNAPSHOTS").is_ok_and(|v| v == "1") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&file, &text).unwrap();
        if keep_json {
            std::fs::write(&json_file, &snap.json).unwrap();
        }
        return;
    }

    let old = std::fs::read_to_string(&file).unwrap_or_else(|_| {
        panic!(
            "{id}: no snapshot at {}; create it with P2H_UPDATE_SNAPSHOTS=1 cargo test --test real_papers",
            file.display()
        )
    });
    if old == text {
        return;
    }
    let changed: Vec<&str> = snap
        .fingerprints
        .iter()
        .filter(|(m, h)| !old.lines().any(|l| l == format!("{m} {h}")))
        .map(|(m, _)| *m)
        .collect();
    let mut msg = format!(
        "{id}: the output changed ({}).\n\
         If the change is intended, run P2H_UPDATE_SNAPSHOTS=1 cargo test --test real_papers \
         and commit tests/data/snapshots.",
        changed.join(", ")
    );
    if keep_json && let Ok(old_json) = std::fs::read_to_string(&json_file) {
        let (a, b): (Vec<&str>, Vec<&str>) = (old_json.lines().collect(), snap.json.lines().collect());
        if let Some(i) = (0..a.len().max(b.len())).find(|&i| a.get(i) != b.get(i)) {
            msg.push_str(&format!("\nFirst difference in the JSON, line {}:", i + 1));
            for l in a.iter().skip(i).take(6) {
                msg.push_str(&format!("\n  - {l}"));
            }
            for l in b.iter().skip(i).take(6) {
                msg.push_str(&format!("\n  + {l}"));
            }
        }
    }
    panic!("{msg}");
}

/// Analyses a paper, renders every mode and measures it.
fn measure(paper: &Paper) -> (Metrics, Snapshot) {
    let analysis = paper.analyze();
    let doc = &analysis.doc;

    let title = doc
        .blocks
        .iter()
        .find(|b| matches!(b.kind, BlockKind::Title))
        .map(|b| plain(&b.spans))
        .unwrap_or_default();
    let tables = doc.blocks.iter().filter(|b| b.table.is_some()).count();

    // A paragraph that stops mid-sentence and is followed by one that
    // continues it: the text was split where it should not have been.
    let paragraphs: Vec<String> = doc
        .blocks
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::Paragraph { .. }))
        .map(|b| plain(&b.spans))
        .collect();
    let broken_paragraphs = paragraphs
        .windows(2)
        .filter(|w| {
            let ends_open = !w[0].ends_with(['.', '!', '?', ':', ';', ')', ']', '"', '\u{201d}']);
            let continues = w[1].chars().next().is_some_and(|c| c.is_lowercase());
            ends_open && continues
        })
        .count();
    // Words still split by a line-end hyphen ("trans- lation"), leaving out
    // shared prefixes such as "first- and second-order".
    let split = Regex::new(r"\b[a-z]{2,}- ([a-z]{2,})\b").unwrap();
    let split_words = paragraphs
        .iter()
        .flat_map(|p| split.captures_iter(p))
        .filter(|c| !matches!(&c[1], "and" | "or" | "to"))
        .count();

    // Lower-case words joined by a hyphen ("two-stage", "trans-lation"). Real
    // compounds keep this count stable; a word broken at the end of a line
    // and not joined back raises it.
    let compound = Regex::new(r"\b[a-z]{2,}-[a-z]{2,}\b").unwrap();
    let hyphenated = paragraphs.iter().map(|p| compound.find_iter(p).count()).sum();

    let json = serde_json::to_string_pretty(doc).expect("serialisable document");
    let mut fingerprints = Vec::new();
    let mut glyphs_left = 0;
    for mode in [Mode::Hard, Mode::Hard95, Mode::Medium, Mode::Friendly] {
        let out = paper.to_html(
            &analysis,
            &HtmlOptions {
                mode,
                inline_images: false,
                ..Default::default()
            },
        );
        assert!(out.html.contains("</html>"), "{mode:?}: incomplete document");
        assert!(
            !out.html.contains('\u{1}'),
            "{mode:?}: image placeholder left in the HTML"
        );
        for (name, data) in &out.files {
            assert!(out.html.contains(name.as_str()), "{mode:?}: {name} is not referenced");
            assert!(!data.is_empty(), "{mode:?}: {name} is empty");
        }
        fingerprints.push((mode.name(), fingerprint(&out.html, &out.files)));
        if mode == Mode::Hard95 {
            // Text inside figures stays in the page pictures on purpose (it
            // is drawn in the figure's own order); the rest should be HTML.
            let in_pictures: usize = out
                .files
                .iter()
                .filter(|(n, _)| n.ends_with(".svg"))
                .map(|(_, d)| d.windows(5).filter(|w| w == b"<use ").count())
                .sum();
            glyphs_left = in_pictures.saturating_sub(figure_text_chars(&out.html));
        }
    }
    fingerprints.push(("json", hex(Sha256::digest(json.as_bytes()))));
    let metrics = Metrics {
        title,
        tables,
        broken_paragraphs,
        split_words,
        glyphs_left,
        hyphenated,
    };
    (metrics, Snapshot { fingerprints, json })
}

fn check(e: &Expect) -> Option<Metrics> {
    let paper = load(e.id)?;
    let (m, snapshot) = measure(&paper);
    if std::env::var("P2H_PRINT_METRICS").is_ok() {
        eprintln!("{}: {m:?}", e.id);
    }
    assert!(m.title.contains(e.title), "{}: title {:?}", e.id, m.title);
    assert!(
        m.tables >= e.min_tables,
        "{}: {} tables rebuilt, expected at least {}",
        e.id,
        m.tables,
        e.min_tables
    );
    assert!(
        m.broken_paragraphs <= e.max_broken_paragraphs,
        "{}: {} paragraphs broken mid-sentence, expected at most {}",
        e.id,
        m.broken_paragraphs,
        e.max_broken_paragraphs
    );
    assert!(
        m.split_words <= e.max_split_words,
        "{}: {} words still split by a hyphen, expected at most {}",
        e.id,
        m.split_words,
        e.max_split_words
    );
    assert!(
        m.glyphs_left <= e.max_glyphs_left,
        "{}: {} glyphs left in hard-95 page pictures, expected at most {}",
        e.id,
        m.glyphs_left,
        e.max_glyphs_left
    );
    assert!(
        m.hyphenated <= e.max_hyphenated,
        "{}: {} hyphenated words, expected at most {} (line-end hyphens not removed?)",
        e.id,
        m.hyphenated,
        e.max_hyphenated
    );
    check_snapshot(e.id, &snapshot);
    Some(m)
}

/// The rebuilt table whose caption starts with `label` ("Table 2"), as rows
/// of cell texts. `None` when the paper is not downloaded.
fn table(id: &str, label: &str) -> Option<Vec<Vec<String>>> {
    let paper = load(id)?;
    let analysis = paper.analyze();
    let block = analysis
        .doc
        .blocks
        .iter()
        .find(|b| plain(&b.spans).starts_with(&format!("{label}:")))
        .unwrap_or_else(|| panic!("{id}: no caption starting with {label:?}"));
    let t = block
        .table
        .as_ref()
        .unwrap_or_else(|| panic!("{id}: {label} was not rebuilt"));
    Some(
        t.rows
            .iter()
            .map(|r| r.iter().map(|c| plain(&c.spans)).collect())
            .collect(),
    )
}

macro_rules! paper_test {
    ($name:ident, $id:literal, $title:literal, tables >= $t:literal, broken <= $b:literal, split <= $s:literal, glyphs <= $g:literal, hyphenated <= $h:literal) => {
        #[test]
        fn $name() {
            check(&Expect {
                id: $id,
                title: $title,
                min_tables: $t,
                max_broken_paragraphs: $b,
                max_split_words: $s,
                max_glyphs_left: $g,
                max_hyphenated: $h,
            });
        }
    };
}

paper_test!(
    attention,
    "1706.03762v7",
    "Attention Is All You Need",
    tables >= 4,
    broken <= 1,
    split <= 0,
    glyphs <= 85,
    hyphenated <= 89
);
paper_test!(
    resnet,
    "1512.03385v1",
    "Deep Residual Learning",
    tables >= 13,
    broken <= 0,
    split <= 0,
    glyphs <= 0,
    hyphenated <= 87
);
paper_test!(
    bert,
    "1810.04805v2",
    "BERT",
    tables >= 8,
    broken <= 3,
    split <= 0,
    glyphs <= 0,
    hyphenated <= 228
);
paper_test!(
    vit,
    "2010.11929v2",
    "AN IMAGE IS WORTH 16X16 WORDS",
    tables >= 8,
    broken <= 2,
    split <= 0,
    glyphs <= 2381,
    hyphenated <= 186
);
paper_test!(
    ligo,
    "1602.03837v1",
    "Observation of Gravitational Waves",
    tables >= 1,
    broken <= 1,
    split <= 0,
    glyphs <= 3,
    hyphenated <= 76
);
paper_test!(
    faster_rcnn,
    "1506.01497v3",
    "Faster R-CNN",
    tables >= 12,
    broken <= 3,
    split <= 0,
    // 188 on p10, the rest are labels in the detection examples of p12-p13,
    // drawn twice (outline and fill) and kept in the figure pictures.
    glyphs <= 372,
    hyphenated <= 162
);
paper_test!(
    gpt4,
    "2303.08774v6",
    "GPT-4 Technical Report",
    tables >= 8,
    broken <= 46,
    split <= 0,
    glyphs <= 498,
    hyphenated <= 302
);
paper_test!(
    adam,
    "1412.6980v9",
    "ADAM",
    tables >= 0,
    broken <= 0,
    split <= 0,
    glyphs <= 806,
    hyphenated <= 54
);

/// Table 2 of the Transformer paper: "EN-DE" and "EN-FR" are two columns
/// under a shared header ("BLEU"), not one merged cell.
#[test]
fn attention_table_columns_are_split() {
    let Some(rows) = table("1706.03762v7", "Table 2") else {
        return;
    };
    assert!(
        rows.iter()
            .any(|r| r.contains(&"EN-DE".to_string()) && r.contains(&"EN-FR".to_string())),
        "no row with separate EN-DE and EN-FR cells: {:?}",
        &rows[..rows.len().min(3)]
    );
}

/// The header rows of BERT's Tables 2, 3 and 4 sit at the top of a page and
/// repeat the same words; they must not be removed as running heads.
#[test]
fn bert_table_headers_are_kept() {
    for label in ["Table 2", "Table 3", "Table 4"] {
        let Some(rows) = table("1810.04805v2", label) else {
            return;
        };
        assert_eq!(rows[0], ["System", "Dev", "Test"], "{label}: header row");
    }
}

/// The APS fonts of the LIGO paper declare a text encoding but keep the TeX
/// math layout: the glyph coded as `=` is a slash and, in the Math Pi font,
/// `f` and `g` are braces. The text must say what the glyphs show.
#[test]
fn mislabelled_math_glyphs_are_read_by_shape() {
    let Some(paper) = load("1602.03837v1") else { return };
    let analysis = paper.analyze();
    let text: String = analysis
        .doc
        .blocks
        .iter()
        .map(|block| plain(&block.spans) + "\n")
        .collect();
    for expected in ["ρ/{[1 + (χ", "erg/s", "0031-9007/16/116(6)/061102", "(m1 + m2)1/5"] {
        assert!(text.contains(expected), "missing {expected:?}");
    }
    assert!(!text.contains("erg=s"), "a slash read as an equals sign");
}

/// The algorithms of the Adam paper are rebuilt as text in the friendly
/// mode: numbered statements, indentation and right-aligned comments.
#[test]
fn adam_algorithms_are_text() {
    let Some(paper) = load("1412.6980v9") else { return };
    let html = paper.to_html(&paper.analyze(), &HtmlOptions::default()).html;
    assert_eq!(
        html.matches("<div class=\"algo\">").count(),
        2,
        "both algorithms rebuilt"
    );
    assert!(html.contains("<span class=\"cmt\">(Update parameters)</span>"));
    assert!(html.contains("<div class=\"ln req\"><i></i><div class=\"st in0\"><span><strong>Require: </strong>"));
    assert!(html.contains("<div class=\"st in1\"><span><em>t </em>← <em>t </em>+ 1</span>"));
    assert!(!html.contains("alt=\"Algorithm 1"), "no picture of the algorithm");
}

/// The appendix of the Adam paper stacks fractions of the same width in
/// several equations: their bars are not the rules of a table, so no
/// picture is cut out of the text. A wrapped "T ≥ 1." stays in its
/// paragraph instead of joining the display below it.
#[test]
fn adam_fraction_bars_are_not_tables() {
    let Some(paper) = load("1412.6980v9") else { return };
    let analysis = paper.analyze();
    let graphics: Vec<usize> = analysis
        .doc
        .blocks
        .iter()
        .filter(|block| {
            matches!(
                block.kind,
                BlockKind::Figure {
                    kind: FloatKind::Graphic,
                    ..
                }
            )
        })
        .map(|block| block.page + 1)
        .collect();
    assert!(
        graphics.is_empty(),
        "graphics cut out of equations on pages {graphics:?}"
    );
    let corollary = analysis
        .doc
        .blocks
        .iter()
        .map(|block| plain(&block.spans))
        .find(|text| text.starts_with("Corollary 4.2."))
        .expect("Corollary 4.2");
    assert!(corollary.trim_end().ends_with("T ≥ 1."), "{corollary}");
}

/// Faster R-CNN numbers its footnotes on the baseline ("1. Since ..."):
/// they are footnotes, not list items, and the paragraph that runs past
/// one into the next column stays whole. The caption of Table 4 ends in
/// URLs set small; it still pairs with the table below it, not with
/// Table 3 above. Its bold run-in heads that wrap onto a second line are
/// not headings.
#[test]
fn faster_rcnn_footnotes_and_tables() {
    let Some(paper) = load("1506.01497v3") else { return };
    let analysis = paper.analyze();
    let blocks = &analysis.doc.blocks;
    let markers: Vec<&str> = blocks
        .iter()
        .filter_map(|block| match &block.kind {
            BlockKind::Footnote { marker } => Some(marker.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(markers, ["1", "2", "3", "4", "5", "6", "7", "8"]);
    assert!(
        blocks
            .iter()
            .any(|block| plain(&block.spans).contains("built in commercial systems such as at Pinterests")),
        "paragraph split by footnote 1"
    );
    // Bold run-in heads long enough to fill a line stay in their paragraph.
    for head in [
        "One-Stage Detection vs. Two-Stage Proposal + Detection. The OverFeat paper",
        "Faster R-CNN in ILSVRC & COCO 2015 competitions We have demonstrated",
    ] {
        assert!(
            blocks
                .iter()
                .any(|block| matches!(block.kind, BlockKind::Paragraph { .. }) && plain(&block.spans).starts_with(head)),
            "run-in head not kept in its paragraph: {head:?}"
        );
    }
    let tables: Vec<(String, f32)> = blocks
        .iter()
        .filter_map(|block| match &block.kind {
            BlockKind::Figure { label: Some(label), .. } if block.page == 7 => Some((label.clone(), block.bbox.y0)),
            _ => None,
        })
        .collect();
    let y = |label: &str| tables.iter().find(|(l, _)| l == label).map(|(_, y)| *y);
    assert!(y("Table 4").unwrap() > y("Table 3").unwrap() + 100.0, "{tables:?}");
    let graphics = blocks
        .iter()
        .filter(|block| block.page == 7 && matches!(block.kind, BlockKind::Figure { label: None, .. }))
        .count();
    assert_eq!(graphics, 0, "Table 4 left without its caption");
    // A URL broken across lines after "_" is one link.
    let html = paper.to_html(&analysis, &HtmlOptions::default()).html;
    assert!(
        html.contains("href=\"https://github.com/shaoqingren/faster_rcnn\""),
        "broken GitHub link"
    );
    assert!(html.contains("href=\"https://github.com/rbgirshick/py-faster-rcnn\""));
}

/// GPT-4's references break URLs across lines after ".", "-" and "%": each
/// is one link, with its hyphens, except where TeX hyphenated a word
/// ("ope-" / "nai.com").
#[test]
fn urls_broken_across_lines_are_whole() {
    let Some(paper) = load("2303.08774v6") else { return };
    let html = paper.to_html(&paper.analyze(), &HtmlOptions::default()).html;
    for url in [
        "https://github.com/inverse-scaling/prize",
        "https://bounded-regret.ghost.io/emergent-deception-optimization/",
        "https://satsuite.collegeboard.org/media/pdf/understanding-sat-scores.pdf",
        "https://openai.com/research/language-unsupervised",
    ] {
        assert!(html.contains(&format!("href=\"{url}\"")), "missing link {url}");
    }
    assert!(!html.contains("href=\"https://github\""), "a link cut at a line break");
}

/// No picture cut out of a page (equation or figure) cuts through a glyph
/// of something else: the pictures show whole letters, never halves of
/// the text around them.
#[test]
fn pictures_do_not_cut_text() {
    let mut report = Vec::new();
    for id in [
        "1412.6980v9",
        "1506.01497v3",
        "1512.03385v1",
        "1602.03837v1",
        "1706.03762v7",
        "1810.04805v2",
        "2010.11929v2",
        "2303.08774v6",
    ] {
        let Some(paper) = load(id) else { continue };
        let analysis = paper.analyze();
        for cut in paper2html::analysis::crops::cut_glyphs(&analysis.doc, &analysis.loose) {
            report.push(format!(
                "{id} p{} {} #{}: {} glyphs cut by [{:.0},{:.0} {:.0}x{:.0}]: {}",
                cut.page + 1,
                cut.kind,
                cut.block,
                cut.glyphs,
                cut.rect.x0,
                cut.rect.y0,
                cut.rect.width(),
                cut.rect.height(),
                cut.examples.join(" ")
            ));
        }
    }
    eprintln!("{}", report.join("\n"));
    assert!(report.is_empty(), "{} pictures cut text", report.len());
}

/// The output does not depend on the number of threads (on a real paper).
#[test]
fn threads_do_not_change_real_output() {
    let Some(paper) = load("1602.03837v1") else { return };
    let render = |jobs: usize| {
        let analysis = paper.analyze_with(AnalyzeOptions {
            jobs,
            ..Default::default()
        });
        [Mode::Hard95, Mode::Friendly].map(|mode| {
            paper
                .to_html(
                    &analysis,
                    &HtmlOptions {
                        mode,
                        jobs,
                        ..Default::default()
                    },
                )
                .html
        })
    };
    assert!(render(1) == render(4), "output differs between 1 and 4 threads");
}

/// The first line of a caption set next to a figure has few words, and
/// justification stretches its spaces past the gap that separates columns
/// ("Figure 3:   Transfer to ImageNet.     While"): the caption still runs
/// on to its last line, and the paragraph cut by the figures stays whole.
/// In hard-95 the text inside figures stays in the page picture (a legend
/// box covers tick labels there), with a transparent copy for selecting.
#[test]
fn vit_captions_and_figure_text() {
    let Some(paper) = load("2010.11929v2") else { return };
    let analysis = paper.analyze();
    let blocks = &analysis.doc.blocks;
    let caption = |label: &str| {
        blocks
            .iter()
            .find(|block| matches!(&block.kind, BlockKind::Figure { label: Some(l), .. } if l == label))
            .map(|block| plain(&block.spans))
            .unwrap_or_default()
    };
    assert!(
        caption("Figure 3").ends_with("overtake smaller ones as the dataset grows."),
        "{}",
        caption("Figure 3")
    );
    assert!(
        blocks
            .iter()
            .any(|block| plain(&block.spans)
                .contains("Figure 3 also shows the performance region spanned by BiT models")),
        "paragraph cut by Figures 3 and 4"
    );
    let html = paper
        .to_html(
            &analysis,
            &HtmlOptions {
                mode: Mode::Hard95,
                ..Default::default()
            },
        )
        .html;
    assert!(html.contains("data-kind=\"figure-text\""));
    assert!(html.contains(".b[data-kind=\"figure-text\"]{color:transparent}"));
}
