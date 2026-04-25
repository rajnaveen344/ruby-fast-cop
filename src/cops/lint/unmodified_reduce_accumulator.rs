//! Lint/UnmodifiedReduceAccumulator cop
//!
//! Translates RuboCop's UnmodifiedReduceAccumulator. For `reduce`/`inject`
//! blocks of arity ≥ 2, every return value should either reference the
//! accumulator or modify the element. Otherwise emit an offense.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

#[derive(Default)]
pub struct UnmodifiedReduceAccumulator;

impl UnmodifiedReduceAccumulator {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for UnmodifiedReduceAccumulator {
    fn name(&self) -> &'static str {
        "Lint/UnmodifiedReduceAccumulator"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

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

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_call_node(&mut self, call: &ruby_prism::CallNode) {
        self.handle_call(call);
        ruby_prism::visit_call_node(self, call);
    }
}

impl<'a> Visitor<'a> {
    fn handle_call(&mut self, call: &ruby_prism::CallNode) {
        let method = node_name!(call);
        if method != "reduce" && method != "inject" {
            return;
        }
        let Some(block_node) = call.block() else { return };
        let Node::BlockNode { .. } = &block_node else { return };
        let block = block_node.as_block_node().unwrap();

        // Determine acc/el names. Support BlockParametersNode and numbered params.
        let (acc_name, el_name) = match block_param_names(&block) {
            Some(p) => p,
            None => return,
        };

        let Some(body) = block.body() else { return };

        let return_values = collect_return_values(body, &block);
        if return_values.is_empty() {
            return;
        }

        let method_name = method.to_string();
        let msg_normal = format!(
            "Ensure the accumulator `{}` will be modified by `{}`.",
            acc_name, method_name
        );
        let msg_index = format!(
            "Do not return an element of the accumulator in `{}`.",
            method_name
        );

        // Index check - returns the FIRST matching, no message normal emitted.
        for rv in &return_values {
            if let Some((s, e)) = is_acc_index(rv, &acc_name, &el_name) {
                self.offenses.push(self.ctx.offense_with_range(
                    "Lint/UnmodifiedReduceAccumulator",
                    &msg_index,
                    Severity::Warning,
                    s,
                    e,
                ));
                return;
            }
        }

        // potential_offense?
        let body2 = block.body().unwrap();
        let elem_modified = body_modifies_element(&body2, &el_name);
        let acc_used = return_values.iter().any(|rv| rv_uses_lvar(rv, &acc_name));
        if elem_modified || acc_used {
            return;
        }

        for rv in &return_values {
            if !acceptable_return(rv, &el_name) {
                let l = rv.location();
                self.offenses.push(self.ctx.offense_with_range(
                    "Lint/UnmodifiedReduceAccumulator",
                    &msg_normal,
                    Severity::Warning,
                    l.start_offset(),
                    l.end_offset(),
                ));
            }
        }
    }
}

/// Extract (acc_name, el_name) from a block.
/// Supports BlockParametersNode (with required, possibly destructured)
/// and NumberedParametersNode.
fn block_param_names(block: &ruby_prism::BlockNode) -> Option<(String, String)> {
    let params_node = block.parameters()?;
    match &params_node {
        Node::BlockParametersNode { .. } => {
            let bp = params_node.as_block_parameters_node().unwrap();
            let pn = bp.parameters()?;
            // Flatten requireds (RuboCop's argument_list flattens destructured args).
            let mut flat: Vec<String> = Vec::new();
            for r in pn.requireds().iter() {
                flatten_required(&r, &mut flat);
                if flat.len() >= 2 {
                    break;
                }
            }
            if flat.len() < 2 {
                return None;
            }
            Some((flat[0].clone(), flat[1].clone()))
        }
        Node::NumberedParametersNode { .. } => {
            let np = params_node.as_numbered_parameters_node().unwrap();
            let max = np.maximum();
            if max < 2 {
                return None;
            }
            Some(("_1".to_string(), "_2".to_string()))
        }
        _ => None,
    }
}

fn flatten_required(node: &Node, out: &mut Vec<String>) {
    match node {
        Node::RequiredParameterNode { .. } => {
            let n = node.as_required_parameter_node().unwrap();
            out.push(String::from_utf8_lossy(n.name().as_slice()).to_string());
        }
        Node::MultiTargetNode { .. } => {
            let m = node.as_multi_target_node().unwrap();
            for left in m.lefts().iter() {
                flatten_required(&left, out);
                if out.len() >= 2 {
                    return;
                }
            }
            if let Some(rest) = m.rest() {
                flatten_required(&rest, out);
            }
            if out.len() >= 2 {
                return;
            }
            for right in m.rights().iter() {
                flatten_required(&right, out);
                if out.len() >= 2 {
                    return;
                }
            }
        }
        _ => {}
    }
}

/// Last statement of body + any next/break with arg not inside an inner block.
fn collect_return_values<'a>(body: Node<'a>, block: &ruby_prism::BlockNode<'a>) -> Vec<Node<'a>> {
    let mut out: Vec<Node<'a>> = Vec::new();
    match &body {
        Node::StatementsNode { .. } => {
            let s = body.as_statements_node().unwrap();
            if let Some(last) = s.body().iter().last() {
                out.push(last);
            }
        }
        _ => {
            out.push(body);
        }
    }
    let block_loc = block.location();
    let bs = block_loc.start_offset();
    let be = block_loc.end_offset();
    let mut f = NextBreakFinder { root_start: bs, root_end: be, block_depth: 0, out: Vec::new() };
    f.visit_block_node(block);
    out.extend(f.out);
    out
}

struct NextBreakFinder<'a> {
    root_start: usize,
    root_end: usize,
    block_depth: usize,
    out: Vec<Node<'a>>,
}

