//! End-to-end test on a small PDF that is generated on the fly.

use paper2html::model::BlockKind;
use paper2html::{HtmlOptions, Mode, Paper};

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

    assert!(doc.blocks.iter().any(|b| matches!(b.kind, BlockKind::Authors) && b.text().contains("Ada Lovelace")));
}

#[test]
fn renders_all_modes() {
    let paper = Paper::from_bytes(tiny_pdf()).expect("valid pdf");
    let analysis = paper.analyze();

    let friendly = paper.to_html(&analysis, &HtmlOptions { mode: Mode::Friendly, ..Default::default() });
    assert!(friendly.html.contains("<h1>A Tiny Test Paper</h1>"));
    assert!(friendly.html.contains("id=\"sec-1\""));
    assert!(friendly.html.contains("pipeline of the documentation tests."));
    assert!(friendly.files.is_empty());

    let medium = paper.to_html(&analysis, &HtmlOptions { mode: Mode::Medium, ..Default::default() });
    assert!(medium.html.contains("class=\"page\""));
    assert!(medium.html.contains("<h1 class=\"title\">A Tiny Test Paper</h1>"));

    let hard = paper.to_html(&analysis, &HtmlOptions { mode: Mode::Hard, ..Default::default() });
    assert!(hard.html.contains("data:image/svg+xml;base64,"));
    assert!(hard.html.contains("data-kind=\"paragraph\""));
    // The text of a paragraph is contiguous in the DOM.
    assert!(hard.html.contains("pipeline of the docu</span>"));

    // External images: the page background becomes a file.
    let opts = HtmlOptions { mode: Mode::Hard, inline_images: false, asset_dir: "assets".into(), ..Default::default() };
    let external = paper.to_html(&analysis, &opts);
    assert_eq!(external.files.len(), 1);
    assert!(external.files[0].0.starts_with("assets/"));
    assert!(external.html.contains(&external.files[0].0));
}

#[test]
fn rejects_garbage() {
    assert!(Paper::from_bytes(b"not a pdf".to_vec()).is_err());
}
