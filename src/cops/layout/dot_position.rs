//! Layout/DotPosition
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/dot_position.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::line_byte_offset;
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

const COP_NAME: &str = "Layout/DotPosition";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DotStyle {
    Leading,
    Trailing,
}

pub struct DotPosition {
    style: DotStyle,
}

impl DotPosition {
    pub fn new(style: DotStyle) -> Self {
        Self { style }
    }
}

impl Default for DotPosition {
    fn default() -> Self {
        Self::new(DotStyle::Leading)
    }
}

fn line_of(source: &str, offset: usize) -> usize {
    1 + source[..offset.min(source.len())].bytes().filter(|&b| b == b'\n').count()
}

fn line_start(source: &str, offset: usize) -> usize {
    source[..offset].rfind('\n').map_or(0, |p| p + 1)
}

struct DotVisitor<'a> {
    source: &'a str,
    filename: &'a str,
    style: DotStyle,
    offenses: Vec<Offense>,
}

impl<'a> DotVisitor<'a> {
    /// Given the text of a heredoc opening (e.g. `<<~HEREDOC`, `<<~\`HEREDOC\``, `<<"EOF"`),
    /// extract the delimiter string (e.g. `HEREDOC`, `EOF`).
    fn heredoc_delimiter(opening: &str) -> Option<String> {
        // Strip leading `<<`, optional `-` or `~`
        let s = opening.trim_start_matches('<');
        let s = s.trim_start_matches(['-', '~']);
        // Optional quote: `"`, `'`, `` ` ``
        let (s, quote) = if s.starts_with('"') || s.starts_with('\'') || s.starts_with('`') {
            (&s[1..], Some(&s[..1]))
        } else {
            (s, None)
        };
        // Read until end quote or end of identifier
        let delim: String = if let Some(q) = quote {
            s.split(q).next().unwrap_or("").to_string()
        } else {
            s.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect()
        };
        if delim.is_empty() { None } else { Some(delim) }
    }

    /// Scan source forward from `after_offset` to find the line that contains only the heredoc
    /// terminator (possibly with leading whitespace for squiggly). Returns the 1-based line number
    /// of the terminator line.
    fn find_heredoc_end_line(&self, after_offset: usize, delimiter: &str) -> Option<usize> {
        let rest = &self.source[after_offset..];
        // We scan line by line in rest. Each line may be the terminator.
        let mut byte_pos = after_offset;
        for line in rest.lines() {
            let line_trimmed = line.trim();
            if line_trimmed == delimiter {
                // The line number of the next line after byte_pos
                return Some(line_of(self.source, byte_pos));
            }
            byte_pos += line.len() + 1; // +1 for '\n'
        }
        None
    }

    /// Get the actual end line of a heredoc node (the terminator line), or None if not a heredoc.
    fn heredoc_end_line_of_opening(&self, opening: &str, after_offset: usize) -> Option<usize> {
        if !opening.contains("<<") { return None; }
        let delim = Self::heredoc_delimiter(opening)?;
        self.find_heredoc_end_line(after_offset, &delim)
    }

    /// Get the end line of receiver, accounting for heredoc arguments
    fn receiver_end_line(&self, receiver: &ruby_prism::Node<'a>) -> usize {
        // Check if receiver itself is a heredoc string
        if let Some(str_node) = receiver.as_string_node() {
            if let Some(open) = str_node.opening_loc() {
                let opening = &self.source[open.start_offset()..open.end_offset()];
                if let Some(line) = self.heredoc_end_line_of_opening(opening, open.end_offset()) {
                    return line;
                }
            }
        }
        if let Some(xstr_node) = receiver.as_x_string_node() {
            let opening = &self.source[xstr_node.opening_loc().start_offset()..xstr_node.opening_loc().end_offset()];
            if let Some(line) = self.heredoc_end_line_of_opening(opening, xstr_node.opening_loc().end_offset()) {
                return line;
            }
        }
        if let Some(istr) = receiver.as_interpolated_string_node() {
            let (opening, after_off) = if let Some(open) = istr.opening_loc() {
                (self.source[open.start_offset()..open.end_offset()].to_string(), open.end_offset())
            } else {
                let start = istr.location().start_offset();
                let end = istr.location().end_offset();
                (self.source[start..end].to_string(), end)
            };
            if let Some(line) = self.heredoc_end_line_of_opening(&opening, after_off) {
                return line;
            }
        }
        // Check for heredoc arguments in receiver call
        if let Some(call) = receiver.as_call_node() {
            if let Some(max_heredoc_line) = self.max_heredoc_end_line_in_call(&call) {
                return max_heredoc_line;
            }
        }
        line_of(self.source, receiver.location().end_offset().saturating_sub(1))
    }

