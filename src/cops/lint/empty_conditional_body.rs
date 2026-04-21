//! Lint/EmptyConditionalBody - flag empty `if`/`elsif`/`unless` branches.
//!
//! Ported from:
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/empty_conditional_body.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{IfNode, Node, UnlessNode, Visit};

pub struct EmptyConditionalBody {
    allow_comments: bool,
}

impl EmptyConditionalBody {
    pub fn new(allow_comments: bool) -> Self {
        Self { allow_comments }
    }
}

impl Default for EmptyConditionalBody {
    fn default() -> Self {
        Self::new(true)
    }
}

impl Cop for EmptyConditionalBody {
    fn name(&self) -> &'static str {
        "Lint/EmptyConditionalBody"
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let comment_lines: Vec<(usize, usize)> = {
            let result = ruby_prism::parse(ctx.source.as_bytes());
            result
                .comments()
                .map(|c| {
                    let loc = c.location();
                    (loc.start_offset(), loc.end_offset())
                })
                .collect()
        };
        let mut v = Visitor {
            ctx,
            allow_comments: self.allow_comments,
            comment_ranges: comment_lines,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    allow_comments: bool,
    comment_ranges: Vec<(usize, usize)>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    /// Byte-range between `start` (inclusive) and `end` (exclusive) contains a comment?
    fn range_contains_comment(&self, start: usize, end: usize) -> bool {
        self.comment_ranges.iter().any(|(s, _)| *s >= start && *s < end)
    }

    fn check_if(&mut self, node: &IfNode, keyword: &str) {
        // node.body or one-line (begin == end line) -> skip
        if node.statements().is_some() {
            return;
        }
        let keyword_loc = node.if_keyword_loc();
        let end_loc = node.end_keyword_loc();
        // Same-line if/end like `if condition; else ... end` -> skip.
        // Only applies to outer `if`/`unless`, not `elsif`.
        if keyword != "elsif" {
            if let (Some(kw), Some(end)) = (keyword_loc.as_ref(), end_loc.as_ref()) {
                if self.ctx.same_line(kw.start_offset(), end.start_offset()) {
                    return;
                }
            }
        }

        // offense_range: from node.source_range.begin to (else begin if exists, else source_range end)
        let src_range = node.location();
        let offense_start = src_range.start_offset();
        let offense_end = match node.subsequent() {
            Some(sub) => sub.location().start_offset(),
            None => match end_loc.as_ref() {
                Some(e) => e.start_offset(),
                None => src_range.end_offset(),
            },
        };

        // AllowComments: skip if branch body region contains any comment.
        // Body region is from end-of-predicate-line to start of next branch/end.
        if self.allow_comments {
            let pred_end = node.predicate().location().end_offset();
            // after the newline on predicate's line:
            let nl = self.ctx.source[pred_end..]
                .find('\n')
                .map(|i| pred_end + i + 1)
                .unwrap_or(pred_end);
            let body_end = match node.subsequent() {
                Some(sub) => sub.location().start_offset(),
                None => end_loc.as_ref().map(|l| l.start_offset()).unwrap_or(src_range.end_offset()),
            };
            // Also inline comment on predicate line counts (e.g. `elsif other # no op`).
            let pred_line_end = self.ctx.source[pred_end..]
                .find('\n')
                .map(|i| pred_end + i)
                .unwrap_or(self.ctx.source.len());
            if self.range_contains_comment(pred_end, pred_line_end)
                || self.range_contains_comment(nl, body_end)
            {
                return;
            }
        }

        self.offenses.push(self.ctx.offense_with_range(
            "Lint/EmptyConditionalBody",
            &format!("Avoid `{}` branches without a body.", keyword),
            Severity::Warning,
            offense_start,
            offense_end,
        ));
    }
}

impl Visit<'_> for Visitor<'_> {
    fn visit_if_node(&mut self, node: &IfNode) {
        // Determine keyword: "if" if node starts with `if`, else "elsif"
        let loc = node.location();
        let kw_src = &self.ctx.source[loc.start_offset()..loc.end_offset().min(loc.start_offset() + 5)];
        let keyword = if kw_src.starts_with("elsif") { "elsif" } else { "if" };
        self.check_if(node, keyword);
        // Recurse into subsequent (elsif chains are IfNodes in subsequent)
        if let Some(sub) = node.subsequent() {
            if let Node::IfNode { .. } = &sub {
                self.visit_if_node(&sub.as_if_node().unwrap());
            } else if let Node::ElseNode { .. } = &sub {
                let e = sub.as_else_node().unwrap();
                if let Some(stmts) = e.statements() {
                    ruby_prism::visit_statements_node(self, &stmts);
                }
            }
        }
        if let Some(stmts) = node.statements() {
            ruby_prism::visit_statements_node(self, &stmts);
        }
        // Also recurse into predicate
        self.visit(&node.predicate());
    }

    fn visit_unless_node(&mut self, node: &UnlessNode) {
        // Reuse the if-node logic but adapted to UnlessNode API.
        if node.statements().is_some() {
            if let Some(stmts) = node.statements() {
                ruby_prism::visit_statements_node(self, &stmts);
            }
            self.visit(&node.predicate());
            if let Some(sub) = node.else_clause() {
                if let Some(stmts) = sub.statements() {
                    ruby_prism::visit_statements_node(self, &stmts);
                }
            }
            return;
        }
        let kw = node.keyword_loc();
        let end_loc = node.end_keyword_loc();
        if let Some(end) = end_loc.as_ref() {
            if self.ctx.same_line(kw.start_offset(), end.start_offset()) {
                // recurse; no offense
                self.visit(&node.predicate());
                if let Some(sub) = node.else_clause() {
                    if let Some(stmts) = sub.statements() {
                        ruby_prism::visit_statements_node(self, &stmts);
                    }
                }
                return;
            }
        }

        let src_range = node.location();
        let offense_start = src_range.start_offset();
        let offense_end = match node.else_clause() {
            Some(else_node) => else_node.location().start_offset(),
            None => src_range.end_offset(),
        };

        let mut emit = true;
        if self.allow_comments {
            let pred_end = node.predicate().location().end_offset();
            let nl = self.ctx.source[pred_end..]
                .find('\n')
                .map(|i| pred_end + i + 1)
                .unwrap_or(pred_end);
            let body_end = match node.else_clause() {
                Some(e) => e.location().start_offset(),
                None => end_loc.as_ref().map(|l| l.start_offset()).unwrap_or(src_range.end_offset()),
            };
            let pred_line_end = self.ctx.source[pred_end..]
                .find('\n')
                .map(|i| pred_end + i)
                .unwrap_or(self.ctx.source.len());
            if self.range_contains_comment(pred_end, pred_line_end)
                || self.range_contains_comment(nl, body_end)
            {
                emit = false;
            }
        }

        if emit {
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/EmptyConditionalBody",
                "Avoid `unless` branches without a body.",
                Severity::Warning,
                offense_start,
                offense_end,
            ));
        }

        self.visit(&node.predicate());
        if let Some(sub) = node.else_clause() {
            if let Some(stmts) = sub.statements() {
                ruby_prism::visit_statements_node(self, &stmts);
            }
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_comments: bool,
}

impl Default for Cfg {
    fn default() -> Self {
        Self { allow_comments: true }
    }
}

crate::register_cop!("Lint/EmptyConditionalBody", |cfg| {
    let c: Cfg = cfg.typed("Lint/EmptyConditionalBody");
    Some(Box::new(EmptyConditionalBody::new(c.allow_comments)))
});
