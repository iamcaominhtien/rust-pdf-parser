# Possible improvements (not done yet)

Notes on points that were measured or found but not fixed, with the numbers and the options
considered, so we can come back to them later.

## 1. Photos are compressed twice in `hard` / `hard-95`

**Current state.** The page background of `hard` and `hard-95` is an SVG drawn by the `hayro-svg`
library. The library decodes every image in the PDF to pixels and writes it back as PNG (safe for
every image format, color space and transparency mask in PDF). Because PNGs of photos are very
large, `compact_images` in `src/pdf/render.rs` decodes the large PNGs again and compresses them to
JPEG (quality 88, longest side at most 2600 px). The result: an image that is already a JPEG in the
PDF gets JPEG-compressed a second time, and goes through the loop
JPEG → pixels → PNG → pixels → JPEG.

**Numbers** (Faster R-CNN paper, 1506.01497, measured with callgrind, one thread): about 70% of the
`hard-95` convert time goes into this loop: PNG encoding in the library ≈ 12%, PNG decoding ≈ 16%,
JPEG encoding ≈ 40%. Time: 2.4 s (1 thread), 1.2 s (4 threads); `medium`/`friendly` take only
0.7–0.8 s.

Number of images that could keep their original JPEG (JPEG, RGB or gray, no transparency mask):

| Paper | Eligible images / total |
| --- | --- |
| Faster R-CNN (1506.01497) | about 520 / 681 |
| ViT (2010.11929) | 254 / 304 |
| GPT-4 (2303.08774) | 7 / 13 |
| BERT (1810.04805) | 0 / 68 (all JPEG images have a transparency mask) |
| Attention, Adam, ResNet, LIGO | no JPEG images |

**Why we cannot do it from outside the library.** `hayro-svg` does not expose its image drawing
(`SvgRenderer` is an internal type, and images are decoded before they reach any place we can hook
into). `hayro-interpret` does let us get the raw data: `RasterImage::stream()`.

**Options:**

| | Approach | Patch the library? | Pros | Cons |
| --- | --- | --- | --- | --- |
| A | Use a faster JPEG encoder for the recompression step | No | Estimated 20–30% faster on image-heavy papers | Still compresses twice |
| B | Before drawing, replace each JPEG image in the PDF with a 1-pixel image that carries an ID (using an incremental update), draw the SVG, then swap the original JPEG back in | No | Close to D | Complex, fragile with unusual PDFs, hard to maintain |
| C | Send hayro a pull request: an option to embed the original JPEG when possible | No (fix goes upstream) | Cleanest | Depends on whether they accept it |
| D | Copy `hayro-svg` into the repo (`[patch.crates-io]`), change about 30 lines in `draw_image` | Yes | Fast, images identical to the PDF, smaller files | Must reapply the patch on every hayro upgrade |

The license of `hayro-svg` is `Apache-2.0 OR MIT`, so D is allowed if we keep the copyright notice,
include `LICENSE-MIT`/`LICENSE-APACHE` and state clearly which files were changed.

Two trade-offs apply to every way of embedding the original JPEG: colors may shift very slightly when
the PDF attaches an ICC color profile to the image (browsers ignore that profile); CMYK JPEGs must be
excluded (browsers often show wrong colors).

**Current decision:** leave as is. Preferred direction when we come back: A + C.

## 2. Parallelism is incomplete

`-j/--jobs` runs page reading and page/image drawing in parallel. Layout analysis and HTML building
still run on one thread, so on 4 cores the speedup is only 1.7–2.7x. A paper with a few image-heavy
pages has uneven load (one thread takes the heaviest page).

## 3. Printing pages whose size differs from the paper

When printing `hard`/`hard-95` (Ctrl+P), each page is stretched to the paper width. A page with a
narrower aspect ratio than Letter (for example Faster R-CNN, 567 × 774 pt) is taller than the sheet,
so it spills onto the next sheet: 14 pages become 28 sheets. Fix: in `@media print`, limit the page
by height as well, or set `@page { size: <PDF page size> }`.

## 4. Leftovers in `hard-95`

- The ViT paper (2010.11929) still has about 2381 characters left in the background image instead of
  being HTML text; the cause has not been investigated. Other papers: BERT 0, LIGO 3, Attention 85,
  Faster R-CNN 188.
- Some lines placed by position (`.pl`) do not yet match the original width (Adam 49 lines, ViT 36
  lines).

## 5. `medium` / `friendly`

- Display equations whose structure cannot be rebuilt stay images (about 7% of them, see section 8).

## 6. Very long documents (about 1000 pages)

Tested with "Dive into Deep Learning" (arXiv 2106.11342, 1185 pages, 39.5 MB PDF), on a 4 vCPU
machine:

| Mode | 4 threads | 1 thread | Peak RAM | Output file | Open in Chromium |
| --- | --- | --- | --- | --- | --- |
| `friendly` | 7.9 s | 20.5 s | 0.9–1.1 GB | 105 MB | 5.4 s |
| `medium` | 7.5 s | – | 1.1 GB | 105 MB | – |
| `hard-95` | 10.9 s | 25.1 s | 1.0–1.2 GB | 142 MB | 9.7 s |
| `hard` | 10.3 s | – | 1.7 GB | 481 MB | 13.7 s |

