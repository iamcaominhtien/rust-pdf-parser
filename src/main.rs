//! The `paper2html` command line: parses the arguments and calls the
//! library. `convert` turns a PDF into HTML (and JSON), `fetch` downloads an
//! arXiv PDF and `inspect` prints the detected structure for debugging.
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};
use paper2html::analysis::pipeline::Analysis;
use paper2html::model::{Block, BlockKind, Loc};
use paper2html::source::arxiv;
use paper2html::{AnalyzeOptions, Equations, HtmlOptions, Mode, PageImage, Paper};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// `--scale` must be larger than this (pixels per PDF point): smaller images
/// are unreadable.
const MIN_SCALE: f32 = 0.2;
/// Largest accepted `--scale` (pixels per PDF point): larger images only
/// waste space.
const MAX_SCALE: f32 = 8.0;
/// `inspect` shortens block texts longer than this many characters.
const INSPECT_MAX_CHARS: usize = 150;
/// Characters kept from the start of a shortened block text.
const INSPECT_HEAD_CHARS: usize = 95;
/// Characters kept from the end of a shortened block text.
const INSPECT_TAIL_CHARS: usize = 45;
/// Bytes per megabyte, for the size report.
const BYTES_PER_MB: f64 = 1e6;

/// Command-line arguments.
#[derive(Parser)]
#[command(
    name = "paper2html",
    version,
    about = "Convert scientific-paper PDFs (arXiv) into clean, readable HTML"
)]
struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    command: Command,
}

/// The subcommands. The doc comments of the variants and their fields are
/// the help texts.
#[derive(Subcommand)]
enum Command {
    /// Convert a PDF (local file, arXiv id or arXiv URL) to HTML.
    Convert(ConvertArgs),
    /// Download the PDF of an arXiv paper.
    Fetch {
        /// arXiv id or URL, e.g. 1706.03762
        id: String,
        /// Output file (default: <id>.pdf)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Print the detected structure of a paper (for debugging).
    Inspect {
        /// Local PDF file, arXiv id or arXiv URL
        input: String,
    },
    /// Read pictures of formulas (black on white, as LaTeX renders them) and
    /// print them as MathML. With several pictures, each line is the file
    /// name, a tab and its MathML (empty when it cannot be read).
    Formula {
        /// Picture files (PNG or JPEG)
        #[arg(required = true)]
        images: Vec<PathBuf>,
        /// Print how sure each reading is (weakest glyph, closest rival)
        /// before its MathML, and give every reading whatever its confidence
        #[arg(long, hide = true)]
        scores: bool,
    },
}

/// Values of `--mode`; the doc comments are the help texts.
#[derive(Clone, Copy, ValueEnum)]
enum ModeArg {
    /// Facsimile of every page with a selectable text layer
    Hard,
    /// Like hard, but all text is real HTML text placed as in the PDF
    #[value(name = "hard-95")]
    Hard95,
    /// Page and column layout kept, text reflowed
    Medium,
    /// Single-column responsive article
    Friendly,
}

impl From<ModeArg> for Mode {
    fn from(arg: ModeArg) -> Self {
        match arg {
            ModeArg::Hard => Mode::Hard,
            ModeArg::Hard95 => Mode::Hard95,
            ModeArg::Medium => Mode::Medium,
            ModeArg::Friendly => Mode::Friendly,
        }
    }
}

/// Values of `--images`; the doc comments are the help texts.
#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum ImagesArg {
    /// Embed images in the HTML file (one self-contained file)
    Inline,
    /// Write images to a folder next to the HTML file
    Files,
}

/// Values of `--fonts`; the doc comments are the help texts.
#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum FontsArg {
    /// Embed the reading fonts in friendly pages (works offline, about 250 KB) and the math font in pages with MathML (about 280 KB)
    Embed,
    /// Use the fonts installed on the reader's system (smaller files)
    System,
}

/// Values of `--tables`; the doc comments are the help texts.
#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum TablesArg {
    /// Rebuild tables as real HTML tables where their structure is clear
    Auto,
    /// Never rebuild tables (pictures in medium/friendly, placed text in hard-95)
    Image,
}

/// Values of `--equations`; the doc comments are the help texts.
#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum EquationsArg {
    /// Pictures of the formulas
    Image,
    /// MathML rebuilt from the glyphs where the structure is clear, pictures elsewhere
    Mathml,
}

