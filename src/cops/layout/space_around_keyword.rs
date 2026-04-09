//! Layout/SpaceAroundKeyword - Checks the spacing around keywords.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/space_around_keyword.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

const MSG_BEFORE: &str = "Space before keyword `%KW%` is missing.";
const MSG_AFTER: &str = "Space after keyword `%KW%` is missing.";

/// Keywords that accept a left parenthesis immediately after (no space needed)
const ACCEPT_LEFT_PAREN: &[&str] = &["break", "defined?", "next", "not", "rescue", "super", "yield"];
/// Keywords that accept a left square bracket immediately after
const ACCEPT_LEFT_SQUARE_BRACKET: &[&str] = &["super", "yield"];

#[derive(Default)]
pub struct SpaceAroundKeyword;

impl SpaceAroundKeyword {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for SpaceAroundKeyword {
    fn name(&self) -> &'static str {
        "Layout/SpaceAroundKeyword"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = KeywordVisitor {
            source: ctx.source,
            offenses: Vec::new(),
            cop_name: self.name(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct KeywordVisitor<'a> {
    source: &'a str,
    offenses: Vec<Offense>,
    cop_name: &'static str,
}

impl<'a> KeywordVisitor<'a> {
    fn bytes(&self) -> &[u8] {
        self.source.as_bytes()
    }

    /// Check a keyword at the given byte range for both before/after spacing.
    fn check_keyword(&mut self, start: usize, end: usize) {
        let kw = &self.source[start..end];
        self.check_space_before(start, end, kw, true);
        self.check_space_after(start, end, kw);
    }

    /// Check just the "before" spacing for an end keyword in a do..end block.
    fn check_end_for_do(&mut self, end_start: usize, end_end: usize, has_do: bool) {
        if !has_do {
            return;
        }
        self.check_space_before(end_start, end_end, "end", false);
    }

    fn check_space_before(
        &mut self,
        start: usize,
        end: usize,
        kw: &str,
        check_preceded_by_operator: bool,
    ) {
        if start == 0 {
            return;
        }
        let prev = self.bytes()[start - 1];
        if matches!(prev, b' ' | b'\t' | b'\n' | b'\r' | b'(' | b'|' | b'{' | b'[' | b';' | b',' | b'*' | b'=') {
            return;
        }
        if check_preceded_by_operator && is_preceded_by_operator(self.source, start) {
            return;
        }
        let msg = MSG_BEFORE.replace("%KW%", kw);
        self.add_offense(start, end, &msg, true);
    }

    fn check_space_after(&mut self, start: usize, end: usize, kw: &str) {
        if end >= self.source.len() {
            return;
        }
        let next_char = self.bytes()[end];

        // Accepted opening delimiters
        if next_char == b'(' && ACCEPT_LEFT_PAREN.contains(&kw) {
            return;
        }
        if next_char == b'[' && ACCEPT_LEFT_SQUARE_BRACKET.contains(&kw) {
            return;
        }

        // Safe navigation: &.
        if next_char == b'&' && end + 1 < self.source.len() && self.bytes()[end + 1] == b'.' {
            return;
        }

        // Namespace operator :: (only for "super")
        if kw == "super" && next_char == b':' && end + 1 < self.source.len() && self.bytes()[end + 1] == b':' {
            return;
        }

        // Allowed chars after keyword
        if matches!(next_char, b' ' | b'\t' | b'\n' | b'\r' | b';' | b',' | b'#' | b'\\' | b')' | b'}' | b']' | b'.') {
            return;
        }

        let msg = MSG_AFTER.replace("%KW%", kw);
        self.add_offense(start, end, &msg, false);
    }

    fn add_offense(&mut self, start: usize, end: usize, msg: &str, is_before: bool) {
        let location = Location::from_offsets(self.source, start, end);
        let correction = if is_before {
            Correction::insert(start, " ")
        } else {
            Correction::insert(end, " ")
        };
        self.offenses.push(
            Offense::new(self.cop_name, msg, Severity::Convention, location, "")
                .with_correction(correction),
        );
    }

    fn loc(&self, loc: &ruby_prism::Location) -> (usize, usize) {
        (loc.start_offset(), loc.end_offset())
    }

    /// Helper: check an ElseNode's else keyword
    fn check_else_node(&mut self, else_node: &ruby_prism::ElseNode) {
        let (s, e) = self.loc(&else_node.else_keyword_loc());
        self.check_keyword(s, e);
    }

    /// Get the keyword text at a Prism location
    fn kw_at(&self, loc: &ruby_prism::Location) -> &str {
        &self.source[loc.start_offset()..loc.end_offset()]
    }
}

/// Check if the character before `start` is an operator character.
fn is_preceded_by_operator(source: &str, start: usize) -> bool {
    if start == 0 {
        return false;
    }
    let bytes = source.as_bytes();
    let prev = bytes[start - 1];
    if matches!(prev, b'+' | b'-' | b'/' | b'<' | b'>' | b'!' | b'~' | b'^' | b'%') {
        return true;
    }
    if prev == b'*' || prev == b'&' {
        return true;
    }
    if prev == b'|' && start >= 2 && bytes[start - 2] == b'|' {
        return true;
    }
    if prev == b'=' {
        return true;
    }
    // Range operators: .. and ...
    if prev == b'.' && start >= 2 && bytes[start - 2] == b'.' {
        return true;
    }
    false
}

impl Visit<'_> for KeywordVisitor<'_> {
    // and/or keywords
    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        let (s, e) = self.loc(&node.operator_loc());
        if &self.source[s..e] == "and" {
            self.check_keyword(s, e);
        }
        ruby_prism::visit_and_node(self, node);
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        let (s, e) = self.loc(&node.operator_loc());
        if &self.source[s..e] == "or" {
            self.check_keyword(s, e);
        }
        ruby_prism::visit_or_node(self, node);
    }

    // block: do...end
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        let (os, oe) = self.loc(&node.opening_loc());
        let opening = &self.source[os..oe];
        if opening == "do" {
            self.check_keyword(os, oe);
        }
        let (cs, ce) = self.loc(&node.closing_loc());
        let closing = &self.source[cs..ce];
        if closing == "end" {
            self.check_end_for_do(cs, ce, opening == "do");
        }
        ruby_prism::visit_block_node(self, node);
    }

