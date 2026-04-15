//! Shared logic for Layout/EmptyLinesAroundClassBody and Layout/EmptyLinesAroundModuleBody.
//!
//! Ported from RuboCop's `Layout::EmptyLinesAroundBody` mixin.

use crate::cops::CheckContext;
use crate::helpers::source::{line_byte_offset, line_end_byte_offset};
use crate::offense::{Correction, Edit, Location, Offense, Severity};
use ruby_prism::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    NoEmptyLines,
    EmptyLines,
    EmptyLinesExceptNamespace,
    EmptyLinesSpecial,
    BeginningOnly,
    EndingOnly,
}

impl Style {
    pub fn parse(s: &str) -> Self {
        match s {
            "empty_lines" => Style::EmptyLines,
            "empty_lines_except_namespace" => Style::EmptyLinesExceptNamespace,
            "empty_lines_special" => Style::EmptyLinesSpecial,
            "beginning_only" => Style::BeginningOnly,
            "ending_only" => Style::EndingOnly,
            _ => Style::NoEmptyLines,
        }
    }
}

/// Runs the EmptyLinesAroundBody check on a class/module/sclass body.
///
/// * `first_line`, `last_line` — 1-indexed line numbers of the class/module keyword line
///   (or the last line of a multiline superclass, via `adjusted_first_line`) and the `end` line.
/// * `body` — Optional body node (StatementsNode or single expression).
/// * `kind` — "class" or "module" (used in messages).
pub fn check(
    cop_name: &'static str,
    severity: Severity,
    kind: &str,
    style: Style,
    first_line: usize,
    last_line: usize,
    body: Option<&Node<'_>>,
    source: &str,
    ctx: &CheckContext,
) -> Vec<Offense> {
    // valid_body_style?: body empty and style is not no_empty_lines → skip
    if body.is_none() && style != Style::NoEmptyLines {
        return Vec::new();
    }
    if first_line == last_line {
        return Vec::new();
    }

    let lines: Vec<&str> = source.split('\n').collect();
    let mut offenses = Vec::new();

    match style {
        Style::EmptyLinesExceptNamespace => {
            if let Some(b) = body {
                if is_namespace(b, true) {
                    check_both(cop_name, severity, kind, Style::NoEmptyLines, first_line, last_line, &lines, source, ctx, &mut offenses);
                } else {
                    check_both(cop_name, severity, kind, Style::EmptyLines, first_line, last_line, &lines, source, ctx, &mut offenses);
                }
            } else {
                check_both(cop_name, severity, kind, Style::EmptyLines, first_line, last_line, &lines, source, ctx, &mut offenses);
            }
        }
        Style::EmptyLinesSpecial => {
            let Some(b) = body else { return offenses };
            if is_namespace(b, true) {
                check_both(cop_name, severity, kind, Style::NoEmptyLines, first_line, last_line, &lines, source, ctx, &mut offenses);
            } else {
                if first_child_requires_empty_line(b) {
                    check_beginning(cop_name, severity, kind, Style::EmptyLines, first_line, &lines, source, ctx, &mut offenses);
                } else {
                    check_beginning(cop_name, severity, kind, Style::NoEmptyLines, first_line, &lines, source, ctx, &mut offenses);
                    check_deferred_empty_line(cop_name, severity, b, &lines, source, ctx, &mut offenses);
                }
                check_ending(cop_name, severity, kind, Style::EmptyLines, last_line, &lines, source, ctx, &mut offenses);
            }
        }
        _ => {
            check_both(cop_name, severity, kind, style, first_line, last_line, &lines, source, ctx, &mut offenses);
        }
    }

    offenses
}

fn check_both(
    cop_name: &'static str,
    severity: Severity,
    kind: &str,
    style: Style,
    first_line: usize,
    last_line: usize,
    lines: &[&str],
    source: &str,
    ctx: &CheckContext,
    offenses: &mut Vec<Offense>,
) {
    let before = offenses.len();
    match style {
        Style::BeginningOnly => {
            check_beginning(cop_name, severity, kind, Style::EmptyLines, first_line, lines, source, ctx, offenses);
            check_ending(cop_name, severity, kind, Style::NoEmptyLines, last_line, lines, source, ctx, offenses);
        }
        Style::EndingOnly => {
            check_beginning(cop_name, severity, kind, Style::NoEmptyLines, first_line, lines, source, ctx, offenses);
            check_ending(cop_name, severity, kind, Style::EmptyLines, last_line, lines, source, ctx, offenses);
        }
        _ => {
            check_beginning(cop_name, severity, kind, style, first_line, lines, source, ctx, offenses);
            let after_begin = offenses.len();
            check_ending(cop_name, severity, kind, style, last_line, lines, source, ctx, offenses);
            // Deduplicate: if ending produced an offense at the same line as beginning, drop it.
            if after_begin > before && offenses.len() > after_begin {
                let begin_line = offenses[after_begin - 1].location.line;
                let end_line = offenses[after_begin].location.line;
                if begin_line == end_line {
                    offenses.remove(after_begin);
                }
            }
        }
    }
}

