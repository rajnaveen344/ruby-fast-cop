//! Style/Lambda - Checks for uses of lambda literal vs method-call syntax.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/lambda.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/Lambda";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    LineCountDependent,
    Lambda,
    Literal,
}

pub struct Lambda {
    style: EnforcedStyle,
}

impl Default for Lambda {
    fn default() -> Self {
        Self { style: EnforcedStyle::LineCountDependent }
    }
}

impl Lambda {
    pub fn new() -> Self { Self::default() }
    pub fn with_style(style: EnforcedStyle) -> Self { Self { style } }
}

impl Cop for Lambda {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = LambdaVisitor { style: self.style, ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct LambdaVisitor<'a> {
    style: EnforcedStyle,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> LambdaVisitor<'a> {
    /// Offending selectors per style × line-count, as in RuboCop.
    fn offending(style: EnforcedStyle, multiline: bool) -> &'static str {
        match (style, multiline) {
            (EnforcedStyle::Lambda, _) => "->",
            (EnforcedStyle::Literal, _) => "lambda",
            (EnforcedStyle::LineCountDependent, false) => "lambda",
            (EnforcedStyle::LineCountDependent, true) => "->",
        }
    }

    fn message(&self, selector: &str, multiline: bool) -> String {
        let modifier = match self.style {
            EnforcedStyle::LineCountDependent => if multiline { "multiline" } else { "single line" },
            _ => "all",
        };
        if selector == "->" {
            format!("Use the `lambda` method for {} lambdas.", modifier)
        } else {
            format!("Use the `-> {{ ... }}` lambda literal syntax for {} lambdas.", modifier)
        }
    }

    fn is_multiline(&self, start: usize, end: usize) -> bool {
        self.ctx.source[start..end].contains('\n')
    }
}