- **RAM:** reading and analysis alone need about 900 MB, because all characters of all pages are
  kept in memory at once (each character is a struct with its own `String`). Possible fixes: a more
  compact character representation; drop the raw page data right after the lines are built; write the
  HTML to the file in pieces instead of holding the whole string.
- **Opening the file:** an HTML file of 100–500 MB opens slowly (5–14 s). Possible fix: split a long
  document into several files by chapter, with a table of contents.

## 7. Batch processing

- There is no command for processing many documents. For now you need an outside script, for example
  `ls *.pdf | xargs -P 2 -I{} paper2html convert {} -m hard-95 -j 1 -o out/`.
  We should add a `batch` command: run N documents in parallel, skip documents that are already done,
  log errors.
- Downloads from arXiv are not rate-limited. arXiv asks automated tools to download slowly (their API
  guidelines say about 1 request every 3 seconds); downloading thousands of papers in a row may get
  blocked. We need to add rate limiting, or use arXiv's bulk data store.

## 8. Formulas rebuilt as MathML from PDFs

**Current state.** On 73 arXiv papers, 2607 of 2822 display formulas are written as MathML. They
were checked by overlaying each MathML rendering on the picture of the formula (see "How to measure"
below): 31 formulas score under 0.5 (from 106 before the fixes of that work), median 0.917.

**Errors seen but not fixed:**

- `\underbrace` and brace pieces (`⎪`) are not supported (the formula stays a picture).
- `argmin` with a long limit under it: part of the limit is read as a subscript.
- A `∑` limit merged with text next to it.
- A hyphen in a word inside a formula (`log-sum-exp`) is read as a minus sign.
- Short explanation lines (`where m̃ = m + max{sᵢ}`, `when z = g(…)`) still join the display above
  them: they have too few sentence words (the rule needs four) to be told from a formula.
- Inline fractions in a sentence: the numerators are given back to the sentence, but their text is
  interleaved with the denominators (`v1/2 αt,t i`).
- The picture of some formulas is cropped too tightly (the top of the formula is cut).

**How to measure (scripts kept outside the repository).** Convert many papers twice, with
`--equations mathml` and with `--equations image` (`medium`, `--images files`). Pair the
equations of the two outputs by order. Take a screenshot of each `<math>` in Chromium with the
embedded math font at 20 px, crop the ink of both pictures, scale the screenshot to the box of the
picture, and score how much ink of each is covered by ink of the other, with each 40 px tile
allowed to shift ±30 px sideways and ±4 px vertically (spacing drifts; misplaced scripts do not).
Sort by score, look at the worst, fix by kind of error, run again and compare with the previous
run by picture (only formulas whose MathML changed need new screenshots). Pairing breaks when the
number of equation blocks changes, so pictures must be rebuilt in the same run.

## 9. Formulas from pictures (`paper2html formula`)

**Current state.** Experimental. The ink of the picture is cut into pieces and each is compared
with glyphs of the TeX fonts (Computer Modern at 10/7/5 pt, AMS fonts, `dsfont`, `rsfs`), then the
glyphs go through the same builder as formulas of PDFs. On the 2607 formula pictures of 73 papers
(the answer being the MathML rebuilt from the PDF text): 58% read, 30% give exactly the same text,
93% of the characters of the pictures read are right. Steps that made a difference: shapes from the
grey level of pixels; boxes from the dark core of the ink (the anti-aliased edge shifted thin
glyphs); templates from the real TeX fonts at their optical sizes (from 44%/22% with Latin Modern);
advance boxes from the font metrics; size of flat symbols (`=`, `→`) from their width.

**Confidence.** Each reading has a confidence (likeness of its least sure glyph, lead over the
best other glyph; `formula --scores`). It does not separate right from wrong readings yet: with
thresholds chosen on half the papers for 95%, the other half gets 77% at 5% of the pictures; for
80%, 74% at 19%. So no threshold is applied. The goal was 95% right among the readings given.

**Most frequent misreadings:** `|` read as `]` (56), comma read as `.` (51) or `′` (40), `D` as
`𝒟` (42), `.` as `·` (37), `F` as `Γ` (25), `1` as `I` (25), `)` as `⟩` (18), `f` as `∫` (17). The
paper 1303.5113 (Fraktur, unusual symbols) makes up much of the unread pictures. Pictures in other
fonts (Times, as in the LIGO paper) are not read.

**Ideas, not done:**

- Tell `|` from `]` by the rows of the piece: a straight stroke has rows of equal width centred at
  the same place. A first try made `;` and `]` read as `!`, and was dropped.
- Use context more: `1` against `I`, `D` against `𝒟` by the letters around them; a comma on the
  baseline after a subscript.
- A stronger confidence: draw the glyphs read at their place and compare with the picture, or
  render the MathML and overlay it on the picture (as in section 8, which needs a browser).
- For pictures not set in TeX fonts, a learned model (pix2tex, Texify) would do better than
  templates.

**How to measure (scripts kept outside the repository).** Run `paper2html formula` on the
equation pictures of the papers of section 8 (several pictures per call; one line per picture),
compare the text of the `mi`/`mn`/`mo` elements with that of the MathML from the PDF, counting
look-alikes as equal (`∆`/`Δ`, `φ`/`ϕ`, `‖`/`||`), and list the confusions of readings of the
same length.

