//! Displayed formulas written as MathML from their rebuilt structure
//! ([`MathNode`]), for the reflowing renderers.

use super::{esc, font_face};
use crate::model::MathNode;
use std::sync::LazyLock;

/// Latin Modern Math (GUST Font License), the math font of LaTeX, so that
/// formulas look as in the papers: subset to the letters, symbols and math
/// alphabets of formulas, with its MATH table (see `fonts/README.md`).
const LATIN_MODERN_MATH: &[u8] = include_bytes!("fonts/latin-modern-math.woff2");

/// Operators that MathML stretches by default: written with
/// `stretchy="false"` unless the PDF drew them taller than the text, so
/// that a parenthesis stays the size it has in the paper.
const STRETCHY: &[&str] = &[
    "(", ")", "[", "]", "{", "}", "|", "‖", "⟨", "⟩", "⌊", "⌋", "⌈", "⌉", "/", "\\", "↑", "↓", "→", "←",
];

/// Operators that TeX sets without space around them (delimiters, the
/// slash), unlike MathML, which spaces them when they are not at the ends of
/// a row.
const UNSPACED: &[&str] = &[
    "(", ")", "[", "]", "{", "}", "|", "‖", "⟨", "⟩", "⌊", "⌋", "⌈", "⌉", "/", "\\",
];

/// How far a prime above a subscript is lowered: written as a superscript,
/// it would be raised twice (the glyph sits high already).
const PRIME_DROP: &str = "-0.45em";

/// Style of the formulas: the math font (the embedded one first, then
/// those of the usual systems) and a size close to the one of the pictures.
const MATH_STYLE: &str =
    "math.eq-math{font-family:\"Latin Modern Math\",\"STIX Two Math\",\"Cambria Math\",math;font-size:1.05em}\n";

/// The `@font-face` rule of the embedded math font, built once.
pub(crate) static MATH_FACE: LazyLock<String> =
    LazyLock::new(|| font_face("Latin Modern Math", "normal", "400", LATIN_MODERN_MATH));

/// The styles a page with MathML formulas needs, with the math font
/// embedded when `embed_font` (otherwise the reader's math font is used).
pub(crate) fn math_css(embed_font: bool) -> String {
    if embed_font {
        format!("{}{MATH_STYLE}", *MATH_FACE)
    } else {
        MATH_STYLE.to_string()
    }
}

/// The `<math>` element of a displayed formula.
pub fn math_html(node: &MathNode, alt: &str) -> String {
    let mut out = format!("<math display=\"block\" class=\"eq-math\" alttext=\"{}\">", esc(alt));
    write_node(node, &mut out);
    out.push_str("</math>");
    out
}

/// Writes `node` as exactly one MathML element.
fn write_node(node: &MathNode, out: &mut String) {
    match node {
        MathNode::Ident { text, upright } => {
            let variant = if *upright && text.chars().count() == 1 {
                " mathvariant=\"normal\""
            } else {
                ""
            };
            out.push_str(&format!("<mi{variant}>{}</mi>", esc(text)));
        }
        MathNode::Number(text) => out.push_str(&format!("<mn>{}</mn>", esc(text))),
        MathNode::Operator { text, height_em } => {
            let mut attrs = match height_em {
                // Chromium stretches `|` and some other delimiters only at
                // the ends of a row unless told to.
                Some(height) => format!(" stretchy=\"true\" minsize=\"{height:.2}em\" maxsize=\"{height:.2}em\""),
                None if STRETCHY.contains(&text.as_str()) => " stretchy=\"false\"".to_string(),
                None => String::new(),
            };
            if UNSPACED.contains(&text.as_str()) {
                attrs.push_str(" lspace=\"0\" rspace=\"0\"");
            }
            out.push_str(&format!("<mo{attrs}>{}</mo>", esc(text)));
        }
        MathNode::Text(text) => out.push_str(&format!("<mtext>{}</mtext>", esc(text))),
        MathNode::Space(width) => out.push_str(&format!("<mspace width=\"{width:.2}em\"></mspace>")),
        MathNode::Row(nodes) => {
            out.push_str("<mrow>");
            nodes.iter().for_each(|node| write_node(node, out));
            out.push_str("</mrow>");
        }
        MathNode::Frac { num, den } => element("mfrac", "", &[num.as_ref(), den.as_ref()], out),
        MathNode::Sqrt(base) => element("msqrt", "", &[base.as_ref()], out),
        MathNode::Root { base, index } => element("mroot", "", &[base.as_ref(), index.as_ref()], out),
        MathNode::Scripts { base, sub, sup } => match (sub, sup.as_deref().and_then(primes)) {
            // A prime is drawn raised already: as a superscript it would be
            // raised twice.
            (None, Some(primes)) => {
                out.push_str("<mrow>");
                write_node(base, out);
                out.push_str(&format!("<mo lspace=\"0\" rspace=\"0\">{primes}</mo></mrow>"));
            }
            (Some(sub), Some(primes)) => {
                out.push_str("<msubsup>");
                write_base(base, out);
                write_node(sub, out);
                out.push_str(&format!(
                    "<mpadded voffset=\"{PRIME_DROP}\"><mo lspace=\"0\" rspace=\"0\">{primes}</mo></mpadded></msubsup>"
                ));
            }
            (sub, None) => write_scripts(base, sub.as_deref(), sup.as_deref(), out),
        },
        MathNode::UnderOver {
            base,
            under,
            over,
            accent,
        } => {
            let accent = if *accent { " accent=\"true\"" } else { "" };
            match (under, over) {
                (Some(under), Some(over)) => {
                    element("munderover", "", &[base.as_ref(), under.as_ref(), over.as_ref()], out)
                }
                (Some(under), None) => element("munder", "", &[base.as_ref(), under.as_ref()], out),
                (None, Some(over)) => element("mover", accent, &[base.as_ref(), over.as_ref()], out),
                (None, None) => write_node(base, out),
            }
        }
        MathNode::Table { rows, align } => write_table(rows, align, out),
    }
}

