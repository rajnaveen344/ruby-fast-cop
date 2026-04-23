//! Lint/NonDeterministicRequireOrder cop
//!
//! `Dir[...].each { |f| require f }` should sort first.
//! Ruby 3.0+ sorts automatically, so the cop is a no-op there.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct NonDeterministicRequireOrder;

impl NonDeterministicRequireOrder {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for NonDeterministicRequireOrder {
    fn name(&self) -> &'static str {
        "Lint/NonDeterministicRequireOrder"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        // `maximum_target_ruby_version 2.7` — Ruby 3.0+ sorts automatically
        if ctx.target_ruby_version >= 3.0 {
            return vec![];
        }

        let mut visitor = NdroVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct NdroVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

/// True if node is `Dir` constant (bare or cbase `::Dir`)
fn is_dir_const(node: &Node) -> bool {
    if let Some(cr) = node.as_constant_read_node() {
        return cr.name().as_slice() == b"Dir";
    }
    if let Some(cp) = node.as_constant_path_node() {
        // `::Dir` — parent is None, name is Dir
        if cp.parent().is_none() {
            if let Some(n) = cp.name() {
                return n.as_slice() == b"Dir";
            }
        }
    }
    false
}

/// `Dir.glob(...)` or `::Dir.glob(...)` — send receiver=Dir, method=glob
fn is_unsorted_dir_block(call: &ruby_prism::CallNode) -> bool {
    if call.name().as_slice() != b"glob" {
        return false;
    }
    match call.receiver() {
        Some(r) => is_dir_const(&r),
        None => false,
    }
}

/// `Dir[...].each` or `Dir.glob(...).each`
fn is_unsorted_dir_each(call: &ruby_prism::CallNode) -> bool {
    if call.name().as_slice() != b"each" {
        return false;
    }
    let recv = match call.receiver() {
        Some(r) => r,
        None => return false,
    };
    let inner = match recv.as_call_node() {
        Some(c) => c,
        None => return false,
    };
    let method = inner.name();
    let m = method.as_slice();
    if m != b"[]" && m != b"glob" {
        return false;
    }
    match inner.receiver() {
        Some(r) => is_dir_const(&r),
        None => false,
    }
}

fn is_unsorted_dir_loop(call: &ruby_prism::CallNode) -> bool {
    is_unsorted_dir_block(call) || is_unsorted_dir_each(call)
}

/// Check if block-arg node is `&method(:require)` / `&method(:require_relative)`
fn is_method_require_block_pass(node: &Node) -> bool {
    let bp = match node.as_block_argument_node() {
        Some(b) => b,
        None => return false,
    };
    let expr = match bp.expression() {
        Some(e) => e,
        None => return false,
    };
    let call = match expr.as_call_node() {
        Some(c) => c,
        None => return false,
    };
    if call.name().as_slice() != b"method" || call.receiver().is_some() {
        return false;
    }
    if let Some(args) = call.arguments() {
        for arg in args.arguments().iter() {
            if let Some(sym) = arg.as_symbol_node() {
                let s = sym.unescaped();
                let b: &[u8] = s.as_ref();
                if b == b"require" || b == b"require_relative" {
                    return true;
                }
            }
        }
    }
    false
}

/// Find block-pass on a call (if any). Block-pass lives on `call.block()` in Prism.
fn find_require_block_pass_arg<'a>(call: &'a ruby_prism::CallNode<'a>) -> Option<Node<'a>> {
    let blk = call.block()?;
    if is_method_require_block_pass(&blk) {
        Some(blk)
    } else {
        None
    }
}

/// Search body for `require <var>` / `require_relative <var>`.
/// If var_name is None, matches any require/require_relative call.
struct RequireFinder {
    var_name: Option<String>,
    found: bool,
}

impl Visit<'_> for RequireFinder {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if !self.found && node.receiver().is_none() {
            let m = node.name();
            let mb = m.as_slice();
            if mb == b"require" || mb == b"require_relative" {
                match &self.var_name {
                    None => {
                        self.found = true;
                        return;
                    }
                    Some(vname) => {
                        if let Some(args) = node.arguments() {
                            for arg in args.arguments().iter() {
                                if let Some(lv) = arg.as_local_variable_read_node() {
                                    if lv.name().as_slice() == vname.as_bytes() {
                                        self.found = true;
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

fn body_has_require(body: &Node, var_name: Option<&str>) -> bool {
    let mut f = RequireFinder { var_name: var_name.map(String::from), found: false };
    f.visit(body);
    f.found
}

/// Extract single loop variable from block parameters.
/// Returns Some(name) if block has exactly one positional param.
fn block_single_param_name(block: &ruby_prism::BlockNode) -> Option<String> {
    let params = block.parameters()?;
    let bp = params.as_block_parameters_node()?;
    let inner = bp.parameters()?;
    let reqs: Vec<_> = inner.requireds().iter().collect();
    if reqs.len() != 1 {
        return None;
    }
    let req = &reqs[0];
    let rpn = req.as_required_parameter_node()?;
    Some(String::from_utf8_lossy(rpn.name().as_slice()).to_string())
}

/// True if block has NumberedParametersNode (e.g. uses _1)
fn block_is_numblock(block: &ruby_prism::BlockNode) -> bool {
    block
        .parameters()
        .map(|p| p.as_numbered_parameters_node().is_some())
        .unwrap_or(false)
}

impl<'a> NdroVisitor<'a> {
    fn handle_call(&mut self, call: &ruby_prism::CallNode) {
        let src = self.ctx.source;

        // Pattern 1: call has block-pass `&method(:require)`
        if let Some(bp_node) = find_require_block_pass_arg(call) {
            if is_unsorted_dir_loop(call) {
                let call_start = call.location().start_offset();
                let call_end = call.location().end_offset();

                let correction = if call.name().as_slice() == b"glob" {
                    // Dir.glob(..., &method(:require)) → Dir.glob(...).sort.each(&method(:require))
                    let bp_src = &src[bp_node.location().start_offset()..bp_node.location().end_offset()];
                    let args: Vec<Node> = call.arguments().map_or_else(Vec::new, |a| a.arguments().iter().collect());
                    let prior_end = args.last().map(|a| a.location().end_offset())
                        .unwrap_or_else(|| call.message_loc().map(|m| m.end_offset()).unwrap_or(call_start));
                    let close_end = call.closing_loc().map(|l| l.end_offset()).unwrap_or(call_end);
                    let prefix = &src[call_start..prior_end];
                    let between = &src[prior_end..close_end];
                    let replacement = if between.contains('\n') {
                        format!("{}\n).sort.each({})", prefix, bp_src)
                    } else {
                        format!("{}).sort.each({})", prefix, bp_src)
                    };
                    Correction::replace(call_start, call_end, &replacement)
                } else {
                    // Dir[...].each(&method(:require)) → Dir[...].sort.each(&method(:require))
                    let recv = call.receiver().unwrap();
                    let recv_end = recv.location().end_offset();
                    Correction::replace(recv_end, recv_end, ".sort")
                };

                let offense = self.ctx.offense_with_range(
                    "Lint/NonDeterministicRequireOrder",
                    "Sort files before requiring them.",
                    Severity::Warning,
                    call_start,
                    call_end,
                );
                self.offenses.push(offense.with_correction(correction));
                return;
            }
        }

        // Pattern 2: call has block AND is unsorted_dir_loop AND body has require(var)
        let block = match call.block() {
            Some(b) => b,
            None => return,
        };
        let block_node = match block.as_block_node() {
            Some(b) => b,
            None => return,
        };
        if !is_unsorted_dir_loop(call) {
            return;
        }
        let body = match block_node.body() {
            Some(b) => b,
            None => return,
        };

        let has_match = if block_is_numblock(&block_node) {
            body_has_require(&body, Some("_1"))
        } else if let Some(name) = block_single_param_name(&block_node) {
            body_has_require(&body, Some(&name))
        } else if block_node.parameters().is_none() {
            // No block params: `Dir[...].each do ... require _1 end` — numblock parsing should catch _1,
            // but if user has no var, just require any require call (treat as bare).
            // Actually `each do ... require _1 end` parses as numblock too.
            body_has_require(&body, Some("_1"))
        } else {
            return;
        };

        if !has_match {
            return;
        }

        let call_start = call.location().start_offset();
        let call_end = call.location().end_offset();

        // Correction: replace call with either `<call>.sort.each` (for dir_block=Dir.glob do ...)
        // or `<receiver>.sort.each` (for dir_each=Dir[...].each do ...)
        let correction = if is_unsorted_dir_block(call) {
            let glob_close = call.closing_loc().map(|l| l.end_offset()).unwrap_or(call_end);
            Correction::replace(glob_close, glob_close, ".sort.each")
        } else {
            let recv = call.receiver().unwrap();
            let recv_end = recv.location().end_offset();
            Correction::replace(recv_end, recv_end, ".sort")
        };

        let offense_end = if is_unsorted_dir_block(call) {
            call.closing_loc().map(|l| l.end_offset()).unwrap_or(call_end)
        } else {
            call.message_loc().map(|m| m.end_offset()).unwrap_or(call_end)
        };

        let offense = self.ctx.offense_with_range(
            "Lint/NonDeterministicRequireOrder",
            "Sort files before requiring them.",
            Severity::Warning,
            call_start,
            offense_end,
        );
        self.offenses.push(offense.with_correction(correction));
    }
}

impl<'a> Visit<'_> for NdroVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.handle_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Lint/NonDeterministicRequireOrder", |_cfg| {
    Some(Box::new(NonDeterministicRequireOrder::new()))
});
