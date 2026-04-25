//! Security/CompoundHash — flag manual hash-value combinators.
//! Ports `RuboCop::Cop::Security::CompoundHash`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct CompoundHash;

impl CompoundHash {
    pub fn new() -> Self { Self }
}

const COMBINATOR_MSG: &str = "Use `[...].hash` instead of combining hash values manually.";
const MONUPLE_MSG: &str =
    "Delegate hash directly without wrapping in an array when only using a single value.";
const REDUNDANT_MSG: &str = "Calling .hash on elements of a hashed array is redundant.";

impl Cop for CompoundHash {
    fn name(&self) -> &'static str { "Security/CompoundHash" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut v = V {
            ctx,
            in_hash_def: 0,
            combinator_depth: 0,
            stack: Vec::new(),
            out: Vec::new(),
        };
        v.visit(&result.node());
        v.out
    }
}

/// Track each ancestor as a marker so redundant_hash can look two levels up.
#[derive(Clone, Copy, PartialEq)]
enum Marker {
    Array,
    HashCall, // a call whose method name is `hash` (no args)
    Other,
}

struct V<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    in_hash_def: usize,
    combinator_depth: usize,
    stack: Vec<Marker>,
    out: Vec<Offense>,
}

fn is_combinator_op(name: &str) -> bool {
    matches!(name, "^" | "+" | "*" | "|")
}

/// Check if the static def is `def hash` with no args.
fn is_static_hash_def(node: &ruby_prism::DefNode) -> bool {
    let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
    if name != "hash" { return false; }
    // No parameters
    match node.parameters() {
        Some(p) => {
            // ParametersNode — check all empty
            p.requireds().iter().count() == 0
                && p.optionals().iter().count() == 0
                && p.rest().is_none()
                && p.posts().iter().count() == 0
                && p.keywords().iter().count() == 0
                && p.keyword_rest().is_none()
                && p.block().is_none()
        }
        None => true,
    }
}

/// Block calling `define_method(:hash)` or `define_singleton_method(:hash)` with empty args.
fn is_dynamic_hash_block(block: &ruby_prism::BlockNode) -> bool {
    // Block params must be empty.
    if let Some(p) = block.parameters() {
        // BlockParametersNode → check inner ParametersNode is None or all-empty
        if let Some(bp) = p.as_block_parameters_node() {
            if let Some(inner) = bp.parameters() {
                let any = inner.requireds().iter().count() > 0
                    || inner.optionals().iter().count() > 0
                    || inner.rest().is_some()
                    || inner.posts().iter().count() > 0
                    || inner.keywords().iter().count() > 0
                    || inner.keyword_rest().is_some()
                    || inner.block().is_some();
                if any { return false; }
            }
            // Locals (`|; x|`) shouldn't appear for define_method(:hash) usage; ignore.
        } else {
            return false;
        }
    }
    true
}

impl<'a, 'b> V<'a, 'b> {
    fn parent(&self) -> Option<Marker> {
        self.stack.last().copied()
    }
    fn grandparent(&self) -> Option<Marker> {
        let n = self.stack.len();
        if n < 2 { None } else { Some(self.stack[n - 2]) }
    }

    fn flag(&mut self, msg: &'static str, start: usize, end: usize) {
        self.out.push(self.ctx.offense_with_range(
            "Security/CompoundHash",
            msg,
            Severity::Warning,
            start,
            end,
        ));
    }
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let is_hash_def = is_static_hash_def(node);
        if is_hash_def { self.in_hash_def += 1; }
        self.stack.push(Marker::Other);
        ruby_prism::visit_def_node(self, node);
        self.stack.pop();
        if is_hash_def { self.in_hash_def -= 1; }
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        // Dynamic hash method? Look at our parent — it should be a CallNode `define_method`/`define_singleton_method`
        // with first arg `:hash`. Easier: when we encounter a CallNode that fits, mark its block child.
        // We'll do that in visit_call_node by pushing in_hash_def around the block traversal.
        self.stack.push(Marker::Other);
        ruby_prism::visit_block_node(self, node);
        self.stack.pop();
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();

