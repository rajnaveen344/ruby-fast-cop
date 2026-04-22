//! Layout/SpaceAfterSemicolon - Checks for space missing after semicolon.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/space_after_semicolon.rb
//! Uses SpaceAfterPunctuation mixin logic.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};

pub struct SpaceAfterSemicolon {
    /// EnforcedStyle for SpaceInsideBlockBraces — if 'space', ';' before '}' needs space
    space_inside_block_braces: bool,
}

impl SpaceAfterSemicolon {
    pub fn new(space_inside_block_braces: bool) -> Self {
        Self { space_inside_block_braces }
    }
}

impl Default for SpaceAfterSemicolon {
    fn default() -> Self {
        Self { space_inside_block_braces: true }
    }
}

impl Cop for SpaceAfterSemicolon {
    fn name(&self) -> &'static str {
        "Layout/SpaceAfterSemicolon"
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
            if bytes[i] == b';' {
                let semi_pos = i;
                // Skip consecutive semicolons (;;) — no offense
                let mut j = i + 1;
                while j < len && bytes[j] == b';' {
                    j += 1;
                }
                if j > i + 1 {
                    // Multiple semicolons, skip all
                    i = j;
                    continue;
                }

                // Check what's after the semicolon (at position i+1)
                let next_pos = i + 1;
                if next_pos >= len {
                    // Semicolon at end of file — no offense
                    i += 1;
                    continue;
                }

                let next = bytes[next_pos];

                // No offense if next char is whitespace or newline
                if next == b' ' || next == b'\t' || next == b'\n' || next == b'\r' {
                    i += 1;
                    continue;
                }

                // No offense if next char is ')' ']' '|'
                if next == b')' || next == b']' || next == b'|' {
                    i += 1;
                    continue;
                }

                // Check '}' — depends on SpaceInsideBlockBraces
                if next == b'}' {
                    if !self.space_inside_block_braces {
                        // no_space style: ';' before '}' OK without space
                        i += 1;
                        continue;
                    }
                    // space style: need space before '}'
                }

                // Check if inside string interpolation: ';' followed by '}'
                // where interpolation context overrides block brace style
                // We detect interpolation by checking if the '}' closes a `#{`
                // Simple heuristic: skip checking — RuboCop skips ';' inside interpolation
                // The test "accepts no space between a semicolon and a closing brace of string interpolation"
                // passes because "#{ ;}" — the '}' is tSTRING_DEND, which is in allowed_type?
                // We need to detect if next '}' is string interpolation close
                if next == b'}' {
                    // Check backwards for matching #{
                    // Simple: scan backwards for #{
                    let mut k = semi_pos;
                    let mut depth = 0i32;
                    let mut in_interp = false;
                    while k > 0 {
                        k -= 1;
                        match bytes[k] {
                            b'}' => depth += 1,
                            b'{' => {
                                if depth == 0 {
                                    if k > 0 && bytes[k - 1] == b'#' {
                                        in_interp = true;
                                    }
                                    break;
                                }
                                depth -= 1;
                            }
                            _ => {}
                        }
                    }
                    if in_interp {
                        i += 1;
                        continue;
                    }
                }

                // Report offense at the semicolon position
                let correction = Correction::insert(semi_pos + 1, " ");
                offenses.push(
                    Offense::new(
                        self.name(),
                        "Space missing after semicolon.",
                        Severity::Convention,
                        Location::from_offsets(source, semi_pos, semi_pos + 1),
                        ctx.filename,
                    ).with_correction(correction)
                );
            }
            i += 1;
        }

        offenses
    }
}

crate::register_cop!("Layout/SpaceAfterSemicolon", |cfg| {
    let style = cfg
        .get_cop_config("Layout/SpaceInsideBlockBraces")
        .and_then(|c| c.enforced_style.as_deref())
        .map(|s| s == "space")
        .unwrap_or(true); // default 'space'
    Some(Box::new(SpaceAfterSemicolon::new(style)))
});
