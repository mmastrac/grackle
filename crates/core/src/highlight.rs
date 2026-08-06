//! A compact syntax highlighter for the four token classes the theme colours
//! (`_rouge.scss`): keyword `k`, string `s`, comment `c1`, name `n`. Everything
//! else, numbers, operators, punctuation, whitespace, is emitted plain and
//! renders in the default colour, so this classifies into those four buckets and
//! no further.
//!
//! It is deliberately not a grammar. For a four-colour palette, comment / string
//! / word lexing is the whole job, and a full highlighter (syntect and its
//! bundled Sublime grammars) would be a heavy dependency, and one that doesn't
//! even ship the nasm grammar this corpus needs, for a handful of posts. A
//! language is a comment style, a set of string quotes, and a keyword list.
//!
//! Escaping matches `markdown::escape_code`: `&`, `<`, `>` only, never
//! quotes, so a highlighted block escapes the same bytes an unhighlighted one
//! does.

use std::fmt::Write as _;

struct Lang {
    /// Line-comment markers (`//`, `#`, `;`). To end of line.
    line: &'static [&'static str],
    /// Block-comment `(open, close)`, if any.
    block: Option<(&'static str, &'static str)>,
    /// String quote characters; `\` escapes the next byte.
    strings: &'static [char],
    keywords: &'static [&'static str],
}

/// The languages this corpus actually fences (`grep '^```'`): rust, c, nasm,
/// bash, java, yaml, json. `text` and anything unknown fall through to plain.
fn lang(name: &str) -> Option<Lang> {
    let l = match name {
        "rust" => Lang {
            // Char literals share `'` with lifetimes (`'a`), which a byte lexer
            // can't tell apart, so rust strings are `"` only, a mis-coloured
            // lifetime is worse than an uncoloured char literal.
            line: &["//"],
            block: Some(("/*", "*/")),
            strings: &['"'],
            keywords: &[
                "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
                "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match",
                "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct",
                "super", "trait", "true", "type", "unsafe", "use", "where", "while",
            ],
        },
        "c" => Lang {
            line: &["//"],
            block: Some(("/*", "*/")),
            strings: &['"', '\''],
            keywords: &[
                "auto", "break", "case", "char", "const", "continue", "default", "do", "double",
                "else", "enum", "extern", "float", "for", "goto", "if", "inline", "int", "long",
                "register", "restrict", "return", "short", "signed", "sizeof", "static", "struct",
                "switch", "typedef", "union", "unsigned", "void", "volatile", "while", "bool",
                "true", "false",
            ],
        },
        "java" => Lang {
            line: &["//"],
            block: Some(("/*", "*/")),
            strings: &['"', '\''],
            keywords: &[
                "abstract",
                "boolean",
                "break",
                "byte",
                "case",
                "catch",
                "char",
                "class",
                "const",
                "continue",
                "default",
                "do",
                "double",
                "else",
                "enum",
                "extends",
                "final",
                "finally",
                "float",
                "for",
                "if",
                "implements",
                "import",
                "instanceof",
                "int",
                "interface",
                "long",
                "native",
                "new",
                "package",
                "private",
                "protected",
                "public",
                "return",
                "short",
                "static",
                "super",
                "switch",
                "synchronized",
                "this",
                "throw",
                "throws",
                "try",
                "void",
                "volatile",
                "while",
                "true",
                "false",
                "null",
            ],
        },
        "nasm" | "asm" => Lang {
            line: &[";"],
            block: None,
            strings: &['"', '\''],
            keywords: &[
                // instructions
                "mov", "movzx", "movsx", "lea", "add", "sub", "mul", "imul", "div", "idiv", "inc",
                "dec", "and", "or", "xor", "not", "neg", "shl", "shr", "sal", "sar", "cmp", "test",
                "jmp", "je", "jne", "jz", "jnz", "jg", "jge", "jl", "jle", "ja", "jb", "call",
                "ret", "push", "pop", "seto", "sete", "setne", "nop", "syscall",
                // registers
                "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "rsp", "eax", "ebx", "ecx", "edx",
                "esi", "edi", "al", "bl", "cl", "dl",
            ],
        },
        "bash" | "sh" | "shell" => Lang {
            line: &["#"],
            block: None,
            strings: &['"', '\''],
            keywords: &[
                "if", "then", "elif", "else", "fi", "for", "while", "until", "do", "done", "case",
                "esac", "in", "function", "return", "local", "export", "echo", "read", "set",
                "unset", "source", "exit", "true", "false",
            ],
        },
        "yaml" | "yml" => Lang {
            line: &["#"],
            block: None,
            strings: &['"', '\''],
            keywords: &["true", "false", "null", "yes", "no", "on", "off"],
        },
        "json" => Lang {
            line: &[],
            block: None,
            strings: &['"'],
            keywords: &["true", "false", "null"],
        },
        _ => return None,
    };
    Some(l)
}

