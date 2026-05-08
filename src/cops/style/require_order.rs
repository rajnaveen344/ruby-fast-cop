//! Style/RequireOrder - Sort `require`/`require_relative` alphabetically within sections.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/require_order.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::line_start_offset;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Sort `%s` in alphabetical order.";

#[derive(Default)]
pub struct RequireOrder;

impl RequireOrder {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for RequireOrder {
    fn name(&self) -> &'static str {
        "Style/RequireOrder"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = ReqOrderVisitor {
            ctx,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct ReqOrderVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

/// A require call extracted from a sibling list, with its enclosing node.
struct RequireInfo {
    /// The string literal value (first arg).
    name: String,
    /// The method name (`require` or `require_relative`).
    method: String,
    /// Start offset of the call node (for offense range)
    send_start: usize,
    send_end: usize,
    /// Start of entire enclosing line range including preceding comments.
    range_start: usize,
    /// End of entire enclosing line (after newline).
    range_end: usize,
}

impl<'a> ReqOrderVisitor<'a> {
    /// Walk a list of sibling statements, looking for require calls and
    /// flagging unsorted ones.
    fn check_siblings<'pr>(&mut self, siblings: Vec<Node<'pr>>) {
        // Build list of require infos in declaration order, with `None` for
        // siblings that aren't requires.
        let infos: Vec<Option<RequireInfo>> = siblings
            .iter()
            .map(|n| Self::extract_require(n, self.ctx))
            .collect();

        // Identify contiguous groups of same-method requires in the same section.
        // For each group that is out of order, emit offenses (existing logic) +
        // a single sort-block correction on the FIRST offense in the group.
        let n = infos.len();
        let mut i = 0;
        while i < n {
            if infos[i].is_none() {
                i += 1;
                continue;
            }
            // Start of a require group at index i.
            let group_method = infos[i].as_ref().unwrap().method.clone();
            // Extend group while same method and same section.
            let mut j = i + 1;
            while j < n {
                match &infos[j] {
                    None => break,
                    Some(next) => {
                        if next.method != group_method {
                            break;
                        }
                        // Same section: no blank line between prev enclosing_start and curr end.
                        let prev = infos[j - 1].as_ref().unwrap();
                        if !Self::in_same_section(self.ctx, prev.range_start, next.range_end) {
                            break;
                        }
                        j += 1;
                    }
                }
            }
            // Group spans infos[i..j].
            let group: Vec<&RequireInfo> = infos[i..j].iter()
                .filter_map(|x| x.as_ref())
                .collect();

            // Check if group is sorted.
            let names: Vec<&str> = group.iter().map(|r| r.name.as_str()).collect();
            let mut sorted_names = names.clone();
            sorted_names.sort_unstable();

            if names != sorted_names {
                // Find out-of-order elements and emit offenses.
                // Also build a sort-block correction.
                let mut first_offense_idx: Option<usize> = None;

                // For offense detection: same as before — find element where name < any prev.
                for k in 0..group.len() {
                    let curr = group[k];
                    let mut is_ooo = false;
                    for l in (0..k).rev() {
                        let prev = group[l];
                        if prev.method != curr.method {
                            break;
                        }
                        if !Self::in_same_section(self.ctx, prev.range_start, curr.range_end) {
                            break;
                        }
                        if curr.name < prev.name {
                            is_ooo = true;
                            break;
                        } else {
                            continue;
                        }
                    }
                    if is_ooo {
                        let msg = MSG.replace("%s", &curr.method);
                        let offense = self.ctx.offense_with_range(
                            "Style/RequireOrder",
                            &msg,
                            Severity::Convention,
                            curr.send_start,
                            curr.send_end,
                        );
                        if first_offense_idx.is_none() {
                            first_offense_idx = Some(self.offenses.len());
                            self.offenses.push(offense);
                        } else {
                            self.offenses.push(offense);
                        }
                    }
                }

                // Attach sort-block correction to first offense.
                if let Some(first_idx) = first_offense_idx {
                    if let Some(correction) = self.build_sort_correction(&group) {
                        let offense = self.offenses.remove(first_idx);
                        self.offenses.insert(first_idx, offense.with_correction(correction));
                    }
                }
            }

            i = j;
        }
    }

    /// Build a correction that sorts the entire require group by replacing each
    /// line's source text with the sorted version.
    fn build_sort_correction(&self, group: &[&RequireInfo]) -> Option<Correction> {
        if group.is_empty() {
            return None;
        }
        // Collect source text for each element (including preceding comments).
        let sources: Vec<&str> = group.iter()
            .map(|r| &self.ctx.source[r.range_start..r.range_end])
            .collect();

        // Sort by require name.
        let mut indexed: Vec<(usize, &&RequireInfo)> = group.iter().enumerate().collect();
        indexed.sort_by(|(_, a), (_, b)| a.name.cmp(&b.name));

        // Check if already sorted (shouldn't happen since we only call this when unsorted).
        let sorted_sources: Vec<&str> = indexed.iter().map(|(orig_i, _)| sources[*orig_i]).collect();

        // Build edits: replace each element's range with the sorted element's text.
        // We're replacing in-place: element k's range gets sorted element k's text.
        let edits: Vec<Edit> = group.iter().enumerate()
            .zip(sorted_sources.iter())
            .filter(|((k, elem), sorted_src)| {
                sources[*k] != **sorted_src
            })
            .map(|((k, elem), sorted_src)| Edit {
                start_offset: elem.range_start,
                end_offset: elem.range_end,
                replacement: sorted_src.to_string(),
            })
            .collect();

        if edits.is_empty() {
            None
        } else {
            Some(Correction { edits })
        }
    }

    /// If `n` is a `require`/`require_relative` send (with single string arg
    /// and no receiver), or a modifier-if wrapping one, return its info.
    fn extract_require<'pr>(n: &Node<'pr>, ctx: &CheckContext) -> Option<RequireInfo> {
        // Direct send
        if let Some(call) = n.as_call_node() {
            return Self::call_to_info(
                &call,
                n.location().start_offset(),
                n.location().end_offset(),
                ctx,
            );
        }
        // Modifier-if wrapping a send
        if let Some(if_node) = n.as_if_node() {
            if !is_modifier_if(&if_node) {
                return None;
            }
            // Modifier-if body = the require call
            let stmts = if_node.statements()?;
            let body: Vec<_> = stmts.body().iter().collect();
            if body.len() != 1 {
                return None;
            }
            if let Some(call) = body[0].as_call_node() {
                return Self::call_to_info(
                    &call,
                    n.location().start_offset(),
                    n.location().end_offset(),
                    ctx,
                );
            }
        }
        // Modifier-unless wrapping a send
        if let Some(un_node) = n.as_unless_node() {
            if !is_modifier_unless(&un_node) {
                return None;
            }
            let stmts = un_node.statements()?;
            let body: Vec<_> = stmts.body().iter().collect();
            if body.len() != 1 {
                return None;
            }
            if let Some(call) = body[0].as_call_node() {
                return Self::call_to_info(
                    &call,
                    n.location().start_offset(),
                    n.location().end_offset(),
                    ctx,
                );
            }
        }
        None
    }

