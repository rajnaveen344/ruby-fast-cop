//! Style/EvenOdd cop
//!
//! Checks for places where Integer#even? or Integer#odd? can be used.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{CallNode, Node};

#[derive(Default)]
pub struct EvenOdd;

impl EvenOdd {
    pub fn new() -> Self {
        Self
    }

    /// Try to match `(send $_ :% (int 2)) {== !=} (int {0 1})`
    /// Returns (base_source, replacement_method) or None
    fn check_even_odd_pattern<'a>(node: &CallNode<'a>, source: &str) -> Option<(String, &'static str)> {
        let method = String::from_utf8_lossy(node.name().as_slice());
        if method != "==" && method != "!=" {
            return None;
        }

        // RHS must be int 0 or 1
        let args = node.arguments()?;
        let args_list: Vec<_> = args.arguments().iter().collect();
        if args_list.len() != 1 {
            return None;
        }
        let rhs = &args_list[0];
        let rhs_val = match rhs {
            Node::IntegerNode { .. } => {
                let s = &source[rhs.location().start_offset()..rhs.location().end_offset()];
                s.parse::<u64>().ok()?
            }
            _ => return None,
        };
        if rhs_val != 0 && rhs_val != 1 {
            return None;
        }

        // LHS (receiver) must be `x % 2` possibly wrapped in parens
        let receiver = node.receiver()?;
        let mod_call = extract_mod2_receiver(&receiver)?;

        let base_start = mod_call.location().start_offset();
        let base_end = mod_call.location().end_offset();
        let base_source = source[base_start..base_end].to_string();

        let is_eq = method == "==";
        let replacement = match (rhs_val, is_eq) {
            (0, true) => "even",
            (0, false) => "odd",
            (1, true) => "odd",
            (1, false) => "even",
            _ => unreachable!(),
        };

        Some((base_source, replacement))
    }
}

/// Extract the receiver of `x % 2` — handles both `x % 2` and `(x % 2)` patterns
fn extract_mod2_receiver<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    // Try direct mod call: `x % 2`
    if let Some(mod_node) = try_extract_mod2(node) {
        return Some(mod_node);
    }
    // Try wrapped in parens: `(x % 2)` — ParenthesesNode or BeginNode
    match node {
        Node::ParenthesesNode { .. } => {
            let paren = node.as_parentheses_node().unwrap();
            let body = paren.body()?;
            if let Some(stmts) = body.as_statements_node() {
                let items: Vec<_> = stmts.body().iter().collect();
                if items.len() == 1 {
                    return try_extract_mod2(&items[0]);
                }
            }
            try_extract_mod2(&body)
        }
        Node::BeginNode { .. } => {
            let begin = node.as_begin_node().unwrap();
            if let Some(stmts) = begin.statements() {
                let items: Vec<_> = stmts.body().iter().collect();
                if items.len() == 1 {
                    return try_extract_mod2(&items[0]);
                }
            }
            None
        }
        _ => None,
    }
}

fn try_extract_mod2<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    let call = node.as_call_node()?;
    let method = String::from_utf8_lossy(call.name().as_slice());
    if method != "%" {
        return None;
    }
    let args = call.arguments()?;
    let args_list: Vec<_> = args.arguments().iter().collect();
    if args_list.len() != 1 {
        return None;
    }
    match &args_list[0] {
        Node::IntegerNode { .. } => {
            // Check it's 2
            let s = args_list[0].location();
            let text = &String::from_utf8_lossy(s.as_slice());
            if text.as_ref() != "2" {
                return None;
            }
        }
        _ => return None,
    }
    call.receiver()
}

impl Cop for EvenOdd {
    fn name(&self) -> &'static str {
        "Style/EvenOdd"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if let Some((base_source, method)) = Self::check_even_odd_pattern(node, ctx.source) {
            let msg = format!("Replace with `Integer#{}?`.", method);
            let start = node.location().start_offset();
            let end = node.location().end_offset();
            let _ = base_source; // used in correction (not impl)
            vec![ctx.offense_with_range(self.name(), &msg, self.severity(), start, end)]
        } else {
            vec![]
        }
    }
}

crate::register_cop!("Style/EvenOdd", |_cfg| {
    Some(Box::new(EvenOdd::new()))
});
