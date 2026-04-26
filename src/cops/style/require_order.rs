//! Style/RequireOrder - Sort `require`/`require_relative` alphabetically within sections.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/require_order.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Sort `%s` in alphabetical order.";

#[derive(Default)]
pub struct RequireOrder;

impl RequireOrder {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for RequireOrder {
    fn name(&self) -> &'static str {
        "Style/RequireOrder"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = ReqOrderVisitor {
            ctx,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct ReqOrderVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

/// A require call extracted from a sibling list, with its enclosing node.
struct RequireInfo<'pr> {
    /// The actual `require` send-node.
    send: ruby_prism::CallNode<'pr>,
    /// The string literal value (first arg).
    name: String,
    /// The method name (`require` or `require_relative`).
    method: String,
    /// The wrapping node when this require is inside a modifier-if;
    /// otherwise the send itself. Used for "section" boundary checks.
    enclosing_start: usize,
    enclosing_end: usize,
}

impl<'a> ReqOrderVisitor<'a> {
    /// Walk a list of sibling statements, looking for require calls and
    /// flagging unsorted ones.
    fn check_siblings<'pr>(&mut self, siblings: Vec<Node<'pr>>) {
        // Build list of require infos in declaration order, with `None` for
        // siblings that aren't requires.
        let infos: Vec<Option<RequireInfo<'pr>>> = siblings
            .iter()
            .map(|n| Self::extract_require(n, self.ctx))
            .collect();

        for (i, info) in infos.iter().enumerate() {
            let Some(curr) = info.as_ref() else { continue };

            // Look backwards for a previous "older" sibling matching RuboCop's logic.
            for j in (0..i).rev() {
                let prev = match &infos[j] {
                    Some(p) => p,
                    None => break, // non-require sibling breaks the chain
                };
                // Same method?
                if prev.method != curr.method {
                    break;
                }
                // Same section: no blank line between prev start and curr end.
                if !Self::in_same_section(self.ctx, prev.enclosing_start, curr.enclosing_end) {
                    break;
                }
                // Both first args must be strings (already enforced in extract_require).
                if curr.name < prev.name {
                    let start = curr.send.location().start_offset();
                    let end = curr.send.location().end_offset();
                    let msg = MSG.replace("%s", &curr.method);
                    self.offenses.push(self.ctx.offense_with_range(
                        "Style/RequireOrder",
                        &msg,
                        Severity::Convention,
                        start,
                        end,
                    ));
                    break;
                } else {
                    // Not older — keep looking further back.
                    continue;
                }
            }
        }
    }

    /// If `n` is a `require`/`require_relative` send (with single string arg
    /// and no receiver), or a modifier-if wrapping one, return its info.
    fn extract_require<'pr>(n: &Node<'pr>, ctx: &CheckContext) -> Option<RequireInfo<'pr>> {
        // Direct send
        if let Some(call) = n.as_call_node() {
            return Self::call_to_info(call, n.location().start_offset(), n.location().end_offset(), ctx);
        }
        // Modifier-if wrapping a send
        if let Some(if_node) = n.as_if_node() {
            if !is_modifier_if(&if_node) {
                return None;
            }
            // Modifier-if body = the require call
            let stmts = if_node.statements()?;
            let body: Vec<_> = stmts.body().iter().collect();
            if body.len() != 1 {
                return None;
            }
            if let Some(call) = body[0].as_call_node() {
                return Self::call_to_info(
                    call,
                    n.location().start_offset(),
                    n.location().end_offset(),
                    ctx,
                );
            }
        }
        // Modifier-unless wrapping a send
        if let Some(un_node) = n.as_unless_node() {
            if !is_modifier_unless(&un_node) {
                return None;
            }
            let stmts = un_node.statements()?;
            let body: Vec<_> = stmts.body().iter().collect();
            if body.len() != 1 {
                return None;
            }
            if let Some(call) = body[0].as_call_node() {
                return Self::call_to_info(
                    call,
                    n.location().start_offset(),
                    n.location().end_offset(),
                    ctx,
                );
            }
        }
        None
    }

    fn call_to_info<'pr>(
        call: ruby_prism::CallNode<'pr>,
        enclosing_start: usize,
        enclosing_end: usize,
        _ctx: &CheckContext,
    ) -> Option<RequireInfo<'pr>> {
        // Receiver must be nil
        if call.receiver().is_some() {
            return None;
        }
        let method = String::from_utf8_lossy(call.name().as_slice()).to_string();
        if method != "require" && method != "require_relative" {
            return None;
        }
        // First argument must be a string literal
        let args = call.arguments()?;
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return None;
        }
        let str_node = arg_list[0].as_string_node()?;
        let name = String::from_utf8_lossy(str_node.unescaped()).to_string();
        Some(RequireInfo {
            send: call,
            name,
            method,
            enclosing_start,
            enclosing_end,
        })
    }

    /// Match RuboCop's `in_same_section?`: source between sibling start and node
    /// end contains no blank line (`\n\n`).
    fn in_same_section(ctx: &CheckContext, prev_start: usize, curr_end: usize) -> bool {
        if prev_start >= curr_end {
            return false;
        }
        !ctx.source[prev_start..curr_end].contains("\n\n")
    }
}

fn is_modifier_if(node: &ruby_prism::IfNode) -> bool {
    if let (Some(kw_loc), Some(stmts)) = (node.if_keyword_loc(), node.statements()) {
        return kw_loc.start_offset() > stmts.location().start_offset();
    }
    false
}

fn is_modifier_unless(node: &ruby_prism::UnlessNode) -> bool {
    if let Some(stmts) = node.statements() {
        let kw_start = node.keyword_loc().start_offset();
        return kw_start > stmts.location().start_offset();
    }
    false
}

impl<'pr, 'a> Visit<'pr> for ReqOrderVisitor<'a> {
    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode<'pr>) {
        let body: Vec<Node<'pr>> = node.body().iter().collect();
        self.check_siblings(body);
        ruby_prism::visit_statements_node(self, node);
    }
}

crate::register_cop!("Style/RequireOrder", |_cfg| Some(Box::new(RequireOrder::new())));
