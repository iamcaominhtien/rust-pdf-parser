//! Syntax colour for code listings, done at conversion time (the page needs
//! no highlighter script).
//!
//! The language of a listing extracted from a PDF is unknown, so it is
//! guessed from the text ([`detect`]). A confident guess gives full
//! colouring; otherwise a generic tokenizer marks only strings, comments
//! and numbers. Tokens become `<span class="tok-*">` around the escaped
//! text: the characters are never changed, so copying the code gives the
//! same text with or without colour.

use crate::render::esc;
use regex::Regex;
use std::sync::LazyLock;

/// A language family with its own keywords and comment syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lang {
    /// Python (and Python-like pseudocode).
    Python,
    /// C, C++, C#, Java and similar brace languages.
    CFamily,
    /// JavaScript and TypeScript.
    JavaScript,
    /// Go.
    Go,
    /// Rust.
    Rust,
    /// Unknown: only strings, comments and numbers are marked.
    Generic,
}

/// Tokenizer state carried from one line to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Carry {
    /// Nothing open.
    #[default]
    None,
    /// Inside a `/* ... */` comment.
    BlockComment,
    /// Inside a Python string opened with `"""` or `'''` (the quote char).
    TripleString(char),
}

/// One clue for [`detect`]: a pattern, the language it points to and its
/// weight.
struct Clue {
    /// What to look for (multi-line mode).
    pattern: Regex,
    /// The language it suggests.
    lang: Lang,
    /// How strongly.
    weight: u32,
}

/// A guess is confident when its score reaches this...
const CONFIDENT_MIN_SCORE: u32 = 3;
/// ...and is at least this many times the score of the runner-up.
const CONFIDENT_MIN_LEAD: u32 = 2;

/// Keywords of Python.
const PYTHON_KEYWORDS: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except",
    "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise",
    "return", "try", "while", "with", "yield",
];
/// Keywords of CFamily.
const CFAMILY_KEYWORDS: &[&str] = &[
    "abstract",
    "auto",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "extends",
    "final",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "include",
    "inline",
    "interface",
    "namespace",
    "new",
    "operator",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "sizeof",
    "static",
    "struct",
    "switch",
    "template",
    "throw",
    "throws",
    "try",
    "typedef",
    "typename",
    "union",
    "using",
    "virtual",
    "volatile",
    "while",
];
/// Keywords of JavaScript.
const JAVASCRIPT_KEYWORDS: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "finally",
    "for",
    "from",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "of",
    "return",
    "switch",
    "throw",
    "try",
    "type",
    "typeof",
    "var",
    "while",
    "yield",
];
/// Keywords of Go.
const GO_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
];
/// Keywords of Rust.
const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "fn", "for", "if",
    "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "static", "struct", "trait",
    "type", "unsafe", "use", "where", "while",
];
/// Builtin names and constants of Python.
const PYTHON_BUILTINS: &[&str] = &[
    "True",
    "False",
    "None",
    "self",
    "cls",
    "print",
    "len",
    "range",
    "int",
    "float",
    "str",
    "list",
    "dict",
    "set",
    "tuple",
    "zip",
    "enumerate",
    "max",
    "min",
    "sum",
    "abs",
    "np",
    "torch",
];
/// Builtin names and constants of CFamily.
const CFAMILY_BUILTINS: &[&str] = &[
    "true", "false", "null", "nullptr", "NULL", "this", "int", "void", "char", "double", "float", "bool", "long",
    "short", "unsigned", "signed", "size_t",
];
/// Builtin names and constants of JavaScript.
const JAVASCRIPT_BUILTINS: &[&str] = &[
    "true",
    "false",
    "null",
    "undefined",
    "this",
    "console",
    "Math",
    "JSON",
    "window",
    "document",
];
/// Builtin names and constants of Go.
const GO_BUILTINS: &[&str] = &[
    "true", "false", "nil", "iota", "int", "string", "bool", "byte", "error", "float64", "make", "len", "append", "new",
];
/// Builtin names and constants of Rust.
const RUST_BUILTINS: &[&str] = &[
    "true", "false", "self", "Self", "Some", "None", "Ok", "Err", "i32", "i64", "u8", "u32", "u64", "usize", "f32",
    "f64", "bool", "str", "String", "Vec",
];

