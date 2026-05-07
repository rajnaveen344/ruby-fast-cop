//! Layout/MultilineAssignmentLayout
//!
//! Checks whether multi-line assignments have a newline after the assignment
//! operator. Two styles: `new_line` (default) and `same_line`.
//!
//! Ported from `rubocop/cop/layout/multiline_assignment_layout.rb`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Layout/MultilineAssignmentLayout";
const NEW_LINE_MSG: &str =
    "Right hand side of multi-line assignment is on the same line as the assignment operator `=`.";
const SAME_LINE_MSG: &str =
    "Right hand side of multi-line assignment is not on the same line as the assignment operator `=`.";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Style {
    NewLine,
    SameLine,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SupportedType {
    Block,  // BLOCK_TYPES = block, numblock, itblock (and lambda)
    Case,
    Class,
    If,
    KwBegin, // begin/end (BeginNode)
    Module,
    Array,
}

pub struct MultilineAssignmentLayout {
    style: Style,
    supported: Vec<SupportedType>,
}

impl MultilineAssignmentLayout {
    pub fn new(style: Style, supported: Vec<SupportedType>) -> Self {
        Self { style, supported }
    }
}

impl Default for MultilineAssignmentLayout {
    fn default() -> Self {
        Self::new(
            Style::NewLine,
            vec![
                SupportedType::Block,
                SupportedType::Case,
                SupportedType::Class,
                SupportedType::If,
                SupportedType::KwBegin,
                SupportedType::Module,
            ],
        )
    }
}

impl Cop for MultilineAssignmentLayout {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a MultilineAssignmentLayout,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    /// Returns (operator_offset, value_node, assignment_node_start, assignment_node_end).
    fn extract_eq<'b>(&self, node: &Node<'b>) -> Option<(usize, Node<'b>, usize, usize)> {
        let nl = node.location();
        let start = nl.start_offset();
        let end = nl.end_offset();
        match node {
            Node::LocalVariableWriteNode { .. } => {
                let n = node.as_local_variable_write_node().unwrap();
                Some((n.operator_loc().start_offset(), n.value(), start, end))
            }
            Node::InstanceVariableWriteNode { .. } => {
                let n = node.as_instance_variable_write_node().unwrap();
                Some((n.operator_loc().start_offset(), n.value(), start, end))
            }
            Node::ClassVariableWriteNode { .. } => {
                let n = node.as_class_variable_write_node().unwrap();
                Some((n.operator_loc().start_offset(), n.value(), start, end))
            }
            Node::GlobalVariableWriteNode { .. } => {
                let n = node.as_global_variable_write_node().unwrap();
                Some((n.operator_loc().start_offset(), n.value(), start, end))
            }
            Node::ConstantWriteNode { .. } => {
                let n = node.as_constant_write_node().unwrap();
                Some((n.operator_loc().start_offset(), n.value(), start, end))
            }
            Node::ConstantPathWriteNode { .. } => {
                let n = node.as_constant_path_write_node().unwrap();
                Some((n.operator_loc().start_offset(), n.value(), start, end))
            }
            Node::MultiWriteNode { .. } => {
                let n = node.as_multi_write_node().unwrap();
                Some((n.operator_loc().start_offset(), n.value(), start, end))
            }
            Node::CallNode { .. } => {
                // Setter / `[]=` -- only when the operator is `=` (attribute write).
                let c = node.as_call_node().unwrap();
                if !c.is_attribute_write() {
                    return None;
                }
                let eq_loc = c.equal_loc()?;
                // value = last argument: `foo.bar = X` -> args=[X];
                // `hash[:foo] = X` -> args=[:foo, X]
                let args = c.arguments()?;
                let v = args.arguments().iter().last()?;
                Some((eq_loc.start_offset(), v, start, end))
            }
            _ => None,
        }
    }

    fn classify(&self, value: &Node<'_>) -> Option<SupportedType> {
        match value {
            Node::IfNode { .. } => Some(SupportedType::If),
            Node::CaseNode { .. } | Node::CaseMatchNode { .. } => Some(SupportedType::Case),
            Node::ClassNode { .. } => Some(SupportedType::Class),
            Node::ModuleNode { .. } => Some(SupportedType::Module),
            Node::BeginNode { .. } => Some(SupportedType::KwBegin),
            Node::ArrayNode { .. } => Some(SupportedType::Array),
            // Block-like RHS:
            Node::LambdaNode { .. } => Some(SupportedType::Block),
            // Call with a block child counts as block in RuboCop's parser AST
            Node::CallNode { .. } => {
                let c = value.as_call_node().unwrap();
                if c.block().is_some() {
                    Some(SupportedType::Block)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Begin location of the value (used for "single-line block on same line as =" exception).
    fn value_begin(&self, value: &Node<'_>, kind: SupportedType) -> Option<usize> {
        // For block-type: begin = block opener `{` or `do` or `->`
        match value {
            Node::LambdaNode { .. } => {
                let l = value.as_lambda_node().unwrap();
                Some(l.opening_loc().start_offset())
            }
            Node::CallNode { .. } if kind == SupportedType::Block => {
                let c = value.as_call_node().unwrap();
                let blk = c.block()?;
                if let Some(b) = blk.as_block_node() {
                    Some(b.opening_loc().start_offset())
                } else {
                    Some(blk.location().start_offset())
                }
            }
            _ => Some(value.location().start_offset()),
        }
    }

    fn check(&mut self, node: &Node<'_>) {
        let (op_off, value, ass_start, ass_end) = match self.extract_eq(node) {
            Some(x) => x,
            None => return,
        };

        // Determine value kind / classify against supported types
        let kind = match self.classify(&value) {
            Some(k) => k,
            None => return,
        };
        if !self.cop.supported.contains(&kind) {
            return;
        }

        // Single-line RHS handling:
        //   skip if rhs.single_line? && (!block? || same_line(node, rhs.loc.begin))
        let value_loc = value.location();
        let rhs_first_line = self.ctx.line_of(value_loc.start_offset());
        let rhs_last_line = self.ctx.line_of(value_loc.end_offset().saturating_sub(1).max(value_loc.start_offset()));
        let rhs_single_line = rhs_first_line == rhs_last_line;
        if rhs_single_line {
            if kind != SupportedType::Block {
                return;
            }
            // For block: skip when assignment-node and block-begin share a line
            let begin_off = match self.value_begin(&value, kind) {
                Some(o) => o,
                None => return,
            };
            let ass_first_line = self.ctx.line_of(ass_start);
            let begin_line = self.ctx.line_of(begin_off);
            if ass_first_line == begin_line {
                return;
            }
        }

        let op_line = self.ctx.line_of(op_off);
        let rhs_line = rhs_first_line;

        match self.cop.style {
            Style::NewLine => {
                if op_line != rhs_line {
                    return;
                }
                // Offense range: assignment node -- but truncated to the assignment node
                // (matches RuboCop `add_offense(node)` which uses the whole node range).
                // Correction: insert newline after `=`
                let correction = Correction::insert(op_off + 1, "\n".to_string());
                self.offenses.push(self.ctx.offense_with_range(
                    COP_NAME,
                    NEW_LINE_MSG,
                    Severity::Convention,
                    ass_start,
                    ass_end,
                ).with_correction(correction));
            }
            Style::SameLine => {
                if op_line == rhs_line {
                    return;
                }
                // Correction: replace whitespace+newline between `=` and value with single space
                let value_start = value_loc.start_offset();
                let correction = Correction::replace(op_off + 1, value_start, " ".to_string());
                self.offenses.push(self.ctx.offense_with_range(
                    COP_NAME,
                    SAME_LINE_MSG,
                    Severity::Convention,
                    ass_start,
                    ass_end,
                ).with_correction(correction));
            }
        }
    }
}

impl Visit<'_> for Visitor<'_> {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        self.check(&node.as_node());
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode) {
        self.check(&node.as_node());
        ruby_prism::visit_instance_variable_write_node(self, node);
    }
    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode) {
        self.check(&node.as_node());
        ruby_prism::visit_class_variable_write_node(self, node);
    }
    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode) {
        self.check(&node.as_node());
        ruby_prism::visit_global_variable_write_node(self, node);
    }
    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        self.check(&node.as_node());
        ruby_prism::visit_constant_write_node(self, node);
    }
    fn visit_constant_path_write_node(&mut self, node: &ruby_prism::ConstantPathWriteNode) {
        self.check(&node.as_node());
        ruby_prism::visit_constant_path_write_node(self, node);
    }
    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        self.check(&node.as_node());
        ruby_prism::visit_multi_write_node(self, node);
    }
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check(&node.as_node());
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Layout/MultilineAssignmentLayout", |cfg| {
    let cc = cfg.get_cop_config("Layout/MultilineAssignmentLayout");
    let style = match cc
        .and_then(|c| c.enforced_style.as_deref())
        .unwrap_or("new_line")
    {
        "same_line" => Style::SameLine,
        _ => Style::NewLine,
    };
    let supported: Vec<SupportedType> = match cc.and_then(|c| c.raw.get("SupportedTypes")) {
        Some(v) if v.is_sequence() => v
            .as_sequence()
            .unwrap()
            .iter()
            .filter_map(|x| x.as_str())
            .filter_map(|s| match s {
                "block" => Some(SupportedType::Block),
                "case" => Some(SupportedType::Case),
                "class" => Some(SupportedType::Class),
                "if" => Some(SupportedType::If),
                "kwbegin" => Some(SupportedType::KwBegin),
                "module" => Some(SupportedType::Module),
                "array" => Some(SupportedType::Array),
                _ => None,
            })
            .collect(),
        _ => vec![
            SupportedType::Block,
            SupportedType::Case,
            SupportedType::Class,
            SupportedType::If,
            SupportedType::KwBegin,
            SupportedType::Module,
        ],
    };
    Some(Box::new(MultilineAssignmentLayout::new(style, supported)))
});
