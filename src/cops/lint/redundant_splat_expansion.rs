//! Lint/RedundantSplatExpansion - Checks for unneeded usages of splat expansion.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Replace splat expansion with comma separated values.";
const ARRAY_PARAM_MSG: &str = "Pass array contents as separate arguments.";

pub struct RedundantSplatExpansion {
    allow_percent_literal_array_argument: bool,
}

impl RedundantSplatExpansion {
    pub fn new(allow_percent_literal_array_argument: bool) -> Self {
        Self { allow_percent_literal_array_argument }
    }
}

impl Default for RedundantSplatExpansion {
    fn default() -> Self { Self::new(true) }
}

impl Cop for RedundantSplatExpansion {
    fn name(&self) -> &'static str { "Lint/RedundantSplatExpansion" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = SplatVisitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ParentKind {
    Call,
    BracketedArray,
    UnbracketedArray,
    When,
    Rescue,
    Assignment,
    #[allow(dead_code)]
    Other,
}

struct SplatVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a RedundantSplatExpansion,
    offenses: Vec<Offense>,
}

impl<'a> SplatVisitor<'a> {
    fn check_splat(&mut self, node: &ruby_prism::SplatNode, parent_kind: ParentKind, parent_array_size: usize, parent_array_loc: Option<(usize, usize)>) {
        let expression = match node.expression() {
            Some(e) => e,
            None => return,
        };

        // Check if the splatted expression is a literal type we flag
        let is_literal_expansion = match &expression {
            Node::StringNode { .. }
            | Node::InterpolatedStringNode { .. }
            | Node::IntegerNode { .. }
            | Node::FloatNode { .. }
            | Node::ArrayNode { .. } => true,
            Node::CallNode { .. } => is_array_new_expr(&expression),
            _ => false,
        };

        if !is_literal_expansion {
            return;
        }

        // For Array.new calls: check context restrictions
        if is_array_new_expr(&expression) {
            // Array.new inside array literal with >1 elements: allow
            if (parent_kind == ParentKind::BracketedArray || parent_kind == ParentKind::UnbracketedArray)
                && parent_array_size > 1
            {
                return;
            }

            // Array.new not in assignment context and not in special positions
            if parent_kind != ParentKind::Assignment
                && parent_kind != ParentKind::Call
                && parent_kind != ParentKind::BracketedArray
                && parent_kind != ParentKind::UnbracketedArray
            {
                return;
            }
        }

        // Handle Array.new special case:
        // `[*Array.new(foo)]` → `Array.new(foo)` (replace parent array span)
        if is_array_new_expr(&expression) {
            let expr_src = &self.ctx.source[expression.location().start_offset()..expression.location().end_offset()];
            let (corr_start, corr_end) = if parent_kind == ParentKind::BracketedArray {
                // Replace parent array: `[*Array.new(foo)]` → `Array.new(foo)`
                parent_array_loc.unwrap_or((node.location().start_offset(), node.location().end_offset()))
            } else {
                // In call args: `send(*Array.new(foo))` → `send(Array.new(foo))`
                // Just replace the splat node
                (node.location().start_offset(), node.location().end_offset())
            };
            let correction = Correction::replace(corr_start, corr_end, expr_src);
            let loc = node.location();
            self.offenses.push(self.ctx.offense_with_range(
                self.cop.name(),
                MSG,
                self.cop.severity(),
                loc.start_offset(),
                loc.end_offset(),
            ).with_correction(correction));
            return;
        }

        let is_array_splat = matches!(&expression, Node::ArrayNode { .. });
        let is_method_arg = parent_kind == ParentKind::Call;
        let is_part_of_array = parent_kind == ParentKind::BracketedArray;
        let is_when = parent_kind == ParentKind::When;
        let is_rescue = parent_kind == ParentKind::Rescue;
        // redundant_brackets? = when/call/bracketed_array/rescue context
        let redundant_brackets = is_when || is_method_arg || is_part_of_array || is_rescue;
        // wrap_in_brackets? = unbracket array context (assignment/unbracket array)
        let wrap_in_brackets = !redundant_brackets && !is_part_of_array;

        let loc = node.location();

        if is_array_splat && (is_method_arg || is_part_of_array) {
            // Check AllowPercentLiteralArrayArgument
            if self.cop.allow_percent_literal_array_argument && is_method_arg {
                if is_percent_literal_array(&expression, self.ctx) {
                    return;
                }
            }
            // Correction: expand array elements inline (remove_brackets)
            let replacement = remove_brackets_str(&expression, self.ctx.source);
            let correction = Correction::replace(loc.start_offset(), loc.end_offset(), replacement);
            self.offenses.push(self.ctx.offense_with_range(
                self.cop.name(),
                ARRAY_PARAM_MSG,
                self.cop.severity(),
                loc.start_offset(),
                loc.end_offset(),
            ).with_correction(correction));
        } else if is_array_splat && is_when {
            // When/rescue: expand array elements
            let replacement = remove_brackets_str(&expression, self.ctx.source);
            let correction = Correction::replace(loc.start_offset(), loc.end_offset(), replacement);
            self.offenses.push(self.ctx.offense_with_range(
                self.cop.name(),
                MSG,
                self.cop.severity(),
                loc.start_offset(),
                loc.end_offset(),
            ).with_correction(correction));
        } else if is_array_splat && is_rescue {
            // Rescue: expand array elements
            let replacement = remove_brackets_str(&expression, self.ctx.source);
            let correction = Correction::replace(loc.start_offset(), loc.end_offset(), replacement);
            self.offenses.push(self.ctx.offense_with_range(
                self.cop.name(),
                MSG,
                self.cop.severity(),
                loc.start_offset(),
                loc.end_offset(),
            ).with_correction(correction));
        } else if is_array_splat {
            // Assignment context: delete `*` only (keep the `[...]`)
            // splat operator is one byte `*`
            let star_end = loc.start_offset() + 1;
            let correction = Correction::delete(loc.start_offset(), star_end);
            self.offenses.push(self.ctx.offense_with_range(
                self.cop.name(),
                MSG,
                self.cop.severity(),
                loc.start_offset(),
                loc.end_offset(),
            ).with_correction(correction));
        } else {
            // Non-array splat (`*'a'`, `*1`, etc.)
            let expr_src = &self.ctx.source[expression.location().start_offset()..expression.location().end_offset()];
            // wrap_in_brackets? in RuboCop = parent is an unbracketed array (i.e., assignment context)
            let should_wrap = parent_kind == ParentKind::Assignment || parent_kind == ParentKind::UnbracketedArray;
            let replacement = if should_wrap {
                format!("[{}]", expr_src)
            } else {
                expr_src.to_string()
            };
            let correction = Correction::replace(loc.start_offset(), loc.end_offset(), replacement);
            self.offenses.push(self.ctx.offense_with_range(
                self.cop.name(),
                MSG,
                self.cop.severity(),
                loc.start_offset(),
                loc.end_offset(),
            ).with_correction(correction));
        }
    }

