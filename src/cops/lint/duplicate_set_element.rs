//! Lint/DuplicateSetElement - flags duplicate elements in Set/SortedSet constructions.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/duplicate_set_element.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Remove the duplicate element in {}.";

#[derive(Default)]
pub struct DuplicateSetElement;

impl DuplicateSetElement {
    pub fn new() -> Self { Self }
}

/// Key for a "pure" element that can be compared for equality.
/// Method calls and ternaries are excluded.
#[derive(PartialEq, Eq, Hash, Clone)]
enum ElemKey {
    Symbol(String),
    Lvar(String),
    Ivar(String),
    Cvar(String),
    Gvar(String),
    Const(String),
}

fn elem_key(node: &Node, source: &str) -> Option<ElemKey> {
    match node {
        Node::SymbolNode { .. } => {
            let sym = node.as_symbol_node().unwrap();
            Some(ElemKey::Symbol(String::from_utf8_lossy(sym.unescaped()).to_string()))
        }
        Node::LocalVariableReadNode { .. } => {
            let lv = node.as_local_variable_read_node().unwrap();
            Some(ElemKey::Lvar(String::from_utf8_lossy(lv.name().as_slice()).to_string()))
        }
        Node::InstanceVariableReadNode { .. } => {
            let iv = node.as_instance_variable_read_node().unwrap();
            let s = &source[iv.location().start_offset()..iv.location().end_offset()];
            Some(ElemKey::Ivar(s.to_string()))
        }
        Node::ClassVariableReadNode { .. } => {
            let cv = node.as_class_variable_read_node().unwrap();
            let s = &source[cv.location().start_offset()..cv.location().end_offset()];
            Some(ElemKey::Cvar(s.to_string()))
        }
        Node::GlobalVariableReadNode { .. } => {
            let gv = node.as_global_variable_read_node().unwrap();
            let s = &source[gv.location().start_offset()..gv.location().end_offset()];
            Some(ElemKey::Gvar(s.to_string()))
        }
        Node::ConstantReadNode { .. } => {
            let c = node.as_constant_read_node().unwrap();
            let s = &source[c.location().start_offset()..c.location().end_offset()];
            Some(ElemKey::Const(s.to_string()))
        }
        _ => None,
    }
}

/// Compute deletion range for removing a duplicate element.
/// Removes `, element` (preceding comma + space/element).
/// Returns (delete_start, delete_end).
fn deletion_range(elements: &[Node], dup_idx: usize, source: &str) -> (usize, usize) {
    let dup = &elements[dup_idx];
    let dup_end = dup.location().end_offset();
    let dup_start = dup.location().start_offset();

    if dup_idx > 0 {
        // Remove from end of previous element up to (but not including) the duplicate.
        // The gap between prev_end and dup_start is `, ` typically.
        let prev_end = elements[dup_idx - 1].location().end_offset();
        (prev_end, dup_end)
    } else {
        // First element is duplicate; remove from dup_end to next element start (`, ` after)
        let next_start = elements[1].location().start_offset();
        (dup_start, next_start)
    }
}

fn check_elements(elements: &[Node], set_name: &str, source: &str, ctx: &CheckContext, offenses: &mut Vec<Offense>) {
    let mut seen: Vec<(ElemKey, usize)> = Vec::new(); // (key, index)
    for (i, el) in elements.iter().enumerate() {
        let key = match elem_key(el, source) {
            Some(k) => k,
            None => continue, // skip method calls etc.
        };
        if seen.iter().any(|(k, _)| k == &key) {
            let loc = el.location();
            let msg = MSG.replace("{}", set_name);
            let (del_start, del_end) = deletion_range(elements, i, source);
            let correction = Correction::delete(del_start, del_end);
            offenses.push(
                ctx.offense_with_range("Lint/DuplicateSetElement", &msg, Severity::Warning,
                    loc.start_offset(), loc.end_offset())
                    .with_correction(correction)
            );
        } else {
            seen.push((key, i));
        }
    }
}

struct Visitor<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    offenses: Vec<Offense>,
}

impl<'a, 'b> Visitor<'a, 'b> {
    /// Try to handle `Set[...]` or `SortedSet[...]` (with optional `::` prefix).
    fn check_subscript_call(&mut self, node: &ruby_prism::CallNode) {
        // `Set[:foo, :bar, :foo]` is a call with message `[]` on receiver `Set`
        let msg = String::from_utf8_lossy(node.name().as_slice()).to_string();
        if msg != "[]" { return; }
        let Some(recv) = node.receiver() else { return };
        let set_name = match const_name(&recv) {
            Some(n) if n == "Set" || n == "SortedSet" => n,
            _ => return,
        };
        let args: Vec<Node> = node.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
        if args.is_empty() { return; }
        check_elements(&args, &set_name, self.ctx.source, self.ctx, &mut self.offenses);
    }

    /// Try to handle `Set.new([...])` or `Set.new(%i[...])`.
    fn check_new_call(&mut self, node: &ruby_prism::CallNode) {
        let msg = String::from_utf8_lossy(node.name().as_slice()).to_string();
        if msg != "new" { return; }
        let Some(recv) = node.receiver() else { return };
        let set_name = match const_name(&recv) {
            Some(n) if n == "Set" || n == "SortedSet" => n,
            _ => return,
        };
        // First argument should be an array literal
        let args: Vec<Node> = node.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
        if args.is_empty() { return; }
        let arr = &args[0];
        let elements = match arr {
            Node::ArrayNode { .. } => {
                let an = arr.as_array_node().unwrap();
                an.elements().iter().collect::<Vec<Node>>()
            }
            _ => return,
        };
        if elements.is_empty() { return; }
        check_elements(&elements, &set_name, self.ctx.source, self.ctx, &mut self.offenses);
    }

    /// Try to handle `[...].to_set` or `[...]&.to_set`.
    fn check_to_set_call(&mut self, node: &ruby_prism::CallNode) {
        let msg = String::from_utf8_lossy(node.name().as_slice()).to_string();
        if msg != "to_set" { return; }
        let Some(recv) = node.receiver() else { return };
        let arr = match &recv {
            Node::ArrayNode { .. } => recv.as_array_node().unwrap(),
            _ => return,
        };
        let elements: Vec<Node> = arr.elements().iter().collect();
        if elements.is_empty() { return; }
        check_elements(&elements, "Set", self.ctx.source, self.ctx, &mut self.offenses);
    }
}

impl<'a, 'b> Visit<'_> for Visitor<'a, 'b> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_subscript_call(node);
        self.check_new_call(node);
        self.check_to_set_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

/// Get the constant name from a node (ConstantReadNode or ConstantPathNode with optional `::` prefix).
fn const_name(node: &Node) -> Option<String> {
    if let Some(c) = node.as_constant_read_node() {
        let name = String::from_utf8_lossy(c.name().as_slice()).to_string();
        return Some(name);
    }
    if let Some(cp) = node.as_constant_path_node() {
        // `::Set` — parent is None, name is the constant name
        if cp.parent().is_none() {
            if let Some(n) = cp.name() {
                let name = String::from_utf8_lossy(n.as_slice()).to_string();
                return Some(name);
            }
        }
    }
    None
}

impl Cop for DuplicateSetElement {
    fn name(&self) -> &'static str { "Lint/DuplicateSetElement" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor { ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

crate::register_cop!("Lint/DuplicateSetElement", |_cfg| Some(Box::new(DuplicateSetElement::new())));
