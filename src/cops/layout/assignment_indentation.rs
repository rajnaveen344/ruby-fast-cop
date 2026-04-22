//! Layout/AssignmentIndentation - Checks indentation of RHS in multi-line assignments.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/assignment_indentation.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::{col_at_offset, line_at_offset};
use crate::offense::{Correction, Edit, Location, Offense, Severity};
use ruby_prism::Visit;

pub struct AssignmentIndentation {
    indentation_width: usize,
}

impl AssignmentIndentation {
    pub fn new(indentation_width: usize) -> Self {
        Self { indentation_width }
    }
}

impl Default for AssignmentIndentation {
    fn default() -> Self {
        Self { indentation_width: 2 }
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    indentation_width: usize,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    /// Find the leftmost assignment column on the same line as `node_start`.
    /// Scans the source for `name =` patterns on the same line, leftmost wins.
    fn leftmost_assignment_col(&self, node_start: usize, op_offset: usize) -> u32 {
        let source = self.ctx.source;
        // Find line start
        let line_start = source[..node_start].rfind('\n').map_or(0, |p| p + 1);
        // Find line end (the newline char after the operator)
        let line_end = op_offset + source[op_offset..].find('\n').unwrap_or(source.len() - op_offset);
        let line = &source[line_start..line_end];

        // Find all `= ` or `=\n` positions on this line (assignment ops, not ==, !=, etc.)
        // Walk left to find the leftmost identifier followed by `=`
        let mut leftmost_col = display_col_at_offset(source, node_start);

        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'=' {
                // Check it's not == or !=, <=, >=
                let prev = if i > 0 { bytes[i - 1] } else { 0 };
                let next = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
                if next != b'=' && prev != b'!' && prev != b'<' && prev != b'>' && prev != b'=' {
                    // Find start of identifier before '='
                    let mut j = i;
                    // Skip space before '='
                    while j > 0 && bytes[j - 1] == b' ' { j -= 1; }
                    // Skip identifier chars
                    while j > 0 && (bytes[j - 1].is_ascii_alphanumeric() || bytes[j - 1] == b'_') {
                        j -= 1;
                    }
                    // col of j
                    let abs_offset = line_start + j;
                    let col = display_col_at_offset(source, abs_offset);
                    if col < leftmost_col {
                        leftmost_col = col;
                    }
                }
            }
            i += 1;
        }
        leftmost_col
    }

    /// Check a single assignment where lhs is at lhs_col and rhs starts at rhs_offset.
    /// base_col: column of the leftmost assignment in a chain.
    fn check_assignment(&mut self, operator_offset: usize, rhs_offset: usize, base_col: u32) {
        let source = self.ctx.source;
        let op_line = line_at_offset(source, operator_offset);
        let rhs_line = line_at_offset(source, rhs_offset);

        // Only flag multi-line (rhs on different line than operator)
        if op_line == rhs_line {
            return;
        }

        let rhs_col = col_at_offset(source, rhs_offset);
        let expected_col = base_col + self.indentation_width as u32;

        if rhs_col == expected_col {
            return;
        }

        // Find end of rhs first line
        let rhs_line_end = source[rhs_offset..].find('\n')
            .map(|p| rhs_offset + p)
            .unwrap_or(source.len());

        let msg = "Indent the first line of the right-hand-side of a multi-line assignment.";

        // Correction: adjust indentation of rhs line
        let line_start = source[..rhs_offset].rfind('\n').map_or(0, |p| p + 1);
        // Current indent
        let current_indent = rhs_col as usize;
        let expected = expected_col as usize;

        let correction = Correction {
            edits: vec![Edit {
                start_offset: line_start,
                end_offset: line_start + current_indent,
                replacement: " ".repeat(expected),
            }],
        };

        self.offenses.push(
            Offense::new(
                "Layout/AssignmentIndentation",
                msg,
                Severity::Convention,
                Location::from_offsets(source, rhs_offset, rhs_line_end),
                self.ctx.filename,
            ).with_correction(correction)
        );
    }
}

/// Compute display column at byte offset, counting fullwidth chars as width 2.
fn display_col_at_offset(source: &str, offset: usize) -> u32 {
    let line_start = source[..offset].rfind('\n').map_or(0, |p| p + 1);
    let line = &source[line_start..offset];
    let mut col = 0u32;
    for ch in line.chars() {
        if ch == '\u{FEFF}' { continue; }
        // CJK fullwidth: approximate using Unicode ranges
        col += char_display_width(ch);
    }
    col
}

