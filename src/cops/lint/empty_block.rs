//! Lint/EmptyBlock cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Empty block detected.";

pub struct EmptyBlock {
    allow_comments: bool,
    allow_empty_lambdas: bool,
}

impl EmptyBlock {
    pub fn new(allow_comments: bool, allow_empty_lambdas: bool) -> Self {
        Self { allow_comments, allow_empty_lambdas }
    }
}

impl Cop for EmptyBlock {
    fn name(&self) -> &'static str { "Lint/EmptyBlock" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();
        let mut v = V {
            ctx,
            allow_comments: self.allow_comments,
            allow_empty_lambdas: self.allow_empty_lambdas,
            out: vec![],
        };
        v.visit(&tree);
        v.out
    }
}

struct V<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    allow_comments: bool,
    allow_empty_lambdas: bool,
    out: Vec<Offense>,
}

fn is_empty_body(body: Option<ruby_prism::Node>) -> bool {
    match body {
        None => true,
        Some(n) => {
            if let Some(s) = n.as_statements_node() {
                s.body().iter().next().is_none()
            } else {
                false
            }
        }
    }
}

fn lambda_or_proc_call(node: &ruby_prism::CallNode) -> bool {
    let n = node_name!(node).into_owned();
    let recv = node.receiver();
    if recv.is_none() && (n == "lambda" || n == "proc") {
        return true;
    }
    if n == "new" {
        if let Some(r) = recv {
            if let Some(c) = r.as_constant_read_node() {
                return String::from_utf8_lossy(c.name().as_slice()) == "Proc";
            }
            if let Some(cp) = r.as_constant_path_node() {
                return cp.parent().is_none()
                    && cp.name()
                        .map(|x| String::from_utf8_lossy(x.as_slice()) == "Proc")
                        .unwrap_or(false);
            }
        }
    }
    false
}

impl<'a, 'b> V<'a, 'b> {
    fn line_range_has_comment(&self, start: usize, end: usize) -> bool {
        let bytes = self.ctx.source.as_bytes();
        let mut s = start;
        while s > 0 && bytes[s-1] != b'\n' { s -= 1; }
        let mut e = end;
        while e < bytes.len() && bytes[e] != b'\n' { e += 1; }
        let mut i = s;
        let mut in_str = false;
        let mut str_ch = 0u8;
        while i < e {
            let b = bytes[i];
            if in_str {
                if b == b'\\' { i += 2; continue; }
                if b == str_ch { in_str = false; }
            } else {
                if b == b'#' { return true; }
                if b == b'\'' || b == b'"' { in_str = true; str_ch = b; }
            }
            i += 1;
        }
        false
    }
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if let Some(block_node) = node.block() {
            if let Some(b) = block_node.as_block_node() {
                if is_empty_body(b.body()) {
                    let allow_lambda = self.allow_empty_lambdas && lambda_or_proc_call(node);
                    if !allow_lambda {
                        let loc = node.location();
                        let s = loc.start_offset();
                        let e = loc.end_offset();
                        let skip_comment = self.allow_comments && self.line_range_has_comment(s, e);
                        if !skip_comment {
                            self.out.push(self.ctx.offense_with_range(
                                "Lint/EmptyBlock", MSG, Severity::Warning, s, e,
                            ));
                        }
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        if is_empty_body(node.body()) {
            if !self.allow_empty_lambdas {
                let loc = node.location();
                let s = loc.start_offset();
                let e = loc.end_offset();
                let skip_comment = self.allow_comments && self.line_range_has_comment(s, e);
                if !skip_comment {
                    self.out.push(self.ctx.offense_with_range(
                        "Lint/EmptyBlock", MSG, Severity::Warning, s, e,
                    ));
                }
            }
        }
        ruby_prism::visit_lambda_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_comments: Option<bool>,
    allow_empty_lambdas: Option<bool>,
}

crate::register_cop!("Lint/EmptyBlock", |cfg| {
    let c: Cfg = cfg.typed("Lint/EmptyBlock");
    Some(Box::new(EmptyBlock::new(
        c.allow_comments.unwrap_or(true),
        c.allow_empty_lambdas.unwrap_or(true),
    )))
});