/// Writes the rows of a table, each cell aligned as `align` says for its
/// column.
fn write_table(rows: &[Vec<MathNode>], align: &[&str], out: &mut String) {
    // Lines of a display are in display style (MathML makes table cells
    // compact by default).
    out.push_str("<mtable displaystyle=\"true\">");
    let aligned = align == ["right", "left"];
    for row in rows {
        out.push_str("<mtr>");
        for (col, cell) in row.iter().enumerate() {
            let align = align.get(col).copied().unwrap_or("center");
            // Firefox and Safari follow columnalign; Chromium ignores it and
            // text-align:right, but follows the -webkit- values. Lines aligned
            // on a relation have no gap at the relation.
            let pad = match (aligned, align) {
                (true, "right") => ";padding-right:0",
                (true, "left") => ";padding-left:0",
                _ => "",
            };
            out.push_str(&format!(
                "<mtd columnalign=\"{align}\" style=\"text-align:-webkit-{align}{pad}\">"
            ));
            write_node(cell, out);
            out.push_str("</mtd>");
        }
        out.push_str("</mtr>");
    }
    out.push_str("</mtable>");
}

/// Writes a base with its subscript and/or superscript.
fn write_scripts(base: &MathNode, sub: Option<&MathNode>, sup: Option<&MathNode>, out: &mut String) {
    let tag = match (sub, sup) {
        (Some(_), Some(_)) => "msubsup",
        (Some(_), None) => "msub",
        (None, Some(_)) => "msup",
        (None, None) => return write_node(base, out),
    };
    out.push_str(&format!("<{tag}>"));
    write_base(base, out);
    sub.into_iter().chain(sup).for_each(|script| write_node(script, out));
    out.push_str(&format!("</{tag}>"));
}

/// Writes the base of scripts. A delimiter of a given height is put in a
/// row: as the base itself, Chromium does not stretch it (`|…|^{-1}`).
fn write_base(base: &MathNode, out: &mut String) {
    if matches!(base, MathNode::Operator { height_em: Some(_), .. }) {
        out.push_str("<mrow>");
        write_node(base, out);
        out.push_str("</mrow>");
    } else {
        write_node(base, out);
    }
}

/// The text of a superscript made of primes only (`′`, `″`), or `None`.
fn primes(node: &MathNode) -> Option<String> {
    let text = match node {
        MathNode::Operator { text, .. } => text.clone(),
        MathNode::Row(nodes) if !nodes.is_empty() => nodes.iter().map(primes).collect::<Option<String>>()?,
        _ => return None,
    };
    text.chars().all(|ch| matches!(ch, '′'..='‴')).then_some(text)
}

/// Writes the element `tag` with attributes `attrs` around `children`.
fn element(tag: &str, attrs: &str, children: &[&MathNode], out: &mut String) {
    out.push_str(&format!("<{tag}{attrs}>"));
    for child in children {
        write_node(child, out);
    }
    out.push_str(&format!("</{tag}>"));
}

