//! Style/TrailingCommaInBlockArgs - flag useless trailing commas in block args.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/trailing_comma_in_block_args.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

const COP_NAME: &str = "Style/TrailingCommaInBlockArgs";
const MSG: &str = "Useless trailing comma present in block arguments.";

#[derive(Default)]
pub struct TrailingCommaInBlockArgs;

impl TrailingCommaInBlockArgs {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for TrailingCommaInBlockArgs {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, call: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Block must be present and a BlockNode (not a BlockArgumentNode).
        let block_node = match call.block() {
            Some(b) => b,
            None => return vec![],
        };
        let block = match block_node.as_block_node() {
            Some(b) => b,
            None => return vec![],
        };

        // BlockParametersNode holds the `|...|`.
        let params_node = match block.parameters() {
            Some(p) => p,
            None => return vec![],
        };
        let bp = match params_node.as_block_parameters_node() {
            Some(bp) => bp,
            None => return vec![],
        };

        let (open, close) = match (bp.opening_loc(), bp.closing_loc()) {
            (Some(o), Some(c)) => (o, c),
            _ => return vec![],
        };

        // Count args matching RuboCop's `:arg, :optarg, :kwoptarg`.
        let pn = match bp.parameters() {
            Some(p) => p,
            None => return vec![],
        };
        let mut arg_count = 0usize;
        arg_count += pn.requireds().iter().count();
        arg_count += pn.optionals().iter().count();
        for kw in pn.keywords().iter() {
            if matches!(kw, Node::OptionalKeywordParameterNode { .. }) {
                arg_count += 1;
            }
        }
        if arg_count <= 1 {
            return vec![];
        }

        // Find trailing comma between pipes.
        let content_start = open.end_offset();
        let content_end = close.start_offset();
        let bytes = ctx.bytes();
        let comma_offset = match find_trailing_comma(bytes, content_start, content_end) {
            Some(o) => o,
            None => return vec![],
        };

        let correction = Correction::delete(comma_offset, comma_offset + 1);
        vec![ctx
            .offense_with_range(
                COP_NAME,
                MSG,
                Severity::Convention,
                comma_offset,
                comma_offset + 1,
            )
            .with_correction(correction)]
    }
}

/// Scan forward from `start..end`, tracking string state, returning byte offset
/// of the trailing `,` (last non-whitespace byte) within the pipes — or None.
/// Returns None if a `;` is present (block-local vars follow it).
fn find_trailing_comma(bytes: &[u8], start: usize, end: usize) -> Option<usize> {
    let mut last_meaningful: Option<(u8, usize)> = None;
    let mut i = start;
    let mut quote: Option<u8> = None;
    while i < end {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' && i + 1 < end {
                i += 2;
                continue;
            }
            if b == q {
                quote = None;
                last_meaningful = Some((b, i));
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' | b'"' => {
                quote = Some(b);
                last_meaningful = Some((b, i));
            }
            b';' => return None,
            b' ' | b'\t' | b'\n' | b'\r' => {}
            _ => {
                last_meaningful = Some((b, i));
            }
        }
        i += 1;
    }
    match last_meaningful {
        Some((b',', pos)) => Some(pos),
        _ => None,
    }
}

crate::register_cop!("Style/TrailingCommaInBlockArgs", |_cfg| {
    Some(Box::new(TrailingCommaInBlockArgs::new()))
});
