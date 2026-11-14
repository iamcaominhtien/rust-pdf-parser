# paper2html

A Rust tool that converts scientific papers in PDF format (from arXiv or a local file) into clean,
readable HTML with continuous text. The output works well as input for translation tools.

- **Pure Rust**, a single executable, no need to install `poppler`, `mupdf` or Python.
  PDF reading and rendering use [hayro](https://github.com/LaurenzV/hayro).
- **Continuous text**: joins lines into paragraphs, removes end-of-line hyphens (`multi-`/`plicative` →
  `multiplicative`), joins paragraphs split across columns and pages, removes headers, footers and
  page numbers, and separates footnotes from the main text.
- **Real tables**: rows, columns, cells merged across columns or rows, and rules are rebuilt as
  `<table>`.
- **Sharp images**: figures stay vector (SVG) or render at high resolution (216 dpi by default).
- **Four modes** that follow the original at different levels of fidelity (see below).
- Fast: a 15-page paper takes about 1–2 seconds for all four modes.

## The four modes

| Mode | Layout | Text | Use when |
| --- | --- | --- | --- |
| `hard` | 1-to-1 with each page (original page redrawn as SVG/PNG) | Hidden text layer, selectable, copyable and searchable | You need an exact comparison with the PDF |
| `hard-95` | Same as `hard`: page by page, same positions, same line breaks | **Real, visible HTML text**; tables are `<table>`; formulas are text | You want the look of the original but still need to translate, edit or copy |
| `medium` | Keeps the pages, the number of columns, and the relative position of figures and tables | Real HTML, reflowed automatically; paragraphs are not cut across columns or pages | You want the "paper feel" with naturally flowing text |
| `friendly` | One column, responsive, Offprint design: Paper / White / Dark themes, collapsible table of contents, embedded reading fonts, syntax-highlighted code, a book-style page-turning reading mode with a recorded paper page-turn sound (CC0, can be turned off) | Real HTML, optimized for reading | Reading on the web or on a phone, feeding translation tools |

In all four modes, the text of a paragraph stays together in the DOM:

- `friendly`, `medium`: each paragraph is a `<p>` tag; headings are `<h2>`–`<h5>`; figure captions are
  `<figcaption>`; references are `<ol class="refs">`; tables are `<table class="tbl">`.
- `hard-95`: each paragraph is a `<div class="fb" data-kind="paragraph" data-block="N">`.
- `hard`: each paragraph is a `<div class="b" data-kind="paragraph" data-block="N">` that contains
  the lines; the part of a paragraph that runs onto the next page has the same `data-block` and
  `data-continued="1"`.

### Formulas as MathML in `medium` and `friendly`

By default, `medium` and `friendly` show each display formula as a picture of its area of the page.
With `--equations mathml`, a formula whose structure is clear is written as MathML instead: real
text that reflows, zooms, can be searched and copied, and is read by screen readers. The structure
is rebuilt from the positions of the glyphs and of the bars drawn among them: fractions, square
roots and roots, subscripts and superscripts, limits above and below `∑`, `lim`, `max`, accents
(`x̂`, `x̃`), tall brackets, operator names, text inside formulas (`where`, `and`), and multi-line
displays aligned on their `=` or `≤`. Bold, calligraphic, blackboard and fraktur letters become the
Unicode mathematical letters (`𝐱`, `ℒ`, `ℝ`, `𝔤`).

A formula with anything that cannot be explained keeps its picture: a character without a Unicode
code, symbols stacked without a bar (matrices, `cases`, `\underbrace`), a bar or a stray symbol
that belongs to nothing. On 13 arXiv papers (the 8 test papers and 5 papers full of formulas),
138 of 147 display formulas are written as MathML.

MathML needs a math font on the reader's machine: Windows (Cambria Math) and macOS (STIX Two Math)
have one; on Linux and Android, install Latin Modern Math or STIX Two Math, or the formulas are drawn
with a fallback font.

### How `hard-95` works

Each page is redrawn from the PDF **after removing all text**, so the page background keeps only
real drawing strokes (plots, diagrams, images, rules, fraction bars). The text is placed back as
HTML in the same position:

- **Paragraphs, headings, captions, footnotes, references**: real text in a box at the exact
  position, with the same line breaks as the PDF, the same font size, line spacing, text color, and
  bold/italic. A paragraph that runs across columns or pages is still **a single element** in the
  DOM; the box in the next column or page shows the continuation (a clipped copy, marked
  `translate="no"`).
- **Tables**: a real `<table>` placed on the original row/column grid, with rules drawn in CSS. For
  a table whose grid cannot be rebuilt, the text of each cell is still real text placed at its
  position.
- **Display formulas**: real text, placed by groups of symbols (numerator, denominator, subscripts,
  summation limits). Large symbols (∑, tall brackets, radical signs) are scaled to match their
  original size.
- **Text inside figures** (including rotated axis labels): stays in the page background image, in
  the figure's original drawing order (a legend box can cover a label), together with a transparent
  HTML copy for selecting, searching and copying. A table whose rebuilt grid does not match the
  printed table (rows shorter than one line of text) is handled the same way.
- **Rotated text outside figures, page numbers, headers**: real text at the exact position and angle.

Font size and line width are adjusted to match the PDF without squeezing the text: a line wider than
in the PDF gets slightly smaller text (at most 12%), a narrower line gets slightly wider spacing
between letters; only justified lines get wider spacing between words.

When the content of a block changes (for example after translation), that block drops the original
line breaks and reflows inside its box, shrinking the font size if the translation is longer. This
part needs JavaScript; without JavaScript the page still displays correctly, but the line width is
not fitted exactly.

Apart from text inside figures, the only thing that does not become visible text is a character for
which the PDF gives no Unicode code (it stays as an image).

## Installation

```bash
cargo build --release
```

The executable is at `target/release/paper2html`.

## Usage

Convert an arXiv paper (the PDF is downloaded automatically and cached):

```bash
paper2html convert 1706.03762
```

Choose the mode and the output file:

```bash
paper2html convert 1706.03762 --mode medium -o attention.html
```

Export all four modes to a directory, with structured JSON:

```bash
paper2html convert paper.pdf --all --json -o out/
```

The input can be a PDF file, an arXiv ID (`1706.03762`, `arXiv:1706.03762v7`, `hep-th/9901001`)
or an arXiv URL (`https://arxiv.org/abs/1706.03762`).

| Option | Meaning |
| --- | --- |
| `-m, --mode <hard\|hard-95\|medium\|friendly>` | Mode (default `friendly`) |
| `--all` | Export all four modes: `<name>.hard.html`, `<name>.hard-95.html`, `<name>.medium.html`, `<name>.friendly.html` |
| `-o, --output <path>` | HTML file, or directory |
| `--images <inline\|files>` | `inline` (default): embed images in the HTML, a single file. `files`: write images to the `<name>_assets/` directory |
| `--scale <number>` | Resolution of figure/formula images, pixels per PDF point (default 3 ≈ 216 dpi) |
| `--page-image <svg\|png>` | Page background in `hard` mode (default `svg`) |
| `--tables <auto\|image>` | `auto` (default): build `<table>` when the table structure is clear. `image`: do not build (the table is an image in `medium`/`friendly`, and text placed by position in `hard-95`) |
| `--equations <image\|mathml>` | How `medium` and `friendly` show display formulas. `image` (default): a picture of the formula. `mathml`: MathML rebuilt from the glyphs where the structure is clear, a picture elsewhere (see above) |
| `--fonts <embed\|system>` | `embed` (default): embed the `friendly` reading fonts (Literata, IBM Plex Sans; IBM Plex Mono when the paper has code), about 250 KB, so offline viewing still uses the right fonts. `system`: use the fonts already on the reader's machine |
| `--json` | Also write `<name>.json`, the document structure |
| `-j, --jobs <N>` | Number of parallel threads. `0` (default): one thread per CPU core. `1`: no parallelism. The result is identical for every value |

Other commands:

```bash
paper2html fetch 1706.03762 -o attention.pdf
```

```bash
paper2html inspect paper.pdf
```

`fetch` only downloads the PDF. `inspect` prints the detected structure (title, headings,
paragraphs, figures, formulas…), which helps when the result is not what you expect. A formula
marked `<math>` has a rebuilt structure (MathML with `--equations mathml`).

### Speed and resources

Measured on a 4 vCPU machine, with the PDF already on disk, one mode per run:

| Paper | Pages | 1 thread | 4 threads | Peak RAM (4 threads) |
| --- | --- | --- | --- | --- |
| Attention (1706.03762) | 15 | 0.34–0.69 s | 0.20–0.36 s | 73–84 MB |
| ViT (2010.11929) | 22 | 0.61–0.70 s | 0.36–0.38 s | 77–95 MB |
| Faster R-CNN (1506.01497), many screenshots | 14 | 0.74–2.44 s | 0.41–1.21 s | 92–156 MB |
| GPT-4 report (2303.08774) | 100 | 1.73–1.97 s | 0.91–1.00 s | 152–211 MB |

Each page is read and drawn independently on one thread, and each thread has its own cache, so RAM
grows with the number of threads. Layout and HTML building still run on one thread. On a machine
with little RAM, use `-j 1` or `-j 2`.

When you open the file, the page background images of `hard`/`hard-95` are drawn only when you
scroll close to them (lazy loading), so long papers open fast: the 100-page GPT-4 report in `hard`
mode drops from about 7 s to about 1–1.5 s. The text of `hard-95` is always fully present from the
start, so translation tools do not have to wait for scrolling. When printing, the browser draws
every page.

Points that were measured but not yet improved (images compressed twice in `hard`/`hard-95`,
printing pages of a different paper size…) are recorded in
[docs/IMPROVEMENTS.md](docs/IMPROVEMENTS.md).

PDFs downloaded from arXiv are cached in `~/.cache/paper2html` (change it with the environment
variable `PAPER2HTML_CACHE`).

## JSON for translation tools

`--json` writes a list of blocks in reading order. Each block has a `type`
(`title`, `authors`, `heading`, `paragraph`, `list_item`, `equation`, `figure`, `footnote`,
`reference`, `note`), the page number `page` (counted from 0), `bbox` and `spans`:

```json
{
  "id": 22,
  "type": "paragraph",
  "in_abstract": false,
  "page": 1,
  "bbox": { "x0": 107.7, "y0": 389.5, "x1": 505.2, "y1": 486.7 },
  "spans": [
    { "text": "The goal of reducing sequential computation ... Extended Neural GPU [", "style": 0 },
    { "text": "16", "style": 0, "href": "#ref-16" }
  ]
}
```

`style` is a combination of bits: 1 = bold, 2 = italic, 4 = monospace, 8 = math font,
16 = superscript, 32 = subscript, 64 = sans-serif. Joining the `text` of the spans gives the
paragraph text exactly. Formulas (`equation`) and figures (`figure`) carry the coordinates of the
image area on the page; the `spans` of a figure are its caption.

A table with a rebuilt structure has an extra `table` field:

```json
{
  "type": "figure", "kind": "table", "label": "Table 2",
  "table": {
    "n_cols": 5,
    "header_rows": 2,
    "col_align": ["left", "center", "center", "center", "center"],
    "rows": [
      [ { "col": 0, "colspan": 1, "rowspan": 2, "spans": [ { "text": "Model", "style": 0 } ] },
        { "col": 1, "colspan": 2, "rowspan": 1, "spans": [ { "text": "BLEU", "style": 0 } ] } ]
    ],
    "rule_above": [[2, 2, 2, 2, 2]],
    "rule_bottom": [2, 2, 2, 2, 2]
  }
}
```

`rule_above[row][col]` is the rule above a cell (0 = none, 1 = thin, 2 = thick).

## Use as a library

```rust
use paper2html::{HtmlOptions, Mode, Paper};

let paper = Paper::from_bytes(std::fs::read("paper.pdf")?)?;
let analysis = paper.analyze();            // analyze once
for block in &analysis.doc.blocks {
    println!("{:?}: {}", block.kind, block.text());
}
let out = paper.to_html(&analysis, &HtmlOptions { mode: Mode::Friendly, ..Default::default() });
std::fs::write("paper.html", out.html)?;
```

## How it works

```
PDF ──► extraction ──► text lines ──► figures/tables ──► reading order ──► block ──► document ──► HTML
        (pdf/)         (layout/lines) (floats)           (order)           (blocks)  (document)   (html/)
```

1. **Extraction** (`src/pdf`): runs the PDF interpreter and records every glyph (Unicode character,
   position, font size, font), image and vector drawing. The font name tells whether it is
   bold, italic, monospace or a math font.
2. **Text lines** (`layout/lines.rs`): groups glyphs by baseline, splits words by gaps, attaches
   superscripts and subscripts to the line, and combines TeX's separate accents (`¨` + `o` → `ö`).
3. **Figures and tables** (`layout/floats.rs`): finds captions (`Figure 3:`, `Table 1.`), groups
   drawings into clusters, matches each caption with the nearest cluster, and detects tables
   through horizontal rules. Text inside figures/tables is removed from the main text flow.
   **Table structure** (`layout/tables.rs`): rows by baseline, columns by empty "corridors" running
   down the table; a cell that spans a corridor becomes a cell merged across columns, a label
   placed between rows becomes a cell merged across rows, and rules attach to cell edges. A
   multi-line cell is merged into one cell when the logical rows are clearly separated (rules
   between rows, or a gap larger than the line spacing) and the lower line continues the upper
   line. Content that is not grid-like (for example a cell that holds a matrix, or two number
   columns merged into one) is not rebuilt. A `Figure N` caption that goes with a table makes that
   table an image. Some PDFs draw table rules with 1-pixel mask images; these images are read as
   rules.
4. **Reading order** (`layout/order.rs`): cuts the page into horizontal regions, each with one or
   more columns (found by looking for gaps between columns), recursively.
5. **Blocks** (`layout/blocks.rs`): splits paragraphs (indentation, short last line, vertical
   gap), and detects display formulas (rendered as a transparent-background image) and footnotes.
6. **Document** (`document.rs`): title, authors, abstract, heading levels, lists, references;
   joins paragraphs across columns/pages; removes end-of-line hyphens based on the paper's own
   vocabulary; creates cross-links (`[12]`, `Figure 2`, `Section 3.1`, `Eq. (4)`, footnotes, URLs).
7. **Formula structure** (`analysis/math.rs`): rebuilds the tree of each display formula from its
   glyphs and the rules among them: tall symbols drawn in pieces, accents, radicals (the bar starting
   at the corner of the radical sign), fractions (inner ones first), limits of big operators, the
   lines of a multi-line display, then scripts by size and baseline. A formula with anything left
   unexplained gets no tree and stays a picture.
8. **HTML** (`src/html`): four renderers share one document model (`model.rs`). `hard-95` takes its
   page background from the text-free SVG (`pdf/render.rs`) and then places HTML text on top.
   With `--equations mathml`, `medium` and `friendly` write the formula trees as MathML
   (`render/mathml.rs`).

## Current limitations

- **Tables**: grid-like tables are rebuilt as `<table>`; on 8 test papers this is 52 of 67 tables
  with captions (44 of 48 without the GPT-4 report, which has many tables whose cells are whole
  paragraphs). A table with cells that hold matrices or large brackets, cells that are long
  paragraphs, or rows that are not regular enough is not rebuilt: `medium` and `friendly` keep the
  image, and `hard-95` uses text placed by position. A cell with several lines of text appears as
  several rows when the table does not show clearly where the logical rows break. Cell background
  color is not kept.
- **Display formulas**: real text in `hard-95`; pictures in `medium` and `friendly`, or MathML with
  `--equations mathml` where the structure is clear (138 of 147 formulas on 13 papers). Matrices,
  `cases`, `\underbrace` and formulas whose picture also holds a piece of a neighbouring formula
  stay pictures. A multi-line display is aligned on one relation per line; a second alignment point
  (`alignat`) is not kept. Equation numbers inside a multi-line display stay at the end of their
  line.
- **Fonts**: `hard-95` uses the fonts already on the viewer's machine (Times/Helvetica/Courier
  families) and does not embed the PDF's fonts, so the letter shapes may differ slightly from the
  original; position and line width are matched.
