//! Fetching papers from arXiv: recognising arXiv identifiers and URLs, and
//! downloading PDFs through an on-disk cache.
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

use anyhow::{Context, Result, anyhow};
use regex::Regex;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

/// Magic bytes at the start of every PDF file, used to tell a PDF from an
/// error page or a truncated cache entry.
const PDF_MAGIC: &[u8] = b"%PDF";
/// Name of the cache directory under the user's cache root.
const CACHE_DIR_NAME: &str = "paper2html";
/// Give up on a download after this long: large papers take a while, but a
/// stalled connection should not hang the program.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
/// Largest download accepted, in bytes; arXiv PDFs are far smaller, so
/// anything beyond this is not a paper.
const MAX_PDF_BYTES: u64 = 300 * 1024 * 1024;

/// A new-style arXiv identifier (`YYMM.NNNNN`, optionally with a version).
static NEW_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{4}\.\d{4,5}(v\d+)?$").unwrap());
/// An old-style arXiv identifier (`archive[.SUB]/YYMMNNN`, optionally with a
/// version), such as `hep-th/9901001` or `math.AG/0101001`.
static OLD_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z][a-z\-]*(\.[A-Z]{2})?/\d{7}(v\d+)?$").unwrap());
/// An arXiv abstract, PDF or HTML page URL; the `id` group holds the
/// identifier without a `.pdf` extension, query or fragment.
static URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https?://(?:www\.|export\.)?arxiv\.org/(?:abs|pdf|html)/(?P<id>[^?#]+?)(?:\.pdf)?(?:[?#].*)?$")
        .unwrap()
});

/// Extracts an arXiv identifier from an id (`1706.03762`, `arXiv:1706.03762v7`,
/// `hep-th/9901001`) or an arXiv URL.
///
/// Returns `None` when the input is neither (for example a file name).
pub fn parse_id(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let bare = trimmed
        .strip_prefix("arXiv:")
        .or_else(|| trimmed.strip_prefix("arxiv:"))
        .unwrap_or(trimmed);
    let candidate = match URL.captures(bare) {
        Some(captures) => captures["id"].to_string(),
        None => bare.to_string(),
    };
    (NEW_ID.is_match(&candidate) || OLD_ID.is_match(&candidate)).then_some(candidate)
}

/// Downloads the PDF of an arXiv paper and returns its bytes. Downloads are
/// cached on disk so that repeated conversions do not hit arXiv again.
///
/// # Errors
/// When the download fails or arXiv does not return a PDF. Failing to
/// write the cache is not an error.
pub fn fetch_pdf(id: &str) -> Result<Vec<u8>> {
    let file = cache_dir().join(format!("{}.pdf", id.replace('/', "_")));
    if let Ok(data) = std::fs::read(&file)
        && data.starts_with(PDF_MAGIC)
    {
        return Ok(data);
    }
    let data = download(id)?;
    if let Some(parent) = file.parent() {
        // Caching is best effort.
        if std::fs::create_dir_all(parent).is_ok() {
            let _ = std::fs::write(&file, &data);
        }
    }
    Ok(data)
}

/// Saves an arXiv PDF to `dest` (fetched through the cache, see
/// [`fetch_pdf`]).
///
/// # Errors
/// When the PDF cannot be fetched or `dest` cannot be written.
pub fn save_pdf(id: &str, dest: &Path) -> Result<()> {
    let data = fetch_pdf(id)?;
    std::fs::write(dest, data).with_context(|| format!("writing {}", dest.display()))
}

/// Directory of the download cache: `$PAPER2HTML_CACHE`, else
/// `$XDG_CACHE_HOME/paper2html`, else `~/.cache/paper2html`, else a
/// directory in the system's temporary directory.
fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("PAPER2HTML_CACHE") {
        return PathBuf::from(dir);
    }
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(dir).join(CACHE_DIR_NAME);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".cache").join(CACHE_DIR_NAME);
    }
    std::env::temp_dir().join(CACHE_DIR_NAME)
}

/// Downloads the PDF of `id` from arXiv (honouring the proxy environment
/// variables).
///
/// # Errors
/// When the request fails, times out, or the response is not a PDF.
fn download(id: &str) -> Result<Vec<u8>> {
    let url = format!("https://arxiv.org/pdf/{id}");
    let agent = ureq::AgentBuilder::new()
        .try_proxy_from_env(true)
        .timeout(DOWNLOAD_TIMEOUT)
        .user_agent(concat!("paper2html/", env!("CARGO_PKG_VERSION")))
        .build();
    let resp = agent.get(&url).call().with_context(|| format!("downloading {url}"))?;
    let mut data = Vec::new();
    resp.into_reader()
        .take(MAX_PDF_BYTES)
        .read_to_end(&mut data)
        .with_context(|| format!("reading {url}"))?;
    if !data.starts_with(PDF_MAGIC) {
        return Err(anyhow!(
            "{url} did not return a PDF (arXiv may not have a PDF for this id)"
        ));
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::parse_id;

    /// New- and old-style ids, prefixed ids and URLs are recognised; file
    /// names are not.
    #[test]
    fn ids() {
        assert_eq!(parse_id("1706.03762").as_deref(), Some("1706.03762"));
        assert_eq!(parse_id("arXiv:1706.03762v7").as_deref(), Some("1706.03762v7"));
        assert_eq!(parse_id("hep-th/9901001").as_deref(), Some("hep-th/9901001"));
        assert_eq!(
            parse_id("https://arxiv.org/abs/1706.03762").as_deref(),
            Some("1706.03762")
        );
        assert_eq!(
            parse_id("https://arxiv.org/pdf/1706.03762v7.pdf").as_deref(),
            Some("1706.03762v7")
        );
        assert_eq!(parse_id("paper.pdf"), None);
        assert_eq!(parse_id("./2010.11929.pdf"), None);
    }
}
