//! Lint/EmptyConditionalBody - flag empty `if`/`elsif`/`unless` branches.
//!
//! Ported from:
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/empty_conditional_body.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
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

    /// Compute indentation (leading spaces/tabs) for offset `off` in source.
    fn indent_at(&self, off: usize) -> &str {
        let src = self.ctx.source;
        // find line start
        let line_start = src[..off].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let from = &src[line_start..];
        let indent_len = from.bytes().take_while(|&b| b == b' ' || b == b'\t').count();
        &src[line_start..line_start + indent_len]
    }

    /// Build correction for `if PRED ... else BODY end` → `unless PRED\n  BODY\nend`
    /// or `unless PRED ... else BODY end` → `if PRED\n  BODY\nend`.
    fn make_inversion_correction_if(
        &self,
        node: &IfNode,
        opposite_kw: &str,
    ) -> Option<Correction> {
        // Ensure subsequent is ElseNode with statements
        let else_node = node.subsequent()?;
        let else_n = else_node.as_else_node()?;
        let stmts = else_n.statements()?;
        let src = self.ctx.source;
        let indent = self.indent_at(node.location().start_offset()).to_string();
        let pred_src = &src[node.predicate().location().start_offset()..node.predicate().location().end_offset()];
        let body_src = &src[stmts.location().start_offset()..stmts.location().end_offset()];
        // Reindent body: body may already be indented by 2 from its position; we want `indent + "  "`
        let body_indent = format!("{}  ", indent);
        // Re-indent each line of body_src to body_indent
        let reindented = reindent_body(body_src, &body_indent);
        let replacement = format!("{} {}\n{}\n{}end", opposite_kw, pred_src, reindented, indent);
        let node_loc = node.location();
        // end of whole node
        Some(Correction::replace(node_loc.start_offset(), node_loc.end_offset(), replacement))
    }

    fn make_inversion_correction_unless(
        &self,
        node: &UnlessNode,
        opposite_kw: &str,
    ) -> Option<Correction> {
        let else_node = node.else_clause()?;
        let stmts = else_node.statements()?;
        let src = self.ctx.source;
        let indent = self.indent_at(node.location().start_offset()).to_string();
        let pred_src = &src[node.predicate().location().start_offset()..node.predicate().location().end_offset()];
        let body_src = &src[stmts.location().start_offset()..stmts.location().end_offset()];
        let body_indent = format!("{}  ", indent);
        let reindented = reindent_body(body_src, &body_indent);
        let replacement = format!("{} {}\n{}\n{}end", opposite_kw, pred_src, reindented, indent);
        let node_loc = node.location();
        Some(Correction::replace(node_loc.start_offset(), node_loc.end_offset(), replacement))
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

        // Build correction: invert to opposite keyword if there's an else clause with body
        // Only for top-level if/unless (not elsif chains)
        let correction = if keyword != "elsif" {
            let opposite = if keyword == "if" { "unless" } else { "if" };
            self.make_inversion_correction_if(node, opposite)
        } else {
            None
        };
        let offense = self.ctx.offense_with_range(
            "Lint/EmptyConditionalBody",
            &format!("Avoid `{}` branches without a body.", keyword),
            Severity::Warning,
            offense_start,
            offense_end,
        );
        self.offenses.push(if let Some(c) = correction { offense.with_correction(c) } else { offense });
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
            let correction = self.make_inversion_correction_unless(node, "if");
            let offense = self.ctx.offense_with_range(
                "Lint/EmptyConditionalBody",
                "Avoid `unless` branches without a body.",
                Severity::Warning,
                offense_start,
                offense_end,
            );
            self.offenses.push(if let Some(c) = correction { offense.with_correction(c) } else { offense });
        }

        self.visit(&node.predicate());
        if let Some(sub) = node.else_clause() {
            if let Some(stmts) = sub.statements() {
                ruby_prism::visit_statements_node(self, &stmts);
            }
        }
    }
}

/// Re-indent body lines to `new_indent`. Detects current indent from first non-empty line.
fn reindent_body(body: &str, new_indent: &str) -> String {
    let lines: Vec<&str> = body.lines().collect();
    // Find current indent from first non-empty line
    let current_indent_len = lines.iter()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.bytes().take_while(|&b| b == b' ' || b == b'\t').count())
        .unwrap_or(0);
    lines.iter().map(|line| {
        if line.trim().is_empty() {
            line.to_string()
        } else {
            let stripped = if line.len() >= current_indent_len { &line[current_indent_len..] } else { line };
            format!("{}{}", new_indent, stripped)
        }
    }).collect::<Vec<_>>().join("\n")
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