- *Inline* formulas are real text with `<sub>`/`<sup>`; complex expressions (fractions, nested
  radicals) may be flattened.
- A character for which the PDF gives no Unicode code appears as `�` in `medium`/`friendly`, and
  keeps its image form in `hard-95`.
- Author–year citations ("Vaswani et al., 2017") are not yet linked to the reference entry; the
  numeric style `[12]` is.
- Scanned PDFs (images only, no text layer) and encrypted PDFs are not supported yet.
- Layout is tested mainly on 1–2 column arXiv papers (NeurIPS, ICLR, CVPR, ACL, IEEE, APS);
  unusual layouts may be detected wrongly — use `inspect` to check.

## Testing

Download the test papers first (only needed once), then run the tests:

```bash
scripts/fetch-test-pdfs.sh
```

```bash
cargo test
```

There are three layers of tests:

- **Unit tests:** hyphen removal, cross-links, font detection, arXiv IDs, splitting work in parallel,
  rebuilding formulas from glyph positions, MathML output.
- **End-to-end tests on small PDFs** generated inside the test: document structure, table building,
  the four modes, `hard-95` page background with no remaining characters, output that does not change
  with the number of threads, lazy-loaded background images.
- **Tests on 8 real arXiv papers** (`tests/real_papers.rs`), with versions pinned in
  `tests/data/papers.txt`. For each paper: the title, the number of tables rebuilt, the number of
  paragraphs broken mid-sentence, the number of words that still have hyphens, the number of
  characters left in the `hard-95` page background, the number of formulas rebuilt as MathML, and
  whether all four modes export. There are also a few specific checks (the EN-DE/EN-FR columns of
  the Transformer table, the header rows of the BERT tables at the top of a page, the structure of
  some formulas of the Transformer and Adam papers). The thresholds are the current results: better passes, worse fails.
  When you make an improvement, tighten the thresholds
  (`P2H_PRINT_METRICS=1 cargo test --test real_papers -- --nocapture` prints the measurements).
