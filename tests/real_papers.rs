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

use paper2html::model::{BlockKind, Span};
use paper2html::{AnalyzeOptions, HtmlOptions, Mode, Paper};
use regex::Regex;
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

fn plain(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| s.text.as_str())
        .collect::<String>()
        .trim()
        .to_string()
}

/// Analyses a paper, renders every mode and measures it.
fn measure(paper: &Paper) -> Metrics {
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
        if mode == Mode::Hard95 {
            glyphs_left = out
                .files
                .iter()
                .filter(|(n, _)| n.ends_with(".svg"))
                .map(|(_, d)| d.windows(5).filter(|w| w == b"<use ").count())
                .sum();
        }
    }
    Metrics {
        title,
        tables,
        broken_paragraphs,
        split_words,
        glyphs_left,
        hyphenated,
    }
}

fn check(e: &Expect) -> Option<Metrics> {
    let paper = load(e.id)?;
    let m = measure(&paper);
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
    glyphs <= 188,
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
