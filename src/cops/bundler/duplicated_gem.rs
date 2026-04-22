//! Bundler/DuplicatedGem cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;
use std::collections::HashMap;

#[derive(Default)]
pub struct DuplicatedGem;

impl DuplicatedGem {
    pub fn new() -> Self { Self }
}

impl Cop for DuplicatedGem {
    fn name(&self) -> &'static str { "Bundler/DuplicatedGem" }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        // Only run on Gemfile-like files
        let basename = ctx.filename.rsplit('/').next().unwrap_or(ctx.filename);
        if basename != "Gemfile" && basename != "gems.rb" && !basename.ends_with(".gemfile") {
            return vec![];
        }

        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();

        let mut collector = GemCallCollector { source: ctx.source, calls: Vec::new(), conditional_stack: Vec::new(), inside_if: false };
        collector.visit(&tree);

        // Group by gem name
        let mut by_name: HashMap<String, Vec<GemCall>> = HashMap::new();
        for call in collector.calls {
            by_name.entry(call.name.clone()).or_default().push(call);
        }

        let mut offenses = Vec::new();
        for (_name, mut calls) in by_name {
            if calls.len() < 2 { continue; }
            calls.sort_by_key(|c| c.line);

            if all_in_same_conditional(&calls, ctx.source) {
                continue;
            }

            let first_line = calls[0].line;
            for dup in &calls[1..] {
                let msg = format!(
                    "Gem `{}` requirements already given on line {} of the Gemfile.",
                    dup.name, first_line
                );
                offenses.push(ctx.offense_with_range(
                    self.name(), &msg, self.severity(),
                    dup.node_start, dup.node_end,
                ));
            }
        }

        offenses.sort_by_key(|o| (o.location.line, o.location.column));
        offenses
    }
}

#[derive(Debug)]
struct GemCall {
    name: String,
    line: usize,
    node_start: usize,
    node_end: usize,
    conditional_parent_start: Option<usize>,
}

struct GemCallCollector<'a> {
    source: &'a str,
    calls: Vec<GemCall>,
    conditional_stack: Vec<usize>,
    // Track if we're already inside an if/elsif chain (don't push nested IfNode)
    inside_if: bool,
}

impl Visit<'_> for GemCallCollector<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice()).to_string();
        if method == "gem" && node.receiver().is_none() {
            if let Some(args) = node.arguments() {
                let arg_list: Vec<_> = args.arguments().iter().collect();
                if let Some(first) = arg_list.first() {
                    if let ruby_prism::Node::StringNode { .. } = first {
                        let s = first.as_string_node().unwrap();
                        let gem_name = String::from_utf8_lossy(s.unescaped()).to_string();
                        let loc = node.location();
                        let line = 1 + self.source[..loc.start_offset()].bytes().filter(|&b| b == b'\n').count();
                        self.calls.push(GemCall {
                            name: gem_name,
                            line,
                            node_start: loc.start_offset(),
                            node_end: loc.end_offset(),
                            conditional_parent_start: self.conditional_stack.last().copied(),
                        });
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        if !self.inside_if {
            // Push root if node — all elsif/else branches share this root
            self.conditional_stack.push(node.location().start_offset());
            self.inside_if = true;
            ruby_prism::visit_if_node(self, node);
            self.inside_if = false;
            self.conditional_stack.pop();
        } else {
            // Inside an elsif chain — don't push again, just recurse
            ruby_prism::visit_if_node(self, node);
        }
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        self.conditional_stack.push(node.location().start_offset());
        ruby_prism::visit_case_node(self, node);
        self.conditional_stack.pop();
    }
}

fn all_in_same_conditional(calls: &[GemCall], source: &str) -> bool {
    if calls.iter().any(|c| c.conditional_parent_start.is_none()) {
        return false;
    }
    let first_parent = calls[0].conditional_parent_start;
    if !calls.iter().all(|c| c.conditional_parent_start == first_parent) {
        return false;
    }
    let parent_start = first_parent.unwrap();
    let result = ruby_prism::parse(source.as_bytes());
    let tree = result.node();

    let mut finder = ConditionalBranchChecker {
        parent_start,
        call_offsets: calls.iter().map(|c| c.node_start).collect(),
        all_in_different_branches: false,
    };
    finder.visit(&tree);
    finder.all_in_different_branches
}

struct ConditionalBranchChecker {
    parent_start: usize,
    call_offsets: Vec<usize>,
    all_in_different_branches: bool,
}

impl Visit<'_> for ConditionalBranchChecker {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        if node.location().start_offset() == self.parent_start {
            let branches = collect_if_branches(node);
            self.all_in_different_branches = each_call_in_own_branch(&self.call_offsets, &branches);
            return;
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        if node.location().start_offset() == self.parent_start {
            let mut branches: Vec<(usize, usize)> = node.conditions().iter().map(|c| {
                (c.location().start_offset(), c.location().end_offset())
            }).collect();
            if let Some(else_cl) = node.else_clause() {
                branches.push((else_cl.location().start_offset(), else_cl.location().end_offset()));
            }
            self.all_in_different_branches = each_call_in_own_branch(&self.call_offsets, &branches);
            return;
        }
        ruby_prism::visit_case_node(self, node);
    }
}

fn collect_if_branches(node: &ruby_prism::IfNode) -> Vec<(usize, usize)> {
    let mut branches = Vec::new();
    if let Some(body) = node.statements() {
        branches.push((body.location().start_offset(), body.location().end_offset()));
    }
    match node.subsequent() {
        Some(cons) => match cons {
            ruby_prism::Node::ElseNode { .. } => {
                let en = cons.as_else_node().unwrap();
                if let Some(body) = en.statements() {
                    branches.push((body.location().start_offset(), body.location().end_offset()));
                }
            }
            ruby_prism::Node::IfNode { .. } => {
                let sub = cons.as_if_node().unwrap();
                let sub_branches = collect_if_branches(&sub);
                branches.extend(sub_branches);
            }
            _ => {}
        },
        None => {}
    }
    branches
}

fn each_call_in_own_branch(offsets: &[usize], branches: &[(usize, usize)]) -> bool {
    let mut used_branches: Vec<usize> = Vec::new();
    for &offset in offsets {
        let branch_idx = branches.iter().position(|&(s, e)| offset >= s && offset < e);
        match branch_idx {
            None => return false,
            Some(idx) => {
                if used_branches.contains(&idx) {
                    return false;
                }
                used_branches.push(idx);
            }
        }
    }
    true
}

crate::register_cop!("Bundler/DuplicatedGem", |_cfg| Some(Box::new(DuplicatedGem::new())));
