//! Style/InfiniteLoop — Prefer `Kernel#loop` for infinite loops.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/infinite_loop.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Use `Kernel#loop` for infinite loops.";

#[derive(Default)]
pub struct InfiniteLoop {
    indentation_width: usize,
}

impl InfiniteLoop {
    pub fn new(indentation_width: usize) -> Self {
        Self { indentation_width }
    }
}

/// Is this node always truthy? (for `while` condition)
fn is_always_truthy(node: &Node) -> bool {
    match node {
        Node::TrueNode { .. } => true,
        Node::IntegerNode { .. } => true,
        Node::FloatNode { .. } => true,
        Node::ArrayNode { .. } => true,
        Node::HashNode { .. } => true,
        Node::StringNode { .. } => true,
        Node::InterpolatedStringNode { .. } => true,
        Node::SymbolNode { .. } => true,
        Node::RationalNode { .. } => true,
        Node::ImaginaryNode { .. } => true,
        _ => false,
    }
}

/// Is this node always falsy? (for `until` condition)
fn is_always_falsy(node: &Node) -> bool {
    matches!(node, Node::FalseNode { .. } | Node::NilNode { .. })
}

/// Collect all local variables written in a node subtree.
fn collect_written_vars(node: &Node) -> Vec<String> {
    let mut collector = VarCollector { vars: Vec::new() };
    collector.visit(node);
    collector.vars
}

struct VarCollector {
    vars: Vec<String>,
}

impl Visit<'_> for VarCollector {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        if !self.vars.contains(&name) {
            self.vars.push(name);
        }
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        for target in node.lefts().iter() {
            if let Some(lv) = target.as_local_variable_target_node() {
                let name = String::from_utf8_lossy(lv.name().as_slice()).to_string();
                if !self.vars.contains(&name) {
                    self.vars.push(name);
                }
            }
        }
        ruby_prism::visit_multi_write_node(self, node);
    }
}

/// Collect all local vars read in a subtree.
fn collect_read_vars(node: &Node) -> Vec<String> {
    let mut collector = ReadVarCollector { vars: Vec::new() };
    collector.visit(node);
    collector.vars
}

struct ReadVarCollector {
    vars: Vec<String>,
}

impl Visit<'_> for ReadVarCollector {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        if !self.vars.contains(&name) {
            self.vars.push(name);
        }
        ruby_prism::visit_local_variable_read_node(self, node);
    }
}

impl Cop for InfiniteLoop {
    fn name(&self) -> &'static str {
        "Style/InfiniteLoop"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = InfiniteLoopVisitor {
            ctx,
            offenses: Vec::new(),
            indentation_width: self.indentation_width,
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct InfiniteLoopVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    indentation_width: usize,
}

impl<'a> InfiniteLoopVisitor<'a> {
    fn indentation_width(&self) -> usize {
        self.indentation_width
    }

    /// Build corrected source for a block-form while/until loop.
    fn correct_block_loop(&self, body_src: &str, keyword_col: usize, is_do: bool) -> String {
        let indent = " ".repeat(keyword_col);
        let inner_indent = " ".repeat(keyword_col + self.indentation_width());
        // Body lines: re-indent by removing old indent and adding new
        let mut lines: Vec<&str> = body_src.lines().collect();
        // Remove trailing empty lines at end
        while lines.last().map_or(false, |l: &&str| l.trim().is_empty()) {
            lines.pop();
        }
        // Add comments from do-keyword line if any
        let body_formatted: Vec<String> = lines.iter().map(|line| {
            format!("{inner_indent}{}", line.trim())
        }).collect();
        format!("loop do\n{}\n{indent}end", body_formatted.join("\n"))
    }

