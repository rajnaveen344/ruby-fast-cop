//! Lint/UselessTimes - Checks for `Integer#times` that will never yield or yields only once.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct UselessTimes;

impl UselessTimes {
    pub fn new() -> Self { Self }
}

impl Cop for UselessTimes {
    fn name(&self) -> &'static str { "Lint/UselessTimes" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn check_call(&mut self, call: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(call.name().as_slice());
        if method.as_ref() != "times" {
            return;
        }
        let recv = match call.receiver() {
            Some(r) => r,
            None => return,
        };

        let count = match extract_integer_value(&recv, self.ctx.source) {
            Some(v) => v,
            None => return,
        };

        if count > 1 {
            return;
        }

        // The offense range covers the call + block
        let loc = call.location();
        let start = loc.start_offset();
        let end = loc.end_offset();

        let msg = format!("Useless call to `{}.times` detected.", count);

        let correction = self.build_correction(call, count);

        let mut offense = self.ctx.offense_with_range(
            "Lint/UselessTimes",
            &msg,
            Severity::Warning,
            start,
            end,
        );
        offense.correction = correction;
        self.offenses.push(offense);
    }

    fn build_correction(&self, call: &ruby_prism::CallNode, count: i64) -> Option<Correction> {
        let source = self.ctx.source;
        let loc = call.location();
        let start = loc.start_offset();
        let end = loc.end_offset();

        // Check if own_line: nothing but whitespace before the call on the same line
        let line_start = source[..start].rfind('\n').map_or(0, |p| p + 1);
        let prefix = &source[line_start..start];
        let own_line = !prefix.chars().any(|c| !c.is_whitespace());

        // Check if parent is a send (call is used as receiver of another call)
        // We detect this by checking if there's a non-whitespace, non-newline char
        // immediately after the call's end that's `.` — simpler: check source after end
        let after = source[end..].trim_start_matches([' ', '\t']);
        let is_chained = after.starts_with('.');

        if !own_line || is_chained {
            return None;
        }

        let block = match call.block() {
            Some(b) => b,
            None => {
                // No block at all (e.g. `1.times` by itself) — no autocorrect
                return None;
            }
        };

        // Check if it's a block_argument (e.g. `&:something`)
        if let Some(block_arg) = block.as_block_argument_node() {
            let line_end_with_nl = source[end..].find('\n').map_or(source.len(), |p| end + p + 1);
            let line_end_no_nl = line_end_with_nl.saturating_sub(1);
            let line_end = if line_end_with_nl >= source.len() {
                // Last line — keep trailing \n
                line_end_no_nl
            } else {
                line_end_with_nl
            };
            if count < 1 {
                // Remove the content of this line
                return Some(Correction::delete(line_start, line_end));
            } else {
                // count == 1: replace with the proc name (e.g. `something`)
                if let Some(expr) = block_arg.expression() {
                    if let Some(sym) = expr.as_symbol_node() {
                        let sym_src = &source[sym.location().start_offset()..sym.location().end_offset()];
                        // sym_src is like `:something`, strip leading `:`
                        let name = sym_src.trim_start_matches(':');
                        let replacement = format!("{}{}\n", prefix, name);
                        let line_end = source[end..].find('\n').map_or(source.len(), |p| end + p + 1);
                        return Some(Correction::replace(line_start, line_end, replacement));
                    }
                }
                return None;
            }
        }

        // It's a proper BlockNode
        if let Some(block_node) = block.as_block_node() {
            let line_end_with_nl = source[end..].find('\n').map_or(source.len(), |p| end + p + 1);
            let line_end_no_nl = line_end_with_nl.saturating_sub(1);
            let line_end_remove = if line_end_with_nl >= source.len() {
                line_end_no_nl
            } else {
                line_end_with_nl
            };

            if count < 1 {
                // Remove the whole line(s)
                return Some(Correction::delete(line_start, line_end_remove));
            }

            // count == 1: extract body and replace
            let body_node = match block_node.body() {
                Some(b) => b,
                None => {
                    // Empty block body — remove the whole thing
                    return Some(Correction::delete(line_start, line_end_remove));
                }
            };

            // Get block argument name if present
            let block_arg_name = get_block_arg_name(&block_node, source);

            // Check if block arg is reassigned in body
            if let Some(ref arg) = block_arg_name {
                let body_src = &source[body_node.location().start_offset()..body_node.location().end_offset()];
                if block_arg_is_reassigned(body_src, arg) {
                    return None;
                }
            }

            // Get body source, substitute block arg with "0"
            let body_src = &source[body_node.location().start_offset()..body_node.location().end_offset()];
            let substituted = if let Some(ref arg) = block_arg_name {
                replace_whole_word(body_src, arg, "0")
            } else {
                body_src.to_string()
            };

            // Fix indentation: remove excess indent (block body is indented relative to call)
            let call_col = prefix.len(); // bytes of indent before call
            let body_start_col = {
                let body_start = body_node.location().start_offset();
                let body_line_start = source[..body_start].rfind('\n').map_or(0, |p| p + 1);
                body_start - body_line_start
            };
            let excess_indent = body_start_col.saturating_sub(call_col);

            let replacement = build_replacement(&substituted, prefix, excess_indent);
            Some(Correction::replace(line_start, line_end_with_nl, replacement))
        } else {
            None
        }
    }
}

