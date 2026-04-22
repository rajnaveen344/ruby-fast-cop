//! Lint/NonDeterministicRequireOrder cop
//!
//! `Dir[...].each { |f| require f }` should sort first.
//! Ruby 3.0+ sorts automatically, so the cop is a no-op there.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

pub struct NonDeterministicRequireOrder;

impl Default for NonDeterministicRequireOrder {
    fn default() -> Self {
        Self
    }
}

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
        // Ruby 3.0+ sorts automatically — no offense
        if ctx.target_ruby_version >= 3.0 {
            return vec![];
        }

        let mut visitor = NdroVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct NdroVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

/// Check if a node is the `Dir` constant (or `::Dir`)
fn is_dir_const(node: &Node) -> bool {
    match node {
        Node::ConstantReadNode { .. } => {
            let cr = node.as_constant_read_node().unwrap();
            String::from_utf8_lossy(cr.name().as_slice()) == "Dir"
        }
        Node::ConstantPathNode { .. } => {
            let cp = node.as_constant_path_node().unwrap();
            if let Some(name) = cp.name() {
                String::from_utf8_lossy(name.as_slice()) == "Dir" && cp.parent().is_none()
            } else {
                false
            }
        }
        _ => false,
    }
}

/// True if the call is `Dir[...]` or `Dir.glob(...)`
fn is_dir_source(node: &ruby_prism::CallNode) -> bool {
    let method = String::from_utf8_lossy(node.name().as_slice()).to_string();
    if matches!(method.as_str(), "glob" | "[]") {
        if let Some(recv) = node.receiver() {
            return is_dir_const(&recv);
        }
    }
    false
}

/// True if call is `Dir[...].each` or `Dir.glob(...).each` (without sort)
fn is_unsorted_dir_each(node: &ruby_prism::CallNode) -> bool {
    let method = String::from_utf8_lossy(node.name().as_slice()).to_string();
    if method != "each" {
        return false;
    }
    if let Some(recv) = node.receiver() {
        if let Some(call) = recv.as_call_node() {
            // Must not already have `.sort` in between
            // i.e., receiver of `.each` is directly Dir[...] or Dir.glob(...)
            return is_dir_source(&call);
        }
    }
    false
}

/// Check if a block-pass argument is `&method(:require)` or `&method(:require_relative)`.
fn is_require_block_pass(node: &Node) -> bool {
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
    if String::from_utf8_lossy(call.name().as_slice()) != "method" {
        return false;
    }
    if let Some(args) = call.arguments() {
        for arg in args.arguments().iter() {
            if let Some(sym) = arg.as_symbol_node() {
                let sym_name = String::from_utf8_lossy(sym.unescaped().as_ref()).to_string();
                if matches!(sym_name.as_str(), "require" | "require_relative") {
                    return true;
                }
            }
        }
    }
    false
}

/// Check if body has a require call using the given variable name (or numbered param _1)
fn body_has_require(body_node: &Node, var_name: Option<&str>) -> bool {
    struct RequireFinder {
        var_name: Option<String>,
        found: bool,
    }
    impl Visit<'_> for RequireFinder {
        fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
            let m = String::from_utf8_lossy(node.name().as_slice()).to_string();
            if matches!(m.as_str(), "require" | "require_relative") && node.receiver().is_none() {
                if let Some(ref vname) = self.var_name {
                    if let Some(args) = node.arguments() {
                        for arg in args.arguments().iter() {
                            if let Some(lv) = arg.as_local_variable_read_node() {
                                if String::from_utf8_lossy(lv.name().as_slice()) == vname.as_str() {
                                    self.found = true;
                                    return;
                                }
                            }
                        }
                    }
                } else {
                    self.found = true;
                    return;
                }
            }
            ruby_prism::visit_call_node(self, node);
        }

        fn visit_numbered_reference_read_node(&mut self, node: &ruby_prism::NumberedReferenceReadNode) {
            // For numblock: `require _1` — _1 is parsed as a local var named `_1`
            ruby_prism::visit_numbered_reference_read_node(self, node);
        }
    }

    let mut f = RequireFinder { var_name: var_name.map(String::from), found: false };
    match body_node {
        Node::StatementsNode { .. } => {
            f.visit_statements_node(&body_node.as_statements_node().unwrap());
        }
        Node::BeginNode { .. } => {
            f.visit_begin_node(&body_node.as_begin_node().unwrap());
        }
        _ => {
            // try visiting as generic
        }
    }
    f.found
}

impl<'a> NdroVisitor<'a> {
    fn offense_and_correct_each(&mut self, call_node: &ruby_prism::CallNode) {
        // `Dir[...].each(...)` — insert `.sort` between receiver and `.each`
        let src = self.ctx.source;
        let start = call_node.location().start_offset();
        let end = call_node.location().end_offset();
        let call_src = &src[start..end];

        // Find ".each" in the call source
        // recv ends at receiver end offset
        let recv = call_node.receiver().unwrap();
        let recv_rel_end = recv.location().end_offset() - start;
        let new_call = format!("{}.sort{}", &call_src[..recv_rel_end], &call_src[recv_rel_end..]);

        let offense = self.ctx.offense_with_range(
            "Lint/NonDeterministicRequireOrder",
            "Sort files before requiring them.",
            Severity::Warning,
            start,
            end,
        );
        self.offenses.push(offense.with_correction(Correction::replace(start, end, &new_call)));
    }

