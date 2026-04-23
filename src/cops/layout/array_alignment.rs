//! Layout/ArrayAlignment — multiline array literal element alignment.
//!
//! Port of `rubocop/cop/layout/array_alignment.rb`.

use crate::cops::{CheckContext, Cop};
use crate::helpers::alignment_check::{display_col_of, display_indent_of, each_bad_alignment};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AAStyle {
    WithFirstElement,
    WithFixedIndentation,
}

pub struct ArrayAlignment {
    style: AAStyle,
    indentation_width: usize,
}

impl ArrayAlignment {
    pub fn new(style: AAStyle, indentation_width: usize) -> Self {
        Self { style, indentation_width }
    }
}

const ALIGN_MSG: &str =
    "Align the elements of an array literal if they span more than one line.";
const FIXED_MSG: &str =
    "Use one level of indentation for elements following the first line of a multi-line array.";

impl Cop for ArrayAlignment {
    fn name(&self) -> &'static str { "Layout/ArrayAlignment" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut v = Visitor {
            ctx,
            style: self.style,
            indentation_width: self.indentation_width,
            cop_name: self.name(),
            severity: self.severity(),
            offenses: Vec::new(),
            parent_is_masgn: false,
            parent_offset: None,
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: AAStyle,
    indentation_width: usize,
    cop_name: &'static str,
    severity: Severity,
    offenses: Vec<Offense>,
    parent_is_masgn: bool,
    /// Start offset of immediate parent node — used for unbracketed array
    /// `target_method_lineno` fallback.
    parent_offset: Option<usize>,
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode<'_>) {
        let prev_masgn = self.parent_is_masgn;
        let prev_off = self.parent_offset;
        self.parent_is_masgn = true;
        self.parent_offset = Some(node.location().start_offset());
        ruby_prism::visit_multi_write_node(self, node);
        self.parent_is_masgn = prev_masgn;
        self.parent_offset = prev_off;
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'_>) {
        let prev_off = self.parent_offset;
        self.parent_offset = Some(node.location().start_offset());
        ruby_prism::visit_local_variable_write_node(self, node);
        self.parent_offset = prev_off;
    }

    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode<'_>) {
        let prev_off = self.parent_offset;
        self.parent_offset = Some(node.location().start_offset());
        ruby_prism::visit_instance_variable_write_node(self, node);
        self.parent_offset = prev_off;
    }

    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode<'_>) {
        let prev_off = self.parent_offset;
        self.parent_offset = Some(node.location().start_offset());
        ruby_prism::visit_global_variable_write_node(self, node);
        self.parent_offset = prev_off;
    }

    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode<'_>) {
        let prev_off = self.parent_offset;
        self.parent_offset = Some(node.location().start_offset());
        ruby_prism::visit_class_variable_write_node(self, node);
        self.parent_offset = prev_off;
    }

    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode<'_>) {
        let prev_off = self.parent_offset;
        self.parent_offset = Some(node.location().start_offset());
        ruby_prism::visit_constant_write_node(self, node);
        self.parent_offset = prev_off;
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode<'_>) {
        if !self.parent_is_masgn {
            self.check(node);
        }
        let prev_masgn = self.parent_is_masgn;
        let prev_off = self.parent_offset;
        self.parent_is_masgn = false;
        self.parent_offset = Some(node.location().start_offset());
        ruby_prism::visit_array_node(self, node);
        self.parent_is_masgn = prev_masgn;
        self.parent_offset = prev_off;
    }
}

impl<'a> Visitor<'a> {
    fn check(&mut self, node: &ruby_prism::ArrayNode<'_>) {
        let elements: Vec<_> = node.elements().iter().collect();
        if elements.len() < 2 {
            return;
        }

        let items: Vec<(usize, usize)> = elements
            .iter()
            .map(|e| (e.location().start_offset(), e.location().end_offset()))
            .collect();

        let base_column = match self.style {
            AAStyle::WithFirstElement => display_col_of(self.ctx, items[0].0),
            AAStyle::WithFixedIndentation => {
                // target_method_lineno: if bracketed, array's own line; else parent line.
                let target_offset = if node.opening_loc().is_some() {
                    node.location().start_offset()
                } else {
                    self.parent_offset.unwrap_or_else(|| node.location().start_offset())
                };
                display_indent_of(self.ctx, target_offset) + self.indentation_width
            }
        };

        let msg = match self.style {
            AAStyle::WithFirstElement => ALIGN_MSG,
            AAStyle::WithFixedIndentation => FIXED_MSG,
        };

        for m in each_bad_alignment(self.ctx, &items, base_column) {
            self.offenses.push(self.ctx.offense_with_range(
                self.cop_name,
                msg,
                self.severity,
                m.start_offset,
                m.end_offset,
            ));
        }
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
    indentation_width: Option<serde_yaml::Value>,
}

crate::register_cop!("Layout/ArrayAlignment", |cfg| {
    let c: Cfg = cfg.typed("Layout/ArrayAlignment");
    let style = if c.enforced_style == "with_fixed_indentation" {
        AAStyle::WithFixedIndentation
    } else {
        AAStyle::WithFirstElement
    };
    let width = match &c.indentation_width {
        Some(serde_yaml::Value::Number(n)) => n.as_u64().map(|n| n as usize),
        Some(serde_yaml::Value::String(s)) if !s.is_empty() => s.parse::<usize>().ok(),
        _ => None,
    };
    let width = width
        .or_else(|| {
            cfg.get_cop_config("Layout/IndentationWidth")
                .and_then(|c| c.raw.get("Width"))
                .and_then(|v| v.as_u64())
                .map(|n| n as usize)
        })
        .unwrap_or(2);
    Some(Box::new(ArrayAlignment::new(style, width)))
});