fn extract_integer_value(node: &Node, source: &str) -> Option<i64> {
    if let Some(int_node) = node.as_integer_node() {
        let src = &source[int_node.location().start_offset()..int_node.location().end_offset()];
        src.parse::<i64>().ok()
    } else {
        // Unary minus: in Prism, `-1` in `-1.times` is parsed as IntegerNode with value -1
        // but the source shows "-1". Check if the node is integer.
        None
    }
}

fn get_block_arg_name(block: &ruby_prism::BlockNode, source: &str) -> Option<String> {
    let params = block.parameters()?;
    // BlockParametersNode — we need to get the first required parameter
    let params_src = &source[params.location().start_offset()..params.location().end_offset()];
    // Strip leading/trailing `|` and whitespace
    let inner = params_src.trim_matches('|').trim();
    if inner.is_empty() {
        return None;
    }
    // First param (before any comma)
    let first = inner.split(',').next()?.trim().to_string();
    if first.is_empty() { None } else { Some(first) }
}

fn block_arg_is_reassigned(body: &str, arg: &str) -> bool {
    // lvasgn: `arg =` or `arg,` (parallel assign)
    let asgn_pattern = format!("{} =", arg);
    let multi_pattern = format!("{},", arg);
    // word-boundary check
    let check = |pattern: &str| {
        body.find(pattern).map_or(false, |pos| {
            let before = pos.checked_sub(1).map_or(true, |i| {
                let c = body.as_bytes()[i] as char;
                !c.is_alphanumeric() && c != '_'
            });
            before
        })
    };
    check(&asgn_pattern) || check(&multi_pattern)
}

fn replace_whole_word(s: &str, word: &str, replacement: &str) -> String {
    let mut result = String::new();
    let mut remaining = s;
    while let Some(pos) = remaining.find(word) {
        let before = &remaining[..pos];
        let after = &remaining[pos + word.len()..];
        let before_ok = pos == 0 || {
            let c = remaining.as_bytes()[pos - 1] as char;
            !c.is_alphanumeric() && c != '_'
        };
        let after_ok = after.is_empty() || {
            let c = after.as_bytes()[0] as char;
            !c.is_alphanumeric() && c != '_'
        };
        if before_ok && after_ok {
            result.push_str(before);
            result.push_str(replacement);
        } else {
            result.push_str(before);
            result.push_str(word);
        }
        remaining = after;
    }
    result.push_str(remaining);
    result
}

fn build_replacement(body_src: &str, call_indent: &str, excess_indent: usize) -> String {
    let lines: Vec<&str> = body_src.lines().collect();
    let mut result_lines: Vec<String> = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            result_lines.push(String::new());
        } else if idx == 0 {
            // First line from Prism's StatementsNode has no leading whitespace;
            // prepend call_indent to place it at the correct level.
            result_lines.push(format!("{}{}", call_indent, line));
        } else {
            // Subsequent lines retain their source indent minus excess_indent.
            // The remaining indent already equals call_indent, so DON'T add it again.
            let stripped = if line.len() >= excess_indent {
                &line[excess_indent..]
            } else {
                line.trim_start()
            };
            result_lines.push(stripped.to_string());
        }
    }

    let mut replacement = result_lines.join("\n");
    replacement.push('\n');
    replacement
}

impl Visit<'_> for Visitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Lint/UselessTimes", |_cfg| Some(Box::new(UselessTimes::new())));