    // lambda block
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        let (os, oe) = self.loc(&node.opening_loc());
        let opening = &self.source[os..oe];
        if opening == "do" {
            self.check_keyword(os, oe);
        }
        let (cs, ce) = self.loc(&node.closing_loc());
        if &self.source[cs..ce] == "end" {
            self.check_end_for_do(cs, ce, opening == "do");
        }
        ruby_prism::visit_lambda_node(self, node);
    }

    // break
    fn visit_break_node(&mut self, node: &ruby_prism::BreakNode) {
        let (s, e) = self.loc(&node.keyword_loc());
        self.check_keyword(s, e);
        ruby_prism::visit_break_node(self, node);
    }

    // case...when...else...end
    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        let (s, e) = self.loc(&node.case_keyword_loc());
        self.check_keyword(s, e);
        if let Some(else_clause) = node.else_clause() {
            self.check_else_node(&else_clause);
        }
        ruby_prism::visit_case_node(self, node);
    }

    // case...in (pattern matching)
    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        let (s, e) = self.loc(&node.case_keyword_loc());
        self.check_keyword(s, e);
        if let Some(else_clause) = node.else_clause() {
            self.check_else_node(&else_clause);
        }
        ruby_prism::visit_case_match_node(self, node);
    }

    // ensure
    fn visit_ensure_node(&mut self, node: &ruby_prism::EnsureNode) {
        let (s, e) = self.loc(&node.ensure_keyword_loc());
        self.check_keyword(s, e);
        ruby_prism::visit_ensure_node(self, node);
    }

    // for...in...do...end
    fn visit_for_node(&mut self, node: &ruby_prism::ForNode) {
        let (s, e) = self.loc(&node.for_keyword_loc());
        self.check_keyword(s, e);
        if let Some(do_loc) = node.do_keyword_loc() {
            if self.kw_at(&do_loc) == "do" {
                let (ds, de) = self.loc(&do_loc);
                self.check_keyword(ds, de);
            }
        }
        let (es, ee) = self.loc(&node.end_keyword_loc());
        let has_do = node.do_keyword_loc().map_or(false, |loc| self.kw_at(&loc) == "do");
        self.check_end_for_do(es, ee, has_do);
        ruby_prism::visit_for_node(self, node);
    }

    // if/elsif/then/end
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        if let Some(kw_loc) = node.if_keyword_loc() {
            let (s, e) = self.loc(&kw_loc);
            let kw = &self.source[s..e];
            // "if" or "elsif"
            self.check_keyword(s, e);

            // then keyword
            if let Some(then_loc) = node.then_keyword_loc() {
                if self.kw_at(&then_loc) == "then" {
                    let (ts, te) = self.loc(&then_loc);
                    self.check_keyword(ts, te);
                }
            }

            // end keyword - only for top-level if (not elsif)
            if kw == "if" {
                if let Some(end_loc) = node.end_keyword_loc() {
                    let (es, ee) = self.loc(&end_loc);
                    self.check_space_before(es, ee, "end", false);
                }
            }
        }

        // else clause (handled via subsequent -> ElseNode)
        if let Some(subsequent) = node.subsequent() {
            match &subsequent {
                ruby_prism::Node::ElseNode { .. } => {
                    let else_node = subsequent.as_else_node().unwrap();
                    self.check_else_node(&else_node);
                }
                _ => {} // elsif is handled as another IfNode visit
            }
        }

        ruby_prism::visit_if_node(self, node);
    }

    // unless
    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        let (s, e) = self.loc(&node.keyword_loc());
        self.check_keyword(s, e);

        if let Some(else_clause) = node.else_clause() {
            self.check_else_node(&else_clause);
        }

        if let Some(end_loc) = node.end_keyword_loc() {
            let (es, ee) = self.loc(&end_loc);
            self.check_space_before(es, ee, "end", false);
        }

        ruby_prism::visit_unless_node(self, node);
    }

    // begin...rescue...ensure...end
    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        if let Some(begin_loc) = node.begin_keyword_loc() {
            let (s, e) = self.loc(&begin_loc);
            self.check_keyword(s, e);
        }
        if let Some(end_loc) = node.end_keyword_loc() {
            let (es, ee) = self.loc(&end_loc);
            self.check_space_before(es, ee, "end", false);
        }
        // else on rescue is via else_clause
        if let Some(else_clause) = node.else_clause() {
            self.check_else_node(&else_clause);
        }
        ruby_prism::visit_begin_node(self, node);
    }

    // rescue clause
    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode) {
        let (s, e) = self.loc(&node.keyword_loc());
        self.check_keyword(s, e);
        ruby_prism::visit_rescue_node(self, node);
    }

    // while...do...end
    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        let (s, e) = self.loc(&node.keyword_loc());
        self.check_keyword(s, e);

        if let Some(do_loc) = node.do_keyword_loc() {
            if self.kw_at(&do_loc) == "do" {
                let (ds, de) = self.loc(&do_loc);
                self.check_keyword(ds, de);
            }
        }

        if let Some(closing_loc) = node.closing_loc() {
            let (cs, ce) = self.loc(&closing_loc);
            if &self.source[cs..ce] == "end" {
                let has_do = node.do_keyword_loc().map_or(false, |loc| self.kw_at(&loc) == "do");
                self.check_end_for_do(cs, ce, has_do);
            }
        }

        ruby_prism::visit_while_node(self, node);
    }

    // until...do...end
    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        let (s, e) = self.loc(&node.keyword_loc());
        self.check_keyword(s, e);

        if let Some(do_loc) = node.do_keyword_loc() {
            if self.kw_at(&do_loc) == "do" {
                let (ds, de) = self.loc(&do_loc);
                self.check_keyword(ds, de);
            }
        }

        if let Some(closing_loc) = node.closing_loc() {
            let (cs, ce) = self.loc(&closing_loc);
            if &self.source[cs..ce] == "end" {
                let has_do = node.do_keyword_loc().map_or(false, |loc| self.kw_at(&loc) == "do");
                self.check_end_for_do(cs, ce, has_do);
            }
        }

        ruby_prism::visit_until_node(self, node);
    }

    // return
    fn visit_return_node(&mut self, node: &ruby_prism::ReturnNode) {
        let (s, e) = self.loc(&node.keyword_loc());
        self.check_keyword(s, e);
        ruby_prism::visit_return_node(self, node);
    }

    // next
    fn visit_next_node(&mut self, node: &ruby_prism::NextNode) {
        let (s, e) = self.loc(&node.keyword_loc());
        self.check_keyword(s, e);
        ruby_prism::visit_next_node(self, node);
    }

    // yield
    fn visit_yield_node(&mut self, node: &ruby_prism::YieldNode) {
        let (s, e) = self.loc(&node.keyword_loc());
        self.check_keyword(s, e);
        ruby_prism::visit_yield_node(self, node);
    }

    // super(args)
    fn visit_super_node(&mut self, node: &ruby_prism::SuperNode) {
        let (s, e) = self.loc(&node.keyword_loc());
        self.check_keyword(s, e);
        ruby_prism::visit_super_node(self, node);
    }

    // super (no args) - zsuper
    fn visit_forwarding_super_node(&mut self, node: &ruby_prism::ForwardingSuperNode) {
        let start = node.location().start_offset();
        let end = start + 5; // "super"
        if end <= self.source.len() && &self.source[start..end] == "super" {
            self.check_keyword(start, end);
        }
        ruby_prism::visit_forwarding_super_node(self, node);
    }

    // defined?
    fn visit_defined_node(&mut self, node: &ruby_prism::DefinedNode) {
        let (s, e) = self.loc(&node.keyword_loc());
        self.check_keyword(s, e);
        ruby_prism::visit_defined_node(self, node);
    }

    // when
    fn visit_when_node(&mut self, node: &ruby_prism::WhenNode) {
        let (s, e) = self.loc(&node.keyword_loc());
        self.check_keyword(s, e);
        ruby_prism::visit_when_node(self, node);
    }

    // in (pattern matching)
    fn visit_in_node(&mut self, node: &ruby_prism::InNode) {
        let (s, e) = self.loc(&node.in_loc());
        self.check_keyword(s, e);
        ruby_prism::visit_in_node(self, node);
    }

    // not (prefix `not` keyword, parsed as a call with method `!`)
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if let Some(msg_loc) = node.message_loc() {
            let (s, e) = self.loc(&msg_loc);
            if e <= self.source.len() && &self.source[s..e] == "not" {
                self.check_keyword(s, e);
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    // BEGIN { }
    fn visit_pre_execution_node(&mut self, node: &ruby_prism::PreExecutionNode) {
        let (s, e) = self.loc(&node.keyword_loc());
        self.check_keyword(s, e);
        ruby_prism::visit_pre_execution_node(self, node);
    }

    // END { }
    fn visit_post_execution_node(&mut self, node: &ruby_prism::PostExecutionNode) {
        let (s, e) = self.loc(&node.keyword_loc());
        self.check_keyword(s, e);
        ruby_prism::visit_post_execution_node(self, node);
    }

    // Modifier rescue: `a rescue b`
    fn visit_rescue_modifier_node(&mut self, node: &ruby_prism::RescueModifierNode) {
        let (s, e) = self.loc(&node.keyword_loc());
        self.check_keyword(s, e);
        ruby_prism::visit_rescue_modifier_node(self, node);
    }

    // One-line pattern matching: `expr in pattern`
    fn visit_match_predicate_node(&mut self, node: &ruby_prism::MatchPredicateNode) {
        let (s, e) = self.loc(&node.operator_loc());
        self.check_keyword(s, e);
        ruby_prism::visit_match_predicate_node(self, node);
    }

    // if/unless guards in pattern matching
    fn visit_pinned_expression_node(&mut self, node: &ruby_prism::PinnedExpressionNode) {
        ruby_prism::visit_pinned_expression_node(self, node);
    }
}
