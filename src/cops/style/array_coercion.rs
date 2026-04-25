//! Style/ArrayCoercion — prefer `Array(x)` over `[*x]` and explicit `Array` checks.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct ArrayCoercion;

impl ArrayCoercion {
    pub fn new() -> Self {
        Self
    }
}

fn node_src<'a>(node: &Node, source: &'a str) -> &'a str {
    let l = node.location();
    &source[l.start_offset()..l.end_offset()]
}

fn lvar_name(node: &Node) -> Option<String> {
    if let Some(n) = node.as_local_variable_read_node() {
        return Some(String::from_utf8_lossy(n.name().as_slice()).into_owned());
    }
    None
}

impl Cop for ArrayCoercion {
    fn name(&self) -> &'static str {
        "Style/ArrayCoercion"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    /// `[*paths]` pattern.
    fn check_array(&self, node: &ruby_prism::ArrayNode, ctx: &CheckContext) -> Vec<Offense> {
        // Must be `[...]` (square brackets), not `%w[]`.
        let opening = match node.opening_loc() {
            Some(o) => o,
            None => return vec![],
        };
        let opening_src = &ctx.source[opening.start_offset()..opening.end_offset()];
        if opening_src != "[" {
            return vec![];
        }

        let elements: Vec<_> = node.elements().iter().collect();
        if elements.len() != 1 {
            return vec![];
        }
        let splat = match elements[0].as_splat_node() {
            Some(s) => s,
            None => return vec![],
        };
        let inner = match splat.expression() {
            Some(e) => e,
            None => return vec![],
        };

        let arg_src = node_src(&inner, ctx.source);
        let nloc = node.location();
        let start = nloc.start_offset();
        let end = nloc.end_offset();
        let replacement = format!("Array({})", arg_src);
        let msg = format!("Use `Array({0})` instead of `[*{0}]`.", arg_src);
        let off = ctx
            .offense_with_range(self.name(), &msg, self.severity(), start, end)
            .with_correction(Correction::replace(start, end, replacement));
        vec![off]
    }

    /// `paths = [paths] unless paths.is_a?(Array)` pattern.
    fn check_unless(&self, node: &ruby_prism::UnlessNode, ctx: &CheckContext) -> Vec<Offense> {
        // Predicate: paths.is_a?(Array)
        let pred_node = node.predicate();
        let call = match pred_node.as_call_node() {
            Some(c) => c,
            None => return vec![],
        };
        let method = String::from_utf8_lossy(call.name().as_slice());
        if method != "is_a?" {
            return vec![];
        }
        // Receiver is local var
        let recv = match call.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        let var_a = match lvar_name(&recv) {
            Some(n) => n,
            None => return vec![],
        };
        // Argument: a single ConstantReadNode :Array
        let args = match call.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return vec![];
        }
        let const_node = match arg_list[0].as_constant_read_node() {
            Some(c) => c,
            None => return vec![],
        };
        let const_name = String::from_utf8_lossy(const_node.name().as_slice());
        if const_name != "Array" {
            return vec![];
        }

        // Body: lvasgn var_b = [lvar var_c]
        let stmts = match node.statements() {
            Some(s) => s,
            None => return vec![],
        };
        let body: Vec<_> = stmts.body().iter().collect();
        if body.len() != 1 {
            return vec![];
        }
        let lvw = match body[0].as_local_variable_write_node() {
            Some(w) => w,
            None => return vec![],
        };
        let var_b = String::from_utf8_lossy(lvw.name().as_slice()).into_owned();
        let value = lvw.value();
        let arr = match value.as_array_node() {
            Some(a) => a,
            None => return vec![],
        };
        let arr_elems: Vec<_> = arr.elements().iter().collect();
        if arr_elems.len() != 1 {
            return vec![];
        }
        let var_c = match lvar_name(&arr_elems[0]) {
            Some(n) => n,
            None => return vec![],
        };

        if var_a != var_b || var_c != var_b {
            return vec![];
        }

        let nloc = node.location();
        let start = nloc.start_offset();
        let end = nloc.end_offset();
        let replacement = format!("{0} = Array({0})", var_a);
        let msg = format!("Use `Array({})` instead of explicit `Array` check.", var_a);
        let off = ctx
            .offense_with_range(self.name(), &msg, self.severity(), start, end)
            .with_correction(Correction::replace(start, end, replacement));
        vec![off]
    }
}

crate::register_cop!("Style/ArrayCoercion", |_cfg| Some(Box::new(ArrayCoercion::new())));