impl<'pr> Visit<'pr> for NextBreakFinder<'pr> {
    fn visit_block_node(&mut self, n: &ruby_prism::BlockNode<'pr>) {
        let l = n.location();
        if l.start_offset() == self.root_start && l.end_offset() == self.root_end {
            ruby_prism::visit_block_node(self, n);
            return;
        }
        self.block_depth += 1;
        ruby_prism::visit_block_node(self, n);
        self.block_depth -= 1;
    }
    fn visit_lambda_node(&mut self, n: &ruby_prism::LambdaNode<'pr>) {
        self.block_depth += 1;
        ruby_prism::visit_lambda_node(self, n);
        self.block_depth -= 1;
    }
    fn visit_next_node(&mut self, n: &ruby_prism::NextNode<'pr>) {
        if self.block_depth == 0 {
            if let Some(args) = n.arguments() {
                if let Some(first) = args.arguments().iter().next() {
                    self.out.push(first);
                }
            }
        }
        ruby_prism::visit_next_node(self, n);
    }
    fn visit_break_node(&mut self, n: &ruby_prism::BreakNode<'pr>) {
        if self.block_depth == 0 {
            if let Some(args) = n.arguments() {
                if let Some(first) = args.arguments().iter().next() {
                    self.out.push(first);
                }
            }
        }
        ruby_prism::visit_break_node(self, n);
    }
}

fn is_lvar(node: &Node, name: &str) -> bool {
    if let Node::LocalVariableReadNode { .. } = node {
        let n = node.as_local_variable_read_node().unwrap();
        return String::from_utf8_lossy(n.name().as_slice()) == name;
    }
    false
}

fn is_acc_index(node: &Node, acc: &str, el: &str) -> Option<(usize, usize)> {
    let Node::CallNode { .. } = node else { return None };
    let c = node.as_call_node().unwrap();
    let m = node_name!(c);
    if m != "[]" && m != "[]=" {
        return None;
    }
    let recv = c.receiver()?;
    if !is_lvar(&recv, acc) {
        return None;
    }
    if m == "[]=" {
        let l = node.location();
        return Some((l.start_offset(), l.end_offset()));
    }
    // m == "[]"
    let any_el = c
        .arguments()
        .map(|a| a.arguments().iter().any(|arg| is_lvar(&arg, el)))
        .unwrap_or(false);
    if !any_el {
        let l = node.location();
        return Some((l.start_offset(), l.end_offset()));
    }
    None
}

