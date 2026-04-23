//! Bundler/OrderedGems - gems sorted alphabetically inside each section.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/bundler/ordered_gems.rb
//! Mixin: lib/rubocop/cop/mixin/ordered_gem_node.rb
//! Corrector: lib/rubocop/cop/correctors/ordered_gem_corrector.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{StatementsNode, Visit};

pub struct OrderedGems {
    consider_punctuation: bool,
    treat_comments_as_separators: bool,
}

impl OrderedGems {
    pub fn new(consider_punctuation: bool, treat_comments_as_separators: bool) -> Self {
        Self { consider_punctuation, treat_comments_as_separators }
    }

    fn canonical(&self, name: &str) -> String {
        let stripped: String = if self.consider_punctuation {
            name.to_string()
        } else {
            name.chars().filter(|c| *c != '-' && *c != '_').collect()
        };
        stripped.to_lowercase()
    }
}

impl Default for OrderedGems {
    fn default() -> Self { Self::new(false, false) }
}

struct GemDecl {
    name: String,
    line_start: usize,
    line_end: usize,
    call_start: usize,
    call_end: usize,
}

fn gem_name_from_call(call: &ruby_prism::CallNode) -> Option<String> {
    if call.receiver().is_some() { return None; }
    let name = node_name!(call);
    if name != "gem" { return None; }
    let args = call.arguments()?;
    let first = args.arguments().iter().next()?;
    let s = first.as_string_node()?;
    Some(String::from_utf8_lossy(s.unescaped()).to_string())
}

fn line_of(source: &str, offset: usize) -> usize {
    1 + source[..offset].bytes().filter(|&b| b == b'\n').count()
}

fn collect_section_gems<'a>(
    stmts: &StatementsNode<'a>,
    source: &str,
) -> Vec<Option<GemDecl>> {
    let mut out: Vec<Option<GemDecl>> = Vec::new();
    for child in stmts.body().iter() {
        if let Some(call) = child.as_call_node() {
            if let Some(name) = gem_name_from_call(&call) {
                let loc = call.location();
                let line_start = line_of(source, loc.start_offset());
                let line_end = line_of(source, loc.end_offset().saturating_sub(1));
                let args = call.arguments().unwrap();
                let first = args.arguments().iter().next().unwrap();
                let first_loc = first.location();
                out.push(Some(GemDecl {
                    name,
                    line_start,
                    line_end,
                    call_start: loc.start_offset(),
                    call_end: first_loc.end_offset(),
                }));
                continue;
            }
        }
        out.push(None);
    }
    out
}

/// Line of a standalone leading-comment block immediately above `line` (or None).
fn leading_comment_line(line: usize, comments: &[(usize, usize, usize)], source: &str) -> Option<usize> {
    let mut target = line;
    let mut first: Option<usize> = None;
    loop {
        let prev = target.checked_sub(1)?;
        let standalone = comments.iter().any(|(l, start, _)| {
            *l == prev && is_standalone_comment(source, *start)
        });
        if standalone {
            first = Some(prev);
            target = prev;
        } else {
            break;
        }
    }
    first
}

fn is_standalone_comment(source: &str, start: usize) -> bool {
    let line_begin = source[..start].rfind('\n').map_or(0, |p| p + 1);
    source[line_begin..start].bytes().all(|b| b == b' ' || b == b'\t')
}

fn line_start_offset(source: &str, line: usize) -> usize {
    if line == 1 { return 0; }
    let mut count = 1;
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            count += 1;
            if count == line { return i + 1; }
        }
    }
    source.len()
}

fn line_end_offset_with_newline(source: &str, line: usize) -> usize {
    let mut count = 1;
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            if count == line { return i + 1; }
            count += 1;
        }
    }
    source.len()
}

/// Byte range [start, end) covering the decl plus leading standalone comments
/// (when not treating comments as separators), snapped to whole lines with
/// trailing newline included.
fn declaration_range(
    source: &str,
    decl: &GemDecl,
    comments: &[(usize, usize, usize)],
    treat_comments_as_separators: bool,
) -> (usize, usize) {
    let begin_line = if !treat_comments_as_separators {
        leading_comment_line(decl.line_start, comments, source).unwrap_or(decl.line_start)
    } else {
        decl.line_start
    };
    (line_start_offset(source, begin_line), line_end_offset_with_newline(source, decl.line_end))
}

impl OrderedGems {
    fn check_section(
        &self,
        decls: &[Option<GemDecl>],
        source: &str,
        comments: &[(usize, usize, usize)],
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
    ) {
        // Partition `decls` into consecutive-line runs. Breaks happen on:
        //  - `None` entries (non-gem statements); and
        //  - non-consecutive line gaps, using a comment-aware line for `cur`
        //    when TreatCommentsAsGroupSeparators is false.
        let mut run: Vec<&GemDecl> = Vec::new();
        for entry in decls {
            match entry {
                None => {
                    self.emit_offenses_for_run(&run, source, comments, ctx, offenses);
                    run.clear();
                }
                Some(cur) => {
                    let break_here = if let Some(prev) = run.last() {
                        let cur_first_line = if !self.treat_comments_as_separators {
                            leading_comment_line(cur.line_start, comments, source).unwrap_or(cur.line_start)
                        } else {
                            cur.line_start
                        };
                        prev.line_end + 1 != cur_first_line
                    } else {
                        false
                    };
                    if break_here {
                        self.emit_offenses_for_run(&run, source, comments, ctx, offenses);
                        run.clear();
                    }
                    run.push(cur);
                }
            }
        }
        self.emit_offenses_for_run(&run, source, comments, ctx, offenses);
    }