#[cfg(test)]
#[allow(clippy::min_ident_chars, clippy::missing_docs_in_private_items)]
mod tests {
    use super::*;

    fn op(text: &str, height_em: Option<f32>) -> MathNode {
        MathNode::Operator {
            text: text.into(),
            height_em,
        }
    }

    fn mi(text: &str) -> MathNode {
        MathNode::Ident {
            text: text.into(),
            upright: false,
        }
    }

    #[test]
    fn every_node_is_one_element() {
        let node = MathNode::Frac {
            num: Box::new(MathNode::Row(vec![
                mi("a"),
                op("+", None),
                MathNode::Number("1".into()),
            ])),
            den: Box::new(MathNode::Sqrt(Box::new(mi("b")))),
        };
        assert_eq!(
            math_html(&node, "a+1 b"),
            "<math display=\"block\" class=\"eq-math\" alttext=\"a+1 b\"><mfrac><mrow><mi>a</mi><mo>+</mo>\
             <mn>1</mn></mrow><msqrt><mi>b</mi></msqrt></mfrac></math>"
        );
    }

    #[test]
    fn delimiters_keep_their_size_and_spacing() {
        let mut out = String::new();
        write_node(&op("(", None), &mut out);
        write_node(&op(")", Some(2.4)), &mut out);
        write_node(&op("=", None), &mut out);
        assert_eq!(
            out,
            "<mo stretchy=\"false\" lspace=\"0\" rspace=\"0\">(</mo>\
             <mo stretchy=\"true\" minsize=\"2.40em\" maxsize=\"2.40em\" lspace=\"0\" rspace=\"0\">)</mo><mo>=</mo>"
        );
    }

    #[test]
    fn primes_are_not_raised_twice() {
        let prime = |sub: Option<MathNode>| MathNode::Scripts {
            base: Box::new(mi("x")),
            sub: sub.map(Box::new),
            sup: Some(Box::new(op("′", None))),
        };
        let mut out = String::new();
        write_node(&prime(None), &mut out);
        assert_eq!(out, "<mrow><mi>x</mi><mo lspace=\"0\" rspace=\"0\">′</mo></mrow>");
        let mut out = String::new();
        write_node(&prime(Some(mi("i"))), &mut out);
        assert_eq!(
            out,
            "<msubsup><mi>x</mi><mi>i</mi><mpadded voffset=\"-0.45em\"><mo lspace=\"0\" rspace=\"0\">′</mo></mpadded></msubsup>"
        );
    }

    #[test]
    fn a_tall_delimiter_with_scripts_is_in_a_row() {
        let node = MathNode::Scripts {
            base: Box::new(op("|", Some(2.4))),
            sub: None,
            sup: Some(Box::new(MathNode::Number("1".into()))),
        };
        let mut out = String::new();
        write_node(&node, &mut out);
        assert_eq!(
            out,
            "<msup><mrow><mo stretchy=\"true\" minsize=\"2.40em\" maxsize=\"2.40em\" lspace=\"0\" rspace=\"0\">|</mo></mrow><mn>1</mn></msup>"
        );
    }

    #[test]
    fn upright_single_letters_say_so() {
        let mut out = String::new();
        write_node(
            &MathNode::Ident {
                text: "d".into(),
                upright: true,
            },
            &mut out,
        );
        write_node(&mi("x"), &mut out);
        assert_eq!(out, "<mi mathvariant=\"normal\">d</mi><mi>x</mi>");
    }

    #[test]
    fn accents_and_tables() {
        let mut out = String::new();
        let hat = MathNode::UnderOver {
            base: Box::new(mi("v")),
            under: None,
            over: Some(Box::new(op("^", None))),
            accent: true,
        };
        write_node(&hat, &mut out);
        assert_eq!(out, "<mover accent=\"true\"><mi>v</mi><mo>^</mo></mover>");
        let mut out = String::new();
        let table = MathNode::Table {
            rows: vec![vec![mi("x"), MathNode::Row(vec![op("=", None), mi("y")])]],
            align: vec!["right", "left"],
        };
        write_node(&table, &mut out);
        assert!(out.starts_with(
            "<mtable displaystyle=\"true\"><mtr><mtd columnalign=\"right\" style=\"text-align:-webkit-right;padding-right:0\"><mi>x</mi></mtd>"
        ));
    }
}
