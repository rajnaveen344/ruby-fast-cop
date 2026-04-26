//! Style/MissingElse - Checks for `if`/`case` expressions without an `else` branch.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/missing_else.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "`%s` condition requires an `else`-clause.";
const MSG_NIL: &str = "`%s` condition requires an `else`-clause with `nil` in it.";
const MSG_EMPTY: &str = "`%s` condition requires an empty `else`-clause.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    If,
    Case,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyElseStyle {
    Empty,
    Nil,
    Other,
}

pub struct MissingElse {
    style: EnforcedStyle,
    /// Cross-cop: Style/UnlessElse enabled? When true, skip `unless` (UnlessElse handles it).
    unless_else_enabled: bool,
    /// Cross-cop: Style/EmptyElse enforced style (controls message + correction).
    empty_else_style: EmptyElseStyle,
}

impl MissingElse {
    pub fn new(
        style: EnforcedStyle,
        unless_else_enabled: bool,
        empty_else_style: EmptyElseStyle,
    ) -> Self {
        Self {
            style,
            unless_else_enabled,
            empty_else_style,
        }
    }

    fn case_style(&self) -> bool {
        matches!(self.style, EnforcedStyle::Case)
    }

    fn if_style(&self) -> bool {
        matches!(self.style, EnforcedStyle::If)
    }

    fn message_template(&self) -> &'static str {
        match self.empty_else_style {
            EmptyElseStyle::Empty => MSG_NIL,
            EmptyElseStyle::Nil => MSG_EMPTY,
            EmptyElseStyle::Other => MSG,
        }
    }

    fn correction_text(&self) -> Option<&'static str> {
        match self.empty_else_style {
            EmptyElseStyle::Empty => Some("else; nil; "),
            EmptyElseStyle::Nil => Some("else; "),
            EmptyElseStyle::Other => None,
        }
    }
}

impl Default for MissingElse {
    fn default() -> Self {
        Self::new(EnforcedStyle::Both, false, EmptyElseStyle::Other)
    }
}

impl Cop for MissingElse {
    fn name(&self) -> &'static str {
        "Style/MissingElse"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = MissingElseVisitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct MissingElseVisitor<'a> {
    cop: &'a MissingElse,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> MissingElseVisitor<'a> {
    fn kw_src(&self, loc: &ruby_prism::Location) -> &str {
        &self.ctx.source[loc.start_offset()..loc.end_offset()]
    }

    /// Detect modifier-form if (e.g. `foo if bar`).
    fn is_modifier_if(&self, node: &ruby_prism::IfNode) -> bool {
        if let (Some(kw_loc), Some(stmts)) = (node.if_keyword_loc(), node.statements()) {
            let kw_start = kw_loc.start_offset();
            let body_start = stmts.location().start_offset();
            return kw_start > body_start;
        }
        false
    }

    /// Detect ternary (`?:`).
    fn is_ternary(&self, node: &ruby_prism::IfNode) -> bool {
        node.then_keyword_loc()
            .map(|loc| self.kw_src(&loc) == "?")
            .unwrap_or(false)
    }

    /// Compute offense range end for an IfNode.
    /// For `if`/`unless` keyword nodes (outermost): use full node location (covers `end`).
    /// For `elsif` nodes (inner): end at last meaningful child (excludes trailing `end`).
    fn if_offense_end(&self, node: &ruby_prism::IfNode) -> usize {
        let is_elsif = node
            .if_keyword_loc()
            .map(|l| self.kw_src(&l) == "elsif")
            .unwrap_or(false);
        if !is_elsif {
            return node.location().end_offset();
        }
        let mut end = node.predicate().location().end_offset();
        if let Some(stmts) = node.statements() {
            let s_end = stmts.location().end_offset();
            if s_end > end {
                end = s_end;
            }
        }
        end
    }

    /// Find the outermost end_keyword_loc starting from this node by walking
    /// up through the chain of elsif IfNodes.
    fn outer_end_keyword_offset(&self, node: &ruby_prism::IfNode) -> Option<usize> {
        // For an inner elsif, we need the OUTERMOST end. Since Prism doesn't
        // give us parents directly, the visitor passes this info down.
        node.end_keyword_loc().map(|l| l.start_offset())
    }

    fn flag_if(&mut self, node: &ruby_prism::IfNode, end_kw_offset: Option<usize>) {
        let start = node.location().start_offset();
        let end = self.if_offense_end(node);
        let msg = self.cop.message_template().replace("%s", "if");
        let mut off = self.ctx.offense_with_range(
            "Style/MissingElse",
            &msg,
            Severity::Convention,
            start,
            end,
        );
        if let (Some(text), Some(end_offset)) = (self.cop.correction_text(), end_kw_offset) {
            off = off.with_correction(Correction::insert(end_offset, text));
        }
        self.offenses.push(off);
    }

    fn flag_unless(&mut self, node: &ruby_prism::UnlessNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let msg = self.cop.message_template().replace("%s", "if");
        let mut off = self.ctx.offense_with_range(
            "Style/MissingElse",
            &msg,
            Severity::Convention,
            start,
            end,
        );
        if let (Some(text), Some(end_kw)) = (self.cop.correction_text(), node.end_keyword_loc()) {
            off = off.with_correction(Correction::insert(end_kw.start_offset(), text));
        }
        self.offenses.push(off);
    }

    fn flag_case(&mut self, node: &ruby_prism::CaseNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let msg = self.cop.message_template().replace("%s", "case");
        let mut off = self.ctx.offense_with_range(
            "Style/MissingElse",
            &msg,
            Severity::Convention,
            start,
            end,
        );
        if let Some(text) = self.cop.correction_text() {
            let end_kw = node.end_keyword_loc();
            off = off.with_correction(Correction::insert(end_kw.start_offset(), text));
        }
        self.offenses.push(off);
    }
}

impl<'pr, 'a> Visit<'pr> for MissingElseVisitor<'a> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'pr>) {
        if self.cop.case_style() {
            ruby_prism::visit_if_node(self, node);
            return;
        }
        // Skip ternary + modifier-form
        if self.is_ternary(node) || self.is_modifier_if(node) {
            ruby_prism::visit_if_node(self, node);
            return;
        }
        // Only flag if no subsequent (no else, no elsif).
        // If subsequent is an IfNode (elsif chain), recurse — inner elsif may be flagged.
        // If subsequent is an ElseNode, the if has an else — skip.
        match node.subsequent() {
            None => {
                // Walk up via stack: inner elsifs need outer's end_keyword. We approximate
                // using stack from visitor state.
                let end_kw = self.find_outer_end(node);
                self.flag_if(node, end_kw);
            }
            Some(_) => {} // has else or elsif → don't flag this one
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode<'pr>) {
        if self.cop.case_style() {
            ruby_prism::visit_unless_node(self, node);
            return;
        }
        if self.cop.unless_else_enabled {
            ruby_prism::visit_unless_node(self, node);
            return;
        }
        if node.else_clause().is_none() {
            self.flag_unless(node);
        }
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode<'pr>) {
        if self.cop.if_style() {
            ruby_prism::visit_case_node(self, node);
            return;
        }
        if node.else_clause().is_none() {
            self.flag_case(node);
        }
        ruby_prism::visit_case_node(self, node);
    }

