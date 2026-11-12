//! Prints the rebuilt tables of a PDF (first rows of each), for checking
//! table reconstruction by hand. usage: cargo run --example dump_tables -- paper.pdf
use paper2html::Paper;

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump_tables <file.pdf>");
    let paper = Paper::from_bytes(std::fs::read(path).expect("readable file")).expect("valid PDF");
    let analysis = paper.analyze();
    for b in &analysis.doc.blocks {
        let Some(t) = &b.table else { continue };
        let caption: String = b.spans.iter().map(|s| s.text.as_str()).collect();
        let caption: String = caption.chars().take(60).collect();
        println!(
            "== page {}: {} rows x {} cols, {} header rows :: {caption}",
            b.page + 1,
            t.rows.len(),
            t.n_cols,
            t.header_rows
        );
        for row in t.rows.iter().take(3) {
            let cells: Vec<String> = row
                .iter()
                .map(|c| c.spans.iter().map(|s| s.text.as_str()).collect())
                .collect();
            println!("   {cells:?}");
        }
    }
}
