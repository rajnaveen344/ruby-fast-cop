//! Layout/EmptyLinesAroundAccessModifier - Access modifiers should be surrounded by blank lines.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/empty_lines_around_access_modifier.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::access_modifier::ACCESS_MODIFIERS;
use crate::helpers::source::{line_byte_offset, line_end_byte_offset};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

const MSG_AFTER: &str = "Keep a blank line after `%MOD%`.";
const MSG_BEFORE_AND_AFTER: &str = "Keep a blank line before and after `%MOD%`.";
const MSG_BEFORE_FOR_ONLY_BEFORE: &str = "Keep a blank line before `%MOD%`.";
const MSG_AFTER_FOR_ONLY_BEFORE: &str = "Remove a blank line after `%MOD%`.";

#[derive(Debug, Clone, PartialEq)]
pub enum EnforcedStyle {
    Around,
    OnlyBefore,
}

pub struct EmptyLinesAroundAccessModifier {
    style: EnforcedStyle,
}

impl EmptyLinesAroundAccessModifier {
    pub fn new(style: EnforcedStyle) -> Self {
        Self { style }
    }
}

struct ModifierInfo {
    name: String,
    first_line: usize,
    last_line: usize,
    start_offset: usize,
    end_offset: usize,
    is_special: bool,
}

/// Collects scope info and bare access modifier locations from the AST
struct InfoCollector {
    source: String,
    /// (first_line, last_line) for class/module/sclass — used for both start and end checks
    class_ranges: Vec<(usize, usize)>,
    /// (first_line, last_line) for blocks — used only for start checks, NOT end
    block_ranges: Vec<(usize, usize)>,
    modifiers: Vec<ModifierInfo>,
    /// Inside def? Access modifiers inside defs are method calls, not modifiers
    def_depth: usize,
    /// Inside arguments? `attr_reader private` makes `private` an argument, not a modifier
    args_depth: usize,
}

impl InfoCollector {
    fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
            class_ranges: Vec::new(),
            block_ranges: Vec::new(),
            modifiers: Vec::new(),
            def_depth: 0,
            args_depth: 0,
        }
    }

    fn line_of(&self, offset: usize) -> usize {
        1 + self.source.as_bytes()[..offset]
            .iter()
            .filter(|&&b| b == b'\n')
            .count()
    }
}

