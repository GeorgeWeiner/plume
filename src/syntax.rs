//! Lightweight hand-rolled syntax highlighter.
//!
//! One pass per line, with a single carried state: "am I inside a block
//! comment". Good enough to make code *look* right without pulling in a
//! grammar engine.

use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tok {
    Keyword,
    Type,
    Fn,
    String,
    Comment,
    Number,
    Constant,
    Punct,
    Attr,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Json,
    Toml,
    Yaml,
    Markdown,
    C,
    Go,
    Shell,
    Html,
    Css,
    Plain,
}

impl Language {
    pub fn from_path(path: &Path) -> Language {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "rs" => Language::Rust,
            "py" | "pyw" => Language::Python,
            "js" | "jsx" | "mjs" | "cjs" => Language::JavaScript,
            "ts" | "tsx" => Language::TypeScript,
            "json" => Language::Json,
            "toml" => Language::Toml,
            "yaml" | "yml" => Language::Yaml,
            "md" | "markdown" => Language::Markdown,
            "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" => Language::C,
            "go" => Language::Go,
            "sh" | "bash" | "zsh" => Language::Shell,
            "html" | "htm" => Language::Html,
            "css" | "scss" => Language::Css,
            _ => Language::Plain,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Language::Rust => "Rust",
            Language::Python => "Python",
            Language::JavaScript => "JavaScript",
            Language::TypeScript => "TypeScript",
            Language::Json => "JSON",
            Language::Toml => "TOML",
            Language::Yaml => "YAML",
            Language::Markdown => "Markdown",
            Language::C => "C/C++",
            Language::Go => "Go",
            Language::Shell => "Shell",
            Language::Html => "HTML",
            Language::Css => "CSS",
            Language::Plain => "Plain Text",
        }
    }

    /// Prefix used by Ctrl+/ toggle-comment.
    pub fn comment_prefix(&self) -> Option<&'static str> {
        match self {
            Language::Rust
            | Language::JavaScript
            | Language::TypeScript
            | Language::C
            | Language::Go => Some("//"),
            Language::Python | Language::Toml | Language::Yaml | Language::Shell => Some("#"),
            _ => None,
        }
    }
}

struct Cfg {
    line_comment: Option<&'static str>,
    block: Option<(&'static str, &'static str)>,
    strings: &'static [char],
    keywords: &'static [&'static str],
    constants: &'static [&'static str],
    types: &'static [&'static str],
    cap_types: bool,
    macro_bang: bool,
    hash_attr: bool,   // rust `#[...]`
    hash_preproc: bool, // c `#include`
    decorator: bool,   // python `@name`
    key_eq: bool,      // toml `key =`
    key_colon: bool,   // yaml/json keys
}

const RUST_KW: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
    "extern", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut",
    "pub", "ref", "return", "static", "struct", "super", "trait", "type", "unsafe", "use",
    "where", "while",
];
const RUST_TYPES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64", "u128", "f32", "f64",
    "usize", "isize", "bool", "char", "str",
];
const RUST_CONST: &[&str] = &["true", "false", "self", "Self", "None", "Some", "Ok", "Err"];

const PY_KW: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "case", "class", "continue", "def",
    "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in",
    "is", "lambda", "match", "nonlocal", "not", "or", "pass", "raise", "return", "try",
    "while", "with", "yield",
];
const PY_CONST: &[&str] = &["True", "False", "None", "self", "cls"];
const PY_TYPES: &[&str] = &["int", "str", "float", "bool", "list", "dict", "set", "tuple"];

const JS_KW: &[&str] = &[
    "async", "await", "break", "case", "catch", "class", "const", "continue", "debugger",
    "default", "delete", "do", "else", "export", "extends", "finally", "for", "from",
    "function", "get", "if", "import", "in", "instanceof", "let", "new", "of", "return",
    "set", "static", "super", "switch", "throw", "try", "typeof", "var", "while", "yield",
];
const TS_KW: &[&str] = &[
    "abstract", "any", "as", "async", "await", "break", "case", "catch", "class", "const",
    "continue", "declare", "default", "delete", "do", "else", "enum", "export", "extends",
    "finally", "for", "from", "function", "get", "if", "implements", "import", "in",
    "instanceof", "interface", "let", "namespace", "new", "of", "private", "protected",
    "public", "readonly", "return", "set", "static", "super", "switch", "throw", "try",
    "type", "typeof", "var", "while", "yield",
];
const JS_CONST: &[&str] = &["true", "false", "null", "undefined", "this", "NaN", "Infinity"];
const TS_TYPES: &[&str] = &["string", "number", "boolean", "void", "never", "unknown", "object"];