fn check_beginning(
    cop_name: &'static str,
    severity: Severity,
    kind: &str,
    style: Style,
    first_line: usize,
    lines: &[&str],
    source: &str,
    ctx: &CheckContext,
    offenses: &mut Vec<Offense>,
) {
    check_source(cop_name, severity, kind, style, first_line, "beginning", lines, source, ctx, offenses);
}

fn check_ending(
    cop_name: &'static str,
    severity: Severity,
    kind: &str,
    style: Style,
    last_line: usize,
    lines: &[&str],
    source: &str,
    ctx: &CheckContext,
    offenses: &mut Vec<Offense>,
) {
    // Pass line_no = last_line - 2 (0-indexed into lines, following RuboCop).
    if last_line < 2 {
        return;
    }
    check_source(cop_name, severity, kind, style, last_line - 2, "end", lines, source, ctx, offenses);
}

fn check_source(
    cop_name: &'static str,
    severity: Severity,
    kind: &str,
    style: Style,
    line_no: usize,  // 0-indexed index into `lines`
    desc: &str,
    lines: &[&str],
    source: &str,
    ctx: &CheckContext,
    offenses: &mut Vec<Offense>,
) {
    let Some(line) = lines.get(line_no) else { return };
    let line_trim = line.trim_end_matches('\r'); // handle CRLF

    let (is_offense, msg_template) = match style {
        Style::NoEmptyLines => (line_trim.is_empty(), "Extra empty line detected at {kind} body {desc}."),
        Style::EmptyLines => (!line_trim.is_empty(), "Empty line missing at {kind} body {desc}."),
        _ => return,
    };
    if !is_offense {
        return;
    }

    let msg = msg_template.replace("{kind}", kind).replace("{desc}", desc);

    // offset = 2 if empty_lines AND desc is "end" (msg includes "end."), else 1
    let offset = if matches!(style, Style::EmptyLines) && desc == "end" { 2 } else { 1 };
    let target_line_1indexed = line_no + offset; // line index (1-indexed) of the reported source_range
    // source_range(buffer, line, 0) — zero-width at col 0 of that line.

    let byte_offset = line_byte_offset(source, target_line_1indexed);
    let loc = Location::from_offsets(source, byte_offset, byte_offset);

    let mut offense = Offense::new(cop_name, &msg, severity, loc, ctx.filename);

    // Corrections: EmptyLineCorrector.correct(corrector, [style, range])
    // For no_empty_lines: remove the blank line. For empty_lines: insert newline.
    let correction = match style {
        Style::NoEmptyLines => {
            // Remove the entire blank line (including its trailing newline).
            let start = line_byte_offset(source, target_line_1indexed);
            let end = line_end_byte_offset(source, target_line_1indexed);
            // Consume the trailing newline too, if present.
            let end_with_nl = if end < source.len() && source.as_bytes()[end] == b'\n' { end + 1 } else { end };
            Some(Correction { edits: vec![Edit { start_offset: start, end_offset: end_with_nl, replacement: String::new() }] })
        }
        Style::EmptyLines => {
            // Insert a newline at column 0 of target_line.
            Some(Correction { edits: vec![Edit { start_offset: byte_offset, end_offset: byte_offset, replacement: "\n".to_string() }] })
        }
        _ => None,
    };
    if let Some(c) = correction {
        offense = offense.with_correction(c);
    }
    offenses.push(offense);
}

