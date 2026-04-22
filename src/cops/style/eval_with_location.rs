//! Style/EvalWithLocation cop
//!
//! Ensures eval methods include proper filename and line number values.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use crate::helpers::source::line_byte_offset;
use ruby_prism::{Node, Visit};

const MSG_MISSING: &str = "Pass `__FILE__` and `__LINE__` to `%method%`.";
const MSG_MISSING_EVAL: &str = "Pass a binding, `__FILE__`, and `__LINE__` to `eval`.";
const MSG_INCORRECT_FILE: &str =
    "Incorrect file for `%method%`; use `__FILE__` instead of `%actual%`.";
const MSG_INCORRECT_LINE: &str =
    "Incorrect line number for `%method%`; use `%expected%` instead of `%actual%`.";

#[derive(Default)]
pub struct EvalWithLocation;

impl EvalWithLocation {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for EvalWithLocation {
    fn name(&self) -> &'static str {
        "Style/EvalWithLocation"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = EvalWithLocationVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct EvalWithLocationVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> EvalWithLocationVisitor<'a> {
    fn node_src(&self, node: &Node) -> &str {
        let s = node.location().start_offset();
        let e = node.location().end_offset();
        &self.ctx.source[s..e]
    }

    fn is_special_file_keyword(&self, node: &Node) -> bool {
        self.node_src(node) == "__FILE__"
    }

    fn is_special_line_keyword(&self, node: &Node) -> bool {
        self.node_src(node) == "__LINE__"
    }

    fn is_line_with_offset(&self, node: &Node, sign: &str, diff: i64) -> bool {
        // __LINE__ + N or __LINE__ - N or just __LINE__
        let src = self.node_src(node).trim();
        if diff == 0 {
            return src == "__LINE__";
        }
        // e.g. "__LINE__ + 1" or "__LINE__ - 3"
        let expected = format!("__LINE__ {} {}", sign, diff.unsigned_abs());
        src == expected
    }

    fn is_variable_or_method_call(&self, node: &Node) -> bool {
        // Accept variables and non-+ method calls (lineno, calc_line, etc.)
        match node {
            Node::LocalVariableReadNode { .. }
            | Node::InstanceVariableReadNode { .. }
            | Node::GlobalVariableReadNode { .. }
            | Node::ClassVariableReadNode { .. }
            | Node::ConstantReadNode { .. } => true,
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let method = call.name();
                // Allow any method call that isn't `+` or `-`
                let m = method.as_slice();
                m != b"+" && m != b"-"
            }
            _ => false,
        }
    }

    /// Get 1-based line number of a node
    fn line_of(&self, node: &Node) -> usize {
        let offset = node.location().start_offset();
        // Count newlines before offset
        let src = self.ctx.source;
        let bytes = src.as_bytes();
        let mut line = 1usize;
        for &b in &bytes[..offset.min(bytes.len())] {
            if b == b'\n' {
                line += 1;
            }
        }
        line
    }

    /// Get first line of a string literal.
    /// For heredocs, it's the body line (opening marker line + 1).
    /// For regular strings, it's the line of the opening quote.
    fn string_first_line(&self, node: &Node) -> usize {
        match node {
            Node::StringNode { .. } => {
                let sn = node.as_string_node().unwrap();
                if let Some(open_loc) = sn.opening_loc() {
                    let open_src = &self.ctx.source[open_loc.start_offset()..open_loc.end_offset()];
                    // Heredoc starts with <<
                    if open_src.starts_with("<<") {
                        return self.offset_to_line(open_loc.end_offset()) + 1;
                    }
                }
                self.line_of(node)
            }
            Node::InterpolatedStringNode { .. } => {
                let isn = node.as_interpolated_string_node().unwrap();
                if let Some(open_loc) = isn.opening_loc() {
                    let open_src = &self.ctx.source[open_loc.start_offset()..open_loc.end_offset()];
                    if open_src.starts_with("<<") {
                        return self.offset_to_line(open_loc.end_offset()) + 1;
                    }
                }
                self.line_of(node)
            }
            _ => self.line_of(node),
        }
    }

    fn offset_to_line(&self, offset: usize) -> usize {
        let bytes = self.ctx.source.as_bytes();
        let mut line = 1usize;
        for &b in &bytes[..offset.min(bytes.len())] {
            if b == b'\n' { line += 1; }
        }
        line
    }

