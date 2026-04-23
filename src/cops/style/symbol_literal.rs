use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

const MSG: &str = "Do not use strings for word-like symbol literals.";

#[derive(Default)]
pub struct SymbolLiteral;

impl SymbolLiteral {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for SymbolLiteral {
    fn name(&self) -> &'static str {
        "Style/SymbolLiteral"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_symbol(&self, node: &ruby_prism::SymbolNode, ctx: &CheckContext) -> Vec<Offense> {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let src = &ctx.source[start..end];

        // Must match :"word" or :'word' where word is [A-Za-z_]\w*
        if !matches_quoted_word_symbol(src) {
            return vec![];
        }

        vec![ctx.offense_with_range(self.name(), MSG, self.severity(), start, end)]
    }
}

fn matches_quoted_word_symbol(src: &str) -> bool {
    // Must start with : followed by " or '
    let bytes = src.as_bytes();
    if bytes.len() < 4 {
        return false;
    }
    if bytes[0] != b':' {
        return false;
    }
    let quote = bytes[1];
    if quote != b'"' && quote != b'\'' {
        return false;
    }
    // Must end with matching quote
    if bytes[bytes.len() - 1] != quote {
        return false;
    }
    // Content between quotes
    let inner = &bytes[2..bytes.len() - 1];
    if inner.is_empty() {
        return false;
    }
    // First char must be letter or _
    if !inner[0].is_ascii_alphabetic() && inner[0] != b'_' {
        return false;
    }
    // Rest must be alphanumeric or _
    for &b in &inner[1..] {
        if !b.is_ascii_alphanumeric() && b != b'_' {
            return false;
        }
    }
    true
}

crate::register_cop!("Style/SymbolLiteral", |_cfg| Some(Box::new(SymbolLiteral::new())));
