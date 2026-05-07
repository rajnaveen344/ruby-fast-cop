//! Style/MultilineMethodSignature - Checks for method signatures that span multiple lines.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/multiline_method_signature.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};

const COP_NAME: &str = "Style/MultilineMethodSignature";
const MSG: &str = "Avoid multi-line method signatures.";

pub struct MultilineMethodSignature {
    max_line_length: Option<usize>,
}

impl Default for MultilineMethodSignature {
    fn default() -> Self {
        Self { max_line_length: None }
    }
}

impl MultilineMethodSignature {
    pub fn new() -> Self { Self::default() }
    pub fn with_config(max_line_length: Option<usize>) -> Self { Self { max_line_length } }
}

impl Cop for MultilineMethodSignature {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        let source = ctx.source;

        // Must have parens
        let lparen = match node.lparen_loc() {
            Some(l) => l,
            None => {
                // Even without params, check if `def` keyword and name are on different lines
                // (but that case produces no offense per fixtures)
                return vec![];
            }
        };
        let rparen = match node.rparen_loc() {
            Some(r) => r,
            None => return vec![],
        };

        // Check if signature spans multiple lines:
        // Either lparen..rparen contains a newline, or def/name are on different lines
        let sig_spans_lines = {
            let lp_start = lparen.start_offset();
            let rp_end = rparen.end_offset();
            source[lp_start..rp_end].contains('\n')
        };

        let def_kw = node.def_keyword_loc();
        let name_loc = node.name_loc();
        let def_name_different_lines = line_of(source, def_kw.start_offset()) != line_of(source, name_loc.start_offset());

        if !sig_spans_lines && !def_name_different_lines {
            return vec![];
        }

        // Check line length limit
        if let Some(max) = self.max_line_length {
            let indent = leading_spaces(source, def_kw.start_offset());
            let width = definition_width(node, source);
            if indent + width > max {
                return vec![];
            }
        }

        // Offense range: start of def_keyword to end of its first line
        let def_start = def_kw.start_offset();
        let first_line_end = source[def_start..].find('\n')
            .map(|p| def_start + p)
            .unwrap_or_else(|| node.location().end_offset());

        let correction = build_correction(node, source, &lparen, &rparen);
        let offense = ctx.offense_with_range(COP_NAME, MSG, Severity::Convention, def_start, first_line_end);
        vec![offense.with_correction(correction)]
    }
}

/// Compute "definition width" = collapsed length of `def [recv.]name(args)`
fn definition_width(node: &ruby_prism::DefNode, source: &str) -> usize {
    let name_src = &source[node.name_loc().start_offset()..node.name_loc().end_offset()];
    let args_joined = joined_args(node.parameters(), source);
    let lparen = node.lparen_loc().unwrap();
    let rparen = node.rparen_loc().unwrap();
    let last_line = last_line_of_range(source, lparen.end_offset(), rparen.end_offset());
    let total_args = if last_line.starts_with(')') {
        format!("{})", args_joined)
    } else {
        format!("{})", args_joined)
    };

    // "def " + [receiver + "."] + name + "(" + args)
    let receiver_prefix = if let Some(recv) = node.receiver() {
        let r = &source[recv.location().start_offset()..recv.location().end_offset()];
        format!("{}.", r)
    } else {
        String::new()
    };

    4 + receiver_prefix.len() + name_src.len() + 1 + total_args.len()
}

fn build_correction(
    node: &ruby_prism::DefNode,
    source: &str,
    lparen: &ruby_prism::Location,
    rparen: &ruby_prism::Location,
) -> Correction {
    let mut edits: Vec<Edit> = Vec::new();

    let args_joined = joined_args(node.parameters(), source);
    let last_line = last_line_of_range(source, lparen.end_offset(), rparen.end_offset());
    let last_starts_with_close = last_line.starts_with(')');

    // If def keyword and name are on different lines, move name after def
    let def_kw = node.def_keyword_loc();
    let name_loc = node.name_loc();
    if line_of(source, def_kw.start_offset()) != line_of(source, name_loc.start_offset()) {
        let name_src = source[name_loc.start_offset()..name_loc.end_offset()].to_string();
        edits.push(Edit {
            start_offset: name_loc.start_offset(),
            end_offset: name_loc.end_offset(),
            replacement: String::new(),
        });
        edits.push(Edit {
            start_offset: def_kw.end_offset(),
            end_offset: def_kw.end_offset(),
            replacement: format!(" {}", name_src),
        });
    }

    // Replace entire content from after `(` to end of `)` with `args)`
    // This collapses both "args on new lines" and "closing paren on own line" cases
    let args_remove_start = lparen.end_offset();
    let args_remove_end = rparen.end_offset();
    let final_args = if last_starts_with_close {
        format!("{})", args_joined)
    } else {
        // rparen is on same line as last arg — replace everything from after `(` to `)`
        // The `)` itself is at rparen.start..rparen.end, and args span lines before it
        format!("{})", args_joined)
    };

    edits.push(Edit {
        start_offset: args_remove_start,
        end_offset: args_remove_end,
        replacement: final_args,
    });

    Correction { edits }
}

/// Collect all param sources joined with ", "
fn joined_args(params: Option<ruby_prism::ParametersNode>, source: &str) -> String {
    let params = match params {
        Some(p) => p,
        None => return String::new(),
    };
    let mut result = Vec::new();
    for param in params.requireds().iter() {
        result.push(source[param.location().start_offset()..param.location().end_offset()].trim().to_string());
    }
    for param in params.optionals().iter() {
        result.push(source[param.location().start_offset()..param.location().end_offset()].trim().to_string());
    }
    if let Some(rest) = params.rest() {
        result.push(source[rest.location().start_offset()..rest.location().end_offset()].trim().to_string());
    }
    for param in params.posts().iter() {
        result.push(source[param.location().start_offset()..param.location().end_offset()].trim().to_string());
    }
    for param in params.keywords().iter() {
        result.push(source[param.location().start_offset()..param.location().end_offset()].trim().to_string());
    }
    if let Some(kw_rest) = params.keyword_rest() {
        result.push(source[kw_rest.location().start_offset()..kw_rest.location().end_offset()].trim().to_string());
    }
    if let Some(block) = params.block() {
        result.push(source[block.location().start_offset()..block.location().end_offset()].trim().to_string());
    }
    result.join(", ")
}

/// Get the stripped last line of source[_..end]
fn last_line_of_range(source: &str, _start: usize, end: usize) -> String {
    let before_end = if end > 0 { end - 1 } else { 0 };
    let line_start = source[..=before_end.min(source.len() - 1)].rfind('\n').map(|p| p + 1).unwrap_or(0);
    source[line_start..end].trim().to_string()
}

fn line_of(source: &str, offset: usize) -> usize {
    source.as_bytes()[..offset].iter().filter(|&&b| b == b'\n').count()
}

fn leading_spaces(source: &str, byte_offset: usize) -> usize {
    let line_start = source[..byte_offset].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let line = &source[line_start..byte_offset];
    line.len() - line.trim_start().len()
}

crate::register_cop!("Style/MultilineMethodSignature", |cfg| {
    let max_line_length = if cfg.is_cop_enabled("Layout/LineLength") {
        cfg.get_cop_config("Layout/LineLength").and_then(|c| c.max).map(|m| m as usize)
    } else {
        None
    };
    Some(Box::new(MultilineMethodSignature::with_config(max_line_length)))
});