/// The clues of [`detect`], compiled once.
static CLUES: LazyLock<Vec<Clue>> = LazyLock::new(|| {
    let clue = |pattern: &str, lang: Lang, weight: u32| Clue {
        pattern: Regex::new(&format!("(?m){pattern}")).unwrap(),
        lang,
        weight,
    };
    vec![
        clue(r"^\s*def \w+\(", Lang::Python, 3),
        clue(r"^\s*(?:import \w|from \w[\w.]* import)", Lang::Python, 3),
        clue(
            r"^\s*(?:if|elif|for|while|with|else|try|except)\b[^{;]*:\s*(?:#.*)?$",
            Lang::Python,
            1,
        ),
        clue(r"\bself\.", Lang::Python, 2),
        clue(r"\b(?:None|True|False|elif|lambda)\b", Lang::Python, 1),
        clue(r"^\s*#\s*\w", Lang::Python, 1),
        clue(r"^\s*#include\b", Lang::CFamily, 4),
        clue(
            r"\bstd::|\bprintf\(|\bpublic static\b|\bSystem\.out\b",
            Lang::CFamily,
            3,
        ),
        clue(
            r"\b(?:int|void|char|double|float|bool|unsigned)\s+\**\w+\s*[(=;,\[]",
            Lang::CFamily,
            2,
        ),
        clue(r";\s*$", Lang::CFamily, 1),
        clue(r"\b(?:function\s+\w*\(|console\.\w+\(|=>|===|!==)", Lang::JavaScript, 3),
        clue(r"^\s*(?:const|let|var)\s+\w+\s*=", Lang::JavaScript, 2),
        clue(r"^\s*(?:package\s+\w+|func\s+(?:\(\w+ \*?\w+\)\s*)?\w+\()", Lang::Go, 4),
        clue(r":=|\bfmt\.\w+\(|\bnil\b", Lang::Go, 2),
        clue(r"^\s*(?:pub\s+)?fn\s+\w+|\blet mut\b|\bimpl\b|\w+!\(", Lang::Rust, 3),
    ]
});

/// Keywords of each language.
fn keywords(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Python => PYTHON_KEYWORDS,
        Lang::CFamily => CFAMILY_KEYWORDS,
        Lang::JavaScript => JAVASCRIPT_KEYWORDS,
        Lang::Go => GO_KEYWORDS,
        Lang::Rust => RUST_KEYWORDS,
        Lang::Generic => &[],
    }
}

/// Builtin names and constants of each language.
fn builtins(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Python => PYTHON_BUILTINS,
        Lang::CFamily => CFAMILY_BUILTINS,
        Lang::JavaScript => JAVASCRIPT_BUILTINS,
        Lang::Go => GO_BUILTINS,
        Lang::Rust => RUST_BUILTINS,
        Lang::Generic => &[],
    }
}

/// Guesses the language of `code`. Returns the language and whether the
/// guess is confident (otherwise the listing is shown plain by default).
pub(crate) fn detect(code: &str) -> (Lang, bool) {
    let mut scores: Vec<(Lang, u32)> = Vec::new();
    for clue in CLUES.iter() {
        let hits = clue.pattern.find_iter(code).count() as u32;
        if hits == 0 {
            continue;
        }
        match scores.iter_mut().find(|(lang, _)| *lang == clue.lang) {
            Some((_, score)) => *score += hits * clue.weight,
            None => scores.push((clue.lang, hits * clue.weight)),
        }
    }
    scores.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
    match scores.as_slice() {
        [] => (Lang::Generic, false),
        [(lang, score)] => (*lang, *score >= CONFIDENT_MIN_SCORE),
        [(lang, score), (_, second), ..] => (
            *lang,
            *score >= CONFIDENT_MIN_SCORE && *score >= CONFIDENT_MIN_LEAD * second,
        ),
    }
}

/// One line of code as HTML with its tokens wrapped in `tok-*` spans.
/// `carry` is the state left by the previous line and is updated.
pub(crate) fn highlight_line(line: &str, lang: Lang, carry: &mut Carry) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut pos = 0;
    while pos < chars.len() {
        let (class, end) = match next_token(&chars, pos, lang, carry) {
            Some(token) => token,
            None => {
                let end = plain_run_end(&chars, pos);
                out.push_str(&esc(&chars[pos..end].iter().collect::<String>()));
                pos = end;
                continue;
            }
        };
        let text: String = chars[pos..end].iter().collect();
        out.push_str(&format!("<span class=\"tok-{class}\">{}</span>", esc(&text)));
        pos = end;
    }
    out
}

