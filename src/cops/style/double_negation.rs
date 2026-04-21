//! Style/DoubleNegation - Checks for uses of double negation (`!!`).
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/double_negation.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/DoubleNegation";
const MSG: &str = "Avoid the use of double negation (`!!`).";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    AllowedInReturns,
    Forbidden,
}

pub struct DoubleNegation {
    style: EnforcedStyle,
}

impl Default for DoubleNegation {
    fn default() -> Self { Self { style: EnforcedStyle::AllowedInReturns } }
}

impl DoubleNegation {
    pub fn new(style: EnforcedStyle) -> Self { Self { style } }
}

impl Cop for DoubleNegation {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Finder { ctx, style: self.style, stack: Vec::new(), offenses: Vec::new(), next_block_is_define_method: false };
        v.visit_program_node(node);
        v.offenses
    }
}

#[derive(Debug, Clone, Copy)]
enum Frame {
    /// Def-scope: tracks body range + last-statement line info.
    DefScope { body_last_first_line: u32, last_is_enum: bool, body_last_line: u32 },
    If { end: usize },
    Unless { end: usize },
    Case { end: usize },
    CaseMatch { end: usize },
    Begin { end: usize },
    Stmts { last_line: u32 }, // Prism StatementsNode (treated like RuboCop's begin)
    Enum, // array/hash/keyword_hash/assoc
    Return,
}

struct Finder<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    style: EnforcedStyle,
    stack: Vec<Frame>,
    offenses: Vec<Offense>,
    next_block_is_define_method: bool,
}

impl<'a, 'b> Finder<'a, 'b> {
    fn src(&self) -> &str { self.ctx.source }

    fn is_prefix_bang(&self, call: &ruby_prism::CallNode) -> bool {
        let name = node_name!(call);
        if name != "!" { return false; }
        let Some(msg) = call.message_loc() else { return false; };
        &self.src()[msg.start_offset()..msg.end_offset()] == "!"
    }

    fn record(&mut self, node: &ruby_prism::CallNode) {
        let msg = node.message_loc().unwrap();
        let (start, end) = (msg.start_offset(), msg.end_offset());
        let node_end = node.location().end_offset();
        let correction = Correction { edits: vec![
            Edit { start_offset: start, end_offset: end, replacement: String::new() },
            Edit { start_offset: node_end, end_offset: node_end, replacement: ".nil?".into() },
        ]};
        self.offenses.push(
            self.ctx.offense_with_range(COP_NAME, MSG, Severity::Convention, start, end)
                .with_correction(correction)
        );
    }

    fn allowed_in_returns(&self, node: &ruby_prism::CallNode) -> bool {
        // Check immediate parent = Return
        if matches!(self.stack.last(), Some(Frame::Return)) { return true; }

        // Find DefScope
        let def_idx = match self.stack.iter().rposition(|f| matches!(f, Frame::DefScope { .. })) {
            Some(i) => i,
            None => return false,
        };
        let Frame::DefScope { body_last_first_line, last_is_enum, body_last_line } = self.stack[def_idx] else { unreachable!() };

        // Find nearest conditional strictly inside def
        let cond_idx = self.stack.iter().enumerate().skip(def_idx + 1)
            .rfind(|(_, f)| matches!(f, Frame::If { .. } | Frame::Unless { .. } | Frame::Case { .. } | Frame::CaseMatch { .. }))
            .map(|(i, _)| i);

        let tline = line_of(node.location().start_offset(), self.src());
        let tlast = line_of(node.location().end_offset(), self.src());

        if let Some(ci) = cond_idx {
            let cond_end = match self.stack[ci] {
                Frame::If { end } | Frame::Unless { end } | Frame::Case { end } | Frame::CaseMatch { end } => end,
                _ => unreachable!(),
            };
            let cond_last_line = line_of(cond_end, self.src());
            // find_parent_not_enumerable: nearest ancestor (from target outward) not being Enum.
            // Stack index ci is conditional; frames after ci are between conditional and target.
            // Walk from innermost (end of stack) toward ci, pick first non-enum.
            let parent_not_enum = self.stack.iter().skip(ci + 1).rev().find(|f| !matches!(f, Frame::Enum));
            match parent_not_enum {
                Some(Frame::Stmts { last_line }) => tline == *last_line,
                Some(Frame::Begin { end }) => tline == line_of(*end, self.src()),
                _ => tlast <= cond_last_line,
            }
        } else {
            // No conditional; last stmt of def must be at/after target
            if last_is_enum {
                // target may still be allowed if it IS inside that enum AND at the last stmt line
                let inside_enum = self.stack.iter().skip(def_idx + 1).any(|f| matches!(f, Frame::Enum));
                if inside_enum {
                    // RuboCop returns false for enum last-child; but fixture says "at return location" with
                    // array/hash containing `!!foo` is allowed. Hmm. Re-read:
                    //   if last_child.type?(:pair, :hash) || last_child.parent.array_type? → false
                    //   else last_child.first_line <= node.first_line
                    // "at return location" fixture tests all use conditionals (if/case). When there IS a
                    // conditional, the enum check doesn't fire. Plain `def foo; [!!bar]; end` isn't in fixtures.
                    return false;
                }
                return false;
            }
            tline >= body_last_first_line && tlast <= body_last_line
        }
    }
}