/// Values of `--page-image`.
#[derive(Clone, Copy, ValueEnum)]
#[allow(
    clippy::missing_docs_in_private_items,
    reason = "a doc comment on a variant becomes its help text and would change --help"
)]
enum PageImageArg {
    // Pages as SVG (vector graphics, sharp at any zoom); a very large page as PNG.
    Svg,
    // Every page as SVG, however large.
    SvgOnly,
    // Pages as PNG bitmaps.
    Png,
}

/// Arguments of `convert`; the doc comments of the fields are the help
/// texts.
#[derive(clap::Args)]
struct ConvertArgs {
    /// Local PDF file, arXiv id (1706.03762) or arXiv URL
    input: String,
    /// Fidelity mode
    #[arg(short, long, value_enum, default_value = "friendly")]
    mode: ModeArg,
    /// Produce all four modes
    #[arg(long, conflicts_with = "mode")]
    all: bool,
    /// Output HTML file, or a directory (required to be a directory with --all)
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// How images are stored
    #[arg(long, value_enum, default_value = "inline")]
    images: ImagesArg,
    /// Resolution of figure/equation images in pixels per PDF point
    #[arg(long, default_value_t = 3.0)]
    scale: f32,
    /// Page background used by hard and hard-95 modes
    #[arg(long, value_enum, default_value = "svg")]
    page_image: PageImageArg,
    /// How tables are handled
    #[arg(long, value_enum, default_value = "auto")]
    tables: TablesArg,
    /// Fonts of friendly mode, and of the formulas of medium and friendly modes
    #[arg(long, value_enum, default_value = "embed")]
    fonts: FontsArg,
    /// How medium and friendly modes show displayed formulas
    #[arg(long, value_enum, default_value = "mathml")]
    equations: EquationsArg,
    /// Also write the structured document as JSON (for translation tools)
    #[arg(long)]
    json: bool,
    /// Number of worker threads: 0 = one per CPU core, 1 = no parallelism
    #[arg(short, long, default_value_t = 0, value_name = "N")]
    jobs: usize,
}

/// Where the files of one `convert` run go.
struct OutputPaths {
    /// The HTML file named by `--output`, when it names a single file.
    file: Option<PathBuf>,
    /// Directory for the generated file names (`<stem>.<mode>.html`, the
    /// JSON); empty for the current directory.
    dir: PathBuf,
}

impl OutputPaths {
    /// Interprets `--output`: a directory when `--all` is given, when it is
    /// an existing directory or when it has no extension; a file otherwise
    /// (then its parent directory takes the JSON). Without `--output`
    /// everything goes to the current directory.
    fn new(output: Option<&PathBuf>, all: bool) -> Self {
        let out_is_dir = all || output.is_some_and(|path| path.is_dir() || path.extension().is_none());
        let dir: PathBuf = match (output, out_is_dir) {
            (Some(path), true) => path.clone(),
            (Some(path), false) => path.parent().map(Path::to_path_buf).unwrap_or_default(),
            (None, _) => PathBuf::new(),
        };
        let file = if out_is_dir { None } else { output.cloned() };
        Self { file, dir }
    }

    /// The HTML file for `mode`: the file named by `--output`, or
    /// `<stem>.<mode>.html` in the output directory.
    fn html_file(&self, stem: &str, mode: Mode) -> PathBuf {
        match &self.file {
            Some(file) => file.clone(),
            None => self.dir.join(format!("{stem}.{}.html", mode.name())),
        }
    }
}

/// Runs the command line and exits with status 1 on error.
fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

/// Parses the arguments and runs the subcommand.
fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Convert(args) => convert(args),
        Command::Fetch { id, output } => {
            let id = arxiv::parse_id(&id).ok_or_else(|| anyhow!("'{id}' is not an arXiv id or URL"))?;
            let dest = output.unwrap_or_else(|| PathBuf::from(format!("{}.pdf", id.replace('/', "_"))));
            arxiv::save_pdf(&id, &dest)?;
            eprintln!("saved {}", dest.display());
            Ok(())
        }
        Command::Inspect { input } => inspect(&input),
        Command::Formula { images, scores } => formula(&images, scores),
    }
}

/// Loads the PDF named by `input` (an existing file, else an arXiv id or
/// URL) and returns it with the stem for output file names.
///
/// # Errors
/// When the file cannot be read, the download fails, the data is not a PDF,
/// or `input` is neither a file nor an arXiv id.
fn load(input: &str) -> Result<(Paper, String)> {
    let path = Path::new(input);
    if path.is_file() {
        let data = std::fs::read(path).with_context(|| format!("reading {input}"))?;
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("paper")
            .to_string();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(input)
            .to_string();
        return Ok((Paper::from_bytes(data)?.with_source(name), stem));
    }
    if let Some(id) = arxiv::parse_id(input) {
        eprintln!("fetching arXiv:{id} ...");
        let data = arxiv::fetch_pdf(&id)?;
        let stem = id.replace('/', "_");
        return Ok((Paper::from_bytes(data)?.with_source(format!("arXiv:{id}")), stem));
    }
    bail!("'{input}' is neither an existing file nor an arXiv id/URL")
}