    // CaseMatchNode (pattern matching: `case x; in ...; end`) — explicitly not flagged.
    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode<'pr>) {
        ruby_prism::visit_case_match_node(self, node);
    }
}

impl<'a> MissingElseVisitor<'a> {
    /// For an inner elsif IfNode (no end_keyword_loc), traverse via Prism's
    /// node tree to find the outermost containing if's `end` keyword. Since
    /// Prism nodes don't expose parents, we re-walk from this elsif: its
    /// `subsequent()` may be a deeper IfNode chain — but `end` is on the
    /// OUTERMOST. We can't find it from inside, so the only reliable way is
    /// to pre-compute on the way down. For simplicity, compute via location:
    /// the outermost `if`'s end_offset surrounds this node, and the `end`
    /// keyword sits exactly at `outer.location.end_offset() - 3`.
    ///
    /// In practice, for `if cond_1; 1; elsif cond_2; 3; end`:
    ///   inner elsif's location is `[14..34]` and ends with `end`.
    ///   So `inner.location.end_offset() - 3` = 31, which is the `end` keyword start.
    fn find_outer_end(&self, node: &ruby_prism::IfNode) -> Option<usize> {
        if let Some(loc) = node.end_keyword_loc() {
            return Some(loc.start_offset());
        }
        // Inner elsif: location stretches to outermost `end`. Last 3 bytes = "end".
        let end = node.location().end_offset();
        if end >= 3 && &self.ctx.source.as_bytes()[end - 3..end] == b"end" {
            return Some(end - 3);
        }
        None
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct EmptyElseCfg {
    enforced_style: String,
}

crate::register_cop!("Style/MissingElse", |cfg| {
    let c: Cfg = cfg.typed("Style/MissingElse");
    let style = match c.enforced_style.as_str() {
        "if" => EnforcedStyle::If,
        "case" => EnforcedStyle::Case,
        _ => EnforcedStyle::Both,
    };

    // Cross-cop: Style/UnlessElse Enabled — fixtures always set it explicitly.
    let unless_else_enabled = cfg
        .get_cop_config("Style/UnlessElse")
        .and_then(|c| c.enabled)
        .unwrap_or(true);

    // Cross-cop: Style/EmptyElse EnforcedStyle (only if Style/EmptyElse explicitly configured).
    let empty_else_style = if cfg.get_cop_config("Style/EmptyElse").is_some() {
        let ec: EmptyElseCfg = cfg.typed("Style/EmptyElse");
        match ec.enforced_style.as_str() {
            "empty" => EmptyElseStyle::Empty,
            "nil" => EmptyElseStyle::Nil,
            _ => EmptyElseStyle::Other,
        }
    } else {
        EmptyElseStyle::Other
    };

    Some(Box::new(MissingElse::new(
        style,
        unless_else_enabled,
        empty_else_style,
    )))
});