    /// Check if loop contains newly-introduced local vars used AFTER the loop.
    /// Returns true if converting to `loop do` would change semantics.
    /// `sibling_stmts` = all statements in the same body as the loop.
    fn would_change_semantics_in(&self, loop_node: &Node, sibling_stmts: &[Node]) -> bool {
        let loop_written = collect_written_vars(loop_node);
        if loop_written.is_empty() { return false; }

        let loop_start = loop_node.location().start_offset();
        let loop_end = loop_node.location().end_offset();

        let loop_stmt_idx = sibling_stmts.iter().position(|n| {
            n.location().start_offset() == loop_start ||
            (n.location().start_offset() <= loop_start && n.location().end_offset() >= loop_end)
        });

        let loop_stmt_idx = match loop_stmt_idx {
            Some(i) => i,
            None => return false,
        };

        let vars_before: Vec<String> = sibling_stmts[..loop_stmt_idx].iter()
            .flat_map(|n| collect_written_vars(n))
            .collect();

        let vars_after: Vec<String> = sibling_stmts[loop_stmt_idx + 1..].iter()
            .flat_map(|n| collect_read_vars(n))
            .collect();

        for var in &loop_written {
            if vars_after.contains(var) && !vars_before.contains(var) {
                return true;
            }
        }

        false
    }

    /// Check semantics using program-level statements.
    fn would_change_semantics(&self, loop_node: &Node, program_node: &ruby_prism::ProgramNode) -> bool {
        let stmts: Vec<Node> = program_node.statements().body().iter().collect();
        self.would_change_semantics_in(loop_node, &stmts)
    }

    /// Check semantics using a StatementsNode (e.g., def body).
    fn would_change_semantics_stmts(&self, loop_node: &Node, stmts_node: &ruby_prism::StatementsNode) -> bool {
        let stmts: Vec<Node> = stmts_node.body().iter().collect();
        self.would_change_semantics_in(loop_node, &stmts)
    }

    fn is_modifier_while(&self, node: &ruby_prism::WhileNode) -> bool {
        let start = node.keyword_loc().start_offset();
        let body_start = node.statements()
            .map(|s| s.location().start_offset())
            .unwrap_or(node.location().start_offset());
        body_start < start
    }

    fn is_modifier_until(&self, node: &ruby_prism::UntilNode) -> bool {
        let start = node.keyword_loc().start_offset();
        let body_start = node.statements()
            .map(|s| s.location().start_offset())
            .unwrap_or(node.location().start_offset());
        body_start < start
    }

    /// `sibling_stmts`: the enclosing body statements, if known.
    fn check_while_node_with_siblings(&mut self, node: &ruby_prism::WhileNode, sibling_stmts: Option<&[Node]>) {
        let cond = node.predicate();
        if !is_always_truthy(&cond) { return; }

        let keyword_loc = node.keyword_loc();
        let start = keyword_loc.start_offset();
        let end = keyword_loc.end_offset();

        let body_start = match node.statements() {
            Some(s) => s.location().start_offset(),
            None => node.location().start_offset(),
        };
        let is_modifier = body_start < start;

        if is_modifier {
            // Modifier `body while true` — check semantics too
            if let Some(siblings) = sibling_stmts {
                if self.would_change_semantics_in(&node.as_node(), siblings) {
                    return;
                }
            }
            let offense = self.ctx.offense_with_range(
                "Style/InfiniteLoop", MSG, Severity::Convention, start, end,
            );
            self.offenses.push(offense);
            return;
        }

        // Block form: check semantics if we have sibling context
        if let Some(siblings) = sibling_stmts {
            if self.would_change_semantics_in(&node.as_node(), siblings) {
                return;
            }
        }

        let offense = self.ctx.offense_with_range(
            "Style/InfiniteLoop", MSG, Severity::Convention, start, end,
        );
        self.offenses.push(offense);
    }

