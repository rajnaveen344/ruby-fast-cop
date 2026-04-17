//! Style/Lambda - Checks for uses of lambda literal vs method-call syntax.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/lambda.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
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
            self.offenses.push(self.ctx.offense_with_range(
                COP_NAME, &msg, Severity::Convention,
                op.start_offset(), op.end_offset(),
            ));
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
                        self.offenses.push(self.ctx.offense_with_range(
                            COP_NAME, &msg, Severity::Convention,
                            msg_loc.start_offset(), msg_loc.end_offset(),
                        ));
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/Lambda", |cfg| {
    let cop_config = cfg.get_cop_config("Style/Lambda");
    let style = cop_config
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "lambda" => EnforcedStyle::Lambda,
            "literal" => EnforcedStyle::Literal,
            _ => EnforcedStyle::LineCountDependent,
        })
        .unwrap_or(EnforcedStyle::LineCountDependent);
    Some(Box::new(Lambda::with_style(style)))
});