    fn call_to_info<'pr>(
        call: &ruby_prism::CallNode<'pr>,
        enclosing_start: usize,
        enclosing_end: usize,
        ctx: &CheckContext,
    ) -> Option<RequireInfo> {
        // Receiver must be nil
        if call.receiver().is_some() {
            return None;
        }
        let method = String::from_utf8_lossy(call.name().as_slice()).to_string();
        if method != "require" && method != "require_relative" {
            return None;
        }
        // First argument must be a string literal
        let args = call.arguments()?;
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return None;
        }
        let str_node = arg_list[0].as_string_node()?;
        let name = String::from_utf8_lossy(str_node.unescaped()).to_string();

        let send_start = call.location().start_offset();
        let send_end = call.location().end_offset();

        // Compute range including preceding comment lines and full enclosing line.
        let range_start = preceding_comments_start(ctx.source, enclosing_start);
        let range_end = line_end_with_newline(ctx.source, enclosing_end);

        Some(RequireInfo {
            name,
            method,
            send_start,
            send_end,
            range_start,
            range_end,
        })
    }

    /// Match RuboCop's `in_same_section?`: source between sibling start and node
    /// end contains no blank line (`\n\n`).
    fn in_same_section(ctx: &CheckContext, prev_start: usize, curr_end: usize) -> bool {
        if prev_start >= curr_end {
            return false;
        }
        !ctx.source[prev_start..curr_end].contains("\n\n")
    }
}

/// Find the start of preceding comment lines (including blank lines? No — stop at blank line).
/// Walk backwards from the line above `node_start`, collecting comment-only lines.
fn preceding_comments_start(source: &str, node_start: usize) -> usize {
    let source_bytes = source.as_bytes();
    let node_line_start = line_start_offset(source, node_start);
    if node_line_start == 0 {
        return node_line_start;
    }
    // Walk backwards one line at a time.
    let mut cursor = node_line_start;
    loop {
        if cursor == 0 {
            break;
        }
        // End of previous line = cursor - 1 (the '\n').
        let prev_line_end = cursor - 1; // byte position of '\n' ending previous line
        let prev_line_start = line_start_offset(source, prev_line_end);
        let prev_line = &source[prev_line_start..prev_line_end];
        let trimmed = prev_line.trim();
        if trimmed.starts_with('#') {
            cursor = prev_line_start;
        } else {
            break;
        }
    }
    cursor
}

/// End of the line containing `offset`, including the trailing newline.
fn line_end_with_newline(source: &str, offset: usize) -> usize {
    let bytes = source.as_bytes();
    let mut pos = offset;
    while pos < bytes.len() && bytes[pos] != b'\n' {
        pos += 1;
    }
    if pos < bytes.len() {
        pos + 1 // include '\n'
    } else {
        pos
    }
}

fn is_modifier_if(node: &ruby_prism::IfNode) -> bool {
    if let (Some(kw_loc), Some(stmts)) = (node.if_keyword_loc(), node.statements()) {
        return kw_loc.start_offset() > stmts.location().start_offset();
    }
    false
}

fn is_modifier_unless(node: &ruby_prism::UnlessNode) -> bool {
    if let Some(stmts) = node.statements() {
        let kw_start = node.keyword_loc().start_offset();
        return kw_start > stmts.location().start_offset();
    }
    false
}

impl<'pr, 'a> Visit<'pr> for ReqOrderVisitor<'a> {
    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode<'pr>) {
        let body: Vec<Node<'pr>> = node.body().iter().collect();
        self.check_siblings(body);
        ruby_prism::visit_statements_node(self, node);
    }
}

crate::register_cop!("Style/RequireOrder", |_cfg| Some(Box::new(RequireOrder::new())));
