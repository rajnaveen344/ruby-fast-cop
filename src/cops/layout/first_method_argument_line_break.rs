//! Layout/FirstMethodArgumentLineBreak
//!
//! Ports https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/first_method_argument_line_break.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::allowed_methods::is_method_allowed;
use crate::helpers::multiline_element_line_breaks as h;
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Layout/FirstMethodArgumentLineBreak";
const MSG: &str = "Add a line break before the first argument of a multi-line method argument list.";

#[derive(Default)]
pub struct FirstMethodArgumentLineBreak {
    allow_multiline_final_element: bool,
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<String>,
}

impl FirstMethodArgumentLineBreak {
    pub fn new(
        allow_multiline_final_element: bool,
        allowed_methods: Vec<String>,
        allowed_patterns: Vec<String>,
    ) -> Self {
        Self { allow_multiline_final_element, allowed_methods, allowed_patterns }
    }

    fn check_args<'a>(
        &self,
        ctx: &CheckContext,
        container_start: usize,
        args: Vec<Node<'a>>,
    ) -> Vec<Offense> {
        // Expand trailing implicit hash (KeywordHashNode) into its assoc children.
        let mut expanded: Vec<Node<'a>> = args;
        if let Some(last) = expanded.last() {
            if let Node::KeywordHashNode { .. } = last {
                let kh = last.as_keyword_hash_node().unwrap();
                let pairs: Vec<Node> = kh.elements().iter().collect();
                expanded.pop();
                expanded.extend(pairs);
            }
        }
        if expanded.is_empty() {
            return vec![];
        }
        // method_uses_parens? — line up to first arg ends in `(`
        let first_start = expanded[0].location().start_offset();
        if !h::method_uses_parens(ctx.source, container_start, first_start) {
            return vec![];
        }
        h::check_first_element_break(
            ctx,
            COP_NAME,
            MSG,
            container_start,
            &expanded,
            self.allow_multiline_final_element,
        )
    }
}

impl Cop for FirstMethodArgumentLineBreak {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if is_method_allowed(&self.allowed_methods, &self.allowed_patterns, &method, None) {
            return vec![];
        }
        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let arg_list: Vec<Node> = args.arguments().iter().collect();
        self.check_args(ctx, node.location().start_offset(), arg_list)
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        // Walk for SuperNode (no Cop trait dispatch for super).
        let mut visitor = SuperVisitor { cop: self, ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct SuperVisitor<'a, 'b> {
    cop: &'b FirstMethodArgumentLineBreak,
    ctx: &'b CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a, 'b> Visit<'a> for SuperVisitor<'a, 'b> {
    fn visit_super_node(&mut self, node: &ruby_prism::SuperNode<'a>) {
        if !is_method_allowed(&self.cop.allowed_methods, &self.cop.allowed_patterns, "super", None) {
            if let Some(args) = node.arguments() {
                let arg_list: Vec<Node> = args.arguments().iter().collect();
                self.offenses.extend(self.cop.check_args(
                    self.ctx,
                    node.location().start_offset(),
                    arg_list,
                ));
            }
        }
        ruby_prism::visit_super_node(self, node);
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_multiline_final_element: bool,
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<String>,
}

crate::register_cop!("Layout/FirstMethodArgumentLineBreak", |cfg| {
    let c: Cfg = cfg.typed("Layout/FirstMethodArgumentLineBreak");
    Some(Box::new(FirstMethodArgumentLineBreak::new(
        c.allow_multiline_final_element,
        c.allowed_methods,
        c.allowed_patterns,
    )))
});
