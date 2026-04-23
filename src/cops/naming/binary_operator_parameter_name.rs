//! Naming/BinaryOperatorParameterName cop
//! When defining binary operators, name the argument `other`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/naming/binary_operator_parameter_name.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

// "OP_LIKE_METHODS" that are word-based but still count as operators
const OP_LIKE_METHODS: &[&str] = &["eql?", "equal?"];

// Operators excluded from the check (EXCLUDED in RuboCop)
const EXCLUDED: &[&str] = &["+@", "-@", "[]", "[]=", "<<", "===", "`", "=~"];

#[derive(Default)]
pub struct BinaryOperatorParameterName;

impl BinaryOperatorParameterName {
    pub fn new() -> Self { Self }
}

impl Cop for BinaryOperatorParameterName {
    fn name(&self) -> &'static str { "Naming/BinaryOperatorParameterName" }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = BinaryOpVisitor {
            source: ctx.source,
            offenses: Vec::new(),
            ctx,
        };
        let result = ruby_prism::parse(ctx.source.as_bytes());
        visitor.visit(&result.node());
        visitor.offenses
    }
}

struct BinaryOpVisitor<'a> {
    source: &'a str,
    offenses: Vec<Offense>,
    ctx: &'a CheckContext<'a>,
}

impl Visit<'_> for BinaryOpVisitor<'_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let method_name = node_name!(node);
        let method_str = method_name.as_ref();

        // Skip excluded operators
        if EXCLUDED.contains(&method_str) {
            ruby_prism::visit_def_node(self, node);
            return;
        }

        // Check if this is a binary operator method:
        // - Non-word operator (like +, -, *, etc.) but not excluded
        // - OR one of the OP_LIKE_METHODS (eql?, equal?)
        let is_op_method = if OP_LIKE_METHODS.contains(&method_str) {
            true
        } else {
            // Does not start with a word character (letter, digit, _)
            method_str.chars().next().map(|c| !c.is_alphanumeric() && c != '_').unwrap_or(false)
        };

        if !is_op_method {
            ruby_prism::visit_def_node(self, node);
            return;
        }

        // Check that the method has exactly one required parameter named `other` or `_other`
        let params = match node.parameters() {
            Some(p) => p,
            None => {
                ruby_prism::visit_def_node(self, node);
                return;
            }
        };

        let requireds: Vec<_> = params.requireds().iter().collect();
        let optionals: Vec<_> = params.optionals().iter().collect();

        // Method must have exactly 1 param (required or optional)
        if requireds.len() + optionals.len() != 1 {
            ruby_prism::visit_def_node(self, node);
            return;
        }

        let (param_name, param_loc) = if !requireds.is_empty() {
            let p = requireds[0].as_required_parameter_node().unwrap();
            let name = node_name!(p);
            let loc = p.location();
            (name, loc)
        } else {
            // optional param
            let p = optionals[0].as_optional_parameter_node().unwrap();
            let name = node_name!(p);
            let loc = p.name_loc();
            (name, loc)
        };

        let name_str = param_name.as_ref();
        if name_str == "other" || name_str == "_other" {
            ruby_prism::visit_def_node(self, node);
            return;
        }

        // Offense: rename the parameter to `other`
        let msg = format!(
            "When defining the `{}` operator, name its argument `other`.",
            method_str
        );

        // Also need to rename all references in the body
        // Collect body source range to replace all occurrences of old name → other
        let body_start = node.location().start_offset();
        let body_end = node.location().end_offset();
        let body_src = &self.source[body_start..body_end];
        let new_body = replace_identifier(body_src, name_str, "other");

        let offense = self.ctx.offense_with_range(
            "Naming/BinaryOperatorParameterName", &msg, Severity::Convention,
            param_loc.start_offset(),
            param_loc.end_offset(),
        ).with_correction(Correction::replace(body_start, body_end, new_body));

        self.offenses.push(offense);
        ruby_prism::visit_def_node(self, node);
    }
}

/// Replace all whole-word occurrences of `old` with `new` in `src`.
fn replace_identifier(src: &str, old: &str, new: &str) -> String {
    let mut result = String::with_capacity(src.len());
    let old_bytes = old.as_bytes();
    let src_bytes = src.as_bytes();
    let mut i = 0;

    while i < src_bytes.len() {
        if src_bytes[i..].starts_with(old_bytes) {
            let before_ok = i == 0 || !is_ident_char(src_bytes[i-1]);
            let after_ok = i + old_bytes.len() >= src_bytes.len()
                || !is_ident_char(src_bytes[i + old_bytes.len()]);
            if before_ok && after_ok {
                result.push_str(new);
                i += old_bytes.len();
                continue;
            }
        }
        result.push(src_bytes[i] as char);
        i += 1;
    }
    result
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

crate::register_cop!("Naming/BinaryOperatorParameterName", |_cfg| Some(Box::new(BinaryOperatorParameterName::new())));
