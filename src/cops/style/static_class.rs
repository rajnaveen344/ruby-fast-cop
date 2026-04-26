//! Style/StaticClass cop
//!
//! Classes containing only class methods (and constants/extends) should be modules.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

const MSG: &str = "Prefer modules to classes with only class methods.";

#[derive(Default)]
pub struct StaticClass;

impl StaticClass {
    pub fn new() -> Self {
        Self
    }

    fn class_elements<'a>(body: Option<Node<'a>>) -> Vec<Node<'a>> {
        let body = match body {
            Some(b) => b,
            None => return vec![],
        };
        if let Some(stmts) = body.as_statements_node() {
            stmts.body().iter().collect()
        } else {
            vec![body]
        }
    }

    /// Visibility tracking for `node_visibility`. Walks siblings, tracking
    /// current modifier from bare `private`/`public`/`protected` calls.
    fn node_visibility<'a>(elements: &[Node<'a>], target: &Node<'a>) -> &'static str {
        let mut current = "public";
        for el in elements {
            // bare access modifier
            if let Some(call) = el.as_call_node() {
                if call.receiver().is_none() && call.arguments().is_none() && call.block().is_none() {
                    let m = node_name!(call);
                    match m.as_ref() {
                        "private" => current = "private",
                        "protected" => current = "protected",
                        "public" => current = "public",
                        _ => {}
                    }
                }
            }
            if el.location().start_offset() == target.location().start_offset() {
                return match current {
                    "private" => "private",
                    "protected" => "protected",
                    _ => "public",
                };
            }
        }
        "public"
    }

    fn is_extend_call(node: &Node) -> bool {
        let call = match node.as_call_node() {
            Some(c) => c,
            None => return false,
        };
        if call.receiver().is_some() {
            return false;
        }
        node_name!(call) == "extend"
    }

    /// `equals_asgn?` — write nodes (constants, IVars, etc.).
    fn is_equals_asgn(node: &Node) -> bool {
        matches!(
            node,
            Node::ConstantWriteNode { .. }
                | Node::ConstantPathWriteNode { .. }
                | Node::ConstantOperatorWriteNode { .. }
                | Node::ConstantOrWriteNode { .. }
                | Node::ConstantAndWriteNode { .. }
                | Node::InstanceVariableWriteNode { .. }
                | Node::ClassVariableWriteNode { .. }
                | Node::GlobalVariableWriteNode { .. }
                | Node::LocalVariableWriteNode { .. }
                | Node::MultiWriteNode { .. }
        )
    }

    /// Whether sclass body contains only public defs and constant assigns.
    fn sclass_convertible_to_module(node: &Node) -> bool {
        let sclass = match node.as_singleton_class_node() {
            Some(s) => s,
            None => return false,
        };
        // Expression should be `self`
        if !matches!(sclass.expression(), Node::SelfNode { .. }) {
            return false;
        }
        let elements = Self::class_elements(sclass.body());
        if elements.is_empty() {
            return false;
        }
        // Must have at least one def; all must be public defs or equals-asgn
        for child in &elements {
            let visibility = Self::node_visibility(&elements, child);
            let ok = (visibility == "public" && (matches!(child, Node::DefNode { .. })))
                || Self::is_equals_asgn(child);
            if !ok {
                return false;
            }
        }
        true
    }

    fn class_convertible_to_module(node: &ruby_prism::ClassNode) -> bool {
        let elements = Self::class_elements(node.body());
        if elements.is_empty() {
            return false;
        }
        for child in &elements {
            let visibility = Self::node_visibility(&elements, child);
            // def_self: a DefNode where receiver is `self`
            let is_def_self = if let Some(d) = child.as_def_node() {
                matches!(d.receiver(), Some(Node::SelfNode { .. }))
            } else {
                false
            };
            let ok = (visibility == "public" && is_def_self)
                || Self::sclass_convertible_to_module(child)
                || Self::is_equals_asgn(child)
                || Self::is_extend_call(child);
            if !ok {
                return false;
            }
        }
        true
    }
}

impl Cop for StaticClass {
    fn name(&self) -> &'static str {
        "Style/StaticClass"
    }

    fn check_class(&self, node: &ruby_prism::ClassNode, ctx: &CheckContext) -> Vec<Offense> {
        // Skip subclasses (have a parent class)
        if node.superclass().is_some() {
            return vec![];
        }
        if !Self::class_convertible_to_module(node) {
            return vec![];
        }
        // Offense range = `class Name` (class keyword + constant_path)
        let kw = node.class_keyword_loc();
        let path = node.constant_path();
        let start = kw.start_offset();
        let end = path.location().end_offset();
        vec![ctx.offense_with_range(self.name(), MSG, Severity::Convention, start, end)]
    }
}

crate::register_cop!("Style/StaticClass", |_cfg| Some(Box::new(StaticClass::new())));
