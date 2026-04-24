//! Style/SwapValues cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct SwapValues;
impl SwapValues { pub fn new() -> Self { Self } }

/// Info about a simple assignment: (lhs_text, rhs_text, rhs_node_start, rhs_node_end).
fn simple_assign<'a>(n: &Node<'a>, source: &str) -> Option<(String, String, Node<'a>)> {
    let (op_loc, value, whole_start) = if let Some(x) = n.as_local_variable_write_node() {
        (x.operator_loc(), x.value(), x.location().start_offset())
    } else if let Some(x) = n.as_instance_variable_write_node() {
        (x.operator_loc(), x.value(), x.location().start_offset())
    } else if let Some(x) = n.as_class_variable_write_node() {
        (x.operator_loc(), x.value(), x.location().start_offset())
    } else if let Some(x) = n.as_global_variable_write_node() {
        (x.operator_loc(), x.value(), x.location().start_offset())
    } else if let Some(x) = n.as_constant_write_node() {
        (x.operator_loc(), x.value(), x.location().start_offset())
    } else if let Some(x) = n.as_constant_path_write_node() {
        (x.operator_loc(), x.value(), x.location().start_offset())
    } else {
        return None;
    };
    let op_start = op_loc.start_offset();
    let lhs = source[whole_start..op_start].trim_end().to_string();
    let v_loc = value.location();
    let rhs = source[v_loc.start_offset()..v_loc.end_offset()].to_string();
    Some((lhs, rhs, value))
}

impl Cop for SwapValues {
    fn name(&self) -> &'static str { "Style/SwapValues" }
    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = V { ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct V<'a> { ctx: &'a CheckContext<'a>, offenses: Vec<Offense> }

impl<'a> V<'a> {
    fn check_stmts(&mut self, stmts: &[Node<'a>]) {
        if stmts.len() < 3 { return; }
        for i in 0..stmts.len().saturating_sub(2) {
            let tmp = &stmts[i];
            let x = &stmts[i + 1];
            let y = &stmts[i + 2];
            let (tmp_lhs, tmp_rhs, _) = match simple_assign(tmp, self.ctx.source) { Some(v) => v, None => continue };
            let (x_lhs, x_rhs, _) = match simple_assign(x, self.ctx.source) { Some(v) => v, None => continue };
            let (y_lhs, y_rhs, _) = match simple_assign(y, self.ctx.source) { Some(v) => v, None => continue };
            if x_lhs != tmp_rhs { continue; }
            if y_lhs != x_rhs { continue; }
            if y_rhs != tmp_lhs { continue; }
            let replacement = format!("{}, {} = {}, {}", x_lhs, x_rhs, x_rhs, x_lhs);
            let x_line = self.line_of(x.location().start_offset());
            let y_line = self.line_of(y.location().start_offset());
            let msg = format!(
                "Replace this and assignments at lines {} and {} with `{}`.",
                x_line, y_line, replacement
            );
            let tmp_loc = tmp.location();
            let start = tmp_loc.start_offset();
            let end = tmp_loc.end_offset();
            // correction range: whole lines from tmp start line_start to y end line_end+1
            let y_end = y.location().end_offset();
            let line_start = self.ctx.source[..start].rfind('\n').map_or(0, |p| p + 1);
            let mut line_end = y_end;
            if let Some(nl) = self.ctx.source[y_end..].find('\n') {
                line_end = y_end + nl + 1;
            }
            let correction = Correction::replace(line_start, line_end, format!("{}\n", replacement));
            self.offenses.push(
                self.ctx.offense_with_range("Style/SwapValues", &msg, Severity::Convention, start, end)
                    .with_correction(correction),
            );
        }
    }

    fn line_of(&self, offset: usize) -> usize {
        self.ctx.source[..offset].bytes().filter(|&b| b == b'\n').count() + 1
    }
}

impl<'a> Visit<'a> for V<'a> {
    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode<'a>) {
        let children: Vec<_> = node.body().iter().collect();
        self.check_stmts(&children);
        ruby_prism::visit_statements_node(self, node);
    }
}

crate::register_cop!("Style/SwapValues", |_cfg| Some(Box::new(SwapValues::new())));
