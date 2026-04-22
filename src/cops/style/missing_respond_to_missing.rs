//! Style/MissingRespondToMissing cop
//!
//! Flags `method_missing` defined without a corresponding `respond_to_missing?`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{ClassNode, ModuleNode, Node, Visit};

#[derive(Default)]
pub struct MissingRespondToMissing;

impl MissingRespondToMissing {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for MissingRespondToMissing {
    fn name(&self) -> &'static str {
        "Style/MissingRespondToMissing"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = RespondToMissingVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct RespondToMissingVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> RespondToMissingVisitor<'a> {
    /// Walk a body node collecting (instance_defs, singleton_defs) as (name, start, end)
    fn collect_defs_in_body(
        body: &Option<Node>,
    ) -> (Vec<String>, Vec<String>, Vec<(String, usize, usize)>, Vec<(String, usize, usize)>) {
        let mut inst_names: Vec<String> = Vec::new();
        let mut sing_names: Vec<String> = Vec::new();
        let mut inst_defs: Vec<(String, usize, usize)> = Vec::new();
        let mut sing_defs: Vec<(String, usize, usize)> = Vec::new();

        let body = match body {
            Some(b) => b,
            None => return (inst_names, sing_names, inst_defs, sing_defs),
        };

        Self::walk_node(body, &mut inst_names, &mut sing_names, &mut inst_defs, &mut sing_defs);
        (inst_names, sing_names, inst_defs, sing_defs)
    }

    fn walk_node(
        node: &Node,
        inst_names: &mut Vec<String>,
        sing_names: &mut Vec<String>,
        inst_defs: &mut Vec<(String, usize, usize)>,
        sing_defs: &mut Vec<(String, usize, usize)>,
    ) {
        match node {
            Node::StatementsNode { .. } => {
                if let Some(stmts) = node.as_statements_node() {
                    for child in stmts.body().iter() {
                        Self::walk_node(&child, inst_names, sing_names, inst_defs, sing_defs);
                    }
                }
            }
            Node::DefNode { .. } => {
                if let Some(def) = node.as_def_node() {
                    let name = String::from_utf8_lossy(def.name().as_slice()).to_string();
                    let start = def.location().start_offset();
                    let end = def.location().end_offset();
                    if def.receiver().is_some() {
                        sing_names.push(name.clone());
                        sing_defs.push((name, start, end));
                    } else {
                        inst_names.push(name.clone());
                        inst_defs.push((name, start, end));
                    }
                }
            }
            Node::CallNode { .. } => {
                // Handle `private def method_missing` pattern
                if let Some(call) = node.as_call_node() {
                    let method = String::from_utf8_lossy(call.name().as_slice()).to_string();
                    if matches!(method.as_str(), "private" | "protected" | "public") {
                        if let Some(args) = call.arguments() {
                            for arg in args.arguments().iter() {
                                Self::walk_node(&arg, inst_names, sing_names, inst_defs, sing_defs);
                            }
                        }
                    }
                }
            }
            Node::BeginNode { .. } => {
                if let Some(begin) = node.as_begin_node() {
                    if let Some(stmts) = begin.statements() {
                        for child in stmts.body().iter() {
                            Self::walk_node(&child, inst_names, sing_names, inst_defs, sing_defs);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn check_class_body(&mut self, body: &Option<Node>) {
        let (inst_names, sing_names, inst_defs, sing_defs) = Self::collect_defs_in_body(body);

        for (name, start, end) in &inst_defs {
            if name == "method_missing" && !inst_names.iter().any(|n| n == "respond_to_missing?") {
                self.offenses.push(self.ctx.offense_with_range(
                    "Style/MissingRespondToMissing",
                    "When using `method_missing`, define `respond_to_missing?`.",
                    Severity::Convention,
                    *start,
                    *end,
                ));
            }
        }

        for (name, start, end) in &sing_defs {
            if name == "method_missing" && !sing_names.iter().any(|n| n == "respond_to_missing?") {
                self.offenses.push(self.ctx.offense_with_range(
                    "Style/MissingRespondToMissing",
                    "When using `method_missing`, define `respond_to_missing?`.",
                    Severity::Convention,
                    *start,
                    *end,
                ));
            }
        }
    }
}

impl Visit<'_> for RespondToMissingVisitor<'_> {
    fn visit_class_node(&mut self, node: &ClassNode) {
        self.check_class_body(&node.body());
        ruby_prism::visit_class_node(self, node);
    }

    fn visit_module_node(&mut self, node: &ModuleNode) {
        self.check_class_body(&node.body());
        ruby_prism::visit_module_node(self, node);
    }
}

crate::register_cop!("Style/MissingRespondToMissing", |_cfg| {
    Some(Box::new(MissingRespondToMissing::new()))
});
