//! Displayed formulas written as MathML from their rebuilt structure
//! ([`MathNode`]), for the reflowing renderers.

use super::esc;
use crate::model::MathNode;

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

/// Styles of the formulas, added to the page when it has any: a math font
/// and a size close to the one of the pictures.
pub(crate) const MATH_CSS: &str =
    "math.eq-math{font-family:\"Latin Modern Math\",\"STIX Two Math\",\"Cambria Math\",math;font-size:1.05em}\n";

/// The `<math>` element of a displayed formula.
pub(crate) fn math_html(node: &MathNode, alt: &str) -> String {
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
                Some(height) => format!(" minsize=\"{height:.2}em\" maxsize=\"{height:.2}em\""),
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
        MathNode::Scripts { base, sub, sup } => match (sub, sup) {
            (Some(sub), Some(sup)) => element("msubsup", "", &[base.as_ref(), sub.as_ref(), sup.as_ref()], out),
            (Some(sub), None) => element("msub", "", &[base.as_ref(), sub.as_ref()], out),
            (None, Some(sup)) => element("msup", "", &[base.as_ref(), sup.as_ref()], out),
            (None, None) => write_node(base, out),
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
        MathNode::Table { rows, align } => {
            // Lines of a display are in display style (MathML makes table
            // cells compact by default).
            out.push_str("<mtable displaystyle=\"true\">");
            for row in rows {
                out.push_str("<mtr>");
                for (col, cell) in row.iter().enumerate() {
                    let align = align.get(col).copied().unwrap_or("center");
                    out.push_str(&format!("<mtd columnalign=\"{align}\" style=\"text-align:{align}\">"));
                    write_node(cell, out);
                    out.push_str("</mtd>");
                }
                out.push_str("</mtr>");
            }
            out.push_str("</mtable>");
        }
    }
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
             <mo minsize=\"2.40em\" maxsize=\"2.40em\" lspace=\"0\" rspace=\"0\">)</mo><mo>=</mo>"
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
        assert!(out.starts_with("<mtable displaystyle=\"true\"><mtr><mtd columnalign=\"right\""));
    }
}