/// The token starting at `pos` (its class and end), or `None` when the
/// character starts no token. Also continues a comment or string opened on
/// an earlier line (see [`Carry`]).
fn next_token(chars: &[char], pos: usize, lang: Lang, carry: &mut Carry) -> Option<(&'static str, usize)> {
    match *carry {
        Carry::BlockComment => return Some(("com", block_comment_end(chars, pos, carry))),
        Carry::TripleString(quote) => return Some(("str", triple_string_end(chars, pos, quote, carry))),
        Carry::None => {}
    }
    let ch = chars[pos];
    let next = chars.get(pos + 1).copied();
    if starts_line_comment(chars, pos, lang) {
        return Some(("com", chars.len()));
    }
    if ch == '/' && next == Some('*') && lang != Lang::Python {
        *carry = Carry::BlockComment;
        return Some(("com", block_comment_end(chars, pos + 2, carry)));
    }
    if matches!(ch, '"' | '\'' | '`') {
        return Some(("str", string_end(chars, pos, lang, carry)));
    }
    if ch.is_ascii_digit() || (ch == '.' && next.is_some_and(|nc| nc.is_ascii_digit())) {
        if pos > 0 && is_ident_char(chars[pos - 1]) {
            return None;
        }
        return Some(("num", number_end(chars, pos)));
    }
    if ch == '@' && lang != Lang::Generic && next.is_some_and(is_ident_start) {
        return Some(("dec", ident_end(chars, pos + 1)));
    }
    if is_ident_start(ch) && lang != Lang::Generic {
        return identifier(chars, pos, lang);
    }
    None
}

/// Whether a line comment starts at `pos`: `#` in Python (and generic
/// code, at the start or after a space), `//` in brace languages.
fn starts_line_comment(chars: &[char], pos: usize, lang: Lang) -> bool {
    let ch = chars[pos];
    let next = chars.get(pos + 1).copied();
    let after_space = pos == 0 || chars[pos - 1].is_whitespace();
    match lang {
        Lang::Python => ch == '#',
        Lang::Generic => (ch == '#' && after_space) || (ch == '/' && next == Some('/') && after_space),
        _ => ch == '/' && next == Some('/'),
    }
}

/// An identifier at `pos`: a keyword, builtin, type (capitalised name in
/// brace languages), function (followed by `(`), or plain (`None`).
fn identifier(chars: &[char], pos: usize, lang: Lang) -> Option<(&'static str, usize)> {
    let end = ident_end(chars, pos);
    let word: String = chars[pos..end].iter().collect();
    if pos > 0 && chars[pos - 1] == '.' {
        // A member: only calls are marked.
        return next_non_space(chars, end).filter(|ch| *ch == '(').map(|_| ("fn", end));
    }
    if keywords(lang).contains(&word.as_str()) {
        return Some(("kw", end));
    }
    if builtins(lang).contains(&word.as_str()) {
        return Some(("builtin", end));
    }
    if next_non_space(chars, end) == Some('(') {
        return Some(("fn", end));
    }
    let capitalised = word.chars().next().is_some_and(char::is_uppercase) && word.chars().any(char::is_lowercase);
    if capitalised && lang != Lang::Python {
        return Some(("type", end));
    }
    None
}

/// The end of a run of characters starting at `pos` that start no token: a
/// whole word, or punctuation and spaces up to the next character that may
/// start one.
fn plain_run_end(chars: &[char], pos: usize) -> usize {
    if is_ident_char(chars[pos]) {
        return ident_end(chars, pos);
    }
    let mut end = pos + 1;
    while end < chars.len()
        && !is_ident_char(chars[end])
        && !matches!(chars[end], '"' | '\'' | '`' | '#' | '/' | '@' | '.')
    {
        end += 1;
    }
    end
}

/// The end of a string starting at `pos` (an unclosed string runs to the
/// end of the line; a Python triple quote may continue on the next lines).
fn string_end(chars: &[char], pos: usize, lang: Lang, carry: &mut Carry) -> usize {
    let quote = chars[pos];
    if lang == Lang::Python && chars.get(pos + 1) == Some(&quote) && chars.get(pos + 2) == Some(&quote) {
        *carry = Carry::TripleString(quote);
        return triple_string_end(chars, pos + 3, quote, carry);
    }
    let mut end = pos + 1;
    while end < chars.len() {
        match chars[end] {
            '\\' => end += 2,
            ch if ch == quote => return end + 1,
            _ => end += 1,
        }
    }
    chars.len()
}

