//! Style/YodaCondition — Enforces or forbids Yoda conditions.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

const COMPARISON_OPERATORS: &[&str] = &["==", "!=", "<", ">", "<=", ">="];
const EQUALITY_OPERATORS: &[&str] = &["==", "!="];
const NONCOMMUTATIVE_OPERATORS: &[&str] = &["==="];
const PROGRAM_NAMES: &[&str] = &["$0", "$PROGRAM_NAME"];

#[derive(Debug, Clone, PartialEq)]
pub enum EnforcedStyle {
    ForbidForAllComparisonOperators,
    ForbidForEqualityOperatorsOnly,
    RequireForAllComparisonOperators,
    RequireForEqualityOperatorsOnly,
}

pub struct YodaCondition {
    style: EnforcedStyle,
}

impl Default for YodaCondition {
    fn default() -> Self {
        Self {
            style: EnforcedStyle::ForbidForAllComparisonOperators,
        }
    }
}

impl YodaCondition {
    pub fn new(style: EnforcedStyle) -> Self {
        Self { style }
    }

    fn enforce_yoda(&self) -> bool {
        matches!(
            self.style,
            EnforcedStyle::RequireForAllComparisonOperators
                | EnforcedStyle::RequireForEqualityOperatorsOnly
        )
    }

    fn equality_only(&self) -> bool {
        matches!(
            self.style,
            EnforcedStyle::ForbidForEqualityOperatorsOnly
                | EnforcedStyle::RequireForEqualityOperatorsOnly
        )
    }

    /// RuboCop's `constant_portion?` — literal or constant reference.
    fn constant_portion(node: &Node) -> bool {
        match node {
            // Literals
            Node::IntegerNode { .. }
            | Node::FloatNode { .. }
            | Node::RationalNode { .. }
            | Node::ImaginaryNode { .. }
            | Node::StringNode { .. }
            | Node::SymbolNode { .. }
            | Node::RegularExpressionNode { .. }
            | Node::NilNode { .. }
            | Node::TrueNode { .. }
            | Node::FalseNode { .. }
            | Node::ArrayNode { .. }
            | Node::HashNode { .. }
            | Node::RangeNode { .. }
            | Node::SourceLineNode { .. }
            | Node::SourceEncodingNode { .. }
            | Node::SourceFileNode { .. } => true,
            // Constants
            Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. } => true,
            // Unary: -1, +1, etc.
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let method = node_name!(call);
                if (method == "-@" || method == "+@") && call.arguments().is_none() {
                    if let Some(recv) = call.receiver() {
                        return Self::constant_portion(&recv);
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// RuboCop's `interpolation?` — dstr or interpolated regexp.
    fn interpolation(node: &Node) -> bool {
        matches!(
            node,
            Node::InterpolatedStringNode { .. }
                | Node::InterpolatedRegularExpressionNode { .. }
        )
    }

    /// `__FILE__ == $0` or `__FILE__ == $PROGRAM_NAME`
    fn file_constant_equal_program_name(
        call: &ruby_prism::CallNode,
        source: &str,
    ) -> bool {
        let method = node_name!(call);
        if method != "==" && method != "!=" {
            return false;
        }
        let receiver = match call.receiver() {
            Some(r) => r,
            None => return false,
        };
        if !matches!(receiver, Node::SourceFileNode { .. }) {
            return false;
        }
        if let Some(args) = call.arguments() {
            let args_vec: Vec<_> = args.arguments().iter().collect();
            if args_vec.len() == 1 {
                if let Node::GlobalVariableReadNode { .. } = &args_vec[0] {
                    let loc = args_vec[0].location();
                    let name = &source[loc.start_offset()..loc.end_offset()];
                    return PROGRAM_NAMES.contains(&name);
                }
            }
        }
        false
    }
}

impl Cop for YodaCondition {
    fn name(&self) -> &'static str {
        "Style/YodaCondition"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);

        // Must be a comparison operator (not ===)
        if !COMPARISON_OPERATORS.contains(&method.as_ref()) {
            return vec![];
        }
        if NONCOMMUTATIVE_OPERATORS.contains(&method.as_ref()) {
            return vec![];
        }

        // Skip if equality_only mode and non-equality operator
        if self.equality_only() && !EQUALITY_OPERATORS.contains(&method.as_ref()) {
            return vec![];
        }

        // Skip __FILE__ == $0 pattern
        if Self::file_constant_equal_program_name(node, ctx.source) {
            return vec![];
        }

        // Get LHS (receiver) and RHS (first argument)
        let lhs = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let args_vec: Vec<_> = args.arguments().iter().collect();
        if args_vec.len() != 1 {
            return vec![];
        }
        let rhs = &args_vec[0];

        let lhs_const = Self::constant_portion(&lhs);
        let rhs_const = Self::constant_portion(rhs);

        // Both constant or both non-constant => always valid
        if (lhs_const && rhs_const) || (!lhs_const && !rhs_const) {
            return vec![];
        }

        // Interpolated string/regexp on LHS is always allowed
        if Self::interpolation(&lhs) {
            return vec![];
        }

        // Check validity
        let valid = if self.enforce_yoda() {
            lhs_const // enforce yoda: constant should be on left
        } else {
            rhs_const // forbid yoda: constant should be on right
        };

        if valid {
            return vec![];
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let node_source = &ctx.source[start..end];
        let message = format!("Reverse the order of the operands `{}`.", node_source);

        vec![ctx.offense_with_range(self.name(), &message, self.severity(), start, end)]
    }
}
