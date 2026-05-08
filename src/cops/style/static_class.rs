//! Style/StaticClass cop
//!
//! Classes containing only class methods (and constants/extends) should be modules.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
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

        let correction = Self::build_correction(node, ctx);
        let offense = ctx.offense_with_range(self.name(), MSG, Severity::Convention, start, end);
        vec![offense.with_correction(correction)]
    }
}

impl StaticClass {
    fn build_correction(node: &ruby_prism::ClassNode, ctx: &CheckContext) -> Correction {
        let mut edits: Vec<Edit> = Vec::new();

        // 1. Replace `class` keyword with `module`
        let kw = node.class_keyword_loc();
        edits.push(Edit {
            start_offset: kw.start_offset(),
            end_offset: kw.end_offset(),
            replacement: "module".to_string(),
        });

        // 2. Insert "\nmodule_function\n" after the class name
        // RuboCop: insert_after(name_loc, "\nmodule_function\n")
        // The original \n after name stays, so result is: name\nmodule_function\n\n body
        let name_end = node.constant_path().location().end_offset();
        edits.push(Edit {
            start_offset: name_end,
            end_offset: name_end,
            replacement: "\nmodule_function\n".to_string(),
        });

        // 3. Process body elements: fix defs and sclass
        let elements = Self::class_elements(node.body());
        for child in &elements {
            if let Some(def_node) = child.as_def_node() {
                // def self.method → remove `self.` (receiver start to name start)
                if let Some(recv) = def_node.receiver() {
                    if matches!(recv, Node::SelfNode { .. }) {
                        // Remove from receiver start to method name start
                        let recv_start = recv.location().start_offset();
                        let name_start = def_node.name_loc().start_offset();
                        edits.push(Edit {
                            start_offset: recv_start,
                            end_offset: name_start,
                            replacement: String::new(),
                        });
                    }
                }
            } else if let Some(sclass) = child.as_singleton_class_node() {
                // class << self ... end
                // Remove "class << self" (keyword_loc start to expression end+1 for newline?)
                // RuboCop: remove range_between(node.loc.keyword.begin_pos, node.identifier.source_range.end_pos)
                // and remove node.loc.end
                let kw_start = sclass.class_keyword_loc().start_offset();
                let expr_end = sclass.expression().location().end_offset();
                // Remove "class << self"
                edits.push(Edit {
                    start_offset: kw_start,
                    end_offset: expr_end,
                    replacement: String::new(),
                });
                // Remove "end" keyword
                let end_loc = sclass.end_keyword_loc();
                edits.push(Edit {
                    start_offset: end_loc.start_offset(),
                    end_offset: end_loc.end_offset(),
                    replacement: String::new(),
                });
            }
        }

        Correction { edits }
    }
}

crate::register_cop!("Style/StaticClass", |_cfg| Some(Box::new(StaticClass::new())));