/// RuboCop `lvar_used?` — node-matcher (not search), checks if the WHOLE node
/// matches one of:
///   - (lvar %1)
///   - (lvasgn %1 ...)
///   - (send (lvar %1) :<< ...)
///   - (dstr (begin (lvar %1)))
///   - (op-asgn/and-asgn/or-asgn (lvasgn %1))   (shorthand assignments)
fn rv_uses_lvar(node: &Node, name: &str) -> bool {
    match node {
        Node::LocalVariableReadNode { .. } => is_lvar(node, name),
        Node::LocalVariableWriteNode { .. } => {
            let w = node.as_local_variable_write_node().unwrap();
            String::from_utf8_lossy(w.name().as_slice()) == name
        }
        Node::CallNode { .. } => {
            let c = node.as_call_node().unwrap();
            let m = node_name!(c);
            if m != "<<" {
                return false;
            }
            match c.receiver() {
                Some(r) => is_lvar(&r, name),
                None => false,
            }
        }
        Node::InterpolatedStringNode { .. } => {
            // (dstr (begin (lvar %1)))
            let s = node.as_interpolated_string_node().unwrap();
            let parts: Vec<_> = s.parts().iter().collect();
            if parts.len() != 1 {
                return false;
            }
            if let Node::EmbeddedStatementsNode { .. } = &parts[0] {
                let e = parts[0].as_embedded_statements_node().unwrap();
                if let Some(stmts) = e.statements() {
                    let body: Vec<_> = stmts.body().iter().collect();
                    if body.len() == 1 {
                        return is_lvar(&body[0], name);
                    }
                }
            }
            false
        }
        // Shorthand assignments would only match RuboCop's `(SHORTHAND (lvasgn %1))`
        // pattern with strict arity — op-asgn nodes always have ≥3 children, so the
        // pattern never fires in practice. Therefore: do NOT match shorthand here.
        _ => false,
    }
}

/// Search the body for any element-modification pattern.
/// RuboCop `element_modified?` patterns:
///   - (send _receiver !{:[] :[]=} <(lvar %1) `_ ...>)        # any send (not [] []=) with el in args + ≥1 other element
///   - (send (lvar %1) _message <{ivar gvar cvar lvar send} ...>)  # el.method(args containing a value)
///   - (lvasgn %1 _)                                           # el = ...
///   - (SHORTHAND_ASSIGNMENTS (lvasgn %1) ... _)               # el += ..., el ||= ..., el &&= ...
fn body_modifies_element(body: &Node, el: &str) -> bool {
    let mut hit = false;
    walk(body, &mut |n| {
        if hit {
            return;
        }
        if matches_element_modified(n, el) {
            hit = true;
        }
    });
    hit
}

fn matches_element_modified(n: &Node, el: &str) -> bool {
    match n {
        Node::LocalVariableWriteNode { .. } => {
            let w = n.as_local_variable_write_node().unwrap();
            String::from_utf8_lossy(w.name().as_slice()) == el
        }
        Node::LocalVariableOperatorWriteNode { .. } => {
            let w = n.as_local_variable_operator_write_node().unwrap();
            String::from_utf8_lossy(w.name().as_slice()) == el
        }
        Node::LocalVariableAndWriteNode { .. } => {
            let w = n.as_local_variable_and_write_node().unwrap();
            String::from_utf8_lossy(w.name().as_slice()) == el
        }
        Node::LocalVariableOrWriteNode { .. } => {
            let w = n.as_local_variable_or_write_node().unwrap();
            String::from_utf8_lossy(w.name().as_slice()) == el
        }
        Node::CallNode { .. } => {
            let c = n.as_call_node().unwrap();
            let m = node_name!(c);
            // Pattern 2: el as receiver (any method, args contain a "value" type).
            if let Some(recv) = c.receiver() {
                if is_lvar(&recv, el) {
                    let value_arg = c
                        .arguments()
                        .map(|a| a.arguments().iter().any(|arg| is_value_node(&arg)))
                        .unwrap_or(false);
                    if value_arg {
                        return true;
                    }
                }
            }
            // Pattern 1: any send (not [] []=) with el among args AND ≥1 other arg.
            if m == "[]" || m == "[]=" {
                return false;
            }
            let arg_count = c.arguments().map(|a| a.arguments().iter().count()).unwrap_or(0);
            if arg_count < 2 {
                return false;
            }
            let has_el = c
                .arguments()
                .map(|a| a.arguments().iter().any(|arg| is_lvar(&arg, el)))
                .unwrap_or(false);
            has_el
        }
        _ => false,
    }
}

/// Loose check for "value-bearing" nodes (ivar/gvar/cvar/lvar/send).
fn is_value_node(n: &Node) -> bool {
    matches!(
        n,
        Node::LocalVariableReadNode { .. }
            | Node::InstanceVariableReadNode { .. }
            | Node::ClassVariableReadNode { .. }
            | Node::GlobalVariableReadNode { .. }
            | Node::CallNode { .. }
    )
}

/// `acceptable_return?(node, el)` — vars from expression_values of node;
///   if vars empty → true
///   if (vars - [el]).any → true
///   else false
fn acceptable_return(node: &Node, el: &str) -> bool {
    let vars = expression_values(node);
    if vars.is_empty() {
        return true;
    }
    vars.iter().any(|v| v != el)
}

