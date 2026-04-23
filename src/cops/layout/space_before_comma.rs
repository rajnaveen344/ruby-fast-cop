//! Layout/SpaceBeforeComma cop
//! Checks for spaces before commas.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};

#[derive(Default)]
pub struct SpaceBeforeComma;

impl SpaceBeforeComma {
    pub fn new() -> Self { Self }
}

impl Cop for SpaceBeforeComma {
    fn name(&self) -> &'static str { "Layout/SpaceBeforeComma" }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let source = ctx.source;
        let bytes = source.as_bytes();
        let mut offenses = Vec::new();

        // Track if we're inside a string or comment to avoid false positives
        // Simple scan: find sequences of spaces/tabs followed by comma
        // We need to skip string contents and comments
        // Strategy: scan tokens using the Prism token approach
        // Simpler: scan char by char tracking string depth

        let mut i = 0;
        let len = bytes.len();
        let mut in_string_depth = 0u32;
        let mut in_single_quote_string = false;

        while i < len {
            let b = bytes[i];

            // Very simple approach: scan for spaces before commas
            // Skip inside strings by tracking quote depth
            if b == b'#' && in_string_depth == 0 && !in_single_quote_string {
                // Comment - skip to end of line
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }

            if b == b'\'' && in_string_depth == 0 {
                in_single_quote_string = !in_single_quote_string;
                i += 1;
                continue;
            }

            if in_single_quote_string {
                if b == b'\\' { i += 2; } else { i += 1; }
                continue;
            }

            if b == b'"' {
                if in_string_depth == 0 {
                    in_string_depth += 1;
                } else {
                    // Could be end of string (simplified - not tracking nested)
                    in_string_depth = in_string_depth.saturating_sub(1);
                }
                i += 1;
                continue;
            }

            if b == b',' && in_string_depth == 0 {
                // Look back for spaces
                let comma_pos = i;
                let mut j = comma_pos;
                while j > 0 && (bytes[j-1] == b' ' || bytes[j-1] == b'\t') {
                    j -= 1;
                }
                if j < comma_pos {
                    let space_start = j;
                    let msg = "Space found before comma.";
                    let offense = ctx.offense_with_range(
                        "Layout/SpaceBeforeComma", msg, Severity::Convention,
                        space_start,
                        comma_pos,
                    ).with_correction(Correction::delete(space_start, comma_pos));
                    offenses.push(offense);
                }
            }

            i += 1;
        }

        // Sort by line/col descending for consistent output (RuboCop reports them in reverse order)
        offenses.sort_by(|a, b| {
            b.location.line.cmp(&a.location.line)
                .then(b.location.column.cmp(&a.location.column))
        });

        offenses
    }
}

crate::register_cop!("Layout/SpaceBeforeComma", |_cfg| Some(Box::new(SpaceBeforeComma::new())));