/// The end of a triple-quoted string continued from `pos`; clears `carry`
/// when it closes on this line.
fn triple_string_end(chars: &[char], pos: usize, quote: char, carry: &mut Carry) -> usize {
    let mut end = pos;
    while end < chars.len() {
        if chars[end] == quote && chars.get(end + 1) == Some(&quote) && chars.get(end + 2) == Some(&quote) {
            *carry = Carry::None;
            return end + 3;
        }
        end += 1;
    }
    chars.len()
}

/// The end of a block comment continued from `pos`; clears `carry` when it
/// closes on this line.
fn block_comment_end(chars: &[char], pos: usize, carry: &mut Carry) -> usize {
    let mut end = pos;
    while end + 1 < chars.len() {
        if chars[end] == '*' && chars[end + 1] == '/' {
            *carry = Carry::None;
            return end + 2;
        }
        end += 1;
    }
    chars.len()
}

/// The end of a number starting at `pos` (digits, `.`, `_`, exponent,
/// hex digits, type suffixes).
fn number_end(chars: &[char], pos: usize) -> usize {
    let mut end = pos;
    while end < chars.len() {
        let ch = chars[end];
        let exponent_sign = matches!(ch, '+' | '-') && end > pos && matches!(chars[end - 1], 'e' | 'E');
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || exponent_sign {
            end += 1;
        } else {
            break;
        }
    }
    end
}

/// The end of an identifier starting at `pos`.
fn ident_end(chars: &[char], pos: usize) -> usize {
    let mut end = pos;
    while end < chars.len() && is_ident_char(chars[end]) {
        end += 1;
    }
    end.max(pos + 1)
}

/// The first non-space character at or after `pos`.
fn next_non_space(chars: &[char], pos: usize) -> Option<char> {
    chars[pos.min(chars.len())..].iter().copied().find(|ch| *ch != ' ')
}

/// Whether `ch` can start an identifier.
fn is_ident_start(ch: char) -> bool {
    ch.is_alphabetic() || ch == '_'
}

/// Whether `ch` can continue an identifier.
fn is_ident_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Highlights a whole snippet line by line.
    fn colour(code: &str, lang: Lang) -> String {
        let mut carry = Carry::None;
        code.lines()
            .map(|line| highlight_line(line, lang, &mut carry))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Removes the token spans and unescapes the text.
    fn strip(html: &str) -> String {
        let tags = Regex::new(r"</?span[^>]*>").unwrap();
        tags.replace_all(html, "")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&")
    }

    #[test]
    fn detects_common_languages() {
        assert_eq!(
            detect("import numpy as np\ndef f(x):\n    return np.exp(x)\n"),
            (Lang::Python, true)
        );
        assert_eq!(
            detect("#include <stdio.h>\nint main(void) {\n  printf(\"hi\");\n}\n").0,
            Lang::CFamily
        );
        assert_eq!(
            detect("const f = (x) => x * 2;\nconsole.log(f(3));\n").0,
            Lang::JavaScript
        );
        assert_eq!(detect("package main\nfunc main() {\n  x := 1\n}\n").0, Lang::Go);
        assert!(!detect("my dog is [MASK]").1);
    }

    #[test]
    fn colouring_keeps_the_text() {
        let code = "def f(a, b=\"x<y\"):  # note & more\n    return a @ b + 1.5e-3\n'''doc\nstring'''";
        assert_eq!(strip(&colour(code, Lang::Python)), code);
        let html = colour(code, Lang::Python);
        assert!(html.contains("<span class=\"tok-kw\">def</span>"));
        assert!(html.contains("<span class=\"tok-fn\">f</span>"));
        assert!(html.contains("<span class=\"tok-com\"># note &amp; more</span>"));
        assert!(html.contains("<span class=\"tok-num\">1.5e-3</span>"));
        assert!(html.contains("<span class=\"tok-str\">string'''</span>"));
        let brace = "/* a\n b */ int x = 0x1F; // end";
        assert_eq!(strip(&colour(brace, Lang::CFamily)), brace);
    }
}
