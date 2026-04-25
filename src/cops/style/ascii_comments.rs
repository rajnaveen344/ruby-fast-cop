//! Style/AsciiComments — checks for non-ASCII characters in comments.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

const MSG: &str = "Use only ascii symbols in comments.";

#[derive(Default)]
pub struct AsciiComments {
    allowed_chars: Vec<String>,
}

impl AsciiComments {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(allowed_chars: Vec<String>) -> Self {
        Self { allowed_chars }
    }

    fn only_allowed_non_ascii(&self, text: &str) -> bool {
        text.chars()
            .filter(|c| !c.is_ascii())
            .all(|c| self.allowed_chars.iter().any(|a| a.chars().any(|ac| ac == c)))
    }
}

impl Cop for AsciiComments {
    fn name(&self) -> &'static str {
        "Style/AsciiComments"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut offenses = Vec::new();

        for c in result.comments() {
            let loc = c.location();
            let cstart = loc.start_offset();
            let cend = loc.end_offset();
            let text = &ctx.source[cstart..cend];

            if text.is_ascii() {
                continue;
            }
            if self.only_allowed_non_ascii(text) {
                continue;
            }

            // Find first run of non-ascii chars (byte range within text)
            let bytes = text.as_bytes();
            let mut i = 0;
            while i < bytes.len() && bytes[i] < 0x80 {
                i += 1;
            }
            let start_byte = i;
            // Continue until we hit ascii again — iterate by char to advance correctly
            let mut end_byte = start_byte;
            for (idx, ch) in text[start_byte..].char_indices() {
                if ch.is_ascii() {
                    end_byte = start_byte + idx;
                    break;
                }
                end_byte = start_byte + idx + ch.len_utf8();
            }

            let off_start = cstart + start_byte;
            let off_end = cstart + end_byte;
            offenses.push(ctx.offense_with_range(self.name(), MSG, self.severity(), off_start, off_end));
        }

        offenses
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allowed_chars: Vec<String>,
}

crate::register_cop!("Style/AsciiComments", |cfg| {
    let c: Cfg = cfg.typed("Style/AsciiComments");
    Some(Box::new(AsciiComments::with_config(c.allowed_chars)))
});
