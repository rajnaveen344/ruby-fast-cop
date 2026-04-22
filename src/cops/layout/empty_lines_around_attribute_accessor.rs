//! Layout/EmptyLinesAroundAttributeAccessor
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/empty_lines_around_attribute_accessor.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::line_byte_offset;
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

const COP_NAME: &str = "Layout/EmptyLinesAroundAttributeAccessor";
const MSG: &str = "Add an empty line after attribute accessor.";
const ATTR_METHODS: &[&str] = &["attr_reader", "attr_writer", "attr_accessor", "attr"];

pub struct EmptyLinesAroundAttributeAccessor {
    allow_alias_syntax: bool,
    allowed_methods: Vec<String>,
}

impl EmptyLinesAroundAttributeAccessor {
    pub fn new(allow_alias_syntax: bool, allowed_methods: Vec<String>) -> Self {
        Self { allow_alias_syntax, allowed_methods }
    }
}

impl Default for EmptyLinesAroundAttributeAccessor {
    fn default() -> Self {
        Self::new(true, vec!["alias_method".into(), "public".into(), "protected".into(), "private".into()])
    }
}

fn line_of(source: &str, offset: usize) -> usize {
    1 + source[..offset.min(source.len())].bytes().filter(|&b| b == b'\n').count()
}

struct AttrVisitor<'a> {
    source: &'a str,
    filename: &'a str,
    allow_alias_syntax: bool,
    allowed_methods: Vec<String>,
    offenses: Vec<Offense>,
    /// Track if inside if/unless node
    in_conditional: usize,
}

impl<'a> AttrVisitor<'a> {
    fn is_attr_method(name: &str) -> bool {
        ATTR_METHODS.contains(&name)
    }

    fn lines(&self) -> Vec<&str> {
        self.source.lines().collect()
    }

    fn is_empty_line(&self, line_idx: usize) -> bool {
        let lines = self.lines();
        lines.get(line_idx).map_or(true, |l| l.trim().is_empty())
    }

    fn next_sibling_allows(&self, next_line_content: &str) -> bool {
        let trimmed = next_line_content.trim();
        // end keyword: attr is the last thing in its scope
        if trimmed == "end" || trimmed.starts_with("end ") || trimmed.starts_with("end\t") {
            return true;
        }
        // closing bracket/brace
        if trimmed.starts_with('}') || trimmed.starts_with(']') || trimmed.starts_with(')') {
            return true;
        }
        // alias syntax
        if self.allow_alias_syntax && trimmed.starts_with("alias ") {
            return true;
        }
        // Check allowed_methods: trimmed starts with one of the method names
        for m in &self.allowed_methods {
            if trimmed.starts_with(m.as_str()) {
                // Must be followed by space or end of line
                let rest = &trimmed[m.len()..];
                if rest.is_empty() || rest.starts_with(' ') || rest.starts_with('(') {
                    return true;
                }
            }
        }
        // Check if next line is another attr_*
        for m in ATTR_METHODS {
            if trimmed.starts_with(m) {
                let rest = &trimmed[m.len()..];
                if rest.is_empty() || rest.starts_with(' ') || rest.starts_with('(') {
                    return true;
                }
            }
        }
        false
    }

    fn check_call(&mut self, node: &ruby_prism::CallNode<'a>) {
        // Must be a bare call (no receiver)
        if node.receiver().is_some() {
            return;
        }
        let name = node.name();
        let name_str = String::from_utf8_lossy(name.as_slice());
        if !Self::is_attr_method(&name_str) {
            return;
        }
        // Must have at least one symbol argument (attribute_accessor? check)
        let has_symbol_arg = node.arguments().map_or(false, |args| {
            args.arguments().iter().any(|a| a.as_symbol_node().is_some())
        });
        if !has_symbol_arg {
            return;
        }
        // Skip if inside conditional
        if self.in_conditional > 0 {
            return;
        }

        let node_end = node.location().end_offset();
        let node_last_line = line_of(self.source, node_end.saturating_sub(1));
        let lines = self.lines();
        let next_line_idx = node_last_line; // 0-indexed = node_last_line (since line_of is 1-based)

        // Check if next line is empty
        if next_line_idx >= lines.len() {
            // attr is last line — no offense
            return;
        }

        let next_line = lines[next_line_idx];

        // Next line empty: ok
        if next_line.trim().is_empty() {
            return;
        }

        // Check for rubocop:enable directive on next line
        if next_line.contains("rubocop:enable") {
            // After the enable comment, check the line after that
            let after_enable_idx = next_line_idx + 1;
            if after_enable_idx >= lines.len() || lines[after_enable_idx].trim().is_empty() {
                return;
            }
            // There's content after the enable comment — offense
            // But the offense is still on the attr line, not on the enable line
            // The correction inserts blank line after the enable comment
            // We fall through to offense
        }

        // Check if next sibling is allowed
        let effective_next = if next_line.contains("rubocop:enable") {
            // look past it
            if next_line_idx + 1 < lines.len() {
                lines[next_line_idx + 1]
            } else {
                return; // no content after enable
            }
        } else {
            next_line
        };

        if self.next_sibling_allows(effective_next) {
            return;
        }

        // Offense: on the attr node
        let node_start = node.location().start_offset();
        let loc = Location::from_offsets(self.source, node_start, node_end);

        // Correction: insert blank line after the attr line (or after rubocop:enable if present)
        let insert_after_line = if next_line.contains("rubocop:enable") {
            next_line_idx + 1
        } else {
            next_line_idx
        };
        let insert_offset = line_byte_offset(self.source, insert_after_line + 1);
        let correction = Correction::insert(insert_offset, "\n");

        self.offenses.push(
            Offense::new(COP_NAME, MSG, Severity::Convention, loc, self.filename)
                .with_correction(correction),
        );
    }
}

impl<'a> Visit<'a> for AttrVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'a>) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'a>) {
        self.in_conditional += 1;
        ruby_prism::visit_if_node(self, node);
        self.in_conditional -= 1;
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode<'a>) {
        self.in_conditional += 1;
        ruby_prism::visit_unless_node(self, node);
        self.in_conditional -= 1;
    }
}

impl Cop for EmptyLinesAroundAttributeAccessor {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = AttrVisitor {
            source: ctx.source,
            filename: ctx.filename,
            allow_alias_syntax: self.allow_alias_syntax,
            allowed_methods: self.allowed_methods.clone(),
            offenses: Vec::new(),
            in_conditional: 0,
        };
        v.visit_program_node(node);
        v.offenses
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_alias_syntax: bool,
    allowed_methods: Vec<String>,
}

impl Default for Cfg {
    fn default() -> Self {
        Self {
            allow_alias_syntax: true,
            allowed_methods: vec!["alias_method".into(), "public".into(), "protected".into(), "private".into()],
        }
    }
}

crate::register_cop!("Layout/EmptyLinesAroundAttributeAccessor", |cfg| {
    let c: Cfg = cfg.typed("Layout/EmptyLinesAroundAttributeAccessor");
    Some(Box::new(EmptyLinesAroundAttributeAccessor::new(c.allow_alias_syntax, c.allowed_methods)))
});