/// Highlight `code` in `name`, or plain-escape it if the language is unknown.
/// The result is the inner HTML of `<code>…</code>`, the caller owns the
/// Rouge wrappers.
pub fn render(name: &str, code: &str) -> String {
    let Some(l) = lang(name) else {
        return escape_plain(code);
    };
    let b = code.as_bytes();
    let mut out = String::with_capacity(code.len() + code.len() / 4);
    let mut i = 0;
    while i < b.len() {
        let rest = &code[i..];

        // Line comment -> end of line.
        if l.line.iter().any(|m| rest.starts_with(m)) {
            let end = i + rest.find('\n').unwrap_or(rest.len());
            span(&mut out, "c1", &code[i..end]);
            i = end;
            continue;
        }
        // Block comment -> past the close delimiter (or EOF if unterminated).
        if let Some((open, close)) = l.block {
            if rest.starts_with(open) {
                let after = &code[i + open.len()..];
                let end = after
                    .find(close)
                    .map(|n| i + open.len() + n + close.len())
                    .unwrap_or(code.len());
                span(&mut out, "c1", &code[i..end]);
                i = end;
                continue;
            }
        }
        let c = b[i] as char;
        // String -> past the matching quote, honouring `\` escapes.
        if c.is_ascii() && l.strings.contains(&c) {
            let end = string_end(b, i, b[i]);
            span(&mut out, "s", &code[i..end]);
            i = end;
            continue;
        }
        // Word -> keyword or name.
        if c == '_' || c.is_ascii_alphabetic() {
            let mut j = i + 1;
            while j < b.len() && (b[j] == b'_' || (b[j] as char).is_ascii_alphanumeric()) {
                j += 1;
            }
            let word = &code[i..j];
            span(
                &mut out,
                if l.keywords.contains(&word) { "k" } else { "n" },
                word,
            );
            i = j;
            continue;
        }
        // Anything else renders in the default colour. Advance one whole char.
        let n = utf8_len(b[i]);
        escape_into(&mut out, &code[i..i + n]);
        i += n;
    }
    out
}

/// The byte index just past a string that opened at `start` with quote `q`.
fn string_end(b: &[u8], start: usize, q: u8) -> usize {
    let mut i = start + 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            x if x == q => return i + 1,
            _ => i += 1,
        }
    }
    b.len()
}

fn utf8_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

fn span(out: &mut String, class: &str, text: &str) {
    let _ = write!(out, "<span class=\"{class}\">");
    escape_into(out, text);
    out.push_str("</span>");
}

fn escape_into(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

/// HTML-escape `&`/`<`/`>` only (not quotes), the escaping code blocks and
/// inline `<code>` share. Using comrak's escaper instead yields `&quot;` and a
/// diff on every code span containing a double quote.
pub(crate) fn escape_plain(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    escape_into(&mut o, s);
    o
}

#[cfg(test)]
mod tests {
    use super::render;

    #[test]
    fn keywords_strings_and_comments_get_their_classes() {
        let out = render("rust", "let x = \"hi\"; // done");
        assert!(out.contains(r#"<span class="k">let</span>"#), "{out}");
        assert!(out.contains(r#"<span class="n">x</span>"#), "{out}");
        assert!(out.contains(r#"<span class="s">"hi"</span>"#), "{out}");
        assert!(out.contains(r#"<span class="c1">// done</span>"#), "{out}");
    }

    #[test]
    fn an_unknown_language_is_plain_but_escaped() {
        assert_eq!(render("text", "a < b && c"), "a &lt; b &amp;&amp; c");
        assert_eq!(render("brainfuck", "x>y"), "x&gt;y");
    }

    /// Rouge escapes `&<>` but not quotes; a highlighted block must match, so a
    /// string's own quotes survive verbatim inside the span.
    #[test]
    fn quotes_are_not_escaped_inside_strings() {
        let out = render("c", "char* s = \"a\";");
        assert!(out.contains(r#"<span class="s">"a"</span>"#), "{out}");
        assert!(!out.contains("&quot;"), "{out}");
    }

    /// An escaped quote inside a string must not end it early.
    #[test]
    fn a_backslash_escape_does_not_close_a_string() {
        let out = render("c", r#"printf("a\"b");"#);
        assert!(out.contains(r#"<span class="s">"a\"b"</span>"#), "{out}");
    }

    #[test]
    fn a_block_comment_spans_lines() {
        let out = render("c", "/* one\ntwo */ x");
        assert!(
            out.contains("<span class=\"c1\">/* one\ntwo */</span>"),
            "{out}"
        );
    }
}
