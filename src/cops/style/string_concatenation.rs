//! Style/StringConcatenation - Prefer string interpolation over `+`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/string_concatenation.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/StringConcatenation";
const MSG: &str = "Prefer string interpolation to string concatenation.";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Aggressive,
    Conservative,
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Aggressive
    }
}

#[derive(Default)]
pub struct StringConcatenation {
    mode: Mode,
}

impl StringConcatenation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_mode(mode: Mode) -> Self {
        Self { mode }
    }
}

impl Cop for StringConcatenation {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut v = Visitor {
            ctx,
            mode: self.mode,
            offenses: Vec::new(),
            in_plus_chain: 0,
        };
        v.visit(&node.as_node());
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    mode: Mode,
    offenses: Vec<Offense>,
    /// Depth counter: when >0 we are inside a `+` call chain (as receiver or
    /// argument). Visiting a nested `+` call at depth 0 means it's topmost.
    in_plus_chain: usize,
}

fn is_plus_call(node: &ruby_prism::CallNode, src: &str) -> bool {
    let name = String::from_utf8_lossy(node.name().as_slice());
    if name != "+" {
        return false;
    }
    // must have a receiver and at least one argument
    if node.receiver().is_none() {
        return false;
    }
    let args = match node.arguments() {
        Some(a) => a,
        None => return false,
    };
    let list: Vec<_> = args.arguments().iter().collect();
    if list.len() != 1 {
        return false;
    }
    // message_loc is "+" operator
    if let Some(msg) = node.message_loc() {
        let s = &src[msg.start_offset()..msg.end_offset()];
        return s == "+";
    }
    false
}

fn is_string_literal(node: &Node) -> bool {
    matches!(
        node,
        Node::StringNode { .. } | Node::InterpolatedStringNode { .. }
    )
}

/// Check if this `+` call is a string concatenation (one side is a string literal).
fn is_string_concat<'pr>(node: &ruby_prism::CallNode<'pr>, src: &str) -> bool {
    if !is_plus_call(node, src) {
        return false;
    }
    let recv = match node.receiver() {
        Some(r) => r,
        None => return false,
    };
    let arg = node.arguments().unwrap().arguments().iter().next().unwrap();
    is_string_literal(&recv) || is_string_literal(&arg)
}

/// Returns true if any `+` call within the plus chain rooted at `node`
/// has a string literal operand. Matches RuboCop's pattern matcher check.
fn chain_has_string_concat<'pr>(node: &Node<'pr>, src: &str) -> bool {
    if let Some(call) = node.as_call_node() {
        if is_plus_call(&call, src) {
            if is_string_concat(&call, src) {
                return true;
            }
            if let Some(recv) = call.receiver() {
                if chain_has_string_concat(&recv, src) {
                    return true;
                }
            }
            if let Some(arg) = call.arguments().and_then(|a| a.arguments().iter().next()) {
                if chain_has_string_concat(&arg, src) {
                    return true;
                }
            }
        }
    }
    false
}

impl<'a> Visitor<'a> {
    fn is_multiline_string_concat(&self, node: &ruby_prism::CallNode) -> bool {
        // Ruby `line_end_concatenation?`: receiver.str_type? && first_arg.str_type?
        // && multiline? && source =~ /\+\s*\n/
        let recv = match node.receiver() {
            Some(r) => r,
            None => return false,
        };
        let arg = match node.arguments().and_then(|a| a.arguments().iter().next()) {
            Some(a) => a,
            None => return false,
        };
        // Both sides must be simple str (not dstr)
        let r_str = matches!(recv, Node::StringNode { .. });
        let a_str = matches!(arg, Node::StringNode { .. });
        if !(r_str && a_str) {
            return false;
        }
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let src = &self.ctx.source[start..end];
        if !src.contains('\n') {
            return false;
        }
        // check for `+\s*\n` pattern
        if let Some(msg) = node.message_loc() {
            let after = &self.ctx.source[msg.end_offset()..end];
            let trimmed = after.trim_start_matches(|c: char| c == ' ' || c == '\t');
            trimmed.starts_with('\n')
        } else {
            false
        }
    }

    /// Returns whether leftmost terminal part is a string literal.
    fn leftmost_is_string<'pr>(&self, node: &Node<'pr>) -> bool {
        if let Some(call) = node.as_call_node() {
            if is_plus_call(&call, self.ctx.source) {
                if let Some(recv) = call.receiver() {
                    return self.leftmost_is_string(&recv);
                }
            }
        }
        is_string_literal(node)
    }
}

impl<'pr> Visit<'pr> for Visitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        let src = self.ctx.source;
        let is_plus = is_plus_call(node, src);
        // Report only at topmost `+` in the chain (in_plus_chain == 0) if any
        // call within that chain has a string literal operand.
        if is_plus && self.in_plus_chain == 0 && chain_has_string_concat(&node.as_node(), src) {
            if !self.is_multiline_string_concat(node) {
                let first_is_str = self.leftmost_is_string(&node.as_node());
                let skip = self.mode == Mode::Conservative && !first_is_str;
                if !skip {
                    let start = node.location().start_offset();
                    let end = node.location().end_offset();
                    self.offenses.push(self.ctx.offense_with_range(
                        COP_NAME,
                        MSG,
                        Severity::Convention,
                        start,
                        end,
                    ));
                }
            }
        }

        // Propagate `in_plus_chain` depth only through `+` chain (receiver/arg).
        // Non-plus sub-expressions (ternary bodies, call args of other methods,
        // block bodies) reset the depth so nested independent plus chains are
        // detected as topmost.
        if is_plus {
            // Only keep depth > 0 when descending into another `+` call directly.
            // Otherwise reset (so nested plus inside ternary, block, etc. is topmost).
            if let Some(recv) = node.receiver() {
                self.descend_plus_child(&recv);
            }
            if let Some(args) = node.arguments() {
                for a in args.arguments().iter() {
                    self.descend_plus_child(&a);
                }
            }
            if let Some(block) = node.block() {
                let saved = self.in_plus_chain;
                self.in_plus_chain = 0;
                self.visit(&block);
                self.in_plus_chain = saved;
            }
        } else {
            let saved = self.in_plus_chain;
            self.in_plus_chain = 0;
            ruby_prism::visit_call_node(self, node);
            self.in_plus_chain = saved;
        }
    }
}

impl<'pr> Visitor<'_> {
    fn descend_plus_child(&mut self, child: &Node<'pr>) {
        let is_child_plus = child
            .as_call_node()
            .map_or(false, |c| is_plus_call(&c, self.ctx.source));
        if is_child_plus {
            self.in_plus_chain += 1;
            self.visit(child);
            self.in_plus_chain -= 1;
        } else {
            let saved = self.in_plus_chain;
            self.in_plus_chain = 0;
            self.visit(child);
            self.in_plus_chain = saved;
        }
    }
}
