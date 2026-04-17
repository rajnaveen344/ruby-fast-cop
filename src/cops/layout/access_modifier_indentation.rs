//! Layout/AccessModifierIndentation - indent or outdent bare access modifiers.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/access_modifier_indentation.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::access_modifier::is_bare_access_modifier;
use crate::helpers::source::col_at_offset;
use crate::offense::{Location, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessModifierIndentationStyle {
    Indent,
    Outdent,
}

pub struct AccessModifierIndentation {
    style: AccessModifierIndentationStyle,
    indentation_width: usize,
}

impl AccessModifierIndentation {
    pub fn new(style: AccessModifierIndentationStyle, indentation_width: usize) -> Self {
        Self { style, indentation_width }
    }
}

impl Default for AccessModifierIndentation {
    fn default() -> Self {
        Self::new(AccessModifierIndentationStyle::Indent, 2)
    }
}

impl Cop for AccessModifierIndentation {
    fn name(&self) -> &'static str {
        "Layout/AccessModifierIndentation"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor {
            ctx,
            style: self.style,
            indentation_width: self.indentation_width,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: AccessModifierIndentationStyle,
    indentation_width: usize,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn expected_offset(&self) -> usize {
        match self.style {
            AccessModifierIndentationStyle::Indent => self.indentation_width,
            AccessModifierIndentationStyle::Outdent => 0,
        }
    }

    fn style_str(&self) -> &'static str {
        match self.style {
            AccessModifierIndentationStyle::Indent => "Indent",
            AccessModifierIndentationStyle::Outdent => "Outdent",
        }
    }

    /// Check each bare access modifier in the body, comparing to `end_col`.
    fn check_body(&mut self, body: Option<Node<'_>>, container_start_line: usize, end_col: usize) {
        let Some(body_node) = body else { return };
        // Get the multi-statement children (rubocop's `begin_type?` check)
        let children: Vec<Node<'_>> = if let Some(stmts) = body_node.as_statements_node() {
            if stmts.body().iter().count() < 2 {
                return;
            }
            stmts.body().iter().collect()
        } else {
            return;
        };

        for child in children {
            let Some(call) = child.as_call_node() else { continue };
            if !is_bare_access_modifier(&call) {
                continue;
            }
            let start = call.location().start_offset();
            let mod_line = self.ctx.line_of(start);
            // Skip if on the same line as the class/module/block header
            if mod_line == container_start_line {
                continue;
            }
            let mod_col = col_at_offset(self.ctx.source, start) as usize;
            let expected = end_col + self.expected_offset();
            if mod_col == expected {
                continue;
            }
            let name = String::from_utf8_lossy(call.name().as_slice()).to_string();
            let msg = format!("{} access modifiers like `{}`.", self.style_str(), name);
            let end = call.location().end_offset();
            let loc = Location::from_offsets(self.ctx.source, start, end);
            self.offenses.push(Offense::new(
                "Layout/AccessModifierIndentation",
                msg,
                Severity::Convention,
                loc,
                self.ctx.filename,
            ));
        }
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        let end_col = col_at_offset(self.ctx.source, node.end_keyword_loc().start_offset()) as usize;
        let start_line = self.ctx.line_of(node.location().start_offset());
        self.check_body(node.body(), start_line, end_col);
        ruby_prism::visit_class_node(self, node);
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        let end_col = col_at_offset(self.ctx.source, node.end_keyword_loc().start_offset()) as usize;
        let start_line = self.ctx.line_of(node.location().start_offset());
        self.check_body(node.body(), start_line, end_col);
        ruby_prism::visit_module_node(self, node);
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        let end_col = col_at_offset(self.ctx.source, node.end_keyword_loc().start_offset()) as usize;
        let start_line = self.ctx.line_of(node.location().start_offset());
        self.check_body(node.body(), start_line, end_col);
        ruby_prism::visit_singleton_class_node(self, node);
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        let end_col = col_at_offset(self.ctx.source, node.closing_loc().start_offset()) as usize;
        let start_line = self.ctx.line_of(node.location().start_offset());
        self.check_body(node.body(), start_line, end_col);
        ruby_prism::visit_block_node(self, node);
    }
}

crate::register_cop!("Layout/AccessModifierIndentation", |cfg| {
    let cop_config = cfg.get_cop_config("Layout/AccessModifierIndentation");
    let style = cop_config
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "outdent" => AccessModifierIndentationStyle::Outdent,
            _ => AccessModifierIndentationStyle::Indent,
        })
        .unwrap_or(AccessModifierIndentationStyle::Indent);
    let indent_width = cop_config
        .and_then(|c| c.raw.get("IndentationWidth"))
        .and_then(|v| v.as_i64())
        .map(|v| v as usize)
        .or_else(|| {
            cfg.get_cop_config("Layout/IndentationWidth")
                .and_then(|c| c.raw.get("Width"))
                .and_then(|v| v.as_i64())
                .map(|v| v as usize)
        })
        .unwrap_or(2);
    Some(Box::new(AccessModifierIndentation::new(style, indent_width)))
});
