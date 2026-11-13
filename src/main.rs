use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};
use paper2html::model::BlockKind;
use paper2html::source::arxiv;
use paper2html::{AnalyzeOptions, HtmlOptions, Mode, PageImage, Paper};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Parser)]
#[command(
    name = "paper2html",
    version,
    about = "Convert scientific-paper PDFs (arXiv) into clean, readable HTML"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

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
}

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
    fn from(m: ModeArg) -> Self {
        match m {
            ModeArg::Hard => Mode::Hard,
            ModeArg::Hard95 => Mode::Hard95,
            ModeArg::Medium => Mode::Medium,
            ModeArg::Friendly => Mode::Friendly,
        }
    }
}

#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum ImagesArg {
    /// Embed images in the HTML file (one self-contained file)
    Inline,
    /// Write images to a folder next to the HTML file
    Files,
}

#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum TablesArg {
    /// Rebuild tables as real HTML tables where their structure is clear
    Auto,
    /// Never rebuild tables (pictures in medium/friendly, placed text in hard-95)
    Image,
}

#[derive(Clone, Copy, ValueEnum)]
enum PageImageArg {
    Svg,
    Png,
}

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
    /// Page background used by hard mode
    #[arg(long, value_enum, default_value = "svg")]
    page_image: PageImageArg,
    /// How tables are handled
    #[arg(long, value_enum, default_value = "auto")]
    tables: TablesArg,
    /// Also write the structured document as JSON (for translation tools)
    #[arg(long)]
    json: bool,
    /// Number of worker threads: 0 = one per CPU core, 1 = no parallelism
    #[arg(short, long, default_value_t = 0, value_name = "N")]
    jobs: usize,
}

fn load(input: &str) -> Result<(Paper, String)> {
    let path = Path::new(input);
    if path.is_file() {
        let data = std::fs::read(path).with_context(|| format!("reading {input}"))?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("paper").to_string();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or(input).to_string();
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

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(path, data).with_context(|| format!("writing {}", path.display()))
}

fn convert(args: ConvertArgs) -> Result<()> {
    if !(args.scale > 0.2 && args.scale <= 8.0) {
        bail!("--scale must be between 0.2 and 8");
    }
    let (paper, stem) = load(&args.input)?;
    let t = Instant::now();
    let jobs = paper2html::common::parallel::resolve_jobs(args.jobs);
    let analysis = paper.analyze_with(AnalyzeOptions {
        tables: args.tables == TablesArg::Auto,
        jobs,
        ..Default::default()
    });
    let tables = analysis.doc.blocks.iter().filter(|b| b.table.is_some()).count();
    eprintln!(
        "analysed {} pages ({jobs} threads), {} blocks, {tables} tables rebuilt in {:.2}s",
        paper.page_count(),
        analysis.doc.blocks.len(),
        t.elapsed().as_secs_f32()
    );

    let modes: Vec<Mode> = if args.all {
        vec![Mode::Hard, Mode::Hard95, Mode::Medium, Mode::Friendly]
    } else {
        vec![args.mode.into()]
    };

    // Resolve where each mode goes.
    let out_is_dir = args.all
        || args
            .output
            .as_ref()
            .is_some_and(|o| o.is_dir() || o.extension().is_none());
    let dir: PathBuf = match (&args.output, out_is_dir) {
        (Some(o), true) => o.clone(),
        (Some(o), false) => o.parent().map(Path::to_path_buf).unwrap_or_default(),
        (None, _) => PathBuf::new(),
    };

    for mode in modes {
        let file: PathBuf = match (&args.output, out_is_dir) {
            (Some(o), false) => o.clone(),
            _ => dir.join(format!("{stem}.{}.html", mode.name())),
        };
        let file_stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("paper").to_string();
        let opts = HtmlOptions {
            mode,
            inline_images: args.images == ImagesArg::Inline,
            asset_dir: format!("{file_stem}_assets"),
            scale: args.scale,
            page_image: match args.page_image {
                PageImageArg::Svg => PageImage::Svg,
                PageImageArg::Png => PageImage::Png,
            },
            jobs,
        };
        let t = Instant::now();
        let out = paper.to_html(&analysis, &opts);
        write_file(&file, out.html.as_bytes())?;
        let base = file.parent().map(Path::to_path_buf).unwrap_or_default();
        for (rel, data) in &out.files {
            write_file(&base.join(rel), data)?;
        }
        eprintln!(
            "{:<8} -> {} ({:.1} MB{}, {:.2}s)",
            mode.name(),
            file.display(),
            out.html.len() as f64 / 1e6,
            if out.files.is_empty() {
                String::new()
            } else {
                format!(" + {} image files", out.files.len())
            },
            t.elapsed().as_secs_f32()
        );
    }

    if args.json {
        let file = dir.join(format!("{stem}.json"));
        let json = serde_json::to_vec_pretty(&analysis.doc)?;
        write_file(&file, &json)?;
        eprintln!("json     -> {}", file.display());
    }
    Ok(())
}

fn inspect(input: &str) -> Result<()> {
    let (paper, _) = load(input)?;
    let a = paper.analyze();
    let mut o = std::io::stdout().lock();
    let _ = writeln!(
        o,
        "title: {:?}\narxiv: {:?}\npages: {}  body font: {}pt  leading: {:.1}pt\n",
        a.doc.meta.title,
        a.doc.meta.arxiv_id,
        a.doc.pages.len(),
        a.doc.body_size,
        a.doc.leading
    );
    for b in &a.doc.blocks {
        let text = b.text();
        let n = text.chars().count();
        let short: String = if n > 150 {
            let head: String = text.chars().take(95).collect();
            let tail: String = text.chars().skip(n - 45).collect();
            format!("{head} … {tail}")
        } else {
            text
        };
        let kind = match &b.kind {
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
                    .map(|i| format!(
                        "[{:.0},{:.0} {:.0}x{:.0}]",
                        i.rect.x0,
                        i.rect.y0,
                        i.rect.width(),
                        i.rect.height()
                    ))
                    .unwrap_or_else(|| "[no image]".into())
            ),
            BlockKind::Footnote { marker } => format!("FN {marker}"),
            BlockKind::Reference { label } => format!("REF {}", label.clone().unwrap_or_default()),
        };
        let kind = match &b.table {
            Some(t) => format!("{kind} <table {}x{} hdr={}>", t.rows.len(), t.n_cols, t.header_rows),
            None => kind,
        };
        let zone = if b.loc.zone_cols > 1 {
            format!("{}/{}", b.loc.col + 1, b.loc.zone_cols)
        } else {
            "-".into()
        };
        if writeln!(o, "p{:<3} {:<4} {:<22} {}", b.page + 1, zone, kind, short).is_err() {
            break;
        }
    }
    Ok(())
}

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
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