        // Is this a `define_method(:hash) do ... end` or `define_singleton_method(:hash) do ... end`?
        let mut entered_dynamic = false;
        if (name == "define_method" || name == "define_singleton_method") && node.block().is_some() {
            // Check first arg is symbol :hash, no other args.
            if let Some(args) = node.arguments() {
                let arglist: Vec<_> = args.arguments().iter().collect();
                if arglist.len() == 1 {
                    if let Some(sym) = arglist[0].as_symbol_node() {
                        if let Some(val) = sym.value_loc() {
                            if val.as_slice() == b"hash" {
                                if let Some(blk) = node.block() {
                                    if let Some(b) = blk.as_block_node() {
                                        if is_dynamic_hash_block(&b) {
                                            entered_dynamic = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if entered_dynamic { self.in_hash_def += 1; }

        // ----- Combinator (^, +, *, |) -----
        let is_send_combinator = is_combinator_op(&name) && node.receiver().is_some();
        let mut entered_combinator = false;
        if is_send_combinator {
            if self.combinator_depth == 0 && self.in_hash_def > 0 {
                let l = node.location();
                self.flag(COMBINATOR_MSG, l.start_offset(), l.end_offset());
            }
            entered_combinator = true;
            self.combinator_depth += 1;
        }

        // ----- Monuple: `(array).hash` with exactly one element, no args -----
        if name == "hash" && node.arguments().is_none() && node.block().is_none() {
            if let Some(recv) = node.receiver() {
                if let Some(arr) = recv.as_array_node() {
                    let elems: Vec<_> = arr.elements().iter().collect();
                    if elems.len() == 1 {
                        // Avoid matching splat-only arrays.
                        if !matches!(&elems[0], Node::SplatNode { .. }) {
                            let l = node.location();
                            self.flag(MONUPLE_MSG, l.start_offset(), l.end_offset());
                        }
                    }
                }
            }
        }

        // ----- Redundant: this is `.hash` whose parent is ArrayNode whose parent is `.hash` -----
        if name == "hash" && node.arguments().is_none() && node.block().is_none() {
            if matches!(self.parent(), Some(Marker::Array))
                && matches!(self.grandparent(), Some(Marker::HashCall))
            {
                let l = node.location();
                self.flag(REDUNDANT_MSG, l.start_offset(), l.end_offset());
            }
        }

        // Determine marker for stack.
        let marker = if name == "hash" && node.arguments().is_none() && node.block().is_none() {
            // Only treat as HashCall if its receiver is an array (for redundant pattern).
            if let Some(recv) = node.receiver() {
                if recv.as_array_node().is_some() {
                    Marker::HashCall
                } else {
                    Marker::Other
                }
            } else {
                Marker::Other
            }
        } else {
            Marker::Other
        };
        self.stack.push(marker);
        ruby_prism::visit_call_node(self, node);
        self.stack.pop();

        if entered_combinator { self.combinator_depth -= 1; }
        if entered_dynamic { self.in_hash_def -= 1; }
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        self.stack.push(Marker::Array);
        ruby_prism::visit_array_node(self, node);
        self.stack.pop();
    }

    // Operator-write nodes — count as combinators when op is in {^, +, *, |}.
    fn visit_local_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableOperatorWriteNode,
    ) {
        self.handle_op_write(
            String::from_utf8_lossy(node.binary_operator().as_slice()).to_string(),
            node.location().start_offset(),
            node.location().end_offset(),
            |this| ruby_prism::visit_local_variable_operator_write_node(this, node),
        );
    }

    fn visit_instance_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::InstanceVariableOperatorWriteNode,
    ) {
        self.handle_op_write(
            String::from_utf8_lossy(node.binary_operator().as_slice()).to_string(),
            node.location().start_offset(),
            node.location().end_offset(),
            |this| ruby_prism::visit_instance_variable_operator_write_node(this, node),
        );
    }

    fn visit_class_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::ClassVariableOperatorWriteNode,
    ) {
        self.handle_op_write(
            String::from_utf8_lossy(node.binary_operator().as_slice()).to_string(),
            node.location().start_offset(),
            node.location().end_offset(),
            |this| ruby_prism::visit_class_variable_operator_write_node(this, node),
        );
    }

    fn visit_global_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::GlobalVariableOperatorWriteNode,
    ) {
        self.handle_op_write(
            String::from_utf8_lossy(node.binary_operator().as_slice()).to_string(),
            node.location().start_offset(),
            node.location().end_offset(),
            |this| ruby_prism::visit_global_variable_operator_write_node(this, node),
        );
    }
}

impl<'a, 'b> V<'a, 'b> {
    fn handle_op_write<F: FnOnce(&mut Self)>(
        &mut self,
        op: String,
        s: usize,
        e: usize,
        recurse: F,
    ) {
        let is_comb = is_combinator_op(&op);
        let mut entered = false;
        if is_comb {
            if self.combinator_depth == 0 && self.in_hash_def > 0 {
                self.flag(COMBINATOR_MSG, s, e);
            }
            entered = true;
            self.combinator_depth += 1;
        }
        self.stack.push(Marker::Other);
        recurse(self);
        self.stack.pop();
        if entered { self.combinator_depth -= 1; }
    }
}

crate::register_cop!("Security/CompoundHash", |_cfg| Some(Box::new(CompoundHash::new())));
