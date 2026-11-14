//! Options and tuning parameters of the analysis.

use crate::analysis::blocks::BlocksConfig;
use crate::analysis::floats::FloatsConfig;
use crate::analysis::furniture::FurnitureConfig;
use crate::analysis::lines::LinesConfig;
use crate::analysis::math::MathConfig;
use crate::analysis::order::OrderConfig;
use crate::analysis::page::PageConfig;
use crate::analysis::semantics::SemanticsConfig;
use crate::analysis::tables::TablesConfig;

/// Options of the analysis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalyzeOptions {
    /// Reconstruct the row/column structure of tables.
    pub tables: bool,
    /// Worker threads for reading the pages (0 = one per CPU).
    pub jobs: usize,
    /// Thresholds of the analysis stages.
    pub layout: LayoutConfig,
}

impl Default for AnalyzeOptions {
    fn default() -> Self {
        Self {
            tables: true,
            jobs: 0,
            layout: LayoutConfig::default(),
        }
    }
}

/// Every threshold of the analysis, grouped by stage. The defaults are the
/// values the analysis is tuned for; each field is documented where its
/// struct is defined.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LayoutConfig {
    /// Document-wide statistics of the pages.
    pub page: PageConfig,
    /// Grouping glyphs into lines.
    pub lines: LinesConfig,
    /// Removal of running heads, page numbers and margin stamps.
    pub furniture: FurnitureConfig,
    /// Detection of figures, tables and their captions.
    pub floats: FloatsConfig,
    /// Reconstruction of table grids.
    pub tables: TablesConfig,
    /// Reading order.
    pub order: OrderConfig,
    /// Grouping lines into blocks.
    pub blocks: BlocksConfig,
    /// Classification, paragraph merging and assembly of the document.
    pub semantics: SemanticsConfig,
    /// Rebuilding the structure of displayed formulas.
    pub math: MathConfig,
}
