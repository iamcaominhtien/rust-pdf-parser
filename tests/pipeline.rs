//! End-to-end test on a small PDF that is generated on the fly.

use base64::Engine;
use paper2html::model::BlockKind;
use paper2html::{AnalyzeOptions, HtmlOptions, Mode, Paper};

/// Builds a one-page PDF using the standard Helvetica fonts.
fn tiny_pdf() -> Vec<u8> {
    let content = "\
BT /F2 20 Tf 180 700 Td (A Tiny Test Paper) Tj ET
BT /F1 10 Tf 250 670 Td (Ada Lovelace) Tj ET
BT /F2 12 Tf 72 620 Td (1 Introduction) Tj ET
BT /F1 10 Tf 72 600 Td (This tiny paper exists only to exercise the conversion pipeline of the docu-) Tj ET
BT /F1 10 Tf 72 588 Td (mentation tests.) Tj ET
BT /F1 10 Tf 72 568 Td (A second paragraph follows the first one after some vertical space and it is) Tj ET
BT /F1 10 Tf 72 556 Td (long enough to span two lines.) Tj ET
BT /F2 12 Tf 72 520 Td (2 Method) Tj ET
BT /F1 10 Tf 72 500 Td (The method section has a single short line.) Tj ET
BT /F1 10 Tf 72 462 Td (Table 1: Results of the tiny experiment.) Tj ET
0.8 w 72 452 m 300 452 l S
BT /F1 10 Tf 80 439 Td (Model) Tj ET
BT /F1 10 Tf 170 439 Td (Score) Tj ET
BT /F1 10 Tf 240 439 Td (Cost) Tj ET
0.4 w 72 433 m 300 433 l S
BT /F1 10 Tf 80 421 Td (Alpha) Tj ET
BT /F1 10 Tf 170 421 Td (1.5) Tj ET
BT /F1 10 Tf 240 421 Td (20) Tj ET
BT /F1 10 Tf 80 409 Td (Beta) Tj ET
BT /F1 10 Tf 170 409 Td (2.5) Tj ET
BT /F1 10 Tf 240 409 Td (30) Tj ET
BT /F1 10 Tf 80 397 Td (Gamma) Tj ET
BT /F1 10 Tf 170 397 Td (3.5) Tj ET
BT /F1 10 Tf 240 397 Td (40) Tj ET
0.8 w 72 390 m 300 390 l S
";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> >>"
            .to_string(),
        format!("<< /Length {} >>\nstream\n{}endstream", content.len(), content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".to_string(),
    ];
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{}\nendobj\n", i + 1, body));
    }
    let xref = pdf.len();
    pdf.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
    for off in offsets {
        pdf.push_str(&format!("{off:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        objects.len() + 1,
        xref
    ));
    pdf.into_bytes()
}

#[test]
fn analyses_structure() {
    let paper = Paper::from_bytes(tiny_pdf()).expect("valid pdf");
    assert_eq!(paper.page_count(), 1);
    let analysis = paper.analyze();
    let doc = &analysis.doc;

    assert_eq!(doc.meta.title.as_deref(), Some("A Tiny Test Paper"));
    assert!((doc.body_size - 10.0).abs() < 0.3, "body size {}", doc.body_size);

    let headings: Vec<String> = doc
        .blocks
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::Heading { .. }))
        .map(|b| b.text())
        .collect();
    assert_eq!(headings, ["1 Introduction", "2 Method"]);

    let paragraphs: Vec<String> = doc
        .blocks
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::Paragraph { .. }))
        .map(|b| b.text())
        .collect();
    assert_eq!(paragraphs.len(), 3, "{paragraphs:#?}");
    // The line-end hyphen is removed and the lines are joined.
    assert_eq!(
        paragraphs[0],
        "This tiny paper exists only to exercise the conversion pipeline of the documentation tests."
    );
    assert!(paragraphs[1].ends_with("and it is long enough to span two lines."));

    assert!(
        doc.blocks
            .iter()
            .any(|b| matches!(b.kind, BlockKind::Authors) && b.text().contains("Ada Lovelace"))
    );
}

