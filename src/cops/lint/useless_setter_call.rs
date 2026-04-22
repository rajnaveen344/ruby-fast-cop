//! Lint/UselessSetterCall - Checks for setter call to local variable as final expression of a method.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use crate::node_name;
use ruby_prism::{Node, Visit};
use std::collections::HashMap;

#[derive(Default)]
pub struct UselessSetterCall;

impl UselessSetterCall {
    pub fn new() -> Self { Self }
}

impl Cop for UselessSetterCall {
    fn name(&self) -> &'static str { "Lint/UselessSetterCall" }
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

impl<'a> Visitor<'a> {
    fn check_def(&mut self, body: Option<Node>, source: &str) {
        let body = match body {
            Some(b) => b,
            None => return,
        };

        let last_expr = last_expression(&body);
        let last_expr = match last_expr {
            Some(e) => e,
            None => return,
        };

        // Check if last_expr is a setter call on a local variable
        let (var_name, var_name_start, var_name_end) = match extract_setter_receiver(&last_expr, source) {
            Some(x) => x,
            None => return,
        };

        // Track variable assignments in the body
        let mut tracker = MethodVariableTracker::new();
        tracker.scan(&body, source);

        if !tracker.contains_local_object(&var_name) {
            return;
        }

        let msg = format!("Useless setter call to local variable `{}`.", var_name);

        // The offense is on the receiver's name location
        let mut offense = self.ctx.offense_with_range(
            "Lint/UselessSetterCall",
            &msg,
            Severity::Warning,
            var_name_start,
            var_name_end,
        );

        // Correction: insert "\n{indent}{var_name}" after last_expr
        let last_end = last_expr.location().end_offset();
        let line_start = source[..last_expr.location().start_offset()].rfind('\n').map_or(0, |p| p + 1);
        let indent = &source[line_start..last_expr.location().start_offset()];
        let insertion = format!("\n{}{}", indent, var_name);
        offense.correction = Some(Correction::insert(last_end, insertion));

        self.offenses.push(offense);
    }
}

/// Get the last expression from a body node
fn last_expression<'a>(body: &Node<'a>) -> Option<Node<'a>> {
    match body {
        Node::BeginNode { .. } => {
            let begin = body.as_begin_node().unwrap();
            begin.statements().and_then(|stmts| {
                let items: Vec<_> = stmts.body().iter().collect();
                items.into_iter().last()
            })
        }
        Node::StatementsNode { .. } => {
            let stmts = body.as_statements_node().unwrap();
            let items: Vec<_> = stmts.body().iter().collect();
            items.into_iter().last()
        }
        _ => None,
    }
}

/// Check if node is a setter call on a local variable: `lvar.attr = val` or `lvar[k] = val`
/// Returns (var_name, name_start, name_end) if it is
fn extract_setter_receiver(node: &Node, source: &str) -> Option<(String, usize, usize)> {
    let call = node.as_call_node()?;

    let method = String::from_utf8_lossy(call.name().as_slice());
    let method_str = method.as_ref();

    // Must be a setter: method ends with `=` but not `==`
    let is_setter = (method_str.ends_with('=') && method_str != "==" && method_str != "!=")
        || method_str == "[]="
        || method_str == "[]=";

    if !is_setter {
        return None;
    }

    let receiver = call.receiver()?;

    // Receiver must be a local variable read
    let lvar = receiver.as_local_variable_read_node()?;

    let name = node_name!(lvar);
    let loc = lvar.location();
    let name_start = loc.start_offset();
    let name_end = loc.end_offset();

    // Verify the name makes sense
    let src_name = &source[name_start..name_end];
    if src_name != name.as_ref() {
        return None;
    }

    Some((name.into_owned(), name_start, name_end))
}

/// Tracks whether a local variable contains a locally-constructed object
struct MethodVariableTracker {
    local: HashMap<String, bool>,
}

impl MethodVariableTracker {
    fn new() -> Self {
        Self { local: HashMap::new() }
    }

    fn contains_local_object(&self, var_name: &str) -> bool {
        *self.local.get(var_name).unwrap_or(&false)
    }

    fn scan(&mut self, node: &Node, source: &str) {
        self.process_node(node, source);
    }

