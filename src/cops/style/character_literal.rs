use crate::cops::{CheckContext, Cop};
use crate::offense::{Location, Offense, Severity};

const MSG: &str = "Do not use the character literal - use string literal instead.";

#[derive(Default)]
pub struct CharacterLiteral;

impl CharacterLiteral {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for CharacterLiteral {
    fn name(&self) -> &'static str {
        "Style/CharacterLiteral"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let source = ctx.source;
        let bytes = source.as_bytes();
        let len = bytes.len();
        let mut i = 0;

        while i < len {
            match bytes[i] {
                // Skip single-line comments
                b'#' => {
                    while i < len && bytes[i] != b'\n' {
                        i += 1;
                    }
                }
                // Skip regular string literals
                b'\'' | b'"' => {
                    let quote = bytes[i];
                    i += 1;
                    while i < len {
                        if bytes[i] == b'\\' {
                            i += 2;
                        } else if bytes[i] == quote {
                            i += 1;
                            break;
                        } else {
                            i += 1;
                        }
                    }
                }
                // Skip heredoc, percent literals etc.
                b'%' if i + 1 < len => {
                    // Skip %w, %W, %i, %I, %q, %Q, %r, %x literals
                    let next = bytes[i + 1];
                    if next.is_ascii_alphabetic() || next == b'{' || next == b'(' || next == b'[' || next == b'|' {
                        // Find the delimiter
                        let delim_idx = if next.is_ascii_alphabetic() { i + 2 } else { i + 1 };
                        if delim_idx < len {
                            let open = bytes[delim_idx];
                            let close = match open {
                                b'{' => b'}',
                                b'(' => b')',
                                b'[' => b']',
                                b'<' => b'>',
                                _ => open,
                            };
                            i = delim_idx + 1;
                            let mut depth = 1usize;
                            while i < len {
                                if bytes[i] == b'\\' {
                                    i += 2;
                                } else if bytes[i] == open && open != close {
                                    depth += 1;
                                    i += 1;
                                } else if bytes[i] == close {
                                    depth -= 1;
                                    i += 1;
                                    if depth == 0 {
                                        break;
                                    }
                                } else {
                                    i += 1;
                                }
                            }
                        } else {
                            i += 1;
                        }
                    } else {
                        i += 1;
                    }
                }
                b'?' => {
                    // Check if this is a character literal
                    // Must NOT be preceded by alphanumeric/_ (operator or method end)
                    let preceded_by_ident = i > 0 && {
                        let prev = bytes[i - 1];
                        prev.is_ascii_alphanumeric() || prev == b'_'
                    };
                    if preceded_by_ident {
                        i += 1;
                        continue;
                    }
                    let start = i;
                    i += 1;
                    if i >= len {
                        break;
                    }
                    if bytes[i] == b'\\' {
                        // Escape sequence
                        i += 1;
                        if i >= len {
                            break;
                        }
                        match bytes[i] {
                            b'C' | b'M' => {
                                // Control/meta: ?\C-x ?\M-x — not an offense, skip
                                while i < len && bytes[i] != b' ' && bytes[i] != b'\n'
                                    && bytes[i] != b')' && bytes[i] != b','
                                    && bytes[i] != b';' && bytes[i] != b'\t'
                                {
                                    i += 1;
                                }
                                continue;
                            }
                            _ => {
                                i += 1; // skip the escaped char
                                // ?\n = 3 chars, qualifies
                                let literal_len = i - start;
                                if literal_len >= 2 && literal_len <= 3 {
                                    let loc = Location::from_offsets(source, start, i);
                                    offenses.push(Offense::new(
                                        self.name(), MSG, self.severity(), loc, ctx.filename,
                                    ));
                                }
                            }
                        }
                    } else {
                        // Single normal character like ?x
                        i += 1;
                        let literal_len = i - start;
                        if literal_len == 2 {
                            let loc = Location::from_offsets(source, start, i);
                            offenses.push(Offense::new(
                                self.name(), MSG, self.severity(), loc, ctx.filename,
                            ));
                        }
                    }
                }
                _ => {
                    i += 1;
                }
            }
        }

        offenses
    }
}

crate::register_cop!("Style/CharacterLiteral", |_cfg| Some(Box::new(CharacterLiteral::new())));
