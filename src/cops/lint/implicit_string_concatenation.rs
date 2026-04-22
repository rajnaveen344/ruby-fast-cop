//! Lint/ImplicitStringConcatenation - Warn about implicit string concatenation on same line.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Combine %lhs and %rhs into a single string literal, rather than using implicit string concatenation.";
const FOR_ARRAY: &str = " Or, if they were intended to be separate array elements, separate them with a comma.";
const FOR_METHOD: &str = " Or, if they were intended to be separate method arguments, separate them with a comma.";

#[derive(Default)]
pub struct ImplicitStringConcatenation;

impl ImplicitStringConcatenation {
    pub fn new() -> Self { Self }
}

impl Cop for ImplicitStringConcatenation {
    fn name(&self) -> &'static str { "Lint/ImplicitStringConcatenation" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl Visit<'_> for Visitor<'_> {
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        // InterpolatedStringNode can represent implicit concatenation: "abc" "def" → dstr [str, str]
        self.check_dstr(node);
        ruby_prism::visit_interpolated_string_node(self, node);
    }
}

impl<'a> Visitor<'a> {
    fn check_dstr(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        let parts: Vec<_> = node.parts().iter().collect();

        // Check consecutive pairs
        let mut i = 0;
        while i + 1 < parts.len() {
            let lhs = &parts[i];
            let rhs = &parts[i + 1];

            if is_string_literal(lhs) && is_string_literal(rhs) {
                // RuboCop: child_node1.last_line == child_node2.first_line
                let lhs_end = lhs.location().end_offset();
                let lhs_last_line = self.ctx.line_of(lhs_end.saturating_sub(1));
                let rhs_first_line = self.ctx.line_of(rhs.location().start_offset());

                if lhs_last_line == rhs_first_line {
                    // Ensure lhs source ends with its closing delimiter
                    let lhs_loc = lhs.location();
                    let lhs_src = match self.ctx.source.get(lhs_loc.start_offset()..lhs_loc.end_offset()) {
                        Some(s) => s,
                        None => { i += 1; continue; }
                    };
                    if !ends_with_delimiter(lhs_src) {
                        i += 1;
                        continue;
                    }

                    let rhs_loc = rhs.location();
                    let range_start = lhs_loc.start_offset();
                    let range_end = rhs_loc.end_offset();

                    let lhs_display = display_str(lhs, self.ctx.source);
                    let rhs_display = display_str(rhs, self.ctx.source);
                    let mut msg = MSG
                        .replace("%lhs", &lhs_display)
                        .replace("%rhs", &rhs_display);

                    // Check parent context for array/method
                    // Simple heuristic: check surrounding context via the dstr's parent
                    // We'll check after visiting — for now append suffix based on dstr parent
                    // (parent info not available here; use source heuristic)
                    let node_loc = node.location();
                    let ctx_src = &self.ctx.source;
                    let in_array = is_in_array_context(ctx_src, node_loc.start_offset());
                    let in_method = is_in_method_context(ctx_src, node_loc.start_offset());

                    if in_array {
                        msg.push_str(FOR_ARRAY);
                    } else if in_method {
                        msg.push_str(FOR_METHOD);
                    }

                    // Correction
                    let lhs_val = string_value(lhs);
                    let rhs_val = string_value(rhs);

                    let mut offense = self.ctx.offense_with_range(
                        "Lint/ImplicitStringConcatenation",
                        &msg,
                        Severity::Warning,
                        range_start,
                        range_end,
                    );

                    // Apply correction: insert " + " between lhs and rhs, or remove empty string
                    let correction = if lhs_val.as_deref() == Some("") {
                        // lhs is empty: remove lhs
                        Correction::delete(lhs_loc.start_offset(), rhs_loc.start_offset())
                    } else if rhs_val.as_deref() == Some("") {
                        // rhs is empty: remove rhs
                        Correction::delete(lhs_loc.end_offset(), rhs_loc.end_offset())
                    } else {
                        // Insert " + " between lhs end and rhs start
                        Correction::replace(lhs_loc.end_offset(), rhs_loc.start_offset(), " + ".to_string())
                    };
                    offense = offense.with_correction(correction);
                    self.offenses.push(offense);
                }
            }
            i += 1;
        }
    }
}

fn is_string_literal(node: &Node) -> bool {
    matches!(node, Node::StringNode { .. } | Node::InterpolatedStringNode { .. })
}

fn ends_with_delimiter(src: &str) -> bool {
    let bytes = src.as_bytes();
    if bytes.is_empty() { return false; }
    let last = bytes[bytes.len() - 1];
    last == b'\'' || last == b'"'
}

fn display_str(node: &Node, source: &str) -> String {
    let loc = node.location();
    let src = match source.get(loc.start_offset()..loc.end_offset()) {
        Some(s) => s,
        None => return String::new(),
    };
    if src.contains('\n') {
        // Show inspect of content
        let content = string_content(node);
        format!("{:?}", content)
    } else {
        src.to_string()
    }
}

fn string_content(node: &Node) -> String {
    match node {
        Node::StringNode { .. } => {
            let sn = node.as_string_node().unwrap();
            String::from_utf8_lossy(sn.unescaped()).to_string()
        }
        Node::InterpolatedStringNode { .. } => {
            let isn = node.as_interpolated_string_node().unwrap();
            isn.parts().iter().map(|p| string_content(&p)).collect()
        }
        _ => String::new(),
    }
}

fn string_value(node: &Node) -> Option<String> {
    match node {
        Node::StringNode { .. } => {
            let sn = node.as_string_node().unwrap();
            Some(String::from_utf8_lossy(sn.unescaped()).to_string())
        }
        _ => None,
    }
}

fn is_in_array_context(source: &str, start: usize) -> bool {
    // Scan backwards for [ not closed before start
    let before = &source[..start];
    let bytes = before.as_bytes();
    let mut depth = 0i32;
    for &b in bytes.iter().rev() {
        match b {
            b']' => depth += 1,
            b'[' => {
                if depth == 0 { return true; }
                depth -= 1;
            }
            _ => {}
        }
    }
    false
}

fn is_in_method_context(source: &str, start: usize) -> bool {
    let before = &source[..start];
    let bytes = before.as_bytes();
    let mut depth = 0i32;
    for &b in bytes.iter().rev() {
        match b {
            b')' => depth += 1,
            b'(' => {
                if depth == 0 { return true; }
                depth -= 1;
            }
            _ => {}
        }
    }
    false
}

crate::register_cop!("Lint/ImplicitStringConcatenation", |_cfg| Some(Box::new(ImplicitStringConcatenation::new())));