#[test]
fn renders_all_modes() {
    let paper = Paper::from_bytes(tiny_pdf()).expect("valid pdf");
    let analysis = paper.analyze();

    let friendly = paper.to_html(
        &analysis,
        &HtmlOptions {
            mode: Mode::Friendly,
            ..Default::default()
        },
    );
    assert!(friendly.html.contains("<h1>A Tiny Test Paper</h1>"));
    assert!(friendly.html.contains("id=\"sec-1\""));
    assert!(friendly.html.contains("pipeline of the documentation tests."));
    assert!(friendly.files.is_empty());

    let medium = paper.to_html(
        &analysis,
        &HtmlOptions {
            mode: Mode::Medium,
            ..Default::default()
        },
    );
    assert!(medium.html.contains("class=\"page\""));
    assert!(medium.html.contains("<h1 class=\"title\">A Tiny Test Paper</h1>"));

    let hard = paper.to_html(
        &analysis,
        &HtmlOptions {
            mode: Mode::Hard,
            ..Default::default()
        },
    );
    assert!(hard.html.contains("data:image/svg+xml;base64,"));
    assert!(hard.html.contains("data-kind=\"paragraph\""));
    // The text of a paragraph is contiguous in the DOM.
    assert!(hard.html.contains("pipeline of the docu</span>"));

    // External images: the page background becomes a file.
    let opts = HtmlOptions {
        mode: Mode::Hard,
        inline_images: false,
        asset_dir: "assets".into(),
        ..Default::default()
    };
    let external = paper.to_html(&analysis, &opts);
    assert_eq!(external.files.len(), 1);
    assert!(external.files[0].0.starts_with("assets/"));
    assert!(external.html.contains(&external.files[0].0));
}

#[test]
fn rejects_garbage() {
    assert!(Paper::from_bytes(b"not a pdf".to_vec()).is_err());
}

#[test]
fn rebuilds_tables() {
    let paper = Paper::from_bytes(tiny_pdf()).expect("valid pdf");
    let analysis = paper.analyze();
    let block = analysis
        .doc
        .blocks
        .iter()
        .find(|b| matches!(b.kind, BlockKind::Figure { .. }))
        .expect("the table float");
    assert!(block.text().starts_with("Table 1: Results"));
    let table = block.table.as_ref().expect("table structure");
    assert_eq!(table.n_cols, 3);
    assert_eq!(table.header_rows, 1);
    let cells: Vec<Vec<String>> = table
        .rows
        .iter()
        .map(|r| {
            r.iter()
                .map(|c| c.spans.iter().map(|s| s.text.as_str()).collect::<String>())
                .collect()
        })
        .collect();
    assert_eq!(
        cells,
        [
            ["Model", "Score", "Cost"],
            ["Alpha", "1.5", "20"],
            ["Beta", "2.5", "30"],
            ["Gamma", "3.5", "40"],
        ]
    );
    // Heavy rule on top, thin rule under the header, heavy rule at the bottom.
    assert_eq!(table.rule_above[0], [2, 2, 2]);
    assert_eq!(table.rule_above[1], [1, 1, 1]);
    assert_eq!(table.rule_bottom, [2, 2, 2]);

    for mode in [Mode::Friendly, Mode::Medium] {
        let out = paper.to_html(
            &analysis,
            &HtmlOptions {
                mode,
                ..Default::default()
            },
        );
        assert!(out.html.contains("<table class=\"tbl\">"), "{mode:?}");
        assert!(out.html.contains(">Gamma</td>"), "{mode:?}");
        // No picture of the table is embedded.
        assert!(!out.html.contains("data:image/png"), "{mode:?}");
    }

    // Tables can be left alone.
    let plain = paper.analyze_with(AnalyzeOptions {
        tables: false,
        ..Default::default()
    });
    assert!(plain.doc.blocks.iter().all(|b| b.table.is_none()));
    let out = paper.to_html(
        &plain,
        &HtmlOptions {
            mode: Mode::Friendly,
            ..Default::default()
        },
    );
    assert!(out.html.contains("data:image/png"));
}