    fn max_heredoc_end_line_in_call(&self, call: &ruby_prism::CallNode<'a>) -> Option<usize> {
        let mut max_line: Option<usize> = None;
        for arg in call.arguments().iter().flat_map(|a| a.arguments().iter()) {
            if let Some(hline) = self.heredoc_end_line_of_node(&arg) {
                max_line = Some(max_line.unwrap_or(0).max(hline));
            }
        }
        max_line
    }

    fn heredoc_end_line_of_node(&self, node: &ruby_prism::Node<'a>) -> Option<usize> {
        if let Some(str_node) = node.as_string_node() {
            if let Some(open) = str_node.opening_loc() {
                let opening = &self.source[open.start_offset()..open.end_offset()];
                return self.heredoc_end_line_of_opening(opening, open.end_offset());
            }
        }
        if let Some(istr) = node.as_interpolated_string_node() {
            // opening_loc() may be None for heredocs in some Prism versions.
            // Fall back to the node's own location start as the opening token position.
            let (opening, after_off) = if let Some(open) = istr.opening_loc() {
                let text = &self.source[open.start_offset()..open.end_offset()];
                (text.to_string(), open.end_offset())
            } else {
                let start = istr.location().start_offset();
                let end = istr.location().end_offset();
                let text = &self.source[start..end];
                (text.to_string(), end)
            };
            return self.heredoc_end_line_of_opening(&opening, after_off);
        }
        if let Some(xstr) = node.as_x_string_node() {
            let opening = &self.source[xstr.opening_loc().start_offset()..xstr.opening_loc().end_offset()];
            return self.heredoc_end_line_of_opening(opening, xstr.opening_loc().end_offset());
        }
        None
    }

    fn check_call(&mut self, call: &ruby_prism::CallNode<'a>) {
        // Only process calls with a dot or &. operator
        let dot_loc = match call.call_operator_loc() {
            Some(d) => d,
            None => return,
        };

        let dot_source = &self.source[dot_loc.start_offset()..dot_loc.end_offset()];

        // Get the receiver
        let receiver = match call.receiver() {
            Some(r) => r,
            None => return,
        };

        // Get the selector (method name) location
        // For `l.(1)` there is no selector, use opening paren
        let selector_start = call.message_loc()
            .map(|s| s.start_offset())
            .or_else(|| call.opening_loc().map(|o| o.start_offset()));
        let selector_start = match selector_start {
            Some(s) => s,
            None => return,
        };

        let dot_line = line_of(self.source, dot_loc.start_offset());
        let selector_line = line_of(self.source, selector_start);

        // Same line: skip (both on same line as each other)
        // receiver end = last part of receiver
        let receiver_end_off = receiver.location().end_offset();
        let receiver_end_line = line_of(self.source, receiver_end_off.saturating_sub(1));
        let actual_receiver_end_line = self.receiver_end_line(&receiver);

        // If selector is on same line as receiver end: single-line call, skip
        if selector_line == receiver_end_line && selector_line == dot_line {
            return;
        }

        // If there's an intervening blank line or comment between the last of receiver/dot and selector,
        // skip (RuboCop: line_between? — gap > 1 line)
        let compare_line = actual_receiver_end_line.max(dot_line);
        if selector_line > compare_line + 1 {
            return;
        }

        // Check if there's a comment line between dot and selector
        if self.has_intervening_comment(dot_line, selector_line) {
            return;
        }

        // Determine if style is correct
        let offense = match self.style {
            DotStyle::Leading => {
                // dot should be on selector_line
                if dot_line != selector_line {
                    // trailing dot is bad for leading style
                    Some((dot_loc.start_offset(), dot_loc.end_offset(), dot_source))
                } else {
                    None
                }
            }
            DotStyle::Trailing => {
                // dot should NOT be on selector_line (should be on receiver line)
                if dot_line == selector_line {
                    Some((dot_loc.start_offset(), dot_loc.end_offset(), dot_source))
                } else {
                    None
                }
            }
        };

        if let Some((dot_start, dot_end, dot_str)) = offense {
            let message = match self.style {
                DotStyle::Leading => format!("Place the {} on the next line, together with the method name.", dot_str),
                DotStyle::Trailing => format!("Place the {} on the previous line, together with the method call receiver.", dot_str),
            };

            let loc = Location::from_offsets(self.source, dot_start, dot_end);
            let correction = self.build_correction(call, dot_start, dot_end, dot_str, selector_start, &receiver, actual_receiver_end_line);
            self.offenses.push(
                Offense::new(COP_NAME, &message, Severity::Convention, loc, self.filename)
                    .with_correction(correction),
            );
        }
    }

