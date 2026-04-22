//! Lint/RedundantWithObject - Detect redundant each_with_object/with_object when object unused.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

const MSG_EACH_WITH_OBJECT: &str = "Use `each` instead of `each_with_object`.";
const MSG_WITH_OBJECT: &str = "Remove redundant `with_object`.";

#[derive(Default)]
pub struct RedundantWithObject;

impl RedundantWithObject {
    pub fn new() -> Self { Self }
}

impl Cop for RedundantWithObject {
    fn name(&self) -> &'static str { "Lint/RedundantWithObject" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl Visit<'_> for Visitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice());
        let method_str = method.as_ref();

        match method_str {
            "each_with_object" => {
                if node.receiver().is_some() && call_has_arg(node) {
                    if let Some(block) = node.block() {
                        if is_redundant_block(&block, self.ctx.source) {
                            self.report_each_with_object(node);
                        }
                    }
                }
            }
            "with_object" => {
                if call_has_arg(node) {
                    if let Some(recv) = node.receiver() {
                        if let Some(recv_call) = recv.as_call_node() {
                            if recv_call.receiver().is_some() {
                                if let Some(block) = node.block() {
                                    if is_redundant_block(&block, self.ctx.source) {
                                        self.report_with_object(node);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        ruby_prism::visit_call_node(self, node);
    }
}

impl<'a> Visitor<'a> {
    fn report_each_with_object(&mut self, node: &ruby_prism::CallNode) {
        let (sel_start, call_end) = selector_to_call_end(node);
        let mut offense = self.ctx.offense_with_range(
            "Lint/RedundantWithObject",
            MSG_EACH_WITH_OBJECT,
            Severity::Warning,
            sel_start,
            call_end,
        );
        offense = offense.with_correction(Correction::replace(sel_start, call_end, "each".to_string()));
        self.offenses.push(offense);
    }

    fn report_with_object(&mut self, node: &ruby_prism::CallNode) {
        let (sel_start, call_end) = selector_to_call_end(node);
        let dot_start = if let Some(op_loc) = node.call_operator_loc() {
            op_loc.start_offset()
        } else {
            sel_start.saturating_sub(1)
        };
        let mut offense = self.ctx.offense_with_range(
            "Lint/RedundantWithObject",
            MSG_WITH_OBJECT,
            Severity::Warning,
            sel_start,
            call_end,
        );
        offense = offense.with_correction(Correction::delete(dot_start, call_end));
        self.offenses.push(offense);
    }
}

fn selector_to_call_end(node: &ruby_prism::CallNode) -> (usize, usize) {
    // Offense end = closing paren of call args, not end of block.
    // closing_loc() = the `)` after arguments, or None if no parens (bare args).
    let call_end = if let Some(cl) = node.closing_loc() {
        cl.end_offset()
    } else if let Some(args) = node.arguments() {
        // No parens — end of last argument
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if let Some(last) = arg_list.last() {
            last.location().end_offset()
        } else {
            node.location().end_offset()
        }
    } else {
        // No args at all — end of selector
        node.message_loc()
            .map(|m| m.end_offset())
            .unwrap_or_else(|| node.location().end_offset())
    };
    let sel_start = if let Some(msg_loc) = node.message_loc() {
        msg_loc.start_offset()
    } else {
        node.location().start_offset()
    };
    (sel_start, call_end)
}

fn call_has_arg(node: &ruby_prism::CallNode) -> bool {
    node.arguments()
        .map(|a| a.arguments().iter().count() > 0)
        .unwrap_or(false)
}

/// Block is redundant if object (2nd param) is not bound/used.
fn is_redundant_block(block: &ruby_prism::Node, source: &str) -> bool {
    if let Some(bn) = block.as_block_node() {
        // Named block: check if 2+ required params
        if let Some(params) = bn.parameters() {
            if let Some(bp) = params.as_block_parameters_node() {
                if let Some(inner) = bp.parameters() {
                    let req_count = inner.requireds().iter().count();
                    if req_count >= 2 { return false; } // object bound
                }
                // Also check locals that shadow
            }
        }
        return true; // 0 or 1 param — object not bound
    }
    if let Some(_np) = block.as_numbered_parameters_node() {
        // Numbered params: check if _2 appears in block body source
        let loc = block.location();
        let block_src = source.get(loc.start_offset()..loc.end_offset()).unwrap_or("");
        // _2 alone means using the second numbered param (the object)
        return !contains_numbered_param_2_or_higher(block_src);
    }
    if block.as_it_parameters_node().is_some() {
        // Ruby 3.4 `it` block — `it` is first param, object not accessible
        return true;
    }
    false
}

fn contains_numbered_param_2_or_higher(src: &str) -> bool {
    // Check for _2, _3, ... patterns (as standalone identifiers)
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'_' {
            if let Some(&next) = bytes.get(i + 1) {
                if next >= b'2' && next <= b'9' {
                    // Check it's not part of a longer identifier
                    let before_ok = i == 0 || !bytes[i-1].is_ascii_alphanumeric() && bytes[i-1] != b'_';
                    let after_ok = bytes.get(i + 2).map(|&b| !b.is_ascii_alphanumeric() && b != b'_').unwrap_or(true);
                    if before_ok && after_ok { return true; }
                }
            }
        }
        i += 1;
    }
    false
}

crate::register_cop!("Lint/RedundantWithObject", |_cfg| Some(Box::new(RedundantWithObject::new())));
