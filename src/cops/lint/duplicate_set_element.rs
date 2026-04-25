//! Lint/DuplicateSetElement cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/duplicate_set_element.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use crate::node_name;
use ruby_prism::{CallNode, Node, Visit};

#[derive(Default)]
pub struct DuplicateSetElement;

impl DuplicateSetElement {
    pub fn new() -> Self { Self }
}

impl Cop for DuplicateSetElement {
    fn name(&self) -> &'static str { "Lint/DuplicateSetElement" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = SetVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct SetVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

/// Returns Some((class_name, elements)) if `node` matches:
///   Set[...] / SortedSet[...]
///   Set.new([...]) / SortedSet.new([...])
///   [...].to_set / [...]&.to_set
fn match_set_init<'a>(node: &CallNode<'a>) -> Option<(String, Vec<Node<'a>>)> {
    let method = node_name!(node);

    // [array].to_set / [array]&.to_set
    if method == "to_set" {
        if let Some(recv) = node.receiver() {
            if let Some(arr) = recv.as_array_node() {
                let elems: Vec<Node> = arr.elements().iter().collect();
                return Some(("Set".to_string(), elems));
            }
        }
        return None;
    }

    // Receiver must be a Set/SortedSet const (possibly cbase ::Set)
    let recv = node.receiver()?;
    let class_name = constant_set_name(&recv)?;

    if method == "[]" {
        // Args are direct
        let args: Vec<Node> = node.arguments()
            .map(|a| a.arguments().iter().collect())
            .unwrap_or_default();
        return Some((class_name, args));
    }

    if method == "new" {
        // Single arg should be ArrayNode
        let args = node.arguments()?;
        let mut iter = args.arguments().iter();
        let first = iter.next()?;
        if iter.next().is_some() { return None; }
        if let Some(arr) = first.as_array_node() {
            let elems: Vec<Node> = arr.elements().iter().collect();
            return Some((class_name, elems));
        }
        return None;
    }

    None
}

fn constant_set_name(node: &Node) -> Option<String> {
    if let Some(cr) = node.as_constant_read_node() {
        let name = String::from_utf8_lossy(cr.name().as_slice()).to_string();
        if name == "Set" || name == "SortedSet" { return Some(name); }
        return None;
    }
    if let Some(cp) = node.as_constant_path_node() {
        // ::Set form: parent is None
        if cp.parent().is_none() {
            let name = String::from_utf8_lossy(cp.name()?.as_slice()).to_string();
            if name == "Set" || name == "SortedSet" { return Some(name); }
        }
    }
    None
}

/// Element must be a literal, constant, or simple variable to be considered.
fn is_considerable(node: &Node) -> bool {
    // Literals: int, float, str (no interp), sym (no interp), true/false/nil, regexp (literal), range with literals
    // Variables: lvar, ivar, cvar, gvar
    // Constants: const_read / const_path
    use ruby_prism::Node::*;
    match node {
        IntegerNode { .. } | FloatNode { .. } | RationalNode { .. } | ImaginaryNode { .. }
        | TrueNode { .. } | FalseNode { .. } | NilNode { .. }
        | SymbolNode { .. } | StringNode { .. }
        | LocalVariableReadNode { .. } | InstanceVariableReadNode { .. }
        | ClassVariableReadNode { .. } | GlobalVariableReadNode { .. }
        | ConstantReadNode { .. } | ConstantPathNode { .. }
        | SourceFileNode { .. } | SourceLineNode { .. } | SourceEncodingNode { .. } => true,
        _ => false,
    }
}

impl<'a> Visit<'_> for SetVisitor<'a> {
    fn visit_call_node(&mut self, node: &CallNode) {
        if let Some((class_name, elements)) = match_set_init(node) {
            let mut seen: Vec<String> = Vec::new();
            for el in elements.iter() {
                if !is_considerable(el) {
                    // Reset? RuboCop just skips this element (keeps seen)
                    continue;
                }
                let loc = el.location();
                let src = self.ctx.src(loc.start_offset(), loc.end_offset()).to_string();
                if seen.contains(&src) {
                    let msg = format!("Remove the duplicate element in {}.", class_name);
                    self.offenses.push(self.ctx.offense_with_range(
                        "Lint/DuplicateSetElement",
                        &msg,
                        Severity::Warning,
                        loc.start_offset(),
                        loc.end_offset(),
                    ));
                } else {
                    seen.push(src);
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Lint/DuplicateSetElement", |_cfg| {
    Some(Box::new(DuplicateSetElement::new()))
});