/// RuboCop `expression_values` is a node-search that yields one of:
///   - VARIABLES → name (lvar/ivar/cvar/gvar/nth_ref/back_ref)
///   - EQUALS_ASSIGNMENTS → name (lvasgn/ivasgn/cvasgn/gvasgn/casgn)
///   - (send (VARIABLES $_) :<< ...) → name of receiver
///   - (send _ _) → opaque token (0-arg send)
///   - (dstr (begin (VARIABLES $_))) → inner var name
///   - (SHORTHAND (EQUALS $_)) → name
fn expression_values(node: &Node) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    walk(node, &mut |n| {
        match n {
            Node::LocalVariableReadNode { .. } => {
                let r = n.as_local_variable_read_node().unwrap();
                out.push(String::from_utf8_lossy(r.name().as_slice()).to_string());
            }
            Node::InstanceVariableReadNode { .. } => {
                let r = n.as_instance_variable_read_node().unwrap();
                out.push(String::from_utf8_lossy(r.name().as_slice()).to_string());
            }
            Node::ClassVariableReadNode { .. } => {
                let r = n.as_class_variable_read_node().unwrap();
                out.push(String::from_utf8_lossy(r.name().as_slice()).to_string());
            }
            Node::GlobalVariableReadNode { .. } => {
                let r = n.as_global_variable_read_node().unwrap();
                out.push(String::from_utf8_lossy(r.name().as_slice()).to_string());
            }
            Node::LocalVariableWriteNode { .. } => {
                let w = n.as_local_variable_write_node().unwrap();
                out.push(String::from_utf8_lossy(w.name().as_slice()).to_string());
            }
            Node::InstanceVariableWriteNode { .. } => {
                let w = n.as_instance_variable_write_node().unwrap();
                out.push(String::from_utf8_lossy(w.name().as_slice()).to_string());
            }
            Node::ClassVariableWriteNode { .. } => {
                let w = n.as_class_variable_write_node().unwrap();
                out.push(String::from_utf8_lossy(w.name().as_slice()).to_string());
            }
            Node::GlobalVariableWriteNode { .. } => {
                let w = n.as_global_variable_write_node().unwrap();
                out.push(String::from_utf8_lossy(w.name().as_slice()).to_string());
            }
            Node::ConstantWriteNode { .. } => {
                let w = n.as_constant_write_node().unwrap();
                out.push(String::from_utf8_lossy(w.name().as_slice()).to_string());
            }
            Node::LocalVariableOperatorWriteNode { .. } => {
                let w = n.as_local_variable_operator_write_node().unwrap();
                out.push(String::from_utf8_lossy(w.name().as_slice()).to_string());
            }
            Node::LocalVariableAndWriteNode { .. } => {
                let w = n.as_local_variable_and_write_node().unwrap();
                out.push(String::from_utf8_lossy(w.name().as_slice()).to_string());
            }
            Node::LocalVariableOrWriteNode { .. } => {
                let w = n.as_local_variable_or_write_node().unwrap();
                out.push(String::from_utf8_lossy(w.name().as_slice()).to_string());
            }
            Node::CallNode { .. } => {
                let c = n.as_call_node().unwrap();
                // (send _ _) — 0-arg, no block.
                let arg_count = c.arguments().map(|a| a.arguments().iter().count()).unwrap_or(0);
                if arg_count == 0 && c.block().is_none() {
                    let l = n.location();
                    out.push(format!("@send@{}", l.start_offset()));
                }
            }
            _ => {}
        }
    });
    let mut seen: HashSet<String> = HashSet::new();
    out.retain(|v| seen.insert(v.clone()));
    out
}