    fn has_intervening_comment(&self, from_line: usize, to_line: usize) -> bool {
        if to_line <= from_line + 1 {
            return false;
        }
        let lines: Vec<&str> = self.source.lines().collect();
        for i in from_line..to_line.saturating_sub(1) {
            if i >= lines.len() { break; }
            let l = lines[i].trim();
            // A Ruby comment starts with `#` but NOT `#{` (interpolation)
            if l.is_empty() || (l.starts_with('#') && !l.starts_with("#{")) {
                return true;
            }
        }
        false
    }

    fn build_correction(
        &self,
        _call: &ruby_prism::CallNode<'a>,
        dot_start: usize,
        dot_end: usize,
        dot_str: &str,
        selector_start: usize,
        receiver: &ruby_prism::Node<'a>,
        _recv_end_line: usize,
    ) -> Correction {
        match self.style {
            DotStyle::Leading => {
                // Remove trailing dot (possibly whole line if dot is on its own line)
                // Insert dot before selector
                let dot_line_start = line_start(self.source, dot_start);
                let dot_line_content = self.source[dot_line_start..dot_start].trim();

                let (remove_start, remove_end) = if dot_line_content.is_empty() {
                    // dot is on its own line — remove the whole line including newline
                    let next_line_start = self.source[dot_end..].find('\n')
                        .map(|p| dot_end + p + 1)
                        .unwrap_or(dot_end);
                    (dot_line_start, next_line_start)
                } else {
                    (dot_start, dot_end)
                };

                // Multi-edit: remove dot, insert before selector
                // Simulate by replacing: remove dot from one place, insert at selector
                // We encode as two edits stored in one Correction via edits vec
                let insert_pos = selector_start;
                let edits = vec![
                    crate::offense::Edit {
                        start_offset: remove_start,
                        end_offset: remove_end,
                        replacement: String::new(),
                    },
                    crate::offense::Edit {
                        start_offset: insert_pos,
                        end_offset: insert_pos,
                        replacement: dot_str.to_string(),
                    },
                ];
                Correction { edits }
            }
            DotStyle::Trailing => {
                // Remove leading dot; insert dot after receiver end
                let receiver_end = receiver.location().end_offset();
                let dot_line_start = line_start(self.source, dot_start);
                let remaining_after_dot = &self.source[dot_end..];
                let before_dot_on_line = &self.source[dot_line_start..dot_start];
                let after_dot_until_eol = remaining_after_dot.split('\n').next().unwrap_or("");

                let (remove_start, remove_end) = if before_dot_on_line.trim().is_empty() && after_dot_until_eol.trim().is_empty() {
                    let nl_before = if dot_line_start > 0 { dot_line_start - 1 } else { 0 };
                    let nl_after = self.source[dot_end..].find('\n').map(|p| dot_end + p + 1).unwrap_or(dot_end);
                    (nl_before.max(dot_start), nl_after)
                } else {
                    (dot_start, dot_end)
                };

                let edits = vec![
                    crate::offense::Edit {
                        start_offset: remove_start,
                        end_offset: remove_end,
                        replacement: String::new(),
                    },
                    crate::offense::Edit {
                        start_offset: receiver_end,
                        end_offset: receiver_end,
                        replacement: dot_str.to_string(),
                    },
                ];
                Correction { edits }
            }
        }
    }
}

impl<'a> Visit<'a> for DotVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'a>) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

impl Cop for DotPosition {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = DotVisitor {
            source: ctx.source,
            filename: ctx.filename,
            style: self.style,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
}

crate::register_cop!("Layout/DotPosition", |cfg| {
    let c: Cfg = cfg.typed("Layout/DotPosition");
    let style = if c.enforced_style == "trailing" { DotStyle::Trailing } else { DotStyle::Leading };
    Some(Box::new(DotPosition::new(style)))
});