fn check_deferred_empty_line(
    cop_name: &'static str,
    severity: Severity,
    body: &Node<'_>,
    lines: &[&str],
    source: &str,
    ctx: &CheckContext,
    offenses: &mut Vec<Offense>,
) {
    let Some(node) = first_empty_line_required_child(body) else { return };
    let node_first_line = node.location().start_offset();
    let first_line_1idx = 1 + source.as_bytes()[..node_first_line].iter().filter(|&&b| b == b'\n').count();

    let prev = previous_line_ignoring_comments(first_line_1idx, lines);
    // prev is 0-indexed (like Ruby); check if lines[prev].empty → no offense.
    if lines.get(prev).map_or(true, |l| l.trim_end_matches('\r').is_empty()) {
        return;
    }
    let target_line = prev + 2; // 1-indexed reported line
    let byte_offset = line_byte_offset(source, target_line);
    let loc = Location::from_offsets(source, byte_offset, byte_offset);

    let node_type = node_type_name(&node);
    let msg = format!("Empty line missing before first {} definition", node_type);
    let correction = Correction { edits: vec![Edit { start_offset: byte_offset, end_offset: byte_offset, replacement: "\n".to_string() }] };
    offenses.push(
        Offense::new(cop_name, &msg, severity, loc, ctx.filename).with_correction(correction),
    );
}

/// First child of body that is any_def/class/module/access-modifier-send.
fn first_empty_line_required_child<'a>(body: &'a Node<'a>) -> Option<Node<'a>> {
    if let Some(stmts) = body.as_statements_node() {
        let children: Vec<Node> = stmts.body().iter().collect();
        if children.len() > 1 {
            for child in children {
                if empty_line_required(&child) {
                    return Some(child);
                }
            }
            return None;
        } else if let Some(only) = children.into_iter().next() {
            if empty_line_required(&only) {
                return Some(only);
            }
            return None;
        }
        None
    } else if empty_line_required(body) {
        // Not a StatementsNode, rare. Can't clone; caller uses `first_line` only.
        // Return None here; callers won't hit this path for prism (body is always Statements).
        None
    } else {
        None
    }
}

fn empty_line_required(node: &Node<'_>) -> bool {
    if node.as_def_node().is_some()
        || node.as_class_node().is_some()
        || node.as_module_node().is_some()
        || node.as_singleton_class_node().is_some()
    {
        return true;
    }
    if let Some(call) = node.as_call_node() {
        if call.receiver().is_none()
            && call.block().is_none()
            && call.arguments().map_or(true, |a| a.arguments().is_empty())
        {
            let name = String::from_utf8_lossy(call.name().as_slice()).to_string();
            return matches!(name.as_str(), "private" | "protected" | "public");
        }
    }
    false
}

fn node_type_name(node: &Node<'_>) -> &'static str {
    if node.as_def_node().is_some() {
        "def"
    } else if node.as_class_node().is_some() {
        "class"
    } else if node.as_module_node().is_some() {
        "module"
    } else if node.as_singleton_class_node().is_some() {
        "sclass"
    } else if node.as_call_node().is_some() {
        "send"
    } else {
        "node"
    }
}

fn previous_line_ignoring_comments(send_line_1idx: usize, lines: &[&str]) -> usize {
    // Start at send_line - 2 (0-indexed = 1-indexed send_line - 2). Iterate down to 0.
    if send_line_1idx < 2 {
        return 0;
    }
    let mut idx = (send_line_1idx as isize) - 2;
    while idx >= 0 {
        let line = lines.get(idx as usize).map(|l| l.trim_start()).unwrap_or("");
        if line.starts_with('#') {
            idx -= 1;
            continue;
        }
        return idx as usize;
    }
    0
}

/// RuboCop: namespace?(body, with_one_child: true)
/// If body is begin-type (multi-statement): false if `with_one_child`, else all children are class/module.
/// Else: body itself is class/module.
fn is_namespace(body: &Node<'_>, with_one_child: bool) -> bool {
    if let Some(stmts) = body.as_statements_node() {
        let count = stmts.body().iter().count();
        if count > 1 {
            if with_one_child {
                return false;
            }
            return stmts.body().iter().all(|c| constant_definition(&c));
        } else if count == 1 {
            if let Some(only) = stmts.body().iter().next() {
                return constant_definition(&only);
            }
        }
        false
    } else {
        constant_definition(body)
    }
}

fn first_child_requires_empty_line(body: &Node<'_>) -> bool {
    if let Some(stmts) = body.as_statements_node() {
        if let Some(first) = stmts.body().iter().next() {
            return empty_line_required(&first);
        }
        false
    } else {
        empty_line_required(body)
    }
}

fn constant_definition(node: &Node<'_>) -> bool {
    node.as_class_node().is_some() || node.as_module_node().is_some()
}
