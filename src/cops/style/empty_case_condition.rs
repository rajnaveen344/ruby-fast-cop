//! Style/EmptyCaseCondition — flag `case` without a condition.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/empty_case_condition.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::{line_start_offset, line_at_offset};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/EmptyCaseCondition";
const MSG: &str = "Do not use empty `case` condition, instead use an `if` expression.";

#[derive(Default)]
pub struct EmptyCaseCondition;

impl EmptyCaseCondition {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for EmptyCaseCondition {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut v = Visitor {
            ctx,
            offenses: Vec::new(),
            parent_skips: false,
        };
        v.visit(&node.as_node());
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// True when the containing node is return/break/next/send,
    /// meaning a case-with-return inside cannot be converted safely.
    parent_skips: bool,
}

fn branch_has_return(stmts_opt: &Option<ruby_prism::StatementsNode>) -> bool {
    if let Some(stmts) = stmts_opt {
        for s in stmts.body().iter() {
            if contains_return(&s) {
                return true;
            }
        }
    }
    false
}

fn contains_return(node: &Node) -> bool {
    struct F {
        found: bool,
    }
    impl<'pr> Visit<'pr> for F {
        fn visit_return_node(&mut self, _n: &ruby_prism::ReturnNode<'pr>) {
            self.found = true;
        }
    }
    let mut f = F { found: false };
    f.visit(node);
    f.found
}

/// Build the correction: case/when → if/elsif, multi-conditions joined with ||.
fn build_correction(node: &ruby_prism::CaseNode<'_>, source: &str) -> Option<Correction> {
    let when_nodes: Vec<ruby_prism::WhenNode<'_>> = node.conditions().iter()
        .filter_map(|c| c.as_when_node())
        .collect();

    if when_nodes.is_empty() {
        return None;
    }

    let mut edits: Vec<Edit> = Vec::new();
    let source_bytes = source.as_bytes();

    // 1. Replace case..first_when_keyword with "if"
    //    Also collect comments between case line and first when line.
    let case_kw_start = node.case_keyword_loc().start_offset();
    let first_when_kw_end = when_nodes[0].keyword_loc().end_offset();

    // Collect comments in lines between case keyword and first when keyword (exclusive of when line)
    let case_line = line_at_offset(source, case_kw_start) as usize; // 1-indexed
    let first_when_line = line_at_offset(source, first_when_kw_end.saturating_sub(1)) as usize; // 1-indexed

    // Indent of the case keyword
    let case_line_start = line_start_offset(source, case_kw_start);
    let indent_bytes = &source_bytes[case_line_start..case_kw_start];
    let indent_str: String = indent_bytes.iter().map(|&b| b as char).collect();

    // Collect inline + intermediate comments
    let mut comments_to_prepend = String::new();
    for line_no in case_line..first_when_line {
        // Get the text of this line
        let line_start = if line_no == 1 {
            0
        } else {
            // find offset of line_no-th line (1-indexed)
            let mut count = 0;
            let mut pos = 0;
            for (i, &b) in source_bytes.iter().enumerate() {
                if b == b'\n' {
                    count += 1;
                    if count == line_no - 1 {
                        pos = i + 1;
                        break;
                    }
                }
            }
            pos
        };
        let line_end = source_bytes[line_start..].iter().position(|&b| b == b'\n')
            .map_or(source.len(), |p| line_start + p);
        let line_text = &source[line_start..line_end];
        // Find comment on this line
        if let Some(comment_pos) = crate::helpers::source::find_comment_start(line_text) {
            let comment_text = &line_text[comment_pos..];
            comments_to_prepend.push_str(&indent_str);
            comments_to_prepend.push_str(comment_text);
            comments_to_prepend.push('\n');
        }
    }

    // Insert comments before the start of the case line
    if !comments_to_prepend.is_empty() {
        edits.push(Edit {
            start_offset: case_line_start,
            end_offset: case_line_start,
            replacement: comments_to_prepend,
        });
    }

    // Replace case_kw_start..first_when_kw_end with "if"
    edits.push(Edit {
        start_offset: case_kw_start,
        end_offset: first_when_kw_end,
        replacement: "if".into(),
    });

    // 2. Replace subsequent "when" keywords with "elsif"
    for when_node in &when_nodes[1..] {
        let kw = when_node.keyword_loc();
        edits.push(Edit {
            start_offset: kw.start_offset(),
            end_offset: kw.end_offset(),
            replacement: "elsif".into(),
        });
    }

    // 3. Multi-condition when: join with " || "
    for when_node in &when_nodes {
        let conditions: Vec<_> = when_node.conditions().iter().collect();
        if conditions.len() > 1 {
            let first_start = conditions[0].location().start_offset();
            let last_end = conditions[conditions.len() - 1].location().end_offset();
            let joined = conditions.iter()
                .map(|c| &source[c.location().start_offset()..c.location().end_offset()])
                .collect::<Vec<_>>()
                .join(" || ");
            edits.push(Edit {
                start_offset: first_start,
                end_offset: last_end,
                replacement: joined,
            });
        }
    }

    Some(Correction { edits })
}

impl<'pr> Visit<'pr> for Visitor<'_> {
    fn visit_return_node(&mut self, node: &ruby_prism::ReturnNode<'pr>) {
        let saved = self.parent_skips;
        self.parent_skips = true;
        ruby_prism::visit_return_node(self, node);
        self.parent_skips = saved;
    }

    fn visit_break_node(&mut self, node: &ruby_prism::BreakNode<'pr>) {
        let saved = self.parent_skips;
        self.parent_skips = true;
        ruby_prism::visit_break_node(self, node);
        self.parent_skips = saved;
    }

    fn visit_next_node(&mut self, node: &ruby_prism::NextNode<'pr>) {
        let saved = self.parent_skips;
        self.parent_skips = true;
        ruby_prism::visit_next_node(self, node);
        self.parent_skips = saved;
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        let saved = self.parent_skips;
        self.parent_skips = true;
        ruby_prism::visit_call_node(self, node);
        self.parent_skips = saved;
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode<'pr>) {
        if node.predicate().is_none() {
            // Check branches for `return`
            let mut any_return = false;
            for c in node.conditions().iter() {
                if let Some(when) = c.as_when_node() {
                    if branch_has_return(&when.statements()) {
                        any_return = true;
                        break;
                    }
                }
            }
            if !any_return {
                if let Some(else_clause) = node.else_clause() {
                    if branch_has_return(&else_clause.statements()) {
                        any_return = true;
                    }
                }
            }

            if !self.parent_skips && !any_return {
                let loc = node.case_keyword_loc();
                let start = loc.start_offset();
                let end = loc.end_offset();
                let correction = build_correction(node, self.ctx.source);
                let offense = self.ctx.offense_with_range(
                    COP_NAME,
                    MSG,
                    Severity::Convention,
                    start,
                    end,
                );
                self.offenses.push(if let Some(c) = correction {
                    offense.with_correction(c)
                } else {
                    offense
                });
            }
        }

        // Reset parent_skips for body traversal — nested case without condition
        // inside a branch is independent.
        let saved = self.parent_skips;
        self.parent_skips = false;
        ruby_prism::visit_case_node(self, node);
        self.parent_skips = saved;
    }
}

crate::register_cop!("Style/EmptyCaseCondition", |_cfg| {
    Some(Box::new(EmptyCaseCondition::new()))
});
