//! Style/FileOpen cop
//!
//! `File.open` without a block may leak a file descriptor. Flag when the return
//! value is unused, assigned to a local variable, or used as the receiver of a
//! chained call.

use crate::cops::{CheckContext, Cop};
use crate::helpers::node_match as m;
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "`File.open` without a block may leak a file descriptor; use the block form.";

#[derive(Default)]
pub struct FileOpen;

impl FileOpen {
    pub fn new() -> Self {
        Self
    }
}

fn is_file_open(call: &ruby_prism::CallNode) -> bool {
    if node_name!(call) != "open" {
        return false;
    }
    let recv = match call.receiver() {
        Some(r) => r,
        None => return false,
    };
    m::is_toplevel_constant_named(&recv, "File")
}

fn has_block_arg(call: &ruby_prism::CallNode) -> bool {
    // Matches `File.open(..., &x)` — block_argument_node passed as `&` arg.
    if let Some(args) = call.arguments() {
        for a in args.arguments().iter() {
            if a.as_block_argument_node().is_some() {
                return true;
            }
        }
    }
    // Or via call.block() returning BlockArgumentNode.
    if let Some(b) = call.block() {
        if b.as_block_argument_node().is_some() {
            return true;
        }
    }
    false
}

fn has_block(call: &ruby_prism::CallNode) -> bool {
    call.block().map(|b| b.as_block_node().is_some()).unwrap_or(false)
}

struct V<'pr, 'ctx, 's> {
    ctx: &'ctx CheckContext<'s>,
    offenses: Vec<Offense>,
    cop: &'pr FileOpen,
    // Parent chain for each call we descend into — store enough info to decide
    // "is node the receiver of parent call" and "is parent an assignment lhs".
    /// Set when currently inside the receiver slot of the parent call.
    parent_receiver_chain: bool,
    /// Set when currently inside the rhs of a local_variable_write.
    parent_is_lvasgn: bool,
    /// Set when the current expression is a standalone statement.
    standalone: bool,
}

impl<'pr, 'ctx, 's> Visit<'pr> for V<'pr, 'ctx, 's>
where
    's: 'pr,
{
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        if is_file_open(node) && !has_block(node) && !has_block_arg(node) {
            // Determine if offensive: always flag if not "value_used" (top-level
            // statement) OR if parent is lvasgn OR if node is receiver of parent
            // call. We model "value_used" by tracking whether we're inside an
            // expression context. Simpler rule matching fixtures: flag if
            // parent_is_lvasgn, parent_receiver_chain, OR neither lvasgn-rhs
            // nor inside-other-expr (standalone statement).
            //
            // Parent tracking below handles the 3 cases. A separate standalone
            // flag tracks "no expression parent seen".
            let flag = self.parent_is_lvasgn || self.parent_receiver_chain || self.standalone;
            if flag {
                let loc = node.location();
                self.offenses.push(self.ctx.offense_with_range(
                    self.cop.name(),
                    MSG,
                    Severity::Convention,
                    loc.start_offset(),
                    loc.end_offset(),
                ));
            }
        }

        // Recurse into receiver slot with parent_receiver_chain flag.
        // Receiver of THIS call → its parent is THIS call, and it's receiver.
        if let Some(recv) = node.receiver() {
            let prev_r = self.parent_receiver_chain;
            let prev_a = self.parent_is_lvasgn;
            let prev_s = self.standalone;
            self.parent_receiver_chain = true;
            self.parent_is_lvasgn = false;
            self.standalone = false;
            self.visit(&recv);
            self.parent_receiver_chain = prev_r;
            self.parent_is_lvasgn = prev_a;
            self.standalone = prev_s;
        }
        // Arguments & block: NOT receiver.
        if let Some(args) = node.arguments() {
            let prev_r = self.parent_receiver_chain;
            let prev_a = self.parent_is_lvasgn;
            let prev_s = self.standalone;
            self.parent_receiver_chain = false;
            self.parent_is_lvasgn = false;
            self.standalone = false;
            for a in args.arguments().iter() {
                self.visit(&a);
            }
            self.parent_receiver_chain = prev_r;
            self.parent_is_lvasgn = prev_a;
            self.standalone = prev_s;
        }
        if let Some(b) = node.block() {
            let prev_r = self.parent_receiver_chain;
            let prev_a = self.parent_is_lvasgn;
            let prev_s = self.standalone;
            self.parent_receiver_chain = false;
            self.parent_is_lvasgn = false;
            self.standalone = false;
            self.visit(&b);
            self.parent_receiver_chain = prev_r;
            self.parent_is_lvasgn = prev_a;
            self.standalone = prev_s;
        }
        // Don't call default visit (already visited children).
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'pr>) {
        let prev_a = self.parent_is_lvasgn;
        let prev_r = self.parent_receiver_chain;
        let prev_s = self.standalone;
        self.parent_is_lvasgn = true;
        self.parent_receiver_chain = false;
        self.standalone = false;
        let v = node.value();
        self.visit(&v);
        self.parent_is_lvasgn = prev_a;
        self.parent_receiver_chain = prev_r;
        self.standalone = prev_s;
    }

    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode<'pr>) {
        let prev_a = self.parent_is_lvasgn;
        self.parent_is_lvasgn = true;
        let v = node.value();
        self.visit(&v);
        self.parent_is_lvasgn = prev_a;
    }

    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
        let prev_a = self.parent_is_lvasgn;
        self.parent_is_lvasgn = true;
        let v = node.value();
        self.visit(&v);
        self.parent_is_lvasgn = prev_a;
    }

    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode<'pr>) {
        let prev_a = self.parent_is_lvasgn;
        self.parent_is_lvasgn = true;
        let v = node.value();
        self.visit(&v);
        self.parent_is_lvasgn = prev_a;
    }

    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode<'pr>) {
        // Each direct statement → standalone context (value not used).
        for s in node.body().iter() {
            let prev_s = self.standalone;
            let prev_r = self.parent_receiver_chain;
            let prev_a = self.parent_is_lvasgn;
            self.standalone = true;
            self.parent_receiver_chain = false;
            self.parent_is_lvasgn = false;
            self.visit(&s);
            self.standalone = prev_s;
            self.parent_receiver_chain = prev_r;
            self.parent_is_lvasgn = prev_a;
        }
    }
}

impl<'pr, 'ctx, 's> V<'pr, 'ctx, 's> {
    fn new(cop: &'pr FileOpen, ctx: &'ctx CheckContext<'s>) -> Self {
        Self {
            ctx,
            offenses: vec![],
            cop,
            parent_receiver_chain: false,
            parent_is_lvasgn: false,
            standalone: false,
        }
    }
}

impl Cop for FileOpen {
    fn name(&self) -> &'static str {
        "Style/FileOpen"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = V::new(self, ctx);
        v.visit_program_node(node);
        v.offenses
    }
}

crate::register_cop!("Style/FileOpen", |_cfg| Some(Box::new(FileOpen::new())));
