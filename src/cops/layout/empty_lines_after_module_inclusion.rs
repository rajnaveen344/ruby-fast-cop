//! Layout/EmptyLinesAfterModuleInclusion - Checks for an empty line after a
//! module inclusion method (`extend`, `include`, `prepend`), or a group of them.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/empty_lines_after_module_inclusion.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Add an empty line after module inclusion.";
const INCLUSION: &[&str] = &["include", "extend", "prepend"];

#[derive(Default)]
pub struct EmptyLinesAfterModuleInclusion;

impl EmptyLinesAfterModuleInclusion {
    pub fn new() -> Self {
        Self
    }
}

struct Candidate {
    /// Byte range of the include/extend/prepend CallNode (without leading ws).
    start_offset: usize,
    end_offset: usize,
    /// `Some(true)` = next sibling is also include/extend/prepend (grouping allowed).
    /// `Some(false)` = there's a next sibling that's NOT inclusion.
    /// `None` = this include is the last statement in its body.
    next_kind: Option<bool>,
}

struct Collector {
    candidates: Vec<Candidate>,
    /// True when the next StatementsNode visited is the body of an If/Unless
    /// branch. Such bodies don't get inclusion candidates recorded (mirrors
    /// RuboCop's "skip when parent is if_type?").
    in_if_branch: bool,
}

fn call_method_name(call: &ruby_prism::CallNode) -> String {
    String::from_utf8_lossy(call.name().as_slice()).to_string()
}

fn is_inclusion_call_at(node: &ruby_prism::Node) -> bool {
    // Direct inclusion call.
    if let Some(call) = node.as_call_node() {
        if call.receiver().is_some() || call.block().is_some() {
            return false;
        }
        let name = call_method_name(&call);
        if !INCLUSION.contains(&name.as_str()) {
            return false;
        }
        return match call.arguments() {
            Some(args) => args.arguments().iter().count() > 0,
            None => false,
        };
    }
    // Modifier-form `include Foo if cond` / `include Foo unless cond`.
    if let Some(if_node) = node.as_if_node() {
        if if_node.end_keyword_loc().is_none() {
            if let Some(stmts) = if_node.statements() {
                if let Some(first) = stmts.body().iter().next() {
                    return is_inclusion_call_at(&first);
                }
            }
        }
        return false;
    }
    if let Some(un) = node.as_unless_node() {
        if un.end_keyword_loc().is_none() {
            if let Some(stmts) = un.statements() {
                if let Some(first) = stmts.body().iter().next() {
                    return is_inclusion_call_at(&first);
                }
            }
        }
        return false;
    }
    false
}

impl Collector {
    fn new() -> Self {
        Self { candidates: Vec::new(), in_if_branch: false }
    }
}

impl<'pr> Visit<'pr> for Collector {
    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode<'pr>) {
        let suppress = std::mem::take(&mut self.in_if_branch);
        if !suppress {
            let body: Vec<_> = node.body().iter().collect();
            for (i, child) in body.iter().enumerate() {
                if is_inclusion_call_at(child) {
                    let loc = child.location();
                    let next_kind = body.get(i + 1).map(is_inclusion_call_at);
                    self.candidates.push(Candidate {
                        start_offset: loc.start_offset(),
                        end_offset: loc.end_offset(),
                        next_kind,
                    });
                }
            }
        }
        // Always recurse into children.
        for child in node.body().iter() {
            self.visit(&child);
        }
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'pr>) {
        // Predicate normally.
        self.visit(&node.predicate());
        if let Some(stmts) = node.statements() {
            self.in_if_branch = true;
            self.visit_statements_node(&stmts);
        }
        if let Some(sub) = node.subsequent() {
            self.visit(&sub);
        }
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode<'pr>) {
        self.visit(&node.predicate());
        if let Some(stmts) = node.statements() {
            self.in_if_branch = true;
            self.visit_statements_node(&stmts);
        }
        if let Some(else_node) = node.else_clause() {
            if let Some(stmts) = else_node.statements() {
                self.in_if_branch = true;
                self.visit_statements_node(&stmts);
            }
        }
    }

    fn visit_else_node(&mut self, node: &ruby_prism::ElseNode<'pr>) {
        if let Some(stmts) = node.statements() {
            self.in_if_branch = true;
            self.visit_statements_node(&stmts);
        }
    }
}

impl Cop for EmptyLinesAfterModuleInclusion {
    fn name(&self) -> &'static str {
        "Layout/EmptyLinesAfterModuleInclusion"
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut col = Collector::new();
        col.visit_program_node(node);

        let lines: Vec<&str> = ctx.source.lines().collect();
        let mut offenses = Vec::new();

        for cand in &col.candidates {
            match cand.next_kind {
                None => continue,
                Some(true) => continue,
                Some(false) => {}
            }

            let last_line = line_of(ctx.source, cand.end_offset.saturating_sub(1));
            if is_satisfied(&lines, last_line) {
                continue;
            }

            let location = Location::from_offsets(ctx.source, cand.start_offset, cand.end_offset);
            let mut offense = Offense::new(
                self.name(),
                MSG,
                self.severity(),
                location,
                ctx.filename,
            );

            let insert_line = next_line_for_insert(&lines, last_line);
            if let Some(end_off) = line_end_byte_offset(ctx.source, insert_line) {
                offense = offense.with_correction(Correction::insert(end_off + 1, "\n"));
            }
            offenses.push(offense);
        }

        offenses
    }
}

fn line_of(src: &str, offset: usize) -> usize {
    1 + src.as_bytes()[..offset.min(src.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

fn line_end_byte_offset(src: &str, line_1based: usize) -> Option<usize> {
    let mut count = 0usize;
    for (i, b) in src.as_bytes().iter().enumerate() {
        if *b == b'\n' {
            count += 1;
            if count == line_1based {
                return Some(i);
            }
        }
    }
    None
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

fn is_enable_directive(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("# rubocop:enable") || t.starts_with("#rubocop:enable")
}

fn is_satisfied(lines: &[&str], last_line: usize) -> bool {
    let next = lines.get(last_line);
    let after_next = lines.get(last_line + 1);
    match next {
        None => true,
        Some(l) if is_blank(l) => true,
        Some(l) if is_enable_directive(l) => {
            matches!(after_next, Some(n) if is_blank(n))
        }
        _ => false,
    }
}

fn next_line_for_insert(lines: &[&str], last_line: usize) -> usize {
    if let Some(l) = lines.get(last_line) {
        if is_enable_directive(l) {
            return last_line + 1;
        }
    }
    last_line
}

crate::register_cop!("Layout/EmptyLinesAfterModuleInclusion", |_cfg| Some(Box::new(
    EmptyLinesAfterModuleInclusion::new()
)));
