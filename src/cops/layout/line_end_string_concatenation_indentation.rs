//! Layout/LineEndStringConcatenationIndentation - Indentation of strings
//! concatenated with backslash across lines.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/line_end_string_concatenation_indentation.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Layout/LineEndStringConcatenationIndentation";
const MSG_ALIGN: &str = "Align parts of a string concatenated with backslash.";
const MSG_INDENT: &str = "Indent the first part of a string concatenated with backslash.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEndStringConcatenationIndentationStyle {
    Aligned,
    Indented,
}

pub struct LineEndStringConcatenationIndentation {
    style: LineEndStringConcatenationIndentationStyle,
    indent_width: usize,
}

impl LineEndStringConcatenationIndentation {
    pub fn new(style: LineEndStringConcatenationIndentationStyle, indent_width: usize) -> Self {
        Self { style, indent_width }
    }
}

impl Default for LineEndStringConcatenationIndentation {
    fn default() -> Self {
        Self {
            style: LineEndStringConcatenationIndentationStyle::Aligned,
            indent_width: 2,
        }
    }
}

impl Cop for LineEndStringConcatenationIndentation {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        if !ctx.source.contains('\\') {
            return vec![];
        }
        let mut v = Visitor {
            ctx,
            style: self.style,
            indent_width: self.indent_width,
            offenses: Vec::new(),
            parent_stack: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ParentKind {
    Program,
    Block,
    Begin,
    Def,
    If,
    Assoc,
    Other,
}

#[derive(Clone, Copy)]
struct ParentFrame {
    kind: ParentKind,
    /// Source byte offset of this parent's start (used for AssocNode column lookup).
    start_offset: usize,
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: LineEndStringConcatenationIndentationStyle,
    indent_width: usize,
    offenses: Vec<Offense>,
    parent_stack: Vec<ParentFrame>,
}

impl<'a, 'pr> Visit<'pr> for Visitor<'a> {
    fn visit_program_node(&mut self, node: &ruby_prism::ProgramNode<'pr>) {
        self.push(ParentKind::Program, node.location().start_offset());
        ruby_prism::visit_program_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'pr>) {
        self.push(ParentKind::Block, node.location().start_offset());
        ruby_prism::visit_block_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
        self.push(ParentKind::Block, node.location().start_offset());
        ruby_prism::visit_lambda_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode<'pr>) {
        self.push(ParentKind::Begin, node.location().start_offset());
        ruby_prism::visit_begin_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'pr>) {
        self.push(ParentKind::Def, node.location().start_offset());
        ruby_prism::visit_def_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'pr>) {
        self.push(ParentKind::If, node.location().start_offset());
        ruby_prism::visit_if_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode<'pr>) {
        self.push(ParentKind::If, node.location().start_offset());
        ruby_prism::visit_unless_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_else_node(&mut self, node: &ruby_prism::ElseNode<'pr>) {
        self.push(ParentKind::If, node.location().start_offset());
        ruby_prism::visit_else_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_assoc_node(&mut self, node: &ruby_prism::AssocNode<'pr>) {
        self.push(ParentKind::Assoc, node.location().start_offset());
        ruby_prism::visit_assoc_node(self, node);
        self.parent_stack.pop();
    }

    // Wrappers — push Other so the dstr's effective parent isn't an outer always-indented frame.
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_call_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode<'pr>) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_constant_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'pr>) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_local_variable_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode<'pr>) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_instance_variable_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode<'pr>) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_class_variable_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode<'pr>) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_global_variable_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_local_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableOperatorWriteNode<'pr>,
    ) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_local_variable_operator_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_local_variable_or_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableOrWriteNode<'pr>,
    ) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_local_variable_or_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_local_variable_and_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableAndWriteNode<'pr>,
    ) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_local_variable_and_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_constant_operator_write_node(
        &mut self,
        node: &ruby_prism::ConstantOperatorWriteNode<'pr>,
    ) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_constant_operator_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_constant_or_write_node(&mut self, node: &ruby_prism::ConstantOrWriteNode<'pr>) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_constant_or_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_constant_and_write_node(&mut self, node: &ruby_prism::ConstantAndWriteNode<'pr>) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_constant_and_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_instance_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::InstanceVariableOperatorWriteNode<'pr>,
    ) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_instance_variable_operator_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_instance_variable_or_write_node(
        &mut self,
        node: &ruby_prism::InstanceVariableOrWriteNode<'pr>,
    ) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_instance_variable_or_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_instance_variable_and_write_node(
        &mut self,
        node: &ruby_prism::InstanceVariableAndWriteNode<'pr>,
    ) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_instance_variable_and_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_class_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::ClassVariableOperatorWriteNode<'pr>,
    ) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_class_variable_operator_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_class_variable_or_write_node(
        &mut self,
        node: &ruby_prism::ClassVariableOrWriteNode<'pr>,
    ) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_class_variable_or_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_class_variable_and_write_node(
        &mut self,
        node: &ruby_prism::ClassVariableAndWriteNode<'pr>,
    ) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_class_variable_and_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_global_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::GlobalVariableOperatorWriteNode<'pr>,
    ) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_global_variable_operator_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_global_variable_or_write_node(
        &mut self,
        node: &ruby_prism::GlobalVariableOrWriteNode<'pr>,
    ) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_global_variable_or_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_global_variable_and_write_node(
        &mut self,
        node: &ruby_prism::GlobalVariableAndWriteNode<'pr>,
    ) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_global_variable_and_write_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_arguments_node(&mut self, node: &ruby_prism::ArgumentsNode<'pr>) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_arguments_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_parentheses_node(&mut self, node: &ruby_prism::ParenthesesNode<'pr>) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_parentheses_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_hash_node(&mut self, node: &ruby_prism::HashNode<'pr>) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_hash_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode<'pr>) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_array_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_embedded_statements_node(
        &mut self,
        node: &ruby_prism::EmbeddedStatementsNode<'pr>,
    ) {
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_embedded_statements_node(self, node);
        self.parent_stack.pop();
    }

    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode<'pr>) {
        // pass-through: don't change effective parent
        ruby_prism::visit_statements_node(self, node);
    }

    fn visit_interpolated_string_node(
        &mut self,
        node: &ruby_prism::InterpolatedStringNode<'pr>,
    ) {
        self.process_dstr(node);
        self.push(ParentKind::Other, node.location().start_offset());
        ruby_prism::visit_interpolated_string_node(self, node);
        self.parent_stack.pop();
    }
}

