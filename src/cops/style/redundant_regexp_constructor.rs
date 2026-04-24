//! Style/RedundantRegexpConstructor - Detects `Regexp.new(/.../)` and `Regexp.compile(/.../)`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/redundant_regexp_constructor.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct RedundantRegexpConstructor;

impl RedundantRegexpConstructor {
    pub fn new() -> Self {
        Self
    }
}

fn is_regexp_const(recv: &Node) -> bool {
    if let Some(c) = recv.as_constant_read_node() {
        return String::from_utf8_lossy(c.name().as_slice()) == "Regexp";
    }
    if let Some(cp) = recv.as_constant_path_node() {
        let name = match cp.name() {
            Some(n) => n,
            None => return false,
        };
        if String::from_utf8_lossy(name.as_slice()) != "Regexp" {
            return false;
        }
        return cp.parent().is_none();
    }
    false
}

impl Cop for RedundantRegexpConstructor {
    fn name(&self) -> &'static str {
        "Style/RedundantRegexpConstructor"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        let method_str = method.as_ref();
        if method_str != "new" && method_str != "compile" {
            return vec![];
        }

        let recv = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        if !is_regexp_const(&recv) {
            return vec![];
        }

        let arg_list: Vec<_> = match node.arguments() {
            Some(a) => a.arguments().iter().collect(),
            None => return vec![],
        };
        if arg_list.len() != 1 {
            return vec![];
        }

        // Arg must be a regexp literal (non-interpolated OR interpolated).
        let arg = &arg_list[0];
        let regexp_src = match arg {
            Node::RegularExpressionNode { .. } => {
                let re = arg.as_regular_expression_node().unwrap();
                let l = re.location();
                ctx.source[l.start_offset()..l.end_offset()].to_string()
            }
            Node::InterpolatedRegularExpressionNode { .. } => {
                let re = arg.as_interpolated_regular_expression_node().unwrap();
                let l = re.location();
                ctx.source[l.start_offset()..l.end_offset()].to_string()
            }
            _ => return vec![],
        };

        let call_loc = node.location();
        let call_start = call_loc.start_offset();
        let call_end = call_loc.end_offset();
        let msg = format!("Remove the redundant `Regexp.{}`.", method_str);
        vec![ctx
            .offense_with_range(self.name(), &msg, self.severity(), call_start, call_end)
            .with_correction(Correction::replace(call_start, call_end, regexp_src))]
    }
}

crate::register_cop!("Style/RedundantRegexpConstructor", |_cfg| Some(Box::new(RedundantRegexpConstructor::new())));