/// Hand-recursive walk over Node. Calls `cb` on each visited node (including root).
fn walk<'a, F: FnMut(&Node<'a>)>(node: &Node<'a>, cb: &mut F) {
    cb(node);
    match node {
        Node::ProgramNode { .. } => {
            let n = node.as_program_node().unwrap();
            for s in n.statements().body().iter() {
                walk(&s, cb);
            }
        }
        Node::StatementsNode { .. } => {
            let n = node.as_statements_node().unwrap();
            for s in n.body().iter() {
                walk(&s, cb);
            }
        }
        Node::CallNode { .. } => {
            let n = node.as_call_node().unwrap();
            if let Some(r) = n.receiver() {
                walk(&r, cb);
            }
            if let Some(args) = n.arguments() {
                for a in args.arguments().iter() {
                    walk(&a, cb);
                }
            }
            if let Some(b) = n.block() {
                walk(&b, cb);
            }
        }
        Node::BlockNode { .. } => {
            let n = node.as_block_node().unwrap();
            if let Some(p) = n.parameters() {
                walk(&p, cb);
            }
            if let Some(b) = n.body() {
                walk(&b, cb);
            }
        }
        Node::ArrayNode { .. } => {
            let n = node.as_array_node().unwrap();
            for e in n.elements().iter() {
                walk(&e, cb);
            }
        }
        Node::HashNode { .. } => {
            let n = node.as_hash_node().unwrap();
            for e in n.elements().iter() {
                walk(&e, cb);
            }
        }
        Node::AssocNode { .. } => {
            let n = node.as_assoc_node().unwrap();
            walk(&n.key(), cb);
            walk(&n.value(), cb);
        }
        Node::IfNode { .. } => {
            let n = node.as_if_node().unwrap();
            walk(&n.predicate(), cb);
            if let Some(s) = n.statements() {
                for st in s.body().iter() {
                    walk(&st, cb);
                }
            }
            if let Some(e) = n.subsequent() {
                walk(&e, cb);
            }
        }
        Node::UnlessNode { .. } => {
            let n = node.as_unless_node().unwrap();
            walk(&n.predicate(), cb);
            if let Some(s) = n.statements() {
                for st in s.body().iter() {
                    walk(&st, cb);
                }
            }
            if let Some(e) = n.else_clause() {
                if let Some(s) = e.statements() {
                    for st in s.body().iter() {
                        walk(&st, cb);
                    }
                }
            }
        }
        Node::AndNode { .. } => {
            let n = node.as_and_node().unwrap();
            walk(&n.left(), cb);
            walk(&n.right(), cb);
        }
        Node::OrNode { .. } => {
            let n = node.as_or_node().unwrap();
            walk(&n.left(), cb);
            walk(&n.right(), cb);
        }
        Node::ParenthesesNode { .. } => {
            let n = node.as_parentheses_node().unwrap();
            if let Some(b) = n.body() {
                walk(&b, cb);
            }
        }
        Node::LocalVariableWriteNode { .. } => {
            let n = node.as_local_variable_write_node().unwrap();
            walk(&n.value(), cb);
        }
        Node::LocalVariableOperatorWriteNode { .. } => {
            let n = node.as_local_variable_operator_write_node().unwrap();
            walk(&n.value(), cb);
        }
        Node::LocalVariableOrWriteNode { .. } => {
            let n = node.as_local_variable_or_write_node().unwrap();
            walk(&n.value(), cb);
        }
        Node::LocalVariableAndWriteNode { .. } => {
            let n = node.as_local_variable_and_write_node().unwrap();
            walk(&n.value(), cb);
        }
        Node::InstanceVariableWriteNode { .. } => {
            let n = node.as_instance_variable_write_node().unwrap();
            walk(&n.value(), cb);
        }
        Node::ClassVariableWriteNode { .. } => {
            let n = node.as_class_variable_write_node().unwrap();
            walk(&n.value(), cb);
        }
        Node::GlobalVariableWriteNode { .. } => {
            let n = node.as_global_variable_write_node().unwrap();
            walk(&n.value(), cb);
        }
        Node::ConstantWriteNode { .. } => {
            let n = node.as_constant_write_node().unwrap();
            walk(&n.value(), cb);
        }
        Node::InterpolatedStringNode { .. } => {
            let n = node.as_interpolated_string_node().unwrap();
            for p in n.parts().iter() {
                walk(&p, cb);
            }
        }
        Node::EmbeddedStatementsNode { .. } => {
            let n = node.as_embedded_statements_node().unwrap();
            if let Some(s) = n.statements() {
                for st in s.body().iter() {
                    walk(&st, cb);
                }
            }
        }
        Node::ReturnNode { .. } => {
            let n = node.as_return_node().unwrap();
            if let Some(args) = n.arguments() {
                for a in args.arguments().iter() {
                    walk(&a, cb);
                }
            }
        }
        Node::NextNode { .. } => {
            let n = node.as_next_node().unwrap();
            if let Some(args) = n.arguments() {
                for a in args.arguments().iter() {
                    walk(&a, cb);
                }
            }
        }
        Node::BreakNode { .. } => {
            let n = node.as_break_node().unwrap();
            if let Some(args) = n.arguments() {
                for a in args.arguments().iter() {
                    walk(&a, cb);
                }
            }
        }
        Node::SplatNode { .. } => {
            let n = node.as_splat_node().unwrap();
            if let Some(e) = n.expression() {
                walk(&e, cb);
            }
        }
        _ => {}
    }
}

crate::register_cop!("Lint/UnmodifiedReduceAccumulator", |_cfg| {
    Some(Box::new(UnmodifiedReduceAccumulator::new()))
});