    fn check_until_node_with_siblings(&mut self, node: &ruby_prism::UntilNode, sibling_stmts: Option<&[Node]>) {
        let cond = node.predicate();
        if !is_always_falsy(&cond) { return; }

        let keyword_loc = node.keyword_loc();
        let start = keyword_loc.start_offset();
        let end = keyword_loc.end_offset();

        let body_start = match node.statements() {
            Some(s) => s.location().start_offset(),
            None => node.location().start_offset(),
        };
        let is_modifier = body_start < start;

        if is_modifier {
            // Modifier `body until false` — check semantics too
            if let Some(siblings) = sibling_stmts {
                if self.would_change_semantics_in(&node.as_node(), siblings) {
                    return;
                }
            }
            let offense = self.ctx.offense_with_range(
                "Style/InfiniteLoop", MSG, Severity::Convention, start, end,
            );
            self.offenses.push(offense);
            return;
        }

        if let Some(siblings) = sibling_stmts {
            if self.would_change_semantics_in(&node.as_node(), siblings) {
                return;
            }
        }

        let offense = self.ctx.offense_with_range(
            "Style/InfiniteLoop", MSG, Severity::Convention, start, end,
        );
        self.offenses.push(offense);
    }
}

impl<'a> InfiniteLoopVisitor<'a> {
    /// Process a statement list, checking while/until with sibling context.
    fn process_stmts(&mut self, stmts_body: Vec<Node>) {
        for stmt in &stmts_body {
            match stmt {
                Node::WhileNode { .. } => {
                    let wn = stmt.as_while_node().unwrap();
                    self.check_while_node_with_siblings(&wn, Some(&stmts_body));
                    // Recurse into body
                    if let Some(body) = wn.statements() {
                        self.visit(&body.as_node());
                    }
                    // Visit condition
                    self.visit(&wn.predicate());
                }
                Node::UntilNode { .. } => {
                    let un = stmt.as_until_node().unwrap();
                    self.check_until_node_with_siblings(&un, Some(&stmts_body));
                    if let Some(body) = un.statements() {
                        self.visit(&body.as_node());
                    }
                    self.visit(&un.predicate());
                }
                _ => {
                    self.visit(stmt);
                }
            }
        }
    }
}

impl<'a> Visit<'_> for InfiniteLoopVisitor<'a> {
    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        // Called for nested while nodes not at statement-list level (e.g. inside if branches)
        // No sibling context available → don't check semantics (conservative: no offense if unsure)
        self.check_while_node_with_siblings(node, None);
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        self.check_until_node_with_siblings(node, None);
        ruby_prism::visit_until_node(self, node);
    }

    fn visit_program_node(&mut self, node: &ruby_prism::ProgramNode) {
        let stmts: Vec<Node> = node.statements().body().iter().collect();
        self.process_stmts(stmts);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        // Process def body with sibling context
        if let Some(body) = node.body() {
            if let Some(stmts_node) = body.as_statements_node() {
                let stmts: Vec<Node> = stmts_node.body().iter().collect();
                self.process_stmts(stmts);
            } else {
                self.visit(&body);
            }
        }
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        if let Some(body) = node.body() {
            if let Some(stmts_node) = body.as_statements_node() {
                let stmts: Vec<Node> = stmts_node.body().iter().collect();
                self.process_stmts(stmts);
            } else {
                self.visit(&body);
            }
        }
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        if let Some(body) = node.body() {
            if let Some(stmts_node) = body.as_statements_node() {
                let stmts: Vec<Node> = stmts_node.body().iter().collect();
                self.process_stmts(stmts);
            } else {
                self.visit(&body);
            }
        }
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        if let Some(body) = node.body() {
            if let Some(stmts_node) = body.as_statements_node() {
                let stmts: Vec<Node> = stmts_node.body().iter().collect();
                self.process_stmts(stmts);
            } else {
                self.visit(&body);
            }
        }
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct LayoutCfg {
    #[serde(rename = "Width")]
    width: Option<u64>,
}

crate::register_cop!("Style/InfiniteLoop", |cfg| {
    let layout_cfg = cfg.get_cop_config("Layout/IndentationWidth");
    let indentation_width = layout_cfg
        .and_then(|c| c.raw.get("Width"))
        .and_then(|v| v.as_i64())
        .unwrap_or(2) as usize;
    Some(Box::new(InfiniteLoop::new(indentation_width)))
});