const GO_KW: &[&str] = &[
    "break", "case", "chan", "const", "continue", "default", "defer", "else", "fallthrough",
    "for", "func", "go", "goto", "if", "import", "interface", "map", "package", "range",
    "return", "select", "struct", "switch", "type", "var",
];
const GO_CONST: &[&str] = &["true", "false", "nil", "iota"];
const GO_TYPES: &[&str] = &[
    "int", "int8", "int16", "int32", "int64", "uint", "uint8", "uint16", "uint32", "uint64",
    "float32", "float64", "string", "bool", "byte", "rune", "error", "any",
];

const C_KW: &[&str] = &[
    "auto", "break", "case", "class", "const", "continue", "default", "delete", "do", "else",
    "enum", "extern", "for", "goto", "if", "inline", "namespace", "new", "operator",
    "private", "protected", "public", "register", "return", "sizeof", "static", "struct",
    "switch", "template", "typedef", "typename", "union", "using", "virtual", "volatile",
    "while",
];
const C_CONST: &[&str] = &["true", "false", "NULL", "nullptr", "this"];
const C_TYPES: &[&str] = &[
    "void", "char", "short", "int", "long", "float", "double", "signed", "unsigned", "bool",
    "size_t", "int8_t", "int16_t", "int32_t", "int64_t", "uint8_t", "uint16_t", "uint32_t",
    "uint64_t",
];

const SH_KW: &[&str] = &[
    "case", "do", "done", "elif", "else", "esac", "exit", "export", "fi", "for", "function",
    "if", "in", "local", "return", "select", "then", "until", "while", "echo", "source",
    "set", "shift", "read",
];

fn cfg(lang: Language) -> Cfg {
    let base = Cfg {
        line_comment: None,
        block: None,
        strings: &['"', '\''],
        keywords: &[],
        constants: &[],
        types: &[],
        cap_types: false,
        macro_bang: false,
        hash_attr: false,
        hash_preproc: false,
        decorator: false,
        key_eq: false,
        key_colon: false,
    };
    match lang {
        Language::Rust => Cfg {
            line_comment: Some("//"),
            block: Some(("/*", "*/")),
            strings: &['"'],
            keywords: RUST_KW,
            constants: RUST_CONST,
            types: RUST_TYPES,
            cap_types: true,
            macro_bang: true,
            hash_attr: true,
            ..base
        },
        Language::Python => Cfg {
            line_comment: Some("#"),
            keywords: PY_KW,
            constants: PY_CONST,
            types: PY_TYPES,
            cap_types: true,
            decorator: true,
            ..base
        },
        Language::JavaScript => Cfg {
            line_comment: Some("//"),
            block: Some(("/*", "*/")),
            strings: &['"', '\'', '`'],
            keywords: JS_KW,
            constants: JS_CONST,
            cap_types: true,
            ..base
        },
        Language::TypeScript => Cfg {
            line_comment: Some("//"),
            block: Some(("/*", "*/")),
            strings: &['"', '\'', '`'],
            keywords: TS_KW,
            constants: JS_CONST,
            types: TS_TYPES,
            cap_types: true,
            ..base
        },
        Language::Json => Cfg { key_colon: true, ..base },
        Language::Toml => Cfg { line_comment: Some("#"), key_eq: true, ..base },
        Language::Yaml => Cfg { line_comment: Some("#"), key_colon: true, ..base },
        Language::C => Cfg {
            line_comment: Some("//"),
            block: Some(("/*", "*/")),
            keywords: C_KW,
            constants: C_CONST,
            types: C_TYPES,
            hash_preproc: true,
            ..base
        },
        Language::Go => Cfg {
            line_comment: Some("//"),
            block: Some(("/*", "*/")),
            strings: &['"', '\'', '`'],
            keywords: GO_KW,
            constants: GO_CONST,
            types: GO_TYPES,
            cap_types: true,
            ..base
        },
        Language::Shell => Cfg {
            line_comment: Some("#"),
            keywords: SH_KW,
            ..base
        },
        Language::Css => Cfg {
            block: Some(("/*", "*/")),
            ..base
        },
        _ => base,
    }
}