impl Visit<'_> for InfoCollector {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        let first_line = if let Some(superclass) = node.superclass() {
            self.line_of(superclass.location().start_offset())
        } else {
            self.line_of(node.location().start_offset())
        };
        let last_line = self.line_of(node.location().end_offset() - 1);
        self.class_ranges.push((first_line, last_line));
        let saved = self.def_depth;
        self.def_depth = 0;
        ruby_prism::visit_class_node(self, node);
        self.def_depth = saved;
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        let first_line = self.line_of(node.location().start_offset());
        let last_line = self.line_of(node.location().end_offset() - 1);
        self.class_ranges.push((first_line, last_line));
        let saved = self.def_depth;
        self.def_depth = 0;
        ruby_prism::visit_module_node(self, node);
        self.def_depth = saved;
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        let first_line = self.line_of(node.expression().location().start_offset());
        let last_line = self.line_of(node.location().end_offset() - 1);
        self.class_ranges.push((first_line, last_line));
        let saved = self.def_depth;
        self.def_depth = 0;
        ruby_prism::visit_singleton_class_node(self, node);
        self.def_depth = saved;
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        let first_line = self.line_of(node.location().start_offset());
        let last_line = self.line_of(node.location().end_offset() - 1);
        self.block_ranges.push((first_line, last_line));
        ruby_prism::visit_block_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        self.def_depth += 1;
        ruby_prism::visit_def_node(self, node);
        self.def_depth -= 1;
    }

    fn visit_arguments_node(&mut self, node: &ruby_prism::ArgumentsNode) {
        self.args_depth += 1;
        ruby_prism::visit_arguments_node(self, node);
        self.args_depth -= 1;
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if self.def_depth == 0 && self.args_depth == 0 {
            let name = node_name!(node);
            if ACCESS_MODIFIERS.contains(&name.as_ref())
                && node.receiver().is_none()
                && node.arguments().map_or(true, |a| a.arguments().is_empty())
                && node.block().is_none()
            {
                let name = name.to_string();
                let start = node.location().start_offset();
                let end = start + name.len();
                let first_line = self.line_of(start);
                let last_line = self.line_of(node.location().end_offset() - 1);
                let is_special = name == "public" || name == "module_function";

                self.modifiers.push(ModifierInfo {
                    name,
                    first_line,
                    last_line,
                    start_offset: start,
                    end_offset: end,
                    is_special,
                });
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

impl Cop for EmptyLinesAroundAccessModifier {
    fn name(&self) -> &'static str {
        "Layout/EmptyLinesAroundAccessModifier"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let source = ctx.source;
        let lines: Vec<&str> = source.lines().collect();
        if lines.is_empty() {
            return vec![];
        }

        let mut collector = InfoCollector::new(source);
        collector.visit_program_node(node);

        let mut offenses = Vec::new();

        for modifier in &collector.modifiers {
            // Skip if modifier and next expression are on the same line (e.g. "private; foo")
            let mod_line = lines[modifier.first_line - 1];
            if let Some(semi_pos) = mod_line.find(';') {
                let after_semi = mod_line[semi_pos + 1..].trim();
                if !after_semi.is_empty() {
                    continue;
                }
            }

            match self.style {
                EnforcedStyle::Around => {
                    self.check_around(
                        modifier, &lines, &collector.class_ranges, &collector.block_ranges,
                        source, ctx, &mut offenses,
                    );
                }
                EnforcedStyle::OnlyBefore => {
                    self.check_only_before(
                        modifier, &lines, &collector.class_ranges, &collector.block_ranges,
                        source, ctx, &mut offenses,
                    );
                }
            }
        }

        offenses
    }
}

impl EmptyLinesAroundAccessModifier {
    fn check_around(
        &self,
        modifier: &ModifierInfo,
        lines: &[&str],
        class_ranges: &[(usize, usize)],
        block_ranges: &[(usize, usize)],
        source: &str,
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
    ) {
        let is_at_class_start = class_ranges.iter().any(|(f, _)| modifier.first_line == f + 1);
        let is_at_block_start = block_ranges.iter().any(|(f, _)| modifier.first_line == f + 1);
        let is_at_scope_start = is_at_class_start || is_at_block_start;

        let prev_empty = if is_at_scope_start {
            true
        } else {
            previous_line_empty(lines, modifier.first_line)
        };

        // body_end only checks class/module/sclass, NOT blocks (per RuboCop)
        let is_at_class_end = class_ranges.iter().any(|(_, l)| modifier.last_line == l - 1);
        let next_empty = if is_at_class_end {
            true
        } else if modifier.last_line < lines.len() {
            lines[modifier.last_line].trim().is_empty()
        } else {
            true
        };

        if prev_empty && next_empty {
            return;
        }

        let msg = if is_at_scope_start {
            MSG_AFTER.replace("%MOD%", &modifier.name)
        } else {
            MSG_BEFORE_AND_AFTER.replace("%MOD%", &modifier.name)
        };

        let loc = Location::from_offsets(source, modifier.start_offset, modifier.end_offset);
        let mut offense = Offense::new(self.name(), &msg, self.severity(), loc, ctx.filename);

        // Whether the modifier is the last statement in a block body.
        // Correction behavior depends on cross-cop Layout/EmptyLinesAroundBlockBody config
        // which we can't detect. Skip corrections for block-end cases to avoid conflicts.
        let is_at_block_end = block_ranges.iter().any(|(_, l)| modifier.last_line == l - 1);

        if !is_at_block_end {
            let mut edits = Vec::new();

            if !prev_empty && !is_at_scope_start {
                let line_start = line_byte_offset(source, modifier.first_line);
                edits.push(crate::offense::Edit {
                    start_offset: line_start,
                    end_offset: line_start,
                    replacement: "\n".to_string(),
                });
            }

            if !next_empty {
                let line_end = line_end_byte_offset(source, modifier.last_line);
                edits.push(crate::offense::Edit {
                    start_offset: line_end,
                    end_offset: line_end,
                    replacement: "\n".to_string(),
                });
            }

            if !edits.is_empty() {
                offense = offense.with_correction(Correction { edits });
            }
        }

        offenses.push(offense);
    }

    fn check_only_before(
        &self,
        modifier: &ModifierInfo,
        lines: &[&str],
        class_ranges: &[(usize, usize)],
        block_ranges: &[(usize, usize)],
        source: &str,
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
    ) {
        let is_at_scope_start = class_ranges.iter().any(|(f, _)| modifier.first_line == f + 1)
            || block_ranges.iter().any(|(f, _)| modifier.first_line == f + 1);
        let prev_empty = if is_at_scope_start {
            true
        } else {
            previous_line_empty(lines, modifier.first_line)
        };

        // For "special" modifiers (public, module_function) in only_before:
        if modifier.is_special {
            if !prev_empty {
                let msg = MSG_BEFORE_FOR_ONLY_BEFORE.replace("%MOD%", &modifier.name);
                let loc = Location::from_offsets(source, modifier.start_offset, modifier.end_offset);
                let offense = Offense::new(self.name(), &msg, self.severity(), loc, ctx.filename)
                    .with_correction(Correction::insert(
                        line_byte_offset(source, modifier.first_line),
                        "\n",
                    ));
                offenses.push(offense);
            }
            return;
        }

        // For private/protected:
        // Check if next line is "end" — accept
        let next_is_end = modifier.last_line < lines.len()
            && lines[modifier.last_line].trim() == "end";
        if next_is_end {
            return;
        }

        // Check if next line is empty and exists — offense (blank line after)
        let next_empty_and_exists = modifier.last_line < lines.len()
            && lines[modifier.last_line].trim().is_empty()
            && modifier.last_line + 1 < lines.len();
        if next_empty_and_exists {
            let msg = MSG_AFTER_FOR_ONLY_BEFORE.replace("%MOD%", &modifier.name);
            let loc = Location::from_offsets(source, modifier.start_offset, modifier.end_offset);
            let remove_start = line_byte_offset(source, modifier.last_line + 1);
            let remove_end = line_end_byte_offset(source, modifier.last_line + 1);
            let offense = Offense::new(self.name(), &msg, self.severity(), loc, ctx.filename)
                .with_correction(Correction::delete(remove_start, remove_end));
            offenses.push(offense);
            return;
        }

        if !prev_empty {
            let msg = MSG_BEFORE_FOR_ONLY_BEFORE.replace("%MOD%", &modifier.name);
            let loc = Location::from_offsets(source, modifier.start_offset, modifier.end_offset);
            let offense = Offense::new(self.name(), &msg, self.severity(), loc, ctx.filename)
                .with_correction(Correction::insert(
                    line_byte_offset(source, modifier.first_line),
                    "\n",
                ));
            offenses.push(offense);
        }
    }
}

/// Check if the previous line (ignoring comments) is empty
fn previous_line_empty(lines: &[&str], send_line: usize) -> bool {
    let mut idx = send_line as isize - 2; // 0-indexed, previous line
    while idx >= 0 {
        let line = lines[idx as usize].trim();
        if line.starts_with('#') {
            idx -= 1;
            continue;
        }
        return line.is_empty();
    }
    true // start of file
}