impl<'a> Visitor<'a> {
    fn push(&mut self, kind: ParentKind, start_offset: usize) {
        self.parent_stack.push(ParentFrame { kind, start_offset });
    }

    fn parent_frame(&self) -> ParentFrame {
        self.parent_stack
            .last()
            .copied()
            .unwrap_or(ParentFrame { kind: ParentKind::Other, start_offset: 0 })
    }

    fn process_dstr<'pr>(&mut self, node: &ruby_prism::InterpolatedStringNode<'pr>) {
        let parts: Vec<Node<'pr>> = node.parts().iter().collect();
        if parts.len() < 2 {
            return;
        }

        let dstr_loc = node.location();
        let dstr_first_line = self.ctx.line_of(dstr_loc.start_offset());
        let dstr_last_line = self.ctx.line_of(dstr_loc.end_offset().saturating_sub(1));
        if dstr_first_line == dstr_last_line {
            return;
        }
        for c in &parts {
            match c {
                Node::StringNode { .. } | Node::InterpolatedStringNode { .. } => {}
                _ => return,
            }
            let cloc = c.location();
            let cf = self.ctx.line_of(cloc.start_offset());
            let cl = self.ctx.line_of(cloc.end_offset().saturating_sub(1));
            if cf != cl {
                return;
            }
        }

        let parent = self.parent_frame();
        let always_indented = matches!(
            parent.kind,
            ParentKind::Program
                | ParentKind::Block
                | ParentKind::Begin
                | ParentKind::Def
                | ParentKind::If
        );

        if self.style == LineEndStringConcatenationIndentationStyle::Aligned && !always_indented {
            self.check_aligned(&parts, 1);
        } else {
            let target = self.check_indented(&parts, parent);
            // For indented branch, children[2..] should align with the TARGET col of
            // children[1] (the post-correction position), so single-pass apply matches
            // RuboCop's iterated final state.
            self.check_aligned_with_anchor(&parts, 2, target);
        }
    }

    fn child_col(&self, c: &Node) -> usize {
        self.ctx.col_of(c.location().start_offset())
    }

    fn check_aligned(&mut self, children: &[Node], start_index: usize) {
        if children.len() <= start_index {
            return;
        }
        let anchor_col = self.child_col(&children[start_index - 1]);
        let mut base_col = anchor_col;
        let n = children.len();
        let mut i = start_index;
        while i < n {
            let c_col = self.child_col(&children[i]);
            if c_col != base_col {
                // Single offense for children[i]. Multi-edit Correction also retargets
                // subsequent children that share children[i]'s col (cascade).
                let mut extra: Vec<Edit> = Vec::new();
                let mut j = i + 1;
                while j < n && self.child_col(&children[j]) == c_col {
                    extra.push(self.indent_edit(&children[j], anchor_col));
                    j += 1;
                }
                self.add_offense_with_extra_edits(&children[i], MSG_ALIGN, anchor_col, extra);
            }
            base_col = c_col;
            i += 1;
        }
    }

    fn check_indented(&mut self, children: &[Node], parent: ParentFrame) -> usize {
        if children.len() < 2 {
            return 0;
        }
        let base = self.base_column(&children[0], parent);
        let target = base + self.indent_width;
        let actual = self.child_col(&children[1]);
        if actual != target {
            // Cascade: subsequent children sharing children[1]'s original col
            // get retargeted in same correction (matches RuboCop iterated apply).
            let mut extra: Vec<Edit> = Vec::new();
            let mut j = 2;
            while j < children.len() && self.child_col(&children[j]) == actual {
                extra.push(self.indent_edit(&children[j], target));
                j += 1;
            }
            self.add_offense_with_extra_edits(&children[1], MSG_INDENT, target, extra);
        }
        target
    }

    fn check_aligned_with_anchor(&mut self, children: &[Node], start_index: usize, anchor_col: usize) {
        if children.len() <= start_index {
            return;
        }
        let mut base_col = self.child_col(&children[start_index - 1]);
        for c in &children[start_index..] {
            let c_col = self.child_col(c);
            if c_col != base_col {
                self.add_offense_and_correction(c, MSG_ALIGN, anchor_col);
            }
            base_col = c_col;
        }
    }

    fn base_column(&self, first_child: &Node, parent: ParentFrame) -> usize {
        if parent.kind == ParentKind::Assoc {
            return self.ctx.col_of(parent.start_offset);
        }
        self.first_non_ws_col(first_child.location().start_offset())
    }

    fn first_non_ws_col(&self, offset: usize) -> usize {
        let line_start = self.ctx.line_start(offset);
        let bytes = self.ctx.bytes();
        let mut i = line_start;
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
            i += 1;
        }
        i - line_start
    }

    fn indent_edit(&self, target: &Node, want_col: usize) -> Edit {
        let start = target.location().start_offset();
        let line_start = self.ctx.line_start(start);
        Edit {
            start_offset: line_start,
            end_offset: start,
            replacement: " ".repeat(want_col),
        }
    }

    fn add_offense_and_correction(&mut self, target: &Node, message: &str, want_col: usize) {
        self.add_offense_with_extra_edits(target, message, want_col, Vec::new());
    }

    fn add_offense_with_extra_edits(
        &mut self,
        target: &Node,
        message: &str,
        want_col: usize,
        mut extra: Vec<Edit>,
    ) {
        let loc = target.location();
        let start = loc.start_offset();
        let end = loc.end_offset();
        let mut edits = vec![self.indent_edit(target, want_col)];
        edits.append(&mut extra);

        let offense = self
            .ctx
            .offense_with_range(COP_NAME, message, Severity::Convention, start, end)
            .with_correction(Correction { edits });
        self.offenses.push(offense);
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: Option<String>,
}

crate::register_cop!("Layout/LineEndStringConcatenationIndentation", |cfg| {
    let c: Cfg = cfg.typed("Layout/LineEndStringConcatenationIndentation");
    let style = match c.enforced_style.as_deref() {
        Some("indented") => LineEndStringConcatenationIndentationStyle::Indented,
        _ => LineEndStringConcatenationIndentationStyle::Aligned,
    };
    let indent_width = cfg
        .get_cop_config("Layout/LineEndStringConcatenationIndentation")
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
    Some(Box::new(LineEndStringConcatenationIndentation::new(
        style,
        indent_width,
    )))
});