fn char_display_width(ch: char) -> u32 {
    // Fullwidth and wide characters: rough approximation
    let c = ch as u32;
    if (0x1100..=0x11FF).contains(&c)   // Hangul Jamo
        || (0x2E80..=0x303F).contains(&c) // CJK Radicals
        || (0x3040..=0x33FF).contains(&c) // Japanese/CJK
        || (0x3400..=0x4DBF).contains(&c) // CJK Unified
        || (0x4E00..=0x9FFF).contains(&c) // CJK Unified
        || (0xA000..=0xA4CF).contains(&c) // Yi
        || (0xAC00..=0xD7AF).contains(&c) // Hangul
        || (0xF900..=0xFAFF).contains(&c) // CJK Compatibility
        || (0xFE10..=0xFE19).contains(&c) // Vertical forms
        || (0xFE30..=0xFE4F).contains(&c) // CJK Compatibility
        || (0xFF01..=0xFF60).contains(&c) // Fullwidth
        || (0xFFE0..=0xFFE6).contains(&c) // Fullwidth Signs
    {
        2
    } else {
        1
    }
}

impl<'a> Visit<'a> for Visitor<'a> {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'a>) {
        let source = self.ctx.source;
        let op_loc = node.operator_loc();
        // Only plain `=` assignments (not +=, -=, etc.)
        if &source[op_loc.start_offset()..op_loc.end_offset()] != "=" {
            ruby_prism::visit_local_variable_write_node(self, node);
            return;
        }
        let rhs = node.value();
        let rhs_offset = rhs.location().start_offset();
        let lhs_col = self.leftmost_assignment_col(node.location().start_offset(), op_loc.start_offset());
        self.check_assignment(op_loc.start_offset(), rhs_offset, lhs_col);
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode<'a>) {
        let source = self.ctx.source;
        let op_loc = node.operator_loc();
        if &source[op_loc.start_offset()..op_loc.end_offset()] != "=" {
            ruby_prism::visit_instance_variable_write_node(self, node);
            return;
        }
        let rhs = node.value();
        let lhs_col = self.leftmost_assignment_col(node.location().start_offset(), op_loc.start_offset());
        self.check_assignment(op_loc.start_offset(), rhs.location().start_offset(), lhs_col);
        ruby_prism::visit_instance_variable_write_node(self, node);
    }

    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode<'a>) {
        let source = self.ctx.source;
        let op_loc = node.operator_loc();
        if &source[op_loc.start_offset()..op_loc.end_offset()] != "=" {
            ruby_prism::visit_class_variable_write_node(self, node);
            return;
        }
        let rhs = node.value();
        let lhs_col = self.leftmost_assignment_col(node.location().start_offset(), op_loc.start_offset());
        self.check_assignment(op_loc.start_offset(), rhs.location().start_offset(), lhs_col);
        ruby_prism::visit_class_variable_write_node(self, node);
    }

    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode<'a>) {
        let source = self.ctx.source;
        let op_loc = node.operator_loc();
        if &source[op_loc.start_offset()..op_loc.end_offset()] != "=" {
            ruby_prism::visit_global_variable_write_node(self, node);
            return;
        }
        let rhs = node.value();
        let lhs_col = self.leftmost_assignment_col(node.location().start_offset(), op_loc.start_offset());
        self.check_assignment(op_loc.start_offset(), rhs.location().start_offset(), lhs_col);
        ruby_prism::visit_global_variable_write_node(self, node);
    }

    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode<'a>) {
        let source = self.ctx.source;
        let op_loc = node.operator_loc();
        if &source[op_loc.start_offset()..op_loc.end_offset()] != "=" {
            ruby_prism::visit_constant_write_node(self, node);
            return;
        }
        let rhs = node.value();
        let lhs_col = self.leftmost_assignment_col(node.location().start_offset(), op_loc.start_offset());
        self.check_assignment(op_loc.start_offset(), rhs.location().start_offset(), lhs_col);
        ruby_prism::visit_constant_write_node(self, node);
    }

    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode<'a>) {
        let source = self.ctx.source;
        let op_loc = node.operator_loc();
        if &source[op_loc.start_offset()..op_loc.end_offset()] != "=" {
            ruby_prism::visit_multi_write_node(self, node);
            return;
        }
        let rhs = node.value();
        let lhs_col = self.leftmost_assignment_col(node.location().start_offset(), op_loc.start_offset());
        self.check_assignment(op_loc.start_offset(), rhs.location().start_offset(), lhs_col);
        ruby_prism::visit_multi_write_node(self, node);
    }
}

impl Cop for AssignmentIndentation {
    fn name(&self) -> &'static str {
        "Layout/AssignmentIndentation"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor {
            ctx,
            indentation_width: self.indentation_width,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

crate::register_cop!("Layout/AssignmentIndentation", |cfg| {
    let cop_cfg = cfg.get_cop_config("Layout/AssignmentIndentation");
    // IndentationWidth: if set on this cop, use it; otherwise use Layout/IndentationWidth.Width
    let width = cop_cfg
        .as_ref()
        .and_then(|c| c.raw.get("IndentationWidth"))
        .and_then(|v| v.as_u64())
        .map(|w| w as usize)
        .or_else(|| {
            cfg.get_cop_config("Layout/IndentationWidth")
                .and_then(|c| c.max)
        })
        .unwrap_or(2);
    Some(Box::new(AssignmentIndentation::new(width)))
});