fn line_of(offset: usize, source: &str) -> u32 {
    1 + source.as_bytes()[..offset.min(source.len())].iter().filter(|&&b| b == b'\n').count() as u32
}

/// Given a body node (StatementsNode/BeginNode/leaf), return (last_stmt_first_line, last_stmt_last_line, last_is_enum).
fn analyze_body(body: &Node) -> (u32, u32, bool) {
    match body {
        Node::StatementsNode { .. } => {
            let s = body.as_statements_node().unwrap();
            let stmts: Vec<_> = s.body().iter().collect();
            if let Some(last) = stmts.last() {
                analyze_last_stmt(last)
            } else { (0, 0, false) }
        }
        Node::BeginNode { .. } => {
            let bn = body.as_begin_node().unwrap();
            if let Some(stmts) = bn.statements() {
                let v: Vec<_> = stmts.body().iter().collect();
                if let Some(last) = v.last() {
                    return analyze_last_stmt(last);
                }
            }
            (0, 0, false)
        }
        _ => analyze_last_stmt(body),
    }
}

fn analyze_last_stmt(node: &Node) -> (u32, u32, bool) {
    let is_enum = matches!(node,
        Node::ArrayNode { .. } | Node::HashNode { .. } | Node::KeywordHashNode { .. } | Node::AssocNode { .. }
    );
    // Note: Can't easily get source here without passing it in. We use raw byte offsets stored
    // later. Caller provides source.
    let start = node.location().start_offset();
    let end = node.location().end_offset();
    (start as u32, end as u32, is_enum)
}

impl<'a, 'b> Visit<'_> for Finder<'a, 'b> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Detect `!!expr`
        if self.is_prefix_bang(node) {
            if let Some(recv) = node.receiver() {
                if let Some(inner) = recv.as_call_node() {
                    if self.is_prefix_bang(&inner) {
                        let allow = self.style == EnforcedStyle::AllowedInReturns && self.allowed_in_returns(node);
                        if !allow {
                            self.record(node);
                        }
                        // Descend
                        ruby_prism::visit_call_node(self, node);
                        return;
                    }
                }
            }
        }

        // define_method / define_singleton_method detection
        let name = node_name!(node);
        if matches!(name.as_ref(), "define_method" | "define_singleton_method") {
            self.next_block_is_define_method = true;
        }

        ruby_prism::visit_call_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let (start, end, enum_, last_first) = compute_def_body_info(node, self.src());
        self.stack.push(Frame::DefScope {
            body_last_first_line: last_first,
            last_is_enum: enum_,
            body_last_line: end,
        });
        let _ = start;
        ruby_prism::visit_def_node(self, node);
        self.stack.pop();
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        if self.next_block_is_define_method {
            self.next_block_is_define_method = false;
            let (start, end, enum_, last_first) = compute_block_body_info(node, self.src());
            self.stack.push(Frame::DefScope { body_last_first_line: last_first, last_is_enum: enum_, body_last_line: end });
            let _ = start;
            ruby_prism::visit_block_node(self, node);
            self.stack.pop();
        } else {
            ruby_prism::visit_block_node(self, node);
        }
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        self.stack.push(Frame::If { end: node.location().end_offset() });
        ruby_prism::visit_if_node(self, node);
        self.stack.pop();
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        self.stack.push(Frame::Unless { end: node.location().end_offset() });
        ruby_prism::visit_unless_node(self, node);
        self.stack.pop();
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        self.stack.push(Frame::Case { end: node.location().end_offset() });
        ruby_prism::visit_case_node(self, node);
        self.stack.pop();
    }

    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        self.stack.push(Frame::CaseMatch { end: node.location().end_offset() });
        ruby_prism::visit_case_match_node(self, node);
        self.stack.pop();
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        self.stack.push(Frame::Begin { end: node.location().end_offset() });
        ruby_prism::visit_begin_node(self, node);
        self.stack.pop();
    }

    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode) {
        // Track last statement's start line as "last_line" to compare against target line
        let body: Vec<_> = node.body().iter().collect();
        let last_line = body.last()
            .map(|n| line_of(n.location().start_offset(), self.src()))
            .unwrap_or(0);
        self.stack.push(Frame::Stmts { last_line });
        ruby_prism::visit_statements_node(self, node);
        self.stack.pop();
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        self.stack.push(Frame::Enum);
        ruby_prism::visit_array_node(self, node);
        self.stack.pop();
    }

    fn visit_hash_node(&mut self, node: &ruby_prism::HashNode) {
        self.stack.push(Frame::Enum);
        ruby_prism::visit_hash_node(self, node);
        self.stack.pop();
    }

    fn visit_keyword_hash_node(&mut self, node: &ruby_prism::KeywordHashNode) {
        self.stack.push(Frame::Enum);
        ruby_prism::visit_keyword_hash_node(self, node);
        self.stack.pop();
    }

    fn visit_assoc_node(&mut self, node: &ruby_prism::AssocNode) {
        self.stack.push(Frame::Enum);
        ruby_prism::visit_assoc_node(self, node);
        self.stack.pop();
    }

    fn visit_return_node(&mut self, node: &ruby_prism::ReturnNode) {
        self.stack.push(Frame::Return);
        ruby_prism::visit_return_node(self, node);
        self.stack.pop();
    }
}

