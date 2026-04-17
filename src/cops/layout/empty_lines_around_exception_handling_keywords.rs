//! Layout/EmptyLinesAroundExceptionHandlingKeywords
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/empty_lines_around_exception_handling_keywords.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::{line_byte_offset, line_end_byte_offset};
use crate::offense::{Correction, Edit, Location, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct EmptyLinesAroundExceptionHandlingKeywords;

impl EmptyLinesAroundExceptionHandlingKeywords {
    pub fn new() -> Self {
        Self
    }
}

fn line_of(source: &str, offset: usize) -> usize {
    1 + source.as_bytes()[..offset].iter().filter(|&&b| b == b'\n').count()
}

fn last_line_of(source: &str, end: usize) -> usize {
    let last_byte = if end > 0 { end - 1 } else { 0 };
    1 + source.as_bytes()[..=last_byte].iter().filter(|&&b| b == b'\n').count()
}

fn line_is_blank(source: &str, line_1idx: usize) -> bool {
    let start = line_byte_offset(source, line_1idx);
    let end = line_end_byte_offset(source, line_1idx);
    let line = &source[start..end];
    line.trim_end_matches('\n').trim_end_matches('\r').is_empty()
}

/// Keyword location captured from a rescue/ensure tree.
struct KeywordLoc {
    line: usize,   // 1-indexed line of the keyword
    keyword: String,
}

/// Collect all rescue/else/ensure keyword locations from a BeginNode.
fn collect_keywords(source: &str, body: &Node<'_>, out: &mut Vec<KeywordLoc>) {
    let Some(begin) = body.as_begin_node() else { return };

    // Rescue chain → each rescue keyword.
    let mut maybe_rescue = begin.rescue_clause();
    while let Some(rescue_node) = maybe_rescue {
        let loc = rescue_node.keyword_loc();
        let start = loc.start_offset();
        let end = loc.end_offset();
        out.push(KeywordLoc {
            line: line_of(source, start),
            keyword: source[start..end].to_string(),
        });
        maybe_rescue = rescue_node.subsequent();
    }

    // Else clause → `else` keyword.
    if let Some(else_node) = begin.else_clause() {
        let loc = else_node.else_keyword_loc();
        let start = loc.start_offset();
        let end = loc.end_offset();
        out.push(KeywordLoc {
            line: line_of(source, start),
            keyword: source[start..end].to_string(),
        });
    }

    // Ensure clause → `ensure` keyword.
    if let Some(ensure_node) = begin.ensure_clause() {
        let loc = ensure_node.ensure_keyword_loc();
        let start = loc.start_offset();
        let end = loc.end_offset();
        out.push(KeywordLoc {
            line: line_of(source, start),
            keyword: source[start..end].to_string(),
        });
    }
}

/// Compute the `end` line of the enclosing construct (def / block / kwbegin).
fn body_and_end_same_line(source: &str, body: &Node<'_>, end_line: usize) -> bool {
    // RuboCop's `last_body_and_end_on_same_line?`:
    //   if body is rescue: last body line = else.line (if has else) else last resbody line.
    //   else (ensure):     last body line = body.last_line.
    //   compare to end_keyword line.
    let Some(begin) = body.as_begin_node() else {
        // Fallback: no BeginNode body. Compare node last line.
        return last_line_of(source, body.location().end_offset()) == end_line;
    };

    if begin.rescue_clause().is_some() {
        if let Some(else_node) = begin.else_clause() {
            let else_line = line_of(source, else_node.else_keyword_loc().start_offset());
            return else_line == end_line;
        }
        // Last rescue clause's keyword line.
        let mut maybe_rescue = begin.rescue_clause();
        let mut last_rescue_line = 0;
        while let Some(r) = maybe_rescue {
            last_rescue_line = line_of(source, r.keyword_loc().start_offset());
            maybe_rescue = r.subsequent();
        }
        return last_rescue_line == end_line;
    }

    // Ensure without rescue: compare body.last_line to end line.
    last_line_of(source, begin.location().end_offset()) == end_line
}

struct Visitor<'a> {
    source: &'a str,
    severity: Severity,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn check_body(&mut self, body: &Node<'_>, enclosing_line: usize, end_line: usize) {
        let mut keywords = Vec::new();
        collect_keywords(self.source, body, &mut keywords);
        if keywords.is_empty() {
            return;
        }
        let same_line = body_and_end_same_line(self.source, body, end_line);

        for kw in keywords {
            if kw.line == enclosing_line || same_line {
                continue;
            }

            // Check line BELOW keyword (kw.line + 1) — "after".
            if self.line_exists(kw.line + 1) && line_is_blank(self.source, kw.line + 1) {
                let line_start = line_byte_offset(self.source, kw.line + 1);
                let line_end = line_end_byte_offset(self.source, kw.line + 1);
                self.push_offense(line_start, line_end, "after", &kw.keyword);
            }

            // Check line ABOVE keyword (kw.line - 1) — "before".
            if kw.line >= 2 && line_is_blank(self.source, kw.line - 1) {
                let target_line = kw.line - 1;
                let line_start = line_byte_offset(self.source, target_line);
                let line_end = line_end_byte_offset(self.source, target_line);
                self.push_offense(line_start, line_end, "before", &kw.keyword);
            }
        }
    }

    fn line_exists(&self, line_1idx: usize) -> bool {
        // Line exists if there are at least line_1idx-1 newlines before EOF,
        // or the file has content on that line.
        let start = line_byte_offset(self.source, line_1idx);
        start < self.source.len()
    }

    fn push_offense(
        &mut self,
        line_start: usize,
        line_end: usize,
        location: &str,
        keyword: &str,
    ) {
        let msg = format!("Extra empty line detected {} the `{}`.", location, keyword);
        let loc = Location::from_offsets(self.source, line_start, line_start);
        // Remove entire blank line including its trailing newline.
        let correction = Correction {
            edits: vec![Edit {
                start_offset: line_start,
                end_offset: line_end,
                replacement: String::new(),
            }],
        };
        self.offenses.push(
            Offense::new(
                "Layout/EmptyLinesAroundExceptionHandlingKeywords",
                &msg,
                self.severity,
                loc,
                self.ctx.filename,
            )
            .with_correction(correction),
        );
    }
}

