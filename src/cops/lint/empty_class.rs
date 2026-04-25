//! Lint/EmptyClass cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

const MSG_CLASS: &str = "Empty class detected.";
const MSG_META: &str = "Empty metaclass detected.";

pub struct EmptyClass { allow_comments: bool }

impl EmptyClass {
    pub fn new(allow_comments: bool) -> Self { Self { allow_comments } }
}

impl Cop for EmptyClass {
    fn name(&self) -> &'static str { "Lint/EmptyClass" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();
        let mut v = V { ctx, allow_comments: self.allow_comments, out: vec![] };
        v.visit(&tree);
        v.out
    }
}

struct V<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    allow_comments: bool,
    out: Vec<Offense>,
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        if node.body().is_none() && node.superclass().is_none() {
            let start = node.class_keyword_loc().start_offset();
            let cp = node.constant_path();
            let end = cp.location().end_offset();
            let end_kw_start = node.end_keyword_loc().start_offset();
            if !self.body_has_comments(cp.location().end_offset(), end_kw_start) {
                self.out.push(self.ctx.offense_with_range(
                    "Lint/EmptyClass", MSG_CLASS, Severity::Warning, start, end,
                ));
            }
        }
        ruby_prism::visit_class_node(self, node);
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        if node.body().is_none() {
            let start = node.class_keyword_loc().start_offset();
            let expr = node.expression();
            let end = expr.location().end_offset();
            let end_kw_start = node.end_keyword_loc().start_offset();
            if !self.body_has_comments(end, end_kw_start) {
                self.out.push(self.ctx.offense_with_range(
                    "Lint/EmptyClass", MSG_META, Severity::Warning, start, end,
                ));
            }
        }
        ruby_prism::visit_singleton_class_node(self, node);
    }
}

impl<'a, 'b> V<'a, 'b> {
    fn body_has_comments(&self, start: usize, end: usize) -> bool {
        if !self.allow_comments { return false; }
        let bytes = self.ctx.source.as_bytes();
        let mut i = start;
        while i < end {
            if bytes[i] == b'#' { return true; }
            i += 1;
        }
        false
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_comments: Option<bool>,
}

crate::register_cop!("Lint/EmptyClass", |cfg| {
    let c: Cfg = cfg.typed("Lint/EmptyClass");
    let allow = c.allow_comments.unwrap_or(true);
    Some(Box::new(EmptyClass::new(allow)))
});
