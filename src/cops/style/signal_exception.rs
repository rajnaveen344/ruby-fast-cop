//! Style/SignalException — Enforces consistent use of raise/fail.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/signal_exception.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    Semantic,
    OnlyRaise,
    OnlyFail,
}

impl Default for EnforcedStyle {
    fn default() -> Self {
        EnforcedStyle::Semantic
    }
}

pub struct SignalException {
    style: EnforcedStyle,
}

impl Default for SignalException {
    fn default() -> Self {
        Self { style: EnforcedStyle::Semantic }
    }
}

impl SignalException {
    pub fn new(style: EnforcedStyle) -> Self {
        Self { style }
    }
}

impl Cop for SignalException {
    fn name(&self) -> &'static str {
        "Style/SignalException"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = SignalExceptionVisitor {
            ctx,
            style: self.style,
            offenses: Vec::new(),
            in_rescue_depth: 0,
            has_custom_fail_method: false,
        };
        // First pass: check for custom fail methods
        let mut fail_checker = CustomFailChecker { has_custom_fail: false };
        fail_checker.visit(&node.as_node());
        visitor.has_custom_fail_method = fail_checker.has_custom_fail;
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

/// Check if a class defines its own `fail` method.
struct CustomFailChecker {
    has_custom_fail: bool,
}

impl<'a> Visit<'_> for CustomFailChecker {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let name = String::from_utf8_lossy(node.name().as_slice());
        if name == "fail" {
            self.has_custom_fail = true;
        }
        ruby_prism::visit_def_node(self, node);
    }
}

struct SignalExceptionVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: EnforcedStyle,
    offenses: Vec<Offense>,
    /// How many rescue clauses deep we are
    in_rescue_depth: usize,
    has_custom_fail_method: bool,
}

impl<'a> SignalExceptionVisitor<'a> {
    fn is_raise_or_fail(&self, node: &ruby_prism::CallNode) -> Option<(&'static str, bool)> {
        // Returns (keyword, is_raise) where is_raise=true for raise/Kernel.raise
        let method = node_name!(node);
        let method_str = method.as_ref();

        // Skip if explicit non-Kernel receiver
        if let Some(recv) = node.receiver() {
            let recv_src = &self.ctx.source[recv.location().start_offset()..recv.location().end_offset()];
            if recv_src != "Kernel" && recv_src != "::Kernel" {
                return None;
            }
        }

        match method_str {
            "raise" => Some(("raise", true)),
            "fail" => Some(("fail", false)),
            _ => None,
        }
    }

    fn check_call_node(&mut self, node: &ruby_prism::CallNode) {
        let (keyword, is_raise) = match self.is_raise_or_fail(node) {
            Some(x) => x,
            None => return,
        };

        match self.style {
            EnforcedStyle::OnlyRaise => {
                if !is_raise && !self.has_custom_fail_method {
                    let msg = "Always use `raise` to signal exceptions.";
                    let loc = node.message_loc().unwrap_or_else(|| node.location());
                    let start = loc.start_offset();
                    let end = loc.end_offset();
                    let offense = self.ctx.offense_with_range(
                        "Style/SignalException", msg, Severity::Convention, start, end,
                    ).with_correction(Correction::replace(start, end, "raise".to_string()));
                    self.offenses.push(offense);
                }
            }
            EnforcedStyle::OnlyFail => {
                if is_raise {
                    let msg = "Always use `fail` to signal exceptions.";
                    let loc = node.message_loc().unwrap_or_else(|| node.location());
                    let start = loc.start_offset();
                    let end = loc.end_offset();
                    let offense = self.ctx.offense_with_range(
                        "Style/SignalException", msg, Severity::Convention, start, end,
                    ).with_correction(Correction::replace(start, end, "fail".to_string()));
                    self.offenses.push(offense);
                }
            }
            EnforcedStyle::Semantic => {
                let in_rescue = self.in_rescue_depth > 0;
                if in_rescue {
                    // In rescue: should use `raise`
                    if !is_raise {
                        let msg = "Use `raise` instead of `fail` to rethrow exceptions.";
                        let loc = node.message_loc().unwrap_or_else(|| node.location());
                        let start = loc.start_offset();
                        let end = loc.end_offset();
                        let offense = self.ctx.offense_with_range(
                            "Style/SignalException", msg, Severity::Convention, start, end,
                        ).with_correction(Correction::replace(start, end, "raise".to_string()));
                        self.offenses.push(offense);
                    }
                } else {
                    // Outside rescue: should use `fail`
                    if is_raise {
                        let msg = "Use `fail` instead of `raise` to signal exceptions.";
                        let loc = node.message_loc().unwrap_or_else(|| node.location());
                        let start = loc.start_offset();
                        let end = loc.end_offset();
                        let offense = self.ctx.offense_with_range(
                            "Style/SignalException", msg, Severity::Convention, start, end,
                        ).with_correction(Correction::replace(start, end, "fail".to_string()));
                        self.offenses.push(offense);
                    }
                }
            }
        }
    }
}

impl<'a> Visit<'_> for SignalExceptionVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call_node(node);
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode) {
        self.in_rescue_depth += 1;
        ruby_prism::visit_rescue_node(self, node);
        self.in_rescue_depth -= 1;
    }

    // begin..rescue..end — rescue is a rescue_modifier_node for inline rescue
    fn visit_rescue_modifier_node(&mut self, node: &ruby_prism::RescueModifierNode) {
        // Visit the expression first (not in rescue)
        ruby_prism::visit_rescue_modifier_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style: String }

crate::register_cop!("Style/SignalException", |cfg| {
    let c: Cfg = cfg.typed("Style/SignalException");
    let style = match c.enforced_style.as_str() {
        "only_raise" => EnforcedStyle::OnlyRaise,
        "only_fail" => EnforcedStyle::OnlyFail,
        _ => EnforcedStyle::Semantic,
    };
    Some(Box::new(SignalException::new(style)))
});
