//! The structure of a displayed formula, rebuilt from the positions of its
//! glyphs and rules: fractions, scripts, radicals, limits, rows. Written
//! out as MathML by the reflowing renderers.

/// A node of a formula tree. The variants follow the MathML elements they
/// become.
#[derive(Debug, Clone, PartialEq)]
pub enum MathNode {
    /// An identifier (`<mi>`): a variable, or a function name of several
    /// letters.
    Ident {
        /// The letters, already mapped to the Unicode mathematical
        /// alphanumerics for bold, script, double-struck and fraktur.
        text: String,
        /// A single letter set upright (`\mathrm{d}`), which MathML would
        /// otherwise show in italics.
        upright: bool,
    },
    /// A number (`<mn>`).
    Number(String),
    /// An operator, a relation, a delimiter or punctuation (`<mo>`).
    Operator {
        /// The symbol.
        text: String,
        /// For a delimiter drawn taller than the text (`\left(`), its
        /// height in em; `None` for a delimiter of normal size, which must
        /// not stretch.
        height_em: Option<f32>,
    },
    /// Running text inside the formula (`<mtext>`): "where", "if".
    Text(String),
    /// A horizontal space (`<mspace>`), in em.
    Space(f32),
    /// A sequence (`<mrow>`).
    Row(Vec<MathNode>),
    /// A fraction (`<mfrac>`).
    Frac {
        /// Above the bar.
        num: Box<MathNode>,
        /// Below the bar.
        den: Box<MathNode>,
    },
    /// A square root (`<msqrt>`).
    Sqrt(Box<MathNode>),
    /// A root with an index (`<mroot>`).
    Root {
        /// Under the radical.
        base: Box<MathNode>,
        /// The index in the crook of the radical sign.
        index: Box<MathNode>,
    },
    /// Subscript and/or superscript (`<msub>`, `<msup>`, `<msubsup>`).
    Scripts {
        /// What the scripts are attached to.
        base: Box<MathNode>,
        /// Lowered script.
        sub: Option<Box<MathNode>>,
        /// Raised script.
        sup: Option<Box<MathNode>>,
    },
    /// Limits or accents above and/or below (`<munder>`, `<mover>`,
    /// `<munderover>`).
    UnderOver {
        /// The operator or expression.
        base: Box<MathNode>,
        /// Below.
        under: Option<Box<MathNode>>,
        /// Above.
        over: Option<Box<MathNode>>,
        /// The part above is an accent (a hat, a bar), drawn close to the
        /// base.
        accent: bool,
    },
    /// Rows of a multi-line display (`<mtable>`), each a list of cells.
    Table {
        /// The cells of each row.
        rows: Vec<Vec<MathNode>>,
        /// Alignment of each column: "left", "center" or "right".
        align: Vec<&'static str>,
    },
}

impl MathNode {
    /// A row of `nodes`, or the node itself when there is exactly one.
    pub fn row(mut nodes: Vec<MathNode>) -> MathNode {
        if nodes.len() == 1 {
            nodes.remove(0)
        } else {
            MathNode::Row(nodes)
        }
    }
}