    fn process_node(&mut self, node: &Node, source: &str) {
        match node {
            // Local/instance/class/global variable assignment
            Node::LocalVariableWriteNode { .. } => {
                let n = node.as_local_variable_write_node().unwrap();
                let name = node_name!(n).into_owned();
                let value = n.value();
                let is_local = self.is_constructor(&value);
                self.local.insert(name, is_local);
            }
            Node::InstanceVariableWriteNode { .. } => {
                let n = node.as_instance_variable_write_node().unwrap();
                let name = String::from_utf8_lossy(n.name().as_slice()).into_owned();
                let value = n.value();
                let is_local = self.is_constructor(&value);
                self.local.insert(name, is_local);
            }
            Node::LocalVariableOperatorWriteNode { .. } => {
                // e.g. `x += something` — mark as non-local
                let n = node.as_local_variable_operator_write_node().unwrap();
                let name = node_name!(n).into_owned();
                self.local.insert(name, true);
                return; // skip children
            }
            Node::LocalVariableOrWriteNode { .. } => {
                // `x ||= something` — track rhs
                let n = node.as_local_variable_or_write_node().unwrap();
                let name = node_name!(n).into_owned();
                let value = n.value();
                let is_local = self.is_constructor(&value);
                // Logical or: could be from arg, so check rhs
                self.local.insert(name, is_local);
                return;
            }
            Node::LocalVariableAndWriteNode { .. } => {
                let n = node.as_local_variable_and_write_node().unwrap();
                let name = node_name!(n).into_owned();
                let value = n.value();
                let is_local = self.is_constructor(&value);
                self.local.insert(name, is_local);
                return;
            }
            Node::MultiWriteNode { .. } => {
                self.process_multi_write(node, source);
                return;
            }
            // For all other nodes, recurse into children
            _ => {}
        }

        // Recurse into children via visitor pattern - we need to walk the tree
        self.recurse_children(node, source);
    }

    fn process_multi_write(&mut self, node: &Node, source: &str) {
        let multi = node.as_multi_write_node().unwrap();
        let targets: Vec<_> = multi.lefts().iter().collect();
        let value = multi.value();

        // Check if rhs is an array literal
        let rhs_is_array = matches!(value, Node::ArrayNode { .. });
        let rhs_elements: Vec<Node> = if rhs_is_array {
            value.as_array_node().unwrap().elements().iter().collect()
        } else {
            Vec::new()
        };

        for (idx, target) in targets.iter().enumerate() {
            let name = match target {
                Node::LocalVariableTargetNode { .. } => {
                    let t = target.as_local_variable_target_node().unwrap();
                    node_name!(t).into_owned()
                }
                _ => continue,
            };

            let is_local = if rhs_is_array {
                if let Some(elem) = rhs_elements.get(idx) {
                    match elem {
                        Node::LocalVariableReadNode { .. }
                        | Node::InstanceVariableReadNode { .. }
                        | Node::GlobalVariableReadNode { .. }
                        | Node::ClassVariableReadNode { .. } => {
                            // variable reference — check what it holds
                            self.is_from_rhs_variable(elem)
                        }
                        _ => self.is_constructor(elem),
                    }
                } else {
                    true
                }
            } else {
                // RHS is not an array — all targets get marked as local (true)
                true
            };
            self.local.insert(name, is_local);
        }
        let _ = source;
    }

    fn is_from_rhs_variable(&self, node: &Node) -> bool {
        match node {
            Node::LocalVariableReadNode { .. } => {
                let lvar = node.as_local_variable_read_node().unwrap();
                let name = node_name!(lvar);
                *self.local.get(name.as_ref()).unwrap_or(&false)
            }
            _ => self.is_constructor(node),
        }
    }

    fn is_constructor(&self, node: &Node) -> bool {
        match node {
            // Literals are "local" objects
            Node::IntegerNode { .. }
            | Node::FloatNode { .. }
            | Node::StringNode { .. }
            | Node::InterpolatedStringNode { .. }
            | Node::SymbolNode { .. }
            | Node::NilNode { .. }
            | Node::TrueNode { .. }
            | Node::FalseNode { .. }
            | Node::ArrayNode { .. }
            | Node::HashNode { .. }
            | Node::RangeNode { .. } => true,
            // Variable read — check what the variable contains
            Node::LocalVariableReadNode { .. } => {
                let lvar = node.as_local_variable_read_node().unwrap();
                let name = node_name!(lvar);
                *self.local.get(name.as_ref()).unwrap_or(&false)
            }
            // Method call: only `Something.new` counts as local
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let method = node_name!(call);
                method.as_ref() == "new"
            }
            _ => false,
        }
    }

    fn recurse_children(&mut self, node: &Node, source: &str) {
        match node {
            Node::StatementsNode { .. } => {
                let stmts = node.as_statements_node().unwrap();
                for child in stmts.body().iter() {
                    self.process_node(&child, source);
                }
            }
            Node::BeginNode { .. } => {
                let begin = node.as_begin_node().unwrap();
                if let Some(stmts) = begin.statements() {
                    for child in stmts.body().iter() {
                        self.process_node(&child, source);
                    }
                }
            }
            _ => {
                // For most nodes we don't need to recurse - we only care about assignments
                // at the top level of the method body
            }
        }
    }
}

impl Visit<'_> for Visitor<'_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        self.check_def(node.body(), self.ctx.source);
        ruby_prism::visit_def_node(self, node);
    }
}

crate::register_cop!("Lint/UselessSetterCall", |_cfg| Some(Box::new(UselessSetterCall::new())));