    fn emit_offenses_for_run(
        &self,
        run: &[&GemDecl],
        source: &str,
        comments: &[(usize, usize, usize)],
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
    ) {
        if run.len() < 2 { return; }
        let mut offense_pairs: Vec<usize> = Vec::new(); // index of `cur` in run
        for i in 1..run.len() {
            if self.canonical(&run[i].name) < self.canonical(&run[i - 1].name) {
                offense_pairs.push(i);
            }
        }
        if offense_pairs.is_empty() { return; }

        let correction = build_full_sort_correction(self, run, source, comments);

        for (rank, cur_idx) in offense_pairs.iter().enumerate() {
            let prev = run[cur_idx - 1];
            let cur = run[*cur_idx];
            let msg = format!(
                "Gems should be sorted in an alphabetical order within their section of the Gemfile. Gem `{}` should appear before `{}`.",
                cur.name, prev.name
            );
            let mut off = ctx.offense_with_range(
                self.name(),
                &msg,
                self.severity(),
                cur.call_start,
                cur.call_end,
            );
            // Only the first offense carries the full-run sort correction so
            // the single-pass `apply_corrections` reaches the final sorted state.
            if rank == 0 {
                if let Some(ref c) = correction { off = off.with_correction(c.clone()); }
            }
            offenses.push(off);
        }
    }
}

fn build_full_sort_correction(
    cop: &OrderedGems,
    run: &[&GemDecl],
    source: &str,
    comments: &[(usize, usize, usize)],
) -> Option<Correction> {
    let ranges: Vec<(usize, usize)> = run
        .iter()
        .map(|d| declaration_range(source, d, comments, cop.treat_comments_as_separators))
        .collect();

    let mut order: Vec<usize> = (0..run.len()).collect();
    order.sort_by(|a, b| cop.canonical(&run[*a].name).cmp(&cop.canonical(&run[*b].name)));

    if order.iter().enumerate().all(|(i, v)| i == *v) { return None; }

    let texts: Vec<String> = ranges.iter().map(|(s, e)| source[*s..*e].to_string()).collect();

    let mut edits = Vec::new();
    for (i, (s, e)) in ranges.iter().enumerate() {
        let replacement = texts[order[i]].clone();
        if source[*s..*e] != replacement {
            edits.push(Edit {
                start_offset: *s,
                end_offset: *e,
                replacement,
            });
        }
    }
    if edits.is_empty() { return None; }
    Some(Correction { edits })
}

impl Cop for OrderedGems {
    fn name(&self) -> &'static str { "Bundler/OrderedGems" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let parsed = ruby_prism::parse(ctx.source.as_bytes());
        let mut comments: Vec<(usize, usize, usize)> = Vec::new();
        for c in parsed.comments() {
            let loc = c.location();
            comments.push((line_of(ctx.source, loc.start_offset()), loc.start_offset(), loc.end_offset()));
        }

        let stmts = node.statements();
        let decls = collect_section_gems(&stmts, ctx.source);
        self.check_section(&decls, ctx.source, &comments, ctx, &mut offenses);

        let mut visitor = SectionVisitor {
            cop: self,
            source: ctx.source,
            comments: &comments,
            ctx,
            offenses: &mut offenses,
        };
        visitor.visit(&parsed.node());

        offenses.sort_by_key(|o| (o.location.line, o.location.column));
        offenses
    }
}

struct SectionVisitor<'a, 'b> {
    cop: &'a OrderedGems,
    source: &'a str,
    comments: &'a [(usize, usize, usize)],
    ctx: &'a CheckContext<'b>,
    offenses: &'a mut Vec<Offense>,
}

impl<'a, 'b> Visit<'_> for SectionVisitor<'a, 'b> {
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        if let Some(body) = node.body() {
            if let Some(stmts) = body.as_statements_node() {
                let decls = collect_section_gems(&stmts, self.source);
                self.cop.check_section(&decls, self.source, self.comments, self.ctx, self.offenses);
            }
        }
        ruby_prism::visit_block_node(self, node);
    }
}

crate::register_cop!("Bundler/OrderedGems", |cfg| {
    let c = cfg.get_cop_config("Bundler/OrderedGems");
    let consider_punctuation = c
        .and_then(|c| c.raw.get("ConsiderPunctuation"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let treat_comments = c
        .and_then(|c| c.raw.get("TreatCommentsAsGroupSeparators"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    Some(Box::new(OrderedGems::new(consider_punctuation, treat_comments)))
});