    fn check_eval_call(&mut self, node: &ruby_prism::CallNode) {
        let method = node.name();
        let method_bytes = method.as_slice();

        let is_plain_eval = method_bytes == b"eval";
        let is_eval_variant = matches!(method_bytes, b"class_eval" | b"module_eval" | b"instance_eval");

        if !is_plain_eval && !is_eval_variant {
            return;
        }

        // For plain eval: only if no receiver or Kernel/::Kernel receiver
        if is_plain_eval {
            if let Some(recv) = node.receiver() {
                let recv_src = self.node_src(&recv);
                if recv_src != "Kernel" && recv_src != "::Kernel" {
                    return;
                }
            }
        }

        // Get arguments
        let args: Vec<_> = node.arguments()
            .map(|a| a.arguments().iter().collect())
            .unwrap_or_default();

        // First arg must be a string literal (str or dstr)
        if args.is_empty() {
            return;
        }
        let code = &args[0];
        if !matches!(code, Node::StringNode { .. } | Node::InterpolatedStringNode { .. }) {
            return;
        }

        // For eval: base index is 2 (after code + binding)
        // For others: base index is 1 (after code)
        let base = if is_plain_eval { 2 } else { 1 };

        let file_arg = args.get(base);
        let line_arg = args.get(base + 1);

        let method_name = String::from_utf8_lossy(method_bytes);

        match (file_arg, line_arg) {
            (None, _) => {
                // Missing both file and line (and possibly binding for eval)
                if is_plain_eval && args.len() < 2 {
                    // No binding either
                    let start = node.location().start_offset();
                    let end = node.location().end_offset();
                    self.offenses.push(self.ctx.offense_with_range(
                        "Style/EvalWithLocation",
                        MSG_MISSING_EVAL,
                        Severity::Convention,
                        start,
                        end,
                    ));
                } else {
                    // Has binding (for eval) or is a variant — missing __FILE__ and __LINE__
                    let msg = if is_plain_eval {
                        MSG_MISSING_EVAL.to_string()
                    } else {
                        format!("Pass `__FILE__` and `__LINE__` to `{}`.", method_name)
                    };
                    let start = node.location().start_offset();
                    let end = node.location().end_offset();
                    self.offenses.push(self.ctx.offense_with_range(
                        "Style/EvalWithLocation",
                        &msg,
                        Severity::Convention,
                        start,
                        end,
                    ));
                }
            }
            (Some(file_node), None) => {
                // Has file but missing line
                // Check file first
                self.check_file_arg(file_node, &method_name);
                // Then report missing line
                let msg = if is_plain_eval {
                    MSG_MISSING_EVAL.to_string()
                } else {
                    format!("Pass `__FILE__` and `__LINE__` to `{}`.", method_name)
                };
                let start = node.location().start_offset();
                let end = node.location().end_offset();
                self.offenses.push(self.ctx.offense_with_range(
                    "Style/EvalWithLocation",
                    &msg,
                    Severity::Convention,
                    start,
                    end,
                ));
            }
            (Some(file_node), Some(line_node)) => {
                // Has both — check correctness
                self.check_file_arg(file_node, &method_name);
                self.check_line_arg(line_node, code, &method_name);
            }
        }
    }

    fn check_file_arg(&mut self, file_node: &Node, method_name: &str) {
        if self.is_special_file_keyword(file_node) {
            return;
        }
        let actual = self.node_src(file_node);
        let msg = format!(
            "Incorrect file for `{}`; use `__FILE__` instead of `{}`.",
            method_name, actual
        );
        let start = file_node.location().start_offset();
        let end = file_node.location().end_offset();
        self.offenses.push(self.ctx.offense_with_range(
            "Style/EvalWithLocation",
            &msg,
            Severity::Convention,
            start,
            end,
        ));
    }

    fn check_line_arg(&mut self, line_node: &Node, code: &Node, method_name: &str) {
        // If it's a variable or non-+ method call, skip
        if self.is_variable_or_method_call(line_node) {
            return;
        }

        let code_first_line = self.string_first_line(code);
        let line_node_line = self.line_of(line_node);
        let line_diff = code_first_line as i64 - line_node_line as i64;

        let (expected, matches) = if line_diff == 0 {
            let matches = self.is_special_line_keyword(line_node);
            ("__LINE__".to_string(), matches)
        } else {
            let sign = if line_diff > 0 { "+" } else { "-" };
            let abs_diff = line_diff.unsigned_abs();
            let expected = format!("__LINE__ {} {}", sign, abs_diff);
            let matches = self.is_line_with_offset(line_node, sign, line_diff.abs());
            (expected, matches)
        };

        if matches {
            return;
        }

        let actual = self.node_src(line_node);
        let msg = format!(
            "Incorrect line number for `{}`; use `{}` instead of `{}`.",
            method_name, expected, actual
        );
        let start = line_node.location().start_offset();
        let end = line_node.location().end_offset();
        self.offenses.push(self.ctx.offense_with_range(
            "Style/EvalWithLocation",
            &msg,
            Severity::Convention,
            start,
            end,
        ));
    }
}

impl<'a> Visit<'_> for EvalWithLocationVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_eval_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/EvalWithLocation", |_cfg| {
    Some(Box::new(EvalWithLocation::new()))
});