#[test]
fn hard95_is_all_text() {
    let paper = Paper::from_bytes(tiny_pdf()).expect("valid pdf");
    let analysis = paper.analyze();
    let out = paper.to_html(
        &analysis,
        &HtmlOptions {
            mode: Mode::Hard95,
            ..Default::default()
        },
    );
    let html = &out.html;

    // Paragraphs are flowing blocks holding the lines of the PDF ...
    assert!(html.contains("class=\"fb one\" data-kind=\"paragraph\""));
    assert!(html.contains("pipeline of the docu</span>"));
    // ... the removed line-end hyphen is shown without being part of the text.
    assert!(html.contains("class=\"ln hy\""));
    // The table is a table.
    assert!(html.contains("<div class=\"tb\" data-kind=\"table\""));
    // (The test PDF is set in Helvetica, hence the sans-serif class.)
    assert!(html.contains("<span class=\"s\">Gamma</span></td>"));

    // The picture of the page no longer draws any glyph: all text is HTML.
    let marker = "data:image/svg+xml;base64,";
    let start = html.find(marker).expect("page picture") + marker.len();
    let end = start + html[start..].find('"').unwrap();
    let svg = base64::engine::general_purpose::STANDARD
        .decode(&html[start..end])
        .unwrap();
    let svg = String::from_utf8(svg).unwrap();
    assert!(svg.starts_with("<svg"));
    assert!(!svg.contains("<use "), "glyphs left in the page picture");
    // The rules of the table are still drawn.
    assert!(svg.contains("<path"));

    // The facsimile mode still draws the text itself.
    let hard = paper.to_html(
        &analysis,
        &HtmlOptions {
            mode: Mode::Hard,
            ..Default::default()
        },
    );
    let start = hard.html.find(marker).unwrap() + marker.len();
    let end = start + hard.html[start..].find('"').unwrap();
    let svg = base64::engine::general_purpose::STANDARD
        .decode(&hard.html[start..end])
        .unwrap();
    assert!(String::from_utf8(svg).unwrap().contains("<use "));
}

#[test]
fn threads_do_not_change_output() {
    let paper = Paper::from_bytes(tiny_pdf()).expect("valid pdf");
    let render = |jobs: usize| {
        let analysis = paper.analyze_with(AnalyzeOptions {
            jobs,
            ..Default::default()
        });
        let json = serde_json::to_string(&analysis.doc).unwrap();
        let html: Vec<String> = [Mode::Hard, Mode::Hard95, Mode::Medium, Mode::Friendly]
            .into_iter()
            .map(|mode| {
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
            .collect();
        (json, html)
    };
    let one = render(1);
    assert_eq!(one, render(4));
    // No placeholder is left behind.
    assert!(one.1.iter().all(|h| !h.contains('\u{1}')));
}

#[test]
fn page_pictures_load_lazily() {
    let paper = Paper::from_bytes(tiny_pdf()).expect("valid pdf");
    let analysis = paper.analyze();
    for mode in [Mode::Hard, Mode::Hard95] {
        let html = paper
            .to_html(
                &analysis,
                &HtmlOptions {
                    mode,
                    ..Default::default()
                },
            )
            .html;
        let bg = html.find("<img class=\"bg\"").expect("page picture");
        let tag = &html[bg..bg + html[bg..].find('>').unwrap()];
        assert!(tag.contains("loading=\"lazy\""), "{mode:?}");
        // The picture keeps its proportions before it is loaded.
        assert!(tag.contains("width=\"") && tag.contains("height=\""), "{mode:?}");
    }
}
