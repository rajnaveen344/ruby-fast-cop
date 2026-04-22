//! Style/MixinGrouping cop
//!
//! Checks for grouping of mixins in class/module bodies.
//! separated (default): each mixin in its own call
//! grouped: all same-type mixins in one call

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MIXIN_METHODS: &[&str] = &["extend", "include", "prepend"];

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Separated,
    Grouped,
}

pub struct MixinGrouping {
    style: Style,
}

impl Default for MixinGrouping {
    fn default() -> Self {
        Self { style: Style::Separated }
    }
}

impl MixinGrouping {
    pub fn new(style: Style) -> Self {
        Self { style }
    }

    fn is_bare_mixin_call(node: &Node) -> Option<String> {
        let call = node.as_call_node()?;
        if call.receiver().is_some() {
            return None; // has explicit receiver
        }
        let name = node_name!(call);
        if MIXIN_METHODS.contains(&name.as_ref()) {
            Some(name.to_string())
        } else {
            None
        }
    }

    fn arg_count(node: &Node) -> usize {
        if let Some(call) = node.as_call_node() {
            if let Some(args) = call.arguments() {
                return args.arguments().iter().count();
            }
        }
        0
    }

    fn check_body<'a>(
        &self,
        body_nodes: &[Node<'a>],
        ctx: &CheckContext,
        cop_name: &'static str,
    ) -> Vec<Offense> {
        let mut offenses = vec![];

        // Collect all mixin calls with their names
        let mixin_calls: Vec<(usize, String)> = body_nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| Self::is_bare_mixin_call(n).map(|name| (i, name)))
            .collect();

        match self.style {
            Style::Separated => {
                // Flag any mixin call with > 1 argument
                for (idx, mixin_name) in &mixin_calls {
                    let node = &body_nodes[*idx];
                    let argc = Self::arg_count(node);
                    if argc > 1 {
                        let start = node.location().start_offset();
                        let end = node.location().end_offset();
                        let msg = format!(
                            "Put `{}` mixins in separate statements.",
                            mixin_name
                        );
                        offenses.push(ctx.offense_with_range(
                            cop_name,
                            &msg,
                            Severity::Convention,
                            start,
                            end,
                        ));
                    }
                }
            }
            Style::Grouped => {
                // Flag any mixin call that has sibling mixin calls of the same method
                for (idx, mixin_name) in &mixin_calls {
                    let node = &body_nodes[*idx];
                    // Count sibling calls with same name
                    let sibling_count = mixin_calls
                        .iter()
                        .filter(|(_, n)| n == mixin_name)
                        .count();
                    if sibling_count > 1 {
                        let start = node.location().start_offset();
                        let end = node.location().end_offset();
                        let msg = format!(
                            "Put `{}` mixins in a single statement.",
                            mixin_name
                        );
                        offenses.push(ctx.offense_with_range(
                            cop_name,
                            &msg,
                            Severity::Convention,
                            start,
                            end,
                        ));
                    }
                }
            }
        }

        offenses
    }
}

struct MixinGroupingVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    style: Style,
}

impl<'a> Visit<'_> for MixinGroupingVisitor<'a> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        if let Some(body) = node.body() {
            let nodes: Vec<Node> = collect_body_nodes(&body);
            let cop = MixinGrouping { style: self.style };
            let mut new_offenses = cop.check_body(&nodes, self.ctx, "Style/MixinGrouping");
            self.offenses.append(&mut new_offenses);
        }
        ruby_prism::visit_class_node(self, node);
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        if let Some(body) = node.body() {
            let nodes: Vec<Node> = collect_body_nodes(&body);
            let cop = MixinGrouping { style: self.style };
            let mut new_offenses = cop.check_body(&nodes, self.ctx, "Style/MixinGrouping");
            self.offenses.append(&mut new_offenses);
        }
        ruby_prism::visit_module_node(self, node);
    }
}

fn collect_body_nodes<'a>(body: &'a Node<'a>) -> Vec<Node<'a>> {
    if let Some(stmts) = body.as_statements_node() {
        stmts.body().iter().collect()
    } else if let Some(begin) = body.as_begin_node() {
        if let Some(stmts) = begin.statements() {
            stmts.body().iter().collect()
        } else {
            vec![]
        }
    } else {
        vec![]
    }
}

impl Cop for MixinGrouping {
    fn name(&self) -> &'static str {
        "Style/MixinGrouping"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = MixinGroupingVisitor {
            ctx,
            offenses: vec![],
            style: self.style,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: Option<String>,
}

crate::register_cop!("Style/MixinGrouping", |cfg| {
    let c: Cfg = cfg.typed("Style/MixinGrouping");
    let style = match c.enforced_style.as_deref() {
        Some("grouped") => Style::Grouped,
        _ => Style::Separated,
    };
    Some(Box::new(MixinGrouping::new(style)))
});