    fn offense_and_correct_glob_block(
        &mut self,
        call_node: &ruby_prism::CallNode,
        block_start: usize,
        block_end: usize,
        block_src: &str,
    ) {
        let src = self.ctx.source;
        let call_start = call_node.location().start_offset();
        let call_end = call_node.location().end_offset();
        let call_src = &src[call_start..call_end];

        let new_src = format!("{}.sort.each {}", call_src, block_src);
        let offense = self.ctx.offense_with_range(
            "Lint/NonDeterministicRequireOrder",
            "Sort files before requiring them.",
            Severity::Warning,
            call_start,
            call_end,
        );
        self.offenses.push(offense.with_correction(Correction::replace(call_start, block_end, &new_src)));
    }

    fn offense_and_correct_glob_block_pass(
        &mut self,
        call_node: &ruby_prism::CallNode,
        bp_src: &str,
        overall_end: usize,
    ) {
        // `Dir.glob(..., &method(:require))` — remove bp from args, append `.sort.each(bp)`
        let src = self.ctx.source;
        let call_start = call_node.location().start_offset();

        // Find the last non-bp arg and the bp arg
        let args: Vec<Node> = call_node.arguments().map_or_else(Vec::new, |a| a.arguments().iter().collect());
        let bp_idx = args.iter().position(|a| is_require_block_pass(a)).unwrap();

        // Get source up to (not including) the comma before bp, or closing paren
        let replacement = if bp_idx == 0 {
            // No other args — e.g., `Dir.glob(path, &method(:require))` where args before bp...
            // Actually glob always has at least a pattern arg before bp
            // Just take source up to before comma preceding bp
            let bp_start = args[bp_idx].location().start_offset();
            // Walk backwards to find comma
            let pre_bp = &src[call_start..bp_start];
            let comma_pos = pre_bp.rfind(',').map(|p| call_start + p).unwrap_or(bp_start);
            let before_comma = &src[call_start..comma_pos];
            format!("{}).sort.each({})", before_comma, bp_src)
        } else {
            let last_non_bp_end = args[bp_idx - 1].location().end_offset();
            let before_bp = &src[call_start..last_non_bp_end];
            format!("{}).sort.each({})", before_bp, bp_src)
        };

        let offense = self.ctx.offense_with_range(
            "Lint/NonDeterministicRequireOrder",
            "Sort files before requiring them.",
            Severity::Warning,
            call_start,
            call_node.location().end_offset(),
        );
        self.offenses.push(offense.with_correction(Correction::replace(call_start, overall_end, &replacement)));
    }
}

impl<'a> Visit<'_> for NdroVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice()).to_string();

        // Pattern: `Dir[...].each(&method(:require))` or `Dir.glob(...).each(&method(:require))`
        if method == "each" && is_unsorted_dir_each(node) {
            if let Some(args) = node.arguments() {
                let arg_list: Vec<Node> = args.arguments().iter().collect();
                if let Some(bp_node) = arg_list.iter().find(|a| is_require_block_pass(a)) {
                    let bp_src = &self.ctx.source[bp_node.location().start_offset()..bp_node.location().end_offset()];
                    let bp_src = bp_src.to_string();
                    self.offense_and_correct_each(node);
                    ruby_prism::visit_call_node(self, node);
                    return;
                }
            }
        }

        // Pattern: `Dir.glob(..., &method(:require))` — block-pass as last arg of glob
        if method == "glob" && is_dir_source(node) {
            if let Some(args) = node.arguments() {
                let arg_list: Vec<Node> = args.arguments().iter().collect();
                if let Some(bp_node) = arg_list.iter().find(|a| is_require_block_pass(a)) {
                    let bp_start = bp_node.location().start_offset();
                    let bp_end = bp_node.location().end_offset();
                    let bp_src = self.ctx.source[bp_start..bp_end].to_string();
                    let overall_end = node.location().end_offset();
                    self.offense_and_correct_glob_block_pass(node, &bp_src, overall_end);
                    ruby_prism::visit_call_node(self, node);
                    return;
                }
            }
        }

        ruby_prism::visit_call_node(self, node);
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        // Get the call this block is attached to
        // We detect by examining the call node — visit_call_node already ran on the parent.
        // Instead, use a different approach: visit_call_node handles BEFORE the block is visited.
        // But in Prism's Visit trait, visit_call_node visits receiver, name, args, block in order.
        // So we need to intercept at call level.
        // Let's handle block patterns here by examining the "call" stored on the node.
        // Actually in Prism visit order: visit_call_node visits all children including the block.
        // So we handle blocks by checking if their parent call matches patterns.
        // We'll use a pending-call approach like constant_definition_in_block.
        // But that's complex. Instead, let's handle everything in visit_call_node:
        ruby_prism::visit_block_node(self, node);
    }
}

crate::register_cop!("Lint/NonDeterministicRequireOrder", |_cfg| {
    Some(Box::new(NonDeterministicRequireOrder::new()))
});