/// Returns (start_line, end_line, last_is_enum, last_stmt_first_line)
fn compute_def_body_info(node: &ruby_prism::DefNode, source: &str) -> (u32, u32, bool, u32) {
    let Some(body) = node.body() else { return (0, 0, false, 0); };
    compute_body_inner(&body, source)
}

fn compute_block_body_info(node: &ruby_prism::BlockNode, source: &str) -> (u32, u32, bool, u32) {
    let Some(body) = node.body() else { return (0, 0, false, 0); };
    compute_body_inner(&body, source)
}

fn compute_body_inner(body: &Node, source: &str) -> (u32, u32, bool, u32) {
    // Walk to find last statement info
    let info: Option<(usize, usize, bool)> = match body {
        Node::StatementsNode { .. } => {
            let s = body.as_statements_node().unwrap();
            let v: Vec<_> = s.body().iter().collect();
            v.last().map(|n| {
                let is_enum = matches!(n,
                    Node::ArrayNode { .. } | Node::HashNode { .. } | Node::KeywordHashNode { .. } | Node::AssocNode { .. }
                );
                (n.location().start_offset(), n.location().end_offset(), is_enum)
            })
        }
        Node::BeginNode { .. } => {
            let bn = body.as_begin_node().unwrap();
            bn.statements().and_then(|s| {
                let v: Vec<_> = s.body().iter().collect();
                v.last().map(|n| {
                    let is_enum = matches!(n,
                        Node::ArrayNode { .. } | Node::HashNode { .. } | Node::KeywordHashNode { .. } | Node::AssocNode { .. }
                    );
                    (n.location().start_offset(), n.location().end_offset(), is_enum)
                })
            })
        }
        _ => None,
    };
    let (last_start, last_end, is_enum) = match info {
        Some(t) => t,
        None => {
            let s = body.location().start_offset();
            let e = body.location().end_offset();
            let is_enum = matches!(body, Node::ArrayNode { .. } | Node::HashNode { .. } | Node::KeywordHashNode { .. } | Node::AssocNode { .. });
            (s, e, is_enum)
        }
    };
    let start_line = line_of(last_start, source);
    let end_line = line_of(last_end, source);
    (start_line, end_line, is_enum, start_line)
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style: String }

crate::register_cop!("Style/DoubleNegation", |cfg| {
    let c: Cfg = cfg.typed("Style/DoubleNegation");
    let style = match c.enforced_style.as_str() {
        "forbidden" => EnforcedStyle::Forbidden,
        _ => EnforcedStyle::AllowedInReturns,
    };
    Some(Box::new(DoubleNegation::new(style)))
});
