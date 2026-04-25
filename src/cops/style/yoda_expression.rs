//! Style/YodaExpression — flags `1 + x`-style binary ops with literal/const on left.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct YodaExpression {
    supported_operators: Vec<String>,
}

impl YodaExpression {
    pub fn new() -> Self {
        Self {
            supported_operators: vec!["*", "+", "&", "|", "^"]
                .into_iter()
                .map(String::from)
                .collect(),
        }
    }

    pub fn with_config(operators: Vec<String>) -> Self {
        Self { supported_operators: operators }
    }
}

impl Cop for YodaExpression {
    fn name(&self) -> &'static str {
        "Style/YodaExpression"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor {
            ctx,
            offenses: Vec::new(),
            ops: &self.supported_operators,
            offended_ranges: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    ops: &'a [String],
    /// (start, end) byte ranges of already-flagged ancestor send nodes.
    offended_ranges: Vec<(usize, usize)>,
}

/// Recursively transform a node's source: if it's (or contains) a yoda call,
/// produce the swapped form. Otherwise return its raw source.
fn transform(node: &Node, source: &str, ops: &[String]) -> String {
    match node {
        Node::ParenthesesNode { .. } => {
            let p = node.as_parentheses_node().unwrap();
            let opening = p.opening_loc();
            let closing = p.closing_loc();
            let body = match p.body() {
                Some(b) => b,
                None => {
                    let l = node.location();
                    return source[l.start_offset()..l.end_offset()].to_string();
                }
            };
            // Body may be StatementsNode wrapping a single expression.
            let inner_node = if let Some(stmts) = body.as_statements_node() {
                let v: Vec<_> = stmts.body().iter().collect();
                if v.len() == 1 {
                    v.into_iter().next().unwrap()
                } else {
                    let l = node.location();
                    return source[l.start_offset()..l.end_offset()].to_string();
                }
            } else {
                body
            };
            let inner_str = transform(&inner_node, source, ops);
            let open_src = &source[opening.start_offset()..opening.end_offset()];
            let close_src = &source[closing.start_offset()..closing.end_offset()];
            format!("{}{}{}", open_src, inner_str, close_src)
        }
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            let method = String::from_utf8_lossy(call.name().as_slice()).into_owned();
            let nloc = node.location();
            let raw = source[nloc.start_offset()..nloc.end_offset()].to_string();
            if !ops.iter().any(|o| o == &method) {
                return raw;
            }
            let args = call.arguments();
            let arg_list: Vec<Node> = match args.as_ref() {
                Some(a) => a.arguments().iter().collect(),
                None => return raw,
            };
            if arg_list.is_empty() {
                return raw;
            }
            let lhs = match call.receiver() {
                Some(r) => r,
                None => return raw,
            };
            let rhs = &arg_list[0];
            if !is_constant_portion(&lhs) || is_constant_portion(rhs) {
                return raw;
            }
            let lhs_loc = lhs.location();
            let rhs_loc = rhs.location();
            let between = &source[lhs_loc.end_offset()..rhs_loc.start_offset()];
            // Tail after rhs (e.g. closing `)` for `CONST.+(ary)` form).
            let tail = &source[rhs_loc.end_offset()..nloc.end_offset()];
            // Prefix before lhs (rare; e.g. when call source begins before receiver).
            let prefix = &source[nloc.start_offset()..lhs_loc.start_offset()];
            let new_lhs = transform(rhs, source, ops);
            let new_rhs = transform(&lhs, source, ops);
            format!("{}{}{}{}{}", prefix, new_lhs, between, new_rhs, tail)
        }
        _ => {
            let l = node.location();
            source[l.start_offset()..l.end_offset()].to_string()
        }
    }
}

fn is_constant_portion(node: &Node) -> bool {
    matches!(
        node,
        Node::IntegerNode { .. }
            | Node::FloatNode { .. }
            | Node::RationalNode { .. }
            | Node::ImaginaryNode { .. }
            | Node::ConstantReadNode { .. }
            | Node::ConstantPathNode { .. }
    )
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice()).into_owned();
        let cstart = node.location().start_offset();
        let cend = node.location().end_offset();

        let mut flagged = false;
        if self.ops.iter().any(|o| o == &method) {
            // Need at least one argument
            let args = node.arguments();
            let arg_list: Vec<Node> = match args.as_ref() {
                Some(a) => a.arguments().iter().collect(),
                None => Vec::new(),
            };
            if !arg_list.is_empty() {
                if let Some(lhs) = node.receiver() {
                    let rhs = &arg_list[0];
                    if is_constant_portion(&lhs) && !is_constant_portion(rhs) {
                        // Check no offended ancestor contains this node
                        let in_ancestor = self
                            .offended_ranges
                            .iter()
                            .any(|(s, e)| *s <= cstart && cend <= *e);
                        if !in_ancestor {
                            let lhs_loc = lhs.location();
                            let rhs_loc = rhs.location();
                            let rhs_src = &self.ctx.source
                                [rhs_loc.start_offset()..rhs_loc.end_offset()];
                            let msg = format!("Non-literal operand (`{}`) should be first.", rhs_src);

                            // Build replacement for the whole call by transforming
                            // the rhs (recursively swapping nested yoda) and emitting
                            // `rhs_swapped <op> <lhs_src>`.
                            let new_lhs = transform(rhs, self.ctx.source, self.ops);
                            let new_rhs = transform(&lhs, self.ctx.source, self.ops);
                            // Operator source between lhs_end and rhs_start.
                            let between = &self.ctx.source
                                [lhs_loc.end_offset()..rhs_loc.start_offset()];
                            let replacement = format!("{}{}{}", new_lhs, between, new_rhs);

                            let correction = Correction::replace(cstart, cend, replacement);

                            let off = self
                                .ctx
                                .offense_with_range(
                                    "Style/YodaExpression",
                                    &msg,
                                    Severity::Convention,
                                    cstart,
                                    cend,
                                )
                                .with_correction(correction);
                            self.offenses.push(off);
                            self.offended_ranges.push((cstart, cend));
                            flagged = true;
                        }
                    }
                }
            }
        }

        let _ = flagged;
        ruby_prism::visit_call_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    supported_operators: Option<Vec<String>>,
}

crate::register_cop!("Style/YodaExpression", |cfg| {
    let c: Cfg = cfg.typed("Style/YodaExpression");
    let ops = c.supported_operators.unwrap_or_else(|| {
        vec!["*", "+", "&", "|", "^"].into_iter().map(String::from).collect()
    });
    Some(Box::new(YodaExpression::with_config(ops)))
});
