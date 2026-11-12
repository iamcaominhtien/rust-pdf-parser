//! Fetching papers from arXiv.

use anyhow::{Context, Result, anyhow};
use regex::Regex;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static NEW_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{4}\.\d{4,5}(v\d+)?$").unwrap());
static OLD_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z\-]*(\.[A-Z]{2})?/\d{7}(v\d+)?$").unwrap());
static URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https?://(?:www\.|export\.)?arxiv\.org/(?:abs|pdf|html)/(?P<id>[^?#]+?)(?:\.pdf)?(?:[?#].*)?$").unwrap()
});

/// Extracts an arXiv identifier from an id (`1706.03762`, `arXiv:1706.03762v7`,
/// `hep-th/9901001`) or an arXiv URL.
pub fn parse_id(input: &str) -> Option<String> {
    let s = input.trim();
    let s = s.strip_prefix("arXiv:").or_else(|| s.strip_prefix("arxiv:")).unwrap_or(s);
    let candidate = match URL.captures(s) {
        Some(c) => c["id"].to_string(),
        None => s.to_string(),
    };
    (NEW_ID.is_match(&candidate) || OLD_ID.is_match(&candidate)).then_some(candidate)
}

fn cache_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("PAPER2HTML_CACHE") {
        return PathBuf::from(d);
    }
    if let Some(d) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(d).join("paper2html");
    }
    if let Some(h) = std::env::var_os("HOME") {
        return PathBuf::from(h).join(".cache").join("paper2html");
    }
    std::env::temp_dir().join("paper2html")
}

/// Downloads the PDF of an arXiv paper and returns its bytes. Downloads are
/// cached on disk so that repeated conversions do not hit arXiv again.
pub fn fetch_pdf(id: &str) -> Result<Vec<u8>> {
    let file = cache_dir().join(format!("{}.pdf", id.replace('/', "_")));
    if let Ok(data) = std::fs::read(&file) {
        if data.starts_with(b"%PDF") {
            return Ok(data);
        }
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

fn download(id: &str) -> Result<Vec<u8>> {
    let url = format!("https://arxiv.org/pdf/{id}");
    let agent = ureq::AgentBuilder::new()
        .try_proxy_from_env(true)
        .timeout(std::time::Duration::from_secs(120))
        .user_agent(concat!("paper2html/", env!("CARGO_PKG_VERSION")))
        .build();
    let resp = agent.get(&url).call().with_context(|| format!("downloading {url}"))?;
    let mut data = Vec::new();
    resp.into_reader()
        .take(300 * 1024 * 1024)
        .read_to_end(&mut data)
        .with_context(|| format!("reading {url}"))?;
    if !data.starts_with(b"%PDF") {
        return Err(anyhow!("{url} did not return a PDF (arXiv may not have a PDF for this id)"));
    }
    Ok(data)
}

/// Saves an arXiv PDF to `dest`.
pub fn save_pdf(id: &str, dest: &Path) -> Result<()> {
    let data = fetch_pdf(id)?;
    std::fs::write(dest, data).with_context(|| format!("writing {}", dest.display()))
}

#[cfg(test)]
mod tests {
    use super::parse_id;

    #[test]
    fn ids() {
        assert_eq!(parse_id("1706.03762").as_deref(), Some("1706.03762"));
        assert_eq!(parse_id("arXiv:1706.03762v7").as_deref(), Some("1706.03762v7"));
        assert_eq!(parse_id("hep-th/9901001").as_deref(), Some("hep-th/9901001"));
        assert_eq!(parse_id("https://arxiv.org/abs/1706.03762").as_deref(), Some("1706.03762"));
        assert_eq!(parse_id("https://arxiv.org/pdf/1706.03762v7.pdf").as_deref(), Some("1706.03762v7"));
        assert_eq!(parse_id("paper.pdf"), None);
        assert_eq!(parse_id("./2010.11929.pdf"), None);
    }
}