    fn check_splats_in_array(&mut self, node: &ruby_prism::ArrayNode) {
        let elements: Vec<_> = node.elements().iter().collect();
        let is_bracketed = node.opening_loc().is_some();
        let parent_kind = if is_bracketed {
            ParentKind::BracketedArray
        } else {
            ParentKind::UnbracketedArray
        };
        let size = elements.len();
        let array_loc = Some((node.location().start_offset(), node.location().end_offset()));
        for elem in &elements {
            if let Some(splat) = elem.as_splat_node() {
                self.check_splat(&splat, parent_kind, size, array_loc);
            }
        }
    }

    fn check_splats_in_call_args(&mut self, node: &ruby_prism::CallNode) {
        if let Some(args) = node.arguments() {
            for arg in args.arguments().iter() {
                if let Some(splat) = arg.as_splat_node() {
                    self.check_splat(&splat, ParentKind::Call, 0, None);
                }
            }
        }
    }

    fn check_splat_in_assignment(&mut self, value: &Node) {
        if let Some(splat) = value.as_splat_node() {
            self.check_splat(&splat, ParentKind::Assignment, 0, None);
        }
    }
}

impl Visit<'_> for SplatVisitor<'_> {
    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        self.check_splats_in_array(node);
        ruby_prism::visit_array_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_splats_in_call_args(node);
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_when_node(&mut self, node: &ruby_prism::WhenNode) {
        for cond in node.conditions().iter() {
            if let Some(splat) = cond.as_splat_node() {
                self.check_splat(&splat, ParentKind::When, 0, None);
            }
        }
        ruby_prism::visit_when_node(self, node);
    }

    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode) {
        for exc in node.exceptions().iter() {
            if let Some(splat) = exc.as_splat_node() {
                self.check_splat(&splat, ParentKind::Rescue, 0, None);
            }
        }
        ruby_prism::visit_rescue_node(self, node);
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        self.check_splat_in_assignment(&node.value());
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode) {
        self.check_splat_in_assignment(&node.value());
        ruby_prism::visit_instance_variable_write_node(self, node);
    }

    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode) {
        self.check_splat_in_assignment(&node.value());
        ruby_prism::visit_class_variable_write_node(self, node);
    }

    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode) {
        self.check_splat_in_assignment(&node.value());
        ruby_prism::visit_global_variable_write_node(self, node);
    }

    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        self.check_splat_in_assignment(&node.value());
        ruby_prism::visit_constant_write_node(self, node);
    }
}