/// Writes `data` to `path`, creating the parent directories first.
///
/// # Errors
/// When a directory or the file cannot be written.
fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(path, data).with_context(|| format!("writing {}", path.display()))
}

/// The `convert` subcommand: analyses the paper once, then writes the HTML
/// of each requested mode and, with `--json`, the document as JSON.
///
/// # Errors
/// When `--scale` is out of range, the input cannot be loaded or an output
/// file cannot be written.
fn convert(args: ConvertArgs) -> Result<()> {
    if !(args.scale > MIN_SCALE && args.scale <= MAX_SCALE) {
        bail!("--scale must be between 0.2 and 8");
    }
    let (paper, stem) = load(&args.input)?;
    let started = Instant::now();
    let jobs = paper2html::common::parallel::resolve_jobs(args.jobs);
    let analysis = paper.analyze_with(AnalyzeOptions {
        tables: args.tables == TablesArg::Auto,
        jobs,
        ..Default::default()
    });
    let tables = analysis.doc.blocks.iter().filter(|block| block.table.is_some()).count();
    eprintln!(
        "analysed {} pages ({jobs} threads), {} blocks, {tables} tables rebuilt in {:.2}s",
        paper.page_count(),
        analysis.doc.blocks.len(),
        started.elapsed().as_secs_f32()
    );

    let modes: Vec<Mode> = if args.all {
        vec![Mode::Hard, Mode::Hard95, Mode::Medium, Mode::Friendly]
    } else {
        vec![args.mode.into()]
    };

    // Resolve where each mode goes.
    let paths = OutputPaths::new(args.output.as_ref(), args.all);
    for mode in modes {
        let file = paths.html_file(&stem, mode);
        write_mode(&paper, &analysis, &args, jobs, mode, &file)?;
    }

    if args.json {
        let file = paths.dir.join(format!("{stem}.json"));
        let json = serde_json::to_vec_pretty(&analysis.doc)?;
        write_file(&file, &json)?;
        eprintln!("json     -> {}", file.display());
    }
    Ok(())
}

