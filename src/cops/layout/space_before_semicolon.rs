//! Layout/SpaceBeforeSemicolon - Checks for space before semicolon.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/space_before_semicolon.rb
//! Uses SpaceBeforePunctuation mixin logic.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};

pub struct SpaceBeforeSemicolon {
    /// When SpaceInsideBlockBraces is 'space', a space before ';' after '{' is OK
    space_inside_block_braces: bool,
}

impl SpaceBeforeSemicolon {
    pub fn new(space_inside_block_braces: bool) -> Self {
        Self { space_inside_block_braces }
    }
}

impl Default for SpaceBeforeSemicolon {
    fn default() -> Self {
        Self { space_inside_block_braces: true }
    }
}

impl Cop for SpaceBeforeSemicolon {
    fn name(&self) -> &'static str {
        "Layout/SpaceBeforeSemicolon"
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

        // Track state to skip string contents
        // Simple approach: scan for ';' preceded by spaces on same line
        // Skip string/comment content via brace/string tracking is complex,
        // so use the same approach as RuboCop's token-based SpaceBeforePunctuation:
        // detect runs of whitespace immediately before ';' on same line.

        while i < len {
            if bytes[i] == b';' {
                // Is there whitespace before this ';' on the same line?
                if i > 0 {
                    let mut j = i - 1;
                    // Scan backwards for spaces
                    while j > 0 && (bytes[j] == b' ' || bytes[j] == b'\t') {
                        j -= 1;
                    }
                    let prev_char = bytes[j];
                    let space_start = j + 1;
                    if space_start < i {
                        // There is whitespace before the semicolon
                        // Check: is prev_char '{' and space_inside_block_braces enabled?
                        if prev_char == b'{' && self.space_inside_block_braces {
                            // Space after '{' is required by SpaceInsideBlockBraces, skip
                            i += 1;
                            continue;
                        }
                        // Report offense: range is the whitespace
                        let correction = Correction::delete(space_start, i);
                        offenses.push(
                            Offense::new(
                                self.name(),
                                "Space found before semicolon.",
                                Severity::Convention,
                                Location::from_offsets(source, space_start, i),
                                ctx.filename,
                            ).with_correction(correction)
                        );
                    }
                }
            }
            i += 1;
        }

        offenses
    }
}

crate::register_cop!("Layout/SpaceBeforeSemicolon", |cfg| {
    let style = cfg
        .get_cop_config("Layout/SpaceInsideBlockBraces")
        .and_then(|c| c.enforced_style.as_deref())
        .map(|s| s == "space")
        .unwrap_or(true); // default is 'space'
    Some(Box::new(SpaceBeforeSemicolon::new(style)))
});