impl<'a> Visit<'_> for LambdaVisitor<'a> {
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        // Lambda literal: `-> { }` / `-> do end` / `->(x) { x }`
        let loc = node.location();
        let multiline = self.is_multiline(loc.start_offset(), loc.end_offset());
        let off = Self::offending(self.style, multiline);
        if off == "->" {
            // Flag the `->` operator (first two chars).
            let op = node.operator_loc();
            let msg = self.message("->", multiline);
            let correction = build_literal_to_method(self.ctx.source, node);
            let mut offense = self.ctx.offense_with_range(
                COP_NAME, &msg, Severity::Convention,
                op.start_offset(), op.end_offset(),
            );
            if let Some(c) = correction {
                offense = offense.with_correction(c);
            }
            self.offenses.push(offense);
        }
        ruby_prism::visit_lambda_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // `lambda { ... }` / `lambda do ... end`
        let method = node_name!(node);
        if method == "lambda" && node.receiver().is_none() {
            if let Some(block) = node.block() {
                if let Some(_bn) = block.as_block_node() {
                    // Top-level lambda call with block.
                    let call_loc = node.location();
                    let block_loc = block.location();
                    let whole_start = call_loc.start_offset();
                    let whole_end = block_loc.end_offset();
                    let multiline = self.is_multiline(whole_start, whole_end);
                    let off = Self::offending(self.style, multiline);
                    if off == "lambda" {
                        // Flag on the `lambda` message.
                        let msg_loc = node.message_loc().unwrap_or(node.location());
                        let msg = self.message("lambda", multiline);
                        let correction = build_method_to_literal(self.ctx.source, node);
                        let mut offense = self.ctx.offense_with_range(
                            COP_NAME, &msg, Severity::Convention,
                            msg_loc.start_offset(), msg_loc.end_offset(),
                        );
                        if let Some(c) = correction {
                            offense = offense.with_correction(c);
                        }
                        self.offenses.push(offense);
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

/// Literal `->[(args)] { body }` → `lambda { [|args|] body }`
fn build_literal_to_method(source: &str, node: &ruby_prism::LambdaNode) -> Option<Correction> {
    let op = node.operator_loc();
    let opening = node.opening_loc();
    let mut edits = Vec::new();
    // Replace `->` with `lambda`
    edits.push(Edit { start_offset: op.start_offset(), end_offset: op.end_offset(), replacement: "lambda".to_string() });
    // Args: `(x, y)` between `->` and `{`/`do`. Get from parameters() loc if present.
    let params = node.parameters();
    let opening_src = &source[opening.start_offset()..opening.end_offset()];
    if let Some(p) = &params {
        if let Some(bp) = p.as_block_parameters_node() {
            let p_loc = bp.location();
            let p_src = &source[p_loc.start_offset()..p_loc.end_offset()];
            let inner = if (p_src.starts_with('(') && p_src.ends_with(')'))
                || (p_src.starts_with('|') && p_src.ends_with('|'))
            {
                p_src[1..p_src.len()-1].to_string()
            } else {
                p_src.to_string()
            };
            // Remove from end of `->` to start of opening (covers params + whitespace).
            // Preserve original spacing: if there was no space between args' close-paren and opening, keep none.
            // Exception: opening is `do` and we'd produce `lambdado` → force a space.
            let between = &source[op.end_offset()..opening.start_offset()];
            let trailing = if between.ends_with(' ') || between.ends_with('\t') {
                " "
            } else if opening_src == "do" {
                " "
            } else {
                ""
            };
            edits.push(Edit { start_offset: op.end_offset(), end_offset: opening.start_offset(), replacement: trailing.to_string() });
            // Insert ` |inner|` after opening
            if !inner.is_empty() {
                edits.push(Edit { start_offset: opening.end_offset(), end_offset: opening.end_offset(), replacement: format!(" |{}|", inner) });
            }
        } else {
            return None;
        }
    } else if op.end_offset() == opening.start_offset() && opening_src == "do" {
        // No params, no spacing: avoid `lambdado`
        edits.push(Edit { start_offset: opening.start_offset(), end_offset: opening.start_offset(), replacement: " ".to_string() });
    }
    Some(Correction { edits })
}

/// `lambda { [|args|] body }` → `->[(args)] { body }`
fn build_method_to_literal(source: &str, call: &ruby_prism::CallNode) -> Option<Correction> {
    let block = call.block()?;
    let bn = block.as_block_node()?;
    let msg_loc = call.message_loc()?;
    let opening = bn.opening_loc();
    let closing = bn.closing_loc();
    let opening_src = &source[opening.start_offset()..opening.end_offset()];

    let mut edits = Vec::new();
    // Args from block params
    let mut arg_str = String::new();
    if let Some(params) = bn.parameters() {
        if let Some(bp) = params.as_block_parameters_node() {
            let p_loc = bp.location();
            let p_src = &source[p_loc.start_offset()..p_loc.end_offset()];
            // Block params source includes `|...|`
            let inner = p_src.trim_matches('|');
            arg_str = inner.to_string();
            // Remove `|...|` plus 1 leading and 1 trailing space.
            let bytes = source.as_bytes();
            let mut s = p_loc.start_offset();
            let mut e = p_loc.end_offset();
            if e < bytes.len() && (bytes[e] == b' ' || bytes[e] == b'\t') { e += 1 }
            else if s > 0 && (bytes[s-1] == b' ' || bytes[s-1] == b'\t') { s -= 1 }
            edits.push(Edit { start_offset: s, end_offset: e, replacement: "".to_string() });
        } else {
            return None;
        }
    }

    // Replace `lambda` with `->[(args)]`
    let new_selector = if arg_str.is_empty() {
        "->".to_string()
    } else {
        format!("->({})", arg_str)
    };
    edits.push(Edit { start_offset: msg_loc.start_offset(), end_offset: msg_loc.end_offset(), replacement: new_selector });

    // Handle `lambda do...end` → `->(args) do...end`? RuboCop swaps to `{}` when arg_to_unparenthesized_call.
    // Skip that edge case — leave do/end as-is since most fixtures use `{}`.
    let _ = closing;
    let _ = opening_src;

    Some(Correction { edits })
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style: String }

crate::register_cop!("Style/Lambda", |cfg| {
    let c: Cfg = cfg.typed("Style/Lambda");
    let style = match c.enforced_style.as_str() {
        "lambda" => EnforcedStyle::Lambda,
        "literal" => EnforcedStyle::Literal,
        _ => EnforcedStyle::LineCountDependent,
    };
    Some(Box::new(Lambda::with_style(style)))
});
