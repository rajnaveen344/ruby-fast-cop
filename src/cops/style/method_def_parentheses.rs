//! Style/MethodDefParentheses - Enforce parentheses style in method definitions.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/method_def_parentheses.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

const COP_NAME: &str = "Style/MethodDefParentheses";
const MSG_PRESENT: &str = "Use def without parentheses.";
const MSG_MISSING: &str = "Use def with parentheses when there are parameters.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    RequireParentheses,
    RequireNoParentheses,
    RequireNoParenthesesExceptMultiline,
}

pub struct MethodDefParentheses {
    style: EnforcedStyle,
}

impl MethodDefParentheses {
    pub fn new(style: EnforcedStyle) -> Self {
        Self { style }
    }
}

impl Default for MethodDefParentheses {
    fn default() -> Self {
        Self::new(EnforcedStyle::RequireParentheses)
    }
}

impl Cop for MethodDefParentheses {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        let params = node.parameters();

        // Check if this is an endless method (def foo(x) = ...)
        if is_endless(node, ctx) {
            return vec![];
        }

        let has_params = if let Some(ref p) = params {
            has_any_params(p)
        } else {
            false
        };

        let has_parens = has_parentheses(node, ctx);

        // Determine if params are multiline
        let params_multiline = if let Some(ref p) = params {
            let loc = p.location();
            !ctx.same_line(loc.start_offset(), loc.end_offset())
        } else {
            false
        };

        let require_parens = match self.style {
            EnforcedStyle::RequireParentheses => true,
            EnforcedStyle::RequireNoParentheses => false,
            EnforcedStyle::RequireNoParenthesesExceptMultiline => params_multiline,
        };

        if require_parens {
            // Want parentheses
            if has_params && !has_parens {
                // Missing parentheses - offense on first line of params only
                let p = params.as_ref().unwrap();
                let loc = p.location();
                let start = loc.start_offset();
                let end = loc.end_offset();

                // For offense range, only cover first line of params
                let offense_end = first_line_end(ctx.source, start, end);
                let offense = ctx.offense_with_range(
                    COP_NAME,
                    MSG_MISSING,
                    Severity::Convention,
                    start,
                    offense_end,
                );
                // Correction: replace the space before params with '(' and insert ')' after params
                // The space between method name and first param should become '('
                let paren_insert = if start > 0 && ctx.bytes()[start - 1] == b' ' {
                    // Replace the space with '('
                    crate::offense::Edit { start_offset: start - 1, end_offset: start, replacement: "(".to_string() }
                } else {
                    // Insert '(' before params
                    crate::offense::Edit { start_offset: start, end_offset: start, replacement: "(".to_string() }
                };
                let correction = Correction {
                    edits: vec![
                        paren_insert,
                        crate::offense::Edit { start_offset: end, end_offset: end, replacement: ")".to_string() },
                    ],
                };
                return vec![offense.with_correction(correction)];
            }
        } else {
            // Want no parentheses - but anonymous/forwarding args force parens
            if has_anonymous_arguments(node) {
                return vec![];
            }
            if has_parens {
                // Unwanted parentheses
                let (paren_start, paren_end) = find_paren_range(node, ctx);
                let offense = ctx.offense_with_range(
                    COP_NAME,
                    MSG_PRESENT,
                    Severity::Convention,
                    paren_start,
                    paren_end,
                );
                // Correction: replace opening '(' with ' ', remove closing ')'
                let correction = Correction {
                    edits: vec![
                        crate::offense::Edit { start_offset: paren_start, end_offset: paren_start + 1, replacement: " ".to_string() },
                        crate::offense::Edit { start_offset: paren_end - 1, end_offset: paren_end, replacement: String::new() },
                    ],
                };
                return vec![offense.with_correction(correction)];
            }
        }

        vec![]
    }
}

