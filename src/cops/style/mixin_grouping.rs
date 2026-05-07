//! Style/MixinGrouping cop
//!
//! Checks for grouping of mixins in class/module bodies.
//! separated (default): each mixin in its own call
//! grouped: all same-type mixins in one call

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
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

    fn get_indent(source: &str, node_start: usize) -> String {
        // Find start of the line containing node_start
        let bytes = source.as_bytes();
        let mut line_start = node_start;
        while line_start > 0 && bytes[line_start - 1] != b'\n' {
            line_start -= 1;
        }
        let mut indent = String::new();
        for &b in &bytes[line_start..node_start] {
            if b == b' ' || b == b'\t' {
                indent.push(b as char);
            } else {
                break;
            }
        }
        indent
    }

    fn node_args_sources<'a>(node: &'a Node<'a>, source: &str) -> Vec<String> {
        if let Some(call) = node.as_call_node() {
            if let Some(args) = call.arguments() {
                return args.arguments().iter().map(|a| {
                    source[a.location().start_offset()..a.location().end_offset()].to_string()
                }).collect();
            }
        }
        vec![]
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
                        // Build correction: replace node with N separate lines (reversed)
                        let args = Self::node_args_sources(node, ctx.source);
                        let indent = Self::get_indent(ctx.source, start);
                        // reversed like RuboCop: last arg first
                        let mut lines: Vec<String> = Vec::new();
                        // First line (no indent prefix added - it's already in place)
                        lines.push(format!("{} {}", mixin_name, args.last().unwrap()));
                        for arg in args[..args.len()-1].iter().rev() {
                            lines.push(format!("{}{} {}", indent, mixin_name, arg));
                        }
                        let replacement = lines.join("\n");
                        let correction = Correction::replace(start, end, replacement);
                        offenses.push(
                            ctx.offense_with_range(cop_name, &msg, Severity::Convention, start, end)
                                .with_correction(correction)
                        );
                    }
                }
            }
            Style::Grouped => {
                // Group by mixin method name
                // For each method with >1 sibling: first gets grouped replacement, rest get removed
                let source = ctx.source;
                let mut processed: std::collections::HashSet<String> = std::collections::HashSet::new();

                for (idx, mixin_name) in &mixin_calls {
                    if processed.contains(mixin_name) { continue; }

                    let siblings: Vec<usize> = mixin_calls.iter()
                        .filter(|(_, n)| n == mixin_name)
                        .map(|(i, _)| *i)
                        .collect();

                    if siblings.len() <= 1 { continue; }
                    processed.insert(mixin_name.clone());

                    // Collect all args from all siblings (reversed order like RuboCop)
                    let mut all_args: Vec<String> = Vec::new();
                    for &si in siblings.iter().rev() {
                        let sib_node = &body_nodes[si];
                        let args = Self::node_args_sources(sib_node, source);
                        all_args.extend(args);
                    }
                    let grouped_replacement = format!("{} {}", mixin_name, all_args.join(", "));

                    // Emit offenses and corrections for each sibling
                    // First sibling: replace with grouped, rest: remove (including preceding whitespace/newline)
                    let first_sibling_idx = siblings[0];

                    for &si in &siblings {
                        let sib_node = &body_nodes[si];
                        let sib_start = sib_node.location().start_offset();
                        let sib_end = sib_node.location().end_offset();
                        let msg = format!("Put `{}` mixins in a single statement.", mixin_name);

                        let correction = if si == first_sibling_idx {
                            // Replace with grouped form
                            Correction::replace(sib_start, sib_end, grouped_replacement.clone())
                        } else {
                            // Remove this sibling. Also remove preceding whitespace up to newline.
                            // RuboCop removes from end of previous mixin to end of this node.
                            // Find the range: from end of previous mixin node (in order) to end of this node.
                            // "previous mixin" = the mixin right before this one in sibling order
                            let prev_sib_idx = {
                                let pos = siblings.iter().position(|&x| x == si).unwrap();
                                siblings[pos - 1]
                            };
                            let prev_node = &body_nodes[prev_sib_idx];
                            let prev_end = prev_node.location().end_offset();
                            // Check if between prev_end and sib_end there's only whitespace
                            let between = &source[prev_end..sib_start];
                            let range_start = if between.chars().all(|c| c.is_whitespace()) {
                                prev_end
                            } else {
                                sib_start
                            };
                            Correction::delete(range_start, sib_end)
                        };

                        offenses.push(
                            ctx.offense_with_range(cop_name, &msg, Severity::Convention, sib_start, sib_end)
                                .with_correction(correction)
                        );
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