- **Output snapshots** (also in `tests/real_papers.rs`): the sha256 hash of the HTML of each mode
  (plus `friendly` with `--equations mathml`) and of the JSON, for each paper, stored in
  `tests/data/snapshots/`. If the output changes by even one
  character, the test fails and names the paper and the mode that changed. When you reorganize code,
  the snapshots must stay the same. When you change the output on purpose, update them and commit
  them together with the code change:

  ```bash
  P2H_UPDATE_SNAPSHOTS=1 cargo test --test real_papers
  ```

  The hash ignores the pixels of PNG/JPEG images (they can differ by a few units between CPU
  types) and counts only text, HTML structure, vector SVG and image sizes. The LIGO paper
  (CC BY 4.0) stores the full JSON, so `git diff` shows exactly which content changed.

Tests never download anything on their own. Only the LIGO paper (CC BY 4.0) is in the repo; the
other papers may not be redistributed, so you must download them with the script above. A test skips
any paper that is not downloaded and prints the list of missing ones.

The JavaScript of `hard-95` (line fitting, reflowing text on translation) and of `friendly` (themes,
table of contents, phone screen, book reading mode: pagination, page turning, repaginating after
translation) is checked in a browser:

```bash
node tests/browser/check-hard95.js out/*.hard-95.html
```

```bash
node tests/browser/check-friendly.js out/*.friendly.html
```

`medium` and `friendly` pages must stay still when images (lazy-loaded) appear: each image has
`width`/`height` set in advance, no block moves before and after all images load, images are not
distorted, and opening a link to a section (`#sec-…`) jumps straight to the right section:

```bash
node tests/browser/check-stable.js out/*.medium.html out/*.friendly.html
```

This needs the `playwright` or `playwright-core` package. CI (`.github/workflows/ci.yml`) runs
`cargo fmt`, `cargo clippy`, all tests and the browser checks on every pull request. The papers are
kept in the repo's own cache on GitHub, so only the first run has to download them.

`cargo run --example dump_tables -- paper.pdf` prints the rebuilt tables of a PDF, which helps when
debugging table building.

## Releases and contributing

The version, `CHANGELOG.md` and the GitHub Release (with executables for Linux, macOS, Windows) are
created automatically from commits that follow the Conventional Commits standard. See
[CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT. The main dependencies (hayro, vello) use the Apache-2.0 or MIT license.