/// Get the end offset of the first line starting from `start`, capped at `max_end`
fn first_line_end(source: &str, start: usize, max_end: usize) -> usize {
    let bytes = source.as_bytes();
    let mut i = start;
    while i < max_end && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

/// Check if a def node is an endless method (def foo(x) = expr)
fn is_endless(node: &ruby_prism::DefNode, ctx: &CheckContext) -> bool {
    // Endless methods have no end_keyword_loc
    if let Some(_end_loc) = node.end_keyword_loc() {
        return false;
    }
    // Also verify there's an '=' in the source - endless methods use '='
    // If there's no end keyword and the def has a body, it's endless
    if node.body().is_some() {
        return true;
    }
    // def foo() = x case - check source for '='
    let loc = node.location();
    let src = &ctx.source[loc.start_offset()..loc.end_offset()];
    src.contains(" = ") || src.contains("= ")
}

/// Check if the def has anonymous/forwarding arguments that require parens
fn has_anonymous_arguments(node: &ruby_prism::DefNode) -> bool {
    if let Some(params) = node.parameters() {
        // Check for forwarding parameter (...) - can be in requireds or keyword_rest
        for param in params.requireds().iter() {
            if matches!(param, Node::ForwardingParameterNode { .. }) {
                return true;
            }
        }

        // Check for anonymous rest (*) - RestParameterNode with no name
        if let Some(rest) = params.rest() {
            if let Some(rp) = rest.as_rest_parameter_node() {
                if rp.name().is_none() {
                    return true;
                }
            }
        }

        // Check for anonymous keyword rest (**) or forwarding parameter (...)
        if let Some(kw_rest) = params.keyword_rest() {
            if matches!(kw_rest, Node::ForwardingParameterNode { .. }) {
                return true;
            }
            if let Some(kwrp) = kw_rest.as_keyword_rest_parameter_node() {
                if kwrp.name().is_none() {
                    return true;
                }
            }
        }

        // Check for anonymous block (&) - BlockParameterNode with no name
        if let Some(block) = params.block() {
            if block.name().is_none() {
                return true;
            }
        }
    }
    false
}

/// Check if a ParametersNode has any parameters
fn has_any_params(params: &ruby_prism::ParametersNode) -> bool {
    params.requireds().iter().next().is_some()
        || params.optionals().iter().next().is_some()
        || params.rest().is_some()
        || params.posts().iter().next().is_some()
        || params.keywords().iter().next().is_some()
        || params.keyword_rest().is_some()
        || params.block().is_some()
}

/// Check if the def node uses parentheses around its parameters
fn has_parentheses(node: &ruby_prism::DefNode, ctx: &CheckContext) -> bool {
    // Look for '(' after method name
    if let Some(lparen) = node.lparen_loc() {
        let _ = lparen;
        return true;
    }
    // Also check if there's an empty () with no params
    let name_loc = node.name_loc();
    let name_end = name_loc.end_offset();
    let bytes = ctx.bytes();
    // Skip whitespace after name
    let mut i = name_end;
    while i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }
    i < bytes.len() && bytes[i] == b'('
}

/// Find the byte range of parentheses around def parameters
fn find_paren_range(node: &ruby_prism::DefNode, ctx: &CheckContext) -> (usize, usize) {
    // Try lparen_loc / rparen_loc first
    if let Some(lparen) = node.lparen_loc() {
        if let Some(rparen) = node.rparen_loc() {
            return (lparen.start_offset(), rparen.end_offset());
        }
    }

    // Fallback: scan for ( and ) after method name
    let name_loc = node.name_loc();
    let name_end = name_loc.end_offset();
    let bytes = ctx.bytes();

    let mut paren_start = name_end;
    while paren_start < bytes.len() && bytes[paren_start] != b'(' {
        paren_start += 1;
    }

    // Find matching close paren
    let mut depth = 0;
    let mut paren_end = paren_start;
    for i in paren_start..bytes.len() {
        if bytes[i] == b'(' {
            depth += 1;
        } else if bytes[i] == b')' {
            depth -= 1;
            if depth == 0 {
                paren_end = i + 1;
                break;
            }
        }
    }

    (paren_start, paren_end)
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style: String }

crate::register_cop!("Style/MethodDefParentheses", |cfg| {
    let c: Cfg = cfg.typed("Style/MethodDefParentheses");
    let style = match c.enforced_style.as_str() {
        "require_no_parentheses" => EnforcedStyle::RequireNoParentheses,
        "require_no_parentheses_except_multiline" => EnforcedStyle::RequireNoParenthesesExceptMultiline,
        _ => EnforcedStyle::RequireParentheses,
    };
    Some(Box::new(MethodDefParentheses::new(style)))
});