/// Renders `analysis` in `mode` and writes the HTML to `file`, with the
/// image files next to it when images are not inlined, then reports the
/// result on stderr.
///
/// # Errors
/// When a file cannot be written.
fn write_mode(
    paper: &Paper,
    analysis: &Analysis,
    args: &ConvertArgs,
    jobs: usize,
    mode: Mode,
    file: &Path,
) -> Result<()> {
    let file_stem = file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("paper")
        .to_string();
    let opts = HtmlOptions {
        mode,
        inline_images: args.images == ImagesArg::Inline,
        asset_dir: format!("{file_stem}_assets"),
        scale: args.scale,
        page_image: match args.page_image {
            PageImageArg::Svg => PageImage::Svg,
            PageImageArg::SvgOnly => PageImage::SvgOnly,
            PageImageArg::Png => PageImage::Png,
        },
        jobs,
        embed_fonts: args.fonts == FontsArg::Embed,
        equations: match args.equations {
            EquationsArg::Image => Equations::Image,
            EquationsArg::Mathml => Equations::MathMl,
        },
    };
    let started = Instant::now();
    let out = paper.to_html(analysis, &opts);
    write_file(file, out.html.as_bytes())?;
    let base = file.parent().map(Path::to_path_buf).unwrap_or_default();
    for (rel, data) in &out.files {
        write_file(&base.join(rel), data)?;
    }
    eprintln!(
        "{:<8} -> {} ({:.1} MB{}, {:.2}s)",
        mode.name(),
        file.display(),
        out.html.len() as f64 / BYTES_PER_MB,
        if out.files.is_empty() {
            String::new()
        } else {
            format!(" + {} image files", out.files.len())
        },
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// The `formula` subcommand: prints the MathML of the formulas pictured in `images`.
///
/// # Errors
/// When a picture cannot be opened, or a single picture cannot be read.
fn formula(images: &[PathBuf], scores: bool) -> Result<()> {
    let cfg = paper2html::raster::RasterConfig::default();
    let mut out = std::io::stdout().lock();
    for path in images {
        let image = image::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        if scores {
            let read = paper2html::raster::formula_with_confidence(&image, &cfg);
            let (weakest, closest, math) = read.map_or((f32::NAN, f32::NAN, String::new()), |(node, confidence)| {
                (
                    confidence.weakest,
                    confidence.closest,
                    paper2html::render::math_html(&node, ""),
                )
            });
            writeln!(out, "{}\t{weakest:.3}\t{closest:.3}\t{math}", path.display())?;
            continue;
        }
        let math = paper2html::raster::formula(&image, &cfg).map(|node| paper2html::render::math_html(&node, ""));
        if images.len() == 1 {
            let math = math.ok_or_else(|| anyhow!("cannot read the formula in {}", path.display()))?;
            writeln!(out, "{math}")?;
        } else {
            writeln!(out, "{}\t{}", path.display(), math.unwrap_or_default())?;
        }
    }
    Ok(())
}

/// The `inspect` subcommand: prints the document's metadata and one line
/// per block (page, column, kind, shortened text) to stdout. Stops quietly
/// when stdout is closed (e.g. piped into `head`).
///
/// # Errors
/// When the input cannot be loaded.
fn inspect(input: &str) -> Result<()> {
    let (paper, _) = load(input)?;
    let analysis = paper.analyze();
    let doc = &analysis.doc;
    let mut out = std::io::stdout().lock();
    let _ = writeln!(
        out,
        "title: {:?}\narxiv: {:?}\npages: {}  body font: {}pt  leading: {:.1}pt\n",
        doc.meta.title,
        doc.meta.arxiv_id,
        doc.pages.len(),
        doc.body_size,
        doc.leading
    );
    for block in &doc.blocks {
        let short = shorten(block.text());
        let kind = block_label(block);
        let zone = zone_label(&block.loc);
        if writeln!(out, "p{:<3} {:<4} {:<22} {}", block.page + 1, zone, kind, short).is_err() {
            break;
        }
    }
    Ok(())
}

/// `text` itself, or for texts longer than [`INSPECT_MAX_CHARS`] its start
/// and end joined by an ellipsis.
fn shorten(text: String) -> String {
    let n = text.chars().count();
    if n > INSPECT_MAX_CHARS {
        let head: String = text.chars().take(INSPECT_HEAD_CHARS).collect();
        let tail: String = text.chars().skip(n - INSPECT_TAIL_CHARS).collect();
        format!("{head} … {tail}")
    } else {
        text
    }
}

/// Short description of a block's kind for `inspect` ("H2", "EQ (3)
/// [200x30]", ...), followed by the table's size for rebuilt tables and by
/// `<math>` for equations whose structure was rebuilt (MathML).
fn block_label(block: &Block) -> String {
    let kind = kind_label(&block.kind);
    match &block.table {
        Some(table) => format!(
            "{kind} <table {}x{} hdr={}>",
            table.rows.len(),
            table.n_cols,
            table.header_rows
        ),
        None if block.math.is_some() => format!("{kind} <math>"),
        None => kind,
    }
}

/// Short description of a block kind with its main attributes.
fn kind_label(kind: &BlockKind) -> String {
    match kind {
        BlockKind::Title => "TITLE".to_string(),
        BlockKind::Authors => "AUTHOR".to_string(),
        BlockKind::Note => "NOTE".to_string(),
        BlockKind::Heading { level } => format!("H{level}"),
        BlockKind::Paragraph { in_abstract: true } => "ABSTRACT".into(),
        BlockKind::Paragraph { .. } => "P".into(),
        BlockKind::ListItem { marker } => format!("LI {}", marker.clone().unwrap_or_default()),
        BlockKind::Equation { number, image } => format!(
            "EQ {} [{:.0}x{:.0}]",
            number.clone().unwrap_or_default(),
            image.rect.width(),
            image.rect.height()
        ),
        BlockKind::Figure { kind, image, label } => format!(
            "{:?} {} {}",
            kind,
            label.clone().unwrap_or_default(),
            image
                .map(|spec| format!(
                    "[{:.0},{:.0} {:.0}x{:.0}]",
                    spec.rect.x0,
                    spec.rect.y0,
                    spec.rect.width(),
                    spec.rect.height()
                ))
                .unwrap_or_else(|| "[no image]".into())
        ),
        BlockKind::Footnote { marker } => format!("FN {marker}"),
        BlockKind::Reference { label } => format!("REF {}", label.clone().unwrap_or_default()),
    }
}

/// Column of a block in a multi-column zone as "column/columns", or "-"
/// for single-column zones.
fn zone_label(loc: &Loc) -> String {
    if loc.zone_cols > 1 {
        format!("{}/{}", loc.col + 1, loc.zone_cols)
    } else {
        "-".into()
    }
}