impl<'a> Visit<'a> for Visitor<'a> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'a>) {
        if let Some(body) = node.body() {
            let def_line = line_of(self.source, node.location().start_offset());
            let end_line = node
                .end_keyword_loc()
                .map(|l| line_of(self.source, l.start_offset()))
                .unwrap_or_else(|| last_line_of(self.source, node.location().end_offset()));
            self.check_body(&body, def_line, end_line);
        }
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'a>) {
        if let Some(body) = node.body() {
            let open_line = line_of(self.source, node.opening_loc().start_offset());
            let end_line = line_of(self.source, node.closing_loc().start_offset());
            self.check_body(&body, open_line, end_line);
        }
        ruby_prism::visit_block_node(self, node);
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode<'a>) {
        // Only fire on `begin ... end` (kwbegin), not implicit rescue wrappers.
        if let Some(begin_kw) = node.begin_keyword_loc() {
            let begin_line = line_of(self.source, begin_kw.start_offset());
            let end_line = node
                .end_keyword_loc()
                .map(|l| line_of(self.source, l.start_offset()))
                .unwrap_or_else(|| last_line_of(self.source, node.location().end_offset()));
            // For kwbegin, the body passed to check_body is the BeginNode itself.
            let self_as_node: Node = node.as_node();
            self.check_body(&self_as_node, begin_line, end_line);
        }
        ruby_prism::visit_begin_node(self, node);
    }
}

impl Cop for EmptyLinesAroundExceptionHandlingKeywords {
    fn name(&self) -> &'static str {
        "Layout/EmptyLinesAroundExceptionHandlingKeywords"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor {
            source: ctx.source,
            severity: self.severity(),
            ctx,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

crate::register_cop!("Layout/EmptyLinesAroundExceptionHandlingKeywords", |_cfg| {
    Some(Box::new(EmptyLinesAroundExceptionHandlingKeywords::new()))
});