/// Highlight one line. Returns (ranges in char indices, ends-inside-block-comment).
pub fn scan_line(lang: Language, line: &str, in_block: bool) -> (Vec<(usize, usize, Tok)>, bool) {
    match lang {
        Language::Markdown => (scan_markdown(line), false),
        Language::Html => scan_html(line, in_block),
        Language::Plain => (Vec::new(), false),
        _ => scan_generic(lang, line, in_block),
    }
}

fn starts_with_at(chars: &[char], i: usize, pat: &str) -> bool {
    let mut j = i;
    for pc in pat.chars() {
        if j >= chars.len() || chars[j] != pc {
            return false;
        }
        j += 1;
    }
    true
}

fn find_sub(chars: &[char], from: usize, pat: &str) -> Option<usize> {
    let n = chars.len();
    (from..n).find(|&i| starts_with_at(chars, i, pat))
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn scan_generic(lang: Language, line: &str, mut in_block: bool) -> (Vec<(usize, usize, Tok)>, bool) {
    let cfg = cfg(lang);
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut out: Vec<(usize, usize, Tok)> = Vec::new();
    let mut i = 0usize;

    while i < n {
        if in_block {
            let close = cfg.block.map(|b| b.1).unwrap_or("*/");
            match find_sub(&chars, i, close) {
                Some(j) => {
                    out.push((i, j + close.chars().count(), Tok::Comment));
                    i = j + close.chars().count();
                    in_block = false;
                }
                None => {
                    out.push((i, n, Tok::Comment));
                    return (out, true);
                }
            }
            continue;
        }

        let c = chars[i];

        // line comment
        if let Some(lc) = cfg.line_comment {
            if starts_with_at(&chars, i, lc) {
                out.push((i, n, Tok::Comment));
                break;
            }
        }
        // block comment open
        if let Some((open, _)) = cfg.block {
            if starts_with_at(&chars, i, open) {
                in_block = true;
                continue;
            }
        }
        // strings
        if cfg.strings.contains(&c) && !(lang == Language::Rust && c == '\'') {
            let start = i;
            i += 1;
            while i < n {
                if chars[i] == '\\' {
                    i += 2;
                    continue;
                }
                if chars[i] == c {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let end = i.min(n);
            // JSON/YAML keys: string followed by ':' colors as a key
            let tok = if cfg.key_colon {
                let mut k = end;
                while k < n && chars[k] == ' ' {
                    k += 1;
                }
                if k < n && chars[k] == ':' {
                    Tok::Fn
                } else {
                    Tok::String
                }
            } else {
                Tok::String
            };
            out.push((start, end, tok));
            continue;
        }
        // rust: char literal or lifetime
        if lang == Language::Rust && c == '\'' {
            if i + 2 < n && chars[i + 1] == '\\' {
                if let Some(j) = find_sub(&chars, i + 2, "'") {
                    out.push((i, j + 1, Tok::String));
                    i = j + 1;
                    continue;
                }
            }
            if i + 2 < n && chars[i + 2] == '\'' {
                out.push((i, i + 3, Tok::String));
                i += 3;
                continue;
            }
            // lifetime
            let start = i;
            i += 1;
            while i < n && is_word(chars[i]) {
                i += 1;
            }
            out.push((start, i, Tok::Type));
            continue;
        }
        // rust attribute `#[...]`
        if cfg.hash_attr && c == '#' && i + 1 < n && (chars[i + 1] == '[' || chars[i + 1] == '!') {
            let end = find_sub(&chars, i, "]").map(|j| j + 1).unwrap_or(n);
            out.push((i, end, Tok::Attr));
            i = end;
            continue;
        }
        // c preprocessor
        if cfg.hash_preproc && c == '#' {
            let start = i;
            i += 1;
            while i < n && is_word(chars[i]) {
                i += 1;
            }
            out.push((start, i, Tok::Attr));
            continue;
        }
        // python decorator
        if cfg.decorator && c == '@' {
            let start = i;
            i += 1;
            while i < n && (is_word(chars[i]) || chars[i] == '.') {
                i += 1;
            }
            out.push((start, i, Tok::Attr));
            continue;
        }
        // toml section header
        if lang == Language::Toml && c == '[' && chars[..i].iter().all(|ch| ch.is_whitespace()) {
            out.push((i, n, Tok::Keyword));
            break;
        }
        // numbers
        if c.is_ascii_digit() {
            let start = i;
            while i < n && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                i += 1;
            }
            out.push((start, i, Tok::Number));
            continue;
        }
        // identifiers / keywords
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < n && is_word(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let next = chars.get(i).copied().unwrap_or(' ');
            let tok = if cfg.keywords.contains(&word.as_str()) {
                Some(Tok::Keyword)
            } else if cfg.constants.contains(&word.as_str()) {
                Some(Tok::Constant)
            } else if cfg.types.contains(&word.as_str()) {
                Some(Tok::Type)
            } else if cfg.macro_bang && next == '!' {
                i += 1;
                Some(Tok::Attr)
            } else if cfg.cap_types && word.chars().next().is_some_and(|f| f.is_uppercase()) {
                Some(Tok::Type)
            } else if next == '(' {
                Some(Tok::Fn)
            } else if cfg.key_eq {
                let mut k = i;
                while k < n && chars[k] == ' ' {
                    k += 1;
                }
                (k < n && chars[k] == '=').then_some(Tok::Fn)
            } else if cfg.key_colon && chars[..start].iter().all(|ch| ch.is_whitespace() || *ch == '-') {
                (next == ':').then_some(Tok::Fn)
            } else {
                None
            };
            if let Some(t) = tok {
                out.push((start, i, t));
            }
            continue;
        }
        // punctuation
        if "{}()[]<>=+-*/%&|!?:;,.^~#@$".contains(c) {
            out.push((i, i + 1, Tok::Punct));
            i += 1;
            continue;
        }
        i += 1;
    }
    (out, in_block)
}

fn scan_markdown(line: &str) -> Vec<(usize, usize, Tok)> {
    let n = line.chars().count();
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return vec![(0, n, Tok::Keyword)];
    }
    if trimmed.starts_with('>') {
        return vec![(0, n, Tok::Comment)];
    }
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let indent = n - trimmed.chars().count();
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
        out.push((indent, indent + 1, Tok::Constant));
    }
    let mut i = 0usize;
    while i < n {
        if chars[i] == '`' {
            if let Some(j) = find_sub(&chars, i + 1, "`") {
                out.push((i, j + 1, Tok::String));
                i = j + 1;
                continue;
            }
        }
        if starts_with_at(&chars, i, "**") {
            if let Some(j) = find_sub(&chars, i + 2, "**") {
                out.push((i, j + 2, Tok::Constant));
                i = j + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn scan_html(line: &str, mut in_block: bool) -> (Vec<(usize, usize, Tok)>, bool) {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < n {
        if in_block {
            match find_sub(&chars, i, "-->") {
                Some(j) => {
                    out.push((i, j + 3, Tok::Comment));
                    i = j + 3;
                    in_block = false;
                }
                None => {
                    out.push((i, n, Tok::Comment));
                    return (out, true);
                }
            }
            continue;
        }
        if starts_with_at(&chars, i, "<!--") {
            in_block = true;
            continue;
        }
        if chars[i] == '<' {
            let end = find_sub(&chars, i, ">").map(|j| j + 1).unwrap_or(n);
            out.push((i, end, Tok::Keyword));
            i = end;
            continue;
        }
        if chars[i] == '"' {
            let end = find_sub(&chars, i + 1, "\"").map(|j| j + 1).unwrap_or(n);
            out.push((i, end, Tok::String));
            i = end;
            continue;
        }
        i += 1;
    }
    (out, in_block)
}
