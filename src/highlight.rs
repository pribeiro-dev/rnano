//! Lightweight syntax highlighting (#27).
//!
//! Each language is a list of `Rule`s applied left-to-right on each line.
//! Rules do not span lines (no multi-line states in this version).
//! The highlighter returns a list of `(start_char, end_char, Color)` spans.
//!
//! Auto-disable: if the file size exceeds `highlight_max_kb` KiB the caller
//! should pass an empty slice of rules (or skip calling highlight entirely).

use crossterm::style::Color;

/// A single syntax rule: a literal prefix or a set of start-chars.
pub enum Rule {
    /// Line starts with this string → color the whole line.
    LinePrefix { prefix: &'static str, color: Color },
    /// Keyword surrounded by word boundaries.
    Keyword { word: &'static str, color: Color },
    /// Starts with `open`, ends with `close` (or EOL), both on same line.
    Delimited {
        open: &'static str,
        close: &'static str,
        color: Color,
    },
    /// Everything from `marker` to the end of the line.
    ToEol { marker: &'static str, color: Color },
}

/// Apply `rules` to `line` and return sorted, non-overlapping color spans
/// as `(char_start, char_end, Color)`.
pub fn highlight_line(line: &str, rules: &[Rule]) -> Vec<(usize, usize, Color)> {
    let mut spans: Vec<(usize, usize, Color)> = Vec::new();

    for rule in rules {
        apply_rule(line, rule, &mut spans);
    }

    // Sort and remove overlaps (earlier rules win).
    spans.sort_unstable_by_key(|&(s, _, _)| s);
    let mut merged: Vec<(usize, usize, Color)> = Vec::new();
    let mut end_so_far = 0usize;
    for (s, e, c) in spans {
        if s >= end_so_far {
            merged.push((s, e, c));
            end_so_far = e;
        }
    }
    merged
}

fn apply_rule(line: &str, rule: &Rule, out: &mut Vec<(usize, usize, Color)>) {
    match rule {
        Rule::LinePrefix { prefix, color } => {
            if line.trim_start().starts_with(prefix) {
                let char_len = line.chars().count();
                out.push((0, char_len, *color));
            }
        }
        Rule::Keyword { word, color } => {
            let mut byte_pos = 0;
            let mut char_pos = 0;
            let chars: Vec<char> = line.chars().collect();
            while byte_pos + word.len() <= line.len() {
                if line[byte_pos..].starts_with(word) {
                    let before = byte_pos == 0
                        || !line[..byte_pos]
                            .chars()
                            .next_back()
                            .map(|c| c.is_alphanumeric() || c == '_')
                            .unwrap_or(false);
                    let after_byte = byte_pos + word.len();
                    let after = after_byte >= line.len()
                        || !line[after_byte..]
                            .chars()
                            .next()
                            .map(|c| c.is_alphanumeric() || c == '_')
                            .unwrap_or(false);
                    if before && after {
                        let wlen = word.chars().count();
                        out.push((char_pos, char_pos + wlen, *color));
                        // Advance past the keyword.
                        let _ = &chars; // suppress unused warning
                        byte_pos += word.len();
                        char_pos += wlen;
                        continue;
                    }
                }
                if let Some(ch) = line[byte_pos..].chars().next() {
                    byte_pos += ch.len_utf8();
                    char_pos += 1;
                } else {
                    break;
                }
            }
        }
        Rule::Delimited { open, close, color } => {
            let mut byte_pos = 0;
            let mut char_pos = 0;
            while byte_pos < line.len() {
                if line[byte_pos..].starts_with(open) {
                    let start_char = char_pos;
                    let inner_byte = byte_pos + open.len();
                    let end_byte = line[inner_byte..]
                        .find(close)
                        .map(|r| inner_byte + r + close.len())
                        .unwrap_or(line.len());
                    let end_char = start_char + line[byte_pos..end_byte].chars().count();
                    out.push((start_char, end_char, *color));
                    byte_pos = end_byte;
                    char_pos = end_char;
                    continue;
                }
                if let Some(ch) = line[byte_pos..].chars().next() {
                    byte_pos += ch.len_utf8();
                    char_pos += 1;
                } else {
                    break;
                }
            }
        }
        Rule::ToEol { marker, color } => {
            if let Some(byte_off) = line.find(marker) {
                let char_start = line[..byte_off].chars().count();
                let char_end = line.chars().count();
                if char_start < char_end {
                    out.push((char_start, char_end, *color));
                }
            }
        }
    }
}

// --- built-in language rulesets -----------------------------------------------

pub fn rules_for_extension(ext: &str) -> &'static [Rule] {
    match ext {
        "rs" => RUST_RULES,
        "toml" => TOML_RULES,
        "json" => JSON_RULES,
        "md" | "markdown" => MARKDOWN_RULES,
        "sh" | "bash" | "zsh" | "fish" => SH_RULES,
        "py" => PY_RULES,
        _ => &[],
    }
}

static RUST_RULES: &[Rule] = &[
    Rule::ToEol {
        marker: "//",
        color: Color::DarkGrey,
    },
    Rule::Delimited {
        open: "\"",
        close: "\"",
        color: Color::Green,
    },
    Rule::Delimited {
        open: "'",
        close: "'",
        color: Color::Green,
    },
    Rule::Keyword {
        word: "fn",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "let",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "mut",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "pub",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "use",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "mod",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "struct",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "enum",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "impl",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "trait",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "match",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "if",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "else",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "for",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "while",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "return",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "true",
        color: Color::Cyan,
    },
    Rule::Keyword {
        word: "false",
        color: Color::Cyan,
    },
    Rule::Keyword {
        word: "self",
        color: Color::Cyan,
    },
    Rule::Keyword {
        word: "Self",
        color: Color::Cyan,
    },
];

static TOML_RULES: &[Rule] = &[
    Rule::ToEol {
        marker: "#",
        color: Color::DarkGrey,
    },
    Rule::Delimited {
        open: "\"",
        close: "\"",
        color: Color::Green,
    },
    Rule::LinePrefix {
        prefix: "[",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "true",
        color: Color::Cyan,
    },
    Rule::Keyword {
        word: "false",
        color: Color::Cyan,
    },
];

static JSON_RULES: &[Rule] = &[
    Rule::Delimited {
        open: "\"",
        close: "\"",
        color: Color::Green,
    },
    Rule::Keyword {
        word: "true",
        color: Color::Cyan,
    },
    Rule::Keyword {
        word: "false",
        color: Color::Cyan,
    },
    Rule::Keyword {
        word: "null",
        color: Color::Cyan,
    },
];

static MARKDOWN_RULES: &[Rule] = &[
    Rule::LinePrefix {
        prefix: "#",
        color: Color::Yellow,
    },
    Rule::LinePrefix {
        prefix: "```",
        color: Color::DarkGrey,
    },
    Rule::LinePrefix {
        prefix: ">",
        color: Color::DarkGrey,
    },
    Rule::LinePrefix {
        prefix: "-",
        color: Color::Cyan,
    },
    Rule::LinePrefix {
        prefix: "*",
        color: Color::Cyan,
    },
    Rule::Delimited {
        open: "**",
        close: "**",
        color: Color::White,
    },
    Rule::Delimited {
        open: "`",
        close: "`",
        color: Color::Green,
    },
];

static SH_RULES: &[Rule] = &[
    Rule::ToEol {
        marker: "#",
        color: Color::DarkGrey,
    },
    Rule::Delimited {
        open: "\"",
        close: "\"",
        color: Color::Green,
    },
    Rule::Delimited {
        open: "'",
        close: "'",
        color: Color::Green,
    },
    Rule::Keyword {
        word: "if",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "then",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "else",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "fi",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "for",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "do",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "done",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "while",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "function",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "export",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "local",
        color: Color::Yellow,
    },
];

static PY_RULES: &[Rule] = &[
    Rule::ToEol {
        marker: "#",
        color: Color::DarkGrey,
    },
    Rule::Delimited {
        open: "\"\"\"",
        close: "\"\"\"",
        color: Color::DarkGrey,
    },
    Rule::Delimited {
        open: "\"",
        close: "\"",
        color: Color::Green,
    },
    Rule::Delimited {
        open: "'",
        close: "'",
        color: Color::Green,
    },
    Rule::Keyword {
        word: "def",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "class",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "import",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "from",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "return",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "if",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "elif",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "else",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "for",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "while",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "with",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "as",
        color: Color::Yellow,
    },
    Rule::Keyword {
        word: "True",
        color: Color::Cyan,
    },
    Rule::Keyword {
        word: "False",
        color: Color::Cyan,
    },
    Rule::Keyword {
        word: "None",
        color: Color::Cyan,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_to_eol() {
        let spans = highlight_line("fn main() { // comment", RUST_RULES);
        assert!(spans.iter().any(|(_, _, c)| *c == Color::DarkGrey));
    }

    #[test]
    fn keyword_boundary() {
        let spans = highlight_line("let x = 1;", RUST_RULES);
        assert!(
            spans
                .iter()
                .any(|(s, e, c)| *s == 0 && *e == 3 && *c == Color::Yellow)
        );
        // "inlet" must NOT match "let"
        let spans2 = highlight_line("inlet = 1;", RUST_RULES);
        assert!(
            !spans2
                .iter()
                .any(|(s, _, c)| *s == 2 && *c == Color::Yellow)
        );
    }

    #[test]
    fn string_delimited() {
        let spans = highlight_line("let s = \"hello\";", RUST_RULES);
        assert!(spans.iter().any(|(_, _, c)| *c == Color::Green));
    }

    #[test]
    fn no_rules_for_unknown_ext() {
        assert!(rules_for_extension("xyz").is_empty());
    }

    #[test]
    fn toml_section_header() {
        let spans = highlight_line("[package]", TOML_RULES);
        assert!(spans.iter().any(|(_, _, c)| *c == Color::Yellow));
    }
}
