//! Style/DirEmpty cop
//!
//! `Dir.entries(x).size == 2` / `Dir.children(x).empty?` / `Dir.each_child(x).none?`
//! → `Dir.empty?(x)`.

use crate::cops::{CheckContext, Cop};
use crate::helpers::node_match as m;
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct DirEmpty;

impl DirEmpty {
    pub fn new() -> Self {
        Self
    }
}

fn is_int_value(node: &Node, n: &str) -> bool {
    if let Some(i) = node.as_integer_node() {
        return String::from_utf8_lossy(i.location().as_slice()).trim() == n;
    }
    false
}

fn dir_const(node: &Node) -> Option<String> {
    let name = m::constant_simple_name(node)?;
    if name != "Dir" {
        return None;
    }
    if !m::is_toplevel_constant_named(node, "Dir") {
        return None;
    }
    let loc = node.location();
    Some(String::from_utf8_lossy(loc.as_slice()).to_string())
}

fn arg_src_of(call: &ruby_prism::CallNode, src: &str) -> Option<String> {
    let args = call.arguments()?;
    let list: Vec<_> = args.arguments().iter().collect();
    if list.len() != 1 {
        return None;
    }
    let loc = list[0].location();
    Some(src[loc.start_offset()..loc.end_offset()].to_string())
}

impl Cop for DirEmpty {
    fn name(&self) -> &'static str {
        "Style/DirEmpty"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(2, 4) {
            return vec![];
        }
        let method = node_name!(node).into_owned();

        // Helper to emit.
        let emit = |class_name: &str, arg_src: &str, bang: bool| {
            let replacement = format!(
                "{}{}.empty?({})",
                if bang { "!" } else { "" },
                class_name,
                arg_src
            );
            let msg = format!("Use `{}` instead.", replacement);
            let loc = node.location();
            let start = loc.start_offset();
            let end = loc.end_offset();
            vec![ctx
                .offense_with_range(self.name(), &msg, Severity::Convention, start, end)
                .with_correction(Correction::replace(start, end, replacement))]
        };

        // Pattern 3: Dir.children(x).empty?
        if method == "empty?" {
            let recv = match node.receiver() {
                Some(r) => r,
                None => return vec![],
            };
            let inner = match recv.as_call_node() {
                Some(c) => c,
                None => return vec![],
            };
            if node_name!(inner) != "children" {
                return vec![];
            }
            let inner_recv = match inner.receiver() {
                Some(r) => r,
                None => return vec![],
            };
            let class_name = match dir_const(&inner_recv) {
                Some(v) => v,
                None => return vec![],
            };
            let arg_src = match arg_src_of(&inner, ctx.source) {
                Some(v) => v,
                None => return vec![],
            };
            return emit(&class_name, &arg_src, false);
        }

        // Pattern 4: Dir.each_child(x).none?
        if method == "none?" {
            let recv = match node.receiver() {
                Some(r) => r,
                None => return vec![],
            };
            let inner = match recv.as_call_node() {
                Some(c) => c,
                None => return vec![],
            };
            if node_name!(inner) != "each_child" {
                return vec![];
            }
            // none? with no args
            if node.arguments().is_some() {
                let args = node.arguments().unwrap();
                if args.arguments().iter().next().is_some() {
                    return vec![];
                }
            }
            // Must have no block either.
            if node.block().is_some() {
                return vec![];
            }
            let inner_recv = match inner.receiver() {
                Some(r) => r,
                None => return vec![],
            };
            let class_name = match dir_const(&inner_recv) {
                Some(v) => v,
                None => return vec![],
            };
            let arg_src = match arg_src_of(&inner, ctx.source) {
                Some(v) => v,
                None => return vec![],
            };
            return emit(&class_name, &arg_src, false);
        }

        // Patterns 1 & 2: `Dir.entries(x).size <op> 2` or `Dir.children(x).size <op> 0`
        if !matches!(method.as_str(), "==" | "!=" | ">") {
            return vec![];
        }
        let lhs = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        let rhs_args_node = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let rhs_args: Vec<_> = rhs_args_node.arguments().iter().collect();
        if rhs_args.len() != 1 {
            return vec![];
        }
        // lhs must be (.size)
        let size_call = match lhs.as_call_node() {
            Some(c) => c,
            None => return vec![],
        };
        if node_name!(size_call) != "size" {
            return vec![];
        }
        let inner = match size_call.receiver().and_then(|r| {
            r.as_call_node().map(|c| (c, r.location()))
        }) {
            Some((c, _loc)) => c,
            None => return vec![],
        };
        let inner_name = node_name!(inner).into_owned();
        let (expected_n, needed_inner) = if is_int_value(&rhs_args[0], "2") {
            ("2", "entries")
        } else if is_int_value(&rhs_args[0], "0") {
            ("0", "children")
        } else {
            return vec![];
        };
        if inner_name != needed_inner {
            return vec![];
        }
        let _ = expected_n;
        let inner_recv = match inner.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        let class_name = match dir_const(&inner_recv) {
            Some(v) => v,
            None => return vec![],
        };
        let arg_src = match arg_src_of(&inner, ctx.source) {
            Some(v) => v,
            None => return vec![],
        };
        let bang = matches!(method.as_str(), "!=" | ">");
        emit(&class_name, &arg_src, bang)
    }
}

crate::register_cop!("Style/DirEmpty", |_cfg| Some(Box::new(DirEmpty::new())));