/// Convert array contents to comma-separated representation for removal of splat brackets.
/// Mirrors RuboCop's `remove_brackets` method.
fn remove_brackets_str(array_node: &Node, source: &str) -> String {
    let arr = match array_node.as_array_node() {
        Some(a) => a,
        None => return source[array_node.location().start_offset()..array_node.location().end_offset()].to_string(),
    };

    // Get the opening bracket source to detect %w/%W/%i/%I
    let opening = arr.opening_loc().map(|loc| {
        source[loc.start_offset()..loc.end_offset()].to_string()
    }).unwrap_or_default();

    let elements: Vec<_> = arr.elements().iter().collect();

    if opening.starts_with("%w") || opening.starts_with("%W") {
        // %w or %W: elements are plain words, convert to string literals
        let use_double = opening.starts_with("%W");
        let mut parts = Vec::new();
        for elem in &elements {
            let elem_src = source[elem.location().start_offset()..elem.location().end_offset()].to_string();
            if use_double {
                parts.push(format!("\"{}\"", elem_src));
            } else {
                parts.push(format!("'{}'", elem_src));
            }
        }
        parts.join(", ")
    } else if opening.starts_with("%i") {
        // %i: plain symbols → :word
        let mut parts = Vec::new();
        for elem in &elements {
            let elem_src = source[elem.location().start_offset()..elem.location().end_offset()].to_string();
            parts.push(format!(":{}", elem_src));
        }
        parts.join(", ")
    } else if opening.starts_with("%I") {
        // %I: interpolated symbols → :"word"
        let mut parts = Vec::new();
        for elem in &elements {
            let elem_src = source[elem.location().start_offset()..elem.location().end_offset()].to_string();
            let formatted = format!(":\"{}\"", elem_src);
            parts.push(formatted);
        }
        parts.join(", ")
    } else {
        // Regular array [a, b, c] → a, b, c
        let mut parts = Vec::new();
        for elem in &elements {
            let elem_src = source[elem.location().start_offset()..elem.location().end_offset()].to_string();
            parts.push(elem_src);
        }
        parts.join(", ")
    }
}

fn is_array_new_call(node: &ruby_prism::CallNode) -> bool {
    let method = String::from_utf8_lossy(node.name().as_slice());
    if method.as_ref() != "new" {
        return false;
    }
    if let Some(recv) = node.receiver() {
        return is_array_const(&recv);
    }
    false
}

fn is_array_new_expr(node: &Node) -> bool {
    // In Prism, Array.new { ... } is a CallNode with a block child
    if let Some(call) = node.as_call_node() {
        is_array_new_call(&call)
    } else {
        false
    }
}

fn is_array_const(node: &Node) -> bool {
    match node {
        Node::ConstantReadNode { .. } => {
            let name = String::from_utf8_lossy(node.as_constant_read_node().unwrap().name().as_slice());
            name.as_ref() == "Array"
        }
        Node::ConstantPathNode { .. } => {
            let path = node.as_constant_path_node().unwrap();
            if let Some(name_bytes) = path.name() {
                let name = String::from_utf8_lossy(name_bytes.as_slice());
                if name.as_ref() != "Array" {
                    return false;
                }
                // cbase (::Array) - parent is None
                path.parent().is_none()
            } else {
                false
            }
        }
        _ => false,
    }
}

fn is_percent_literal_array(node: &Node, ctx: &CheckContext) -> bool {
    if let Some(arr) = node.as_array_node() {
        let src = &ctx.source[arr.location().start_offset()..arr.location().end_offset()];
        src.starts_with("%w") || src.starts_with("%W") || src.starts_with("%i") || src.starts_with("%I")
    } else {
        false
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_percent_literal_array_argument: bool,
}

impl Default for Cfg {
    fn default() -> Self {
        Self { allow_percent_literal_array_argument: true }
    }
}

crate::register_cop!("Lint/RedundantSplatExpansion", |cfg| {
    let c: Cfg = cfg.typed("Lint/RedundantSplatExpansion");
    Some(Box::new(RedundantSplatExpansion::new(c.allow_percent_literal_array_argument)))
});
