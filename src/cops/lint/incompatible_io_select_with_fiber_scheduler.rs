//! Lint/IncompatibleIoSelectWithFiberScheduler cop
//!
//! `IO.select([io], [], [], timeout)` → `io.wait_readable(timeout)`
//! `IO.select([], [io], [], timeout)` → `io.wait_writable(timeout)`

use crate::cops::{CheckContext, Cop};
use crate::helpers::node_match as m;
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct IncompatibleIoSelectWithFiberScheduler;

impl IncompatibleIoSelectWithFiberScheduler {
    pub fn new() -> Self {
        Self
    }
}

fn is_nil(node: &Node) -> bool {
    node.as_nil_node().is_some()
}

// Returns Some(Vec<Node>) if array node; None otherwise.
fn array_values<'a>(node: &Node<'a>) -> Option<Vec<Node<'a>>> {
    let a = node.as_array_node()?;
    Some(a.elements().iter().collect())
}

fn scheduler_compatible(io1: Option<&Node>, io2: Option<&Node>) -> bool {
    let io1 = match io1 { Some(n) => n, None => return false };
    let vals1 = match array_values(io1) { Some(v) => v, None => return false };
    if vals1.len() != 1 {
        return false;
    }
    match io2 {
        None => true,
        Some(n) => {
            if is_nil(n) {
                return true;
            }
            match array_values(n) {
                Some(v) => v.is_empty(),
                None => false,
            }
        }
    }
}

fn arg_src<'s>(node: &Node, src: &'s str) -> &'s str {
    let loc = node.location();
    &src[loc.start_offset()..loc.end_offset()]
}

struct V<'ctx, 's> {
    ctx: &'ctx CheckContext<'s>,
    offenses: Vec<Offense>,
    cop_name: &'static str,
    /// Whether currently directly inside an assignment RHS.
    in_assignment_rhs: bool,
}

impl<'pr, 'ctx, 's: 'pr> Visit<'pr> for V<'ctx, 's> {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'pr>) {
        let prev = self.in_assignment_rhs;
        self.in_assignment_rhs = true;
        let v = node.value();
        self.visit(&v);
        self.in_assignment_rhs = prev;
    }
    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode<'pr>) {
        let prev = self.in_assignment_rhs;
        self.in_assignment_rhs = true;
        let v = node.value();
        self.visit(&v);
        self.in_assignment_rhs = prev;
    }
    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode<'pr>) {
        let prev = self.in_assignment_rhs;
        self.in_assignment_rhs = true;
        let v = node.value();
        self.visit(&v);
        self.in_assignment_rhs = prev;
    }
    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode<'pr>) {
        let prev = self.in_assignment_rhs;
        self.in_assignment_rhs = true;
        let v = node.value();
        self.visit(&v);
        self.in_assignment_rhs = prev;
    }
    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode<'pr>) {
        let prev = self.in_assignment_rhs;
        self.in_assignment_rhs = true;
        let v = node.value();
        self.visit(&v);
        self.in_assignment_rhs = prev;
    }
    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode<'pr>) {
        let prev = self.in_assignment_rhs;
        self.in_assignment_rhs = true;
        let v = node.value();
        self.visit(&v);
        self.in_assignment_rhs = prev;
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        // Only check `IO.select(...)` top-level; descend into children for nesting.
        self.check(node);
        // Recurse
        if let Some(r) = node.receiver() {
            let prev = self.in_assignment_rhs;
            self.in_assignment_rhs = false;
            self.visit(&r);
            self.in_assignment_rhs = prev;
        }
        if let Some(args) = node.arguments() {
            let prev = self.in_assignment_rhs;
            self.in_assignment_rhs = false;
            for a in args.arguments().iter() {
                self.visit(&a);
            }
            self.in_assignment_rhs = prev;
        }
        if let Some(b) = node.block() {
            let prev = self.in_assignment_rhs;
            self.in_assignment_rhs = false;
            self.visit(&b);
            self.in_assignment_rhs = prev;
        }
    }
}

impl<'ctx, 's> V<'ctx, 's> {
    fn check<'pr>(&mut self, call: &ruby_prism::CallNode<'pr>) where 's: 'pr {
        if node_name!(call) != "select" {
            return;
        }
        let recv = match call.receiver() {
            Some(r) => r,
            None => return,
        };
        if !m::is_toplevel_constant_named(&recv, "IO") {
            return;
        }
        let args_node = match call.arguments() {
            Some(a) => a,
            None => return,
        };
        let args: Vec<Node> = args_node.arguments().iter().collect();
        if args.is_empty() {
            return;
        }
        let read = args.first();
        let write = args.get(1);
        let excepts = args.get(2);
        let timeout = args.get(3);
        // excepts present and non-empty array → skip
        if let Some(e) = excepts {
            if let Some(vals) = array_values(e) {
                if !vals.is_empty() {
                    return;
                }
            } else if !is_nil(e) {
                // non-nil non-array (e.g. variable) → skip (unsafe)
                return;
            }
        }
        let read_compat = scheduler_compatible(read, write);
        let write_compat = scheduler_compatible(write, read);
        if !read_compat && !write_compat {
            return;
        }

        let src = self.ctx.source;
        // Build preferred: either read's first value .wait_readable or write's.
        let timeout_arg = match timeout {
            None => "".to_string(),
            Some(t) => format!("({})", arg_src(t, src)),
        };
        let preferred = if read_compat {
            // read is array of 1
            let r = read.unwrap();
            let vals = array_values(r).unwrap();
            format!("{}.wait_readable{}", arg_src(&vals[0], src), timeout_arg)
        } else {
            let w = write.unwrap();
            let vals = array_values(w).unwrap();
            format!("{}.wait_writable{}", arg_src(&vals[0], src), timeout_arg)
        };

        let call_loc = call.location();
        let current_src = &src[call_loc.start_offset()..call_loc.end_offset()];
        let msg = format!("Use `{}` instead of `{}`.", preferred, current_src);

        let mut off = self.ctx.offense_with_range(
            self.cop_name,
            &msg,
            Severity::Warning,
            call_loc.start_offset(),
            call_loc.end_offset(),
        );
        if !self.in_assignment_rhs {
            off = off.with_correction(Correction::replace(
                call_loc.start_offset(),
                call_loc.end_offset(),
                preferred,
            ));
        }
        self.offenses.push(off);
    }
}

impl Cop for IncompatibleIoSelectWithFiberScheduler {
    fn name(&self) -> &'static str {
        "Lint/IncompatibleIoSelectWithFiberScheduler"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = V {
            ctx,
            offenses: vec![],
            cop_name: self.name(),
            in_assignment_rhs: false,
        };
        v.visit_program_node(node);
        v.offenses
    }
}

crate::register_cop!(
    "Lint/IncompatibleIoSelectWithFiberScheduler",
    |_cfg| Some(Box::new(IncompatibleIoSelectWithFiberScheduler::new()))
);
