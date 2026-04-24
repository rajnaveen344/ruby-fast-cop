//! Style/RedundantArrayFlatten cop
//!
//! Checks for redundant calls of `Array#flatten` before `join`.
//! `Array#join` joins nested arrays recursively, so flattening beforehand is redundant.
//!
//! Ported from `lib/rubocop/cop/style/redundant_array_flatten.rb`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

const MSG: &str = "Remove the redundant `flatten`.";

#[derive(Default)]
pub struct RedundantArrayFlatten;

impl RedundantArrayFlatten {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for RedundantArrayFlatten {
    fn name(&self) -> &'static str {
        "Style/RedundantArrayFlatten"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // `node` is the `flatten` call. We look for a parent `join` call.
        let method = node_name!(node);
        if method != "flatten" {
            return vec![];
        }

        // Must have a receiver
        let Some(_recv) = node.receiver() else {
            return vec![];
        };

        // flatten can take 0 or 1 argument
        if let Some(args) = node.arguments() {
            let count = args.arguments().iter().count();
            if count > 1 {
                return vec![];
            }
        }

        // Must not have a block
        if node.block().is_some() {
            return vec![];
        }

        // Look for parent join call where node is receiver.
        // We scan forward in source: the outer call begins where this flatten call ends
        // (on whitespace-trimmed basis). Instead, inspect the source character-by-character
        // to find `.join` or `&.join` immediately following our end offset.
        let flatten_end = node.location().end_offset();
        let bytes = ctx.source.as_bytes();

        // Find dot (. or &.) right after flatten call
        let mut i = flatten_end;
        let dot_start = i;
        let is_safe_nav;
        if i + 1 < bytes.len() && bytes[i] == b'&' && bytes[i + 1] == b'.' {
            is_safe_nav = true;
            i += 2;
        } else if i < bytes.len() && bytes[i] == b'.' {
            is_safe_nav = false;
            i += 1;
        } else {
            return vec![];
        }

        // Match "join"
        if i + 4 > bytes.len() || &bytes[i..i + 4] != b"join" {
            return vec![];
        }
        // Ensure it's not a longer identifier (e.g. "joiner")
        if i + 4 < bytes.len() {
            let c = bytes[i + 4];
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'?' || c == b'!' {
                return vec![];
            }
        }
        let _ = is_safe_nav;
        let join_end = i + 4;

        // Check `join`'s argument: no args, or a single `nil` literal
        let mut pos = join_end;
        let mut has_paren = false;
        if pos < bytes.len() && bytes[pos] == b'(' {
            has_paren = true;
            pos += 1;
            // Skip whitespace
            while pos < bytes.len() && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
                pos += 1;
            }
            // Either immediate `)` (no args) or `nil)` (exactly one nil arg)
            if pos < bytes.len() && bytes[pos] == b')' {
                // zero args - ok
            } else if pos + 3 <= bytes.len() && &bytes[pos..pos + 3] == b"nil" {
                let after = pos + 3;
                // boundary check
                if after < bytes.len() {
                    let c = bytes[after];
                    if c.is_ascii_alphanumeric() || c == b'_' || c == b'?' || c == b'!' {
                        return vec![];
                    }
                }
                // skip to close paren
                let mut p = after;
                while p < bytes.len() && (bytes[p] == b' ' || bytes[p] == b'\t') {
                    p += 1;
                }
                if p >= bytes.len() || bytes[p] != b')' {
                    return vec![];
                }
            } else {
                // Has some other argument → not redundant (e.g. join(separator))
                return vec![];
            }
        }
        let _ = has_paren;

        // Build offense range: dot of flatten → end of flatten call
        // For `x.flatten.join`: range = `.flatten` (dot at receiver.end_offset to flatten_end)
        let dot_of_flatten = match node.call_operator_loc() {
            Some(loc) => loc.start_offset(),
            None => return vec![],
        };
        let offense_start = dot_of_flatten;
        let offense_end = flatten_end;

        let _ = dot_start;
        let offense = ctx
            .offense_with_range(self.name(), MSG, self.severity(), offense_start, offense_end)
            .with_correction(Correction::delete(offense_start, offense_end));
        vec![offense]
    }
}

crate::register_cop!("Style/RedundantArrayFlatten", |_cfg| Some(Box::new(RedundantArrayFlatten::new())));
