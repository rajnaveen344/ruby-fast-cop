//! Style/CaseEquality — Avoid use of `===` case equality operator.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/case_equality.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

const MSG: &str = "Avoid the use of the case equality operator `===`.";

pub struct CaseEquality {
    allow_on_constant: bool,
    allow_on_self_class: bool,
}

impl Default for CaseEquality {
    fn default() -> Self {
        Self {
            allow_on_constant: true,
            allow_on_self_class: false,
        }
    }
}

impl CaseEquality {
    pub fn new(allow_on_constant: bool, allow_on_self_class: bool) -> Self {
        Self { allow_on_constant, allow_on_self_class }
    }

    /// Whether a node is a camel-cased constant (e.g. `Array`, `MyClass`).
    fn is_camel_cased_constant(node: &Node) -> bool {
        match node {
            Node::ConstantReadNode { .. } => {
                let n = node.as_constant_read_node().unwrap();
                let name = String::from_utf8_lossy(n.name().as_slice());
                // camel-cased = starts uppercase and contains lowercase
                name.chars().next().map_or(false, |c| c.is_uppercase())
                    && name.chars().any(|c| c.is_lowercase())
            }
            Node::ConstantPathNode { .. } => {
                // e.g. Foo::Bar — check last segment name
                let n = node.as_constant_path_node().unwrap();
                if let Some(name_id) = n.name() {
                    let name = String::from_utf8_lossy(name_id.as_slice());
                    name.chars().next().map_or(false, |c| c.is_uppercase())
                        && name.chars().any(|c| c.is_lowercase())
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Whether receiver is `self.class`.
    fn is_self_class(node: &Node) -> bool {
        let call = match node.as_call_node() {
            Some(c) => c,
            None => return false,
        };
        let method = node_name!(call);
        if method != "class" { return false; }
        matches!(call.receiver(), Some(Node::SelfNode { .. }))
    }

    /// Generate autocorrect: `recv === var` → `recv.include?(var)` or `var.is_a?(recv)`.
    fn correction(
        recv: &Node,
        arg: &Node,
        op_start: usize,
        op_end: usize,
        full_end: usize,
        source: &str,
    ) -> Option<Correction> {
        // Range receiver → recv.include?(arg)
        if matches!(recv, Node::RangeNode { .. } | Node::ParenthesesNode { .. }) {
            // Check it's actually a range (possibly parenthesized)
            let inner_is_range = match recv {
                Node::RangeNode { .. } => true,
                Node::ParenthesesNode { .. } => {
                    let paren = recv.as_parentheses_node().unwrap();
                    let body = paren.body();
                    body.map_or(false, |b| {
                        if let Some(stmts) = b.as_statements_node() {
                            let items: Vec<_> = stmts.body().iter().collect();
                            items.len() == 1 && matches!(items[0], Node::RangeNode { .. })
                        } else {
                            matches!(b, Node::RangeNode { .. })
                        }
                    })
                }
                _ => false,
            };
            if inner_is_range {
                let recv_src = &source[recv.location().start_offset()..recv.location().end_offset()];
                let arg_src = &source[arg.location().start_offset()..arg.location().end_offset()];
                // Replace from op_start-1 (space before ===) to full_end
                // Actually replace op_start..full_end with `.include?(arg)`
                // Full expression: recv_src + " === " + arg_src → recv_src.include?(arg_src)
                let recv_start = recv.location().start_offset();
                let replacement = format!("{recv_src}.include?({arg_src})");
                return Some(Correction::replace(recv_start, full_end, replacement));
            }
        }

        // Constant or self.class receiver → var.is_a?(recv)
        if Self::is_camel_cased_constant(recv) || Self::is_self_class(recv) {
            let recv_src = &source[recv.location().start_offset()..recv.location().end_offset()];
            let arg_src = &source[arg.location().start_offset()..arg.location().end_offset()];
            let recv_start = recv.location().start_offset();
            let replacement = format!("{arg_src}.is_a?({recv_src})");
            return Some(Correction::replace(recv_start, full_end, replacement));
        }

        // No correction for other receivers
        let _ = (op_start, op_end);
        None
    }
}

impl Cop for CaseEquality {
    fn name(&self) -> &'static str {
        "Style/CaseEquality"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if method != "===" { return vec![]; }

        // Must have receiver
        let receiver = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };

        // Skip regexp receivers — RuboCop doesn't flag them
        if matches!(receiver, Node::RegularExpressionNode { .. } | Node::InterpolatedRegularExpressionNode { .. }) {
            return vec![];
        }

        // Skip ALL_CAPS constants (e.g. REGEXP_CONSTANT)
        if let Some(cr) = receiver.as_constant_read_node() {
            let name = String::from_utf8_lossy(cr.name().as_slice());
            if name.chars().all(|c| c.is_uppercase() || c == '_' || c.is_ascii_digit()) {
                return vec![];
            }
        }

        // AllowOnConstant check
        if self.allow_on_constant && Self::is_camel_cased_constant(&receiver) {
            return vec![];
        }

        // AllowOnSelfClass check
        if self.allow_on_self_class && Self::is_self_class(&receiver) {
            return vec![];
        }

        // Get arg (first argument)
        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.is_empty() { return vec![]; }
        let arg = &arg_list[0];

        // Offense on the `===` operator loc
        let op_loc = match node.message_loc() {
            Some(l) => l,
            None => return vec![],
        };
        let op_start = op_loc.start_offset();
        let op_end = op_loc.end_offset();
        let full_end = node.location().end_offset();

        let correction = Self::correction(&receiver, arg, op_start, op_end, full_end, ctx.source);

        let offense = ctx.offense_with_range(self.name(), MSG, self.severity(), op_start, op_end);
        let offense = if let Some(c) = correction {
            offense.with_correction(c)
        } else {
            offense
        };
        vec![offense]
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_on_constant: Option<bool>,
    allow_on_self_class: Option<bool>,
}

crate::register_cop!("Style/CaseEquality", |cfg| {
    let c: Cfg = cfg.typed("Style/CaseEquality");
    Some(Box::new(CaseEquality::new(
        c.allow_on_constant.unwrap_or(true),
        c.allow_on_self_class.unwrap_or(false),
    )))
});
