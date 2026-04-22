//! Lint/UriRegexp - `URI.regexp` is obsolete; use make_regexp instead.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/uri_regexp.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct UriRegexp;

impl UriRegexp {
    pub fn new() -> Self {
        Self
    }
}

struct UriRegexpVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl UriRegexpVisitor<'_> {
    fn check_call(&mut self, node: &ruby_prism::CallNode) {
        let method = node_name!(node);
        if method != "regexp" {
            return;
        }

        let receiver = match node.receiver() {
            Some(r) => r,
            None => return,
        };

        let prefix = match &receiver {
            Node::ConstantReadNode { .. } => {
                let c = receiver.as_constant_read_node().unwrap();
                let name = String::from_utf8_lossy(c.name().as_slice());
                if name != "URI" { return; }
                ""
            }
            Node::ConstantPathNode { .. } => {
                let cp = receiver.as_constant_path_node().unwrap();
                if cp.parent().is_some() { return; }
                let const_id = match cp.name() { Some(id) => id, None => return };
                let name = String::from_utf8_lossy(const_id.as_slice());
                if name != "URI" { return; }
                "::"
            }
            _ => return,
        };

        let parser_const = if self.ctx.ruby_version_at_least(3, 4) {
            "RFC2396_PARSER"
        } else {
            "DEFAULT_PARSER"
        };

        let src = self.ctx.source;
        let node_start = node.location().start_offset();
        let node_end = node.location().end_offset();
        let call_src = &src[node_start..node_end];

        let args_src = if let Some(args) = node.arguments() {
            let loc = args.location();
            &src[loc.start_offset()..loc.end_offset()]
        } else {
            ""
        };

        let replacement = if args_src.is_empty() {
            format!("{}URI::{}.make_regexp", prefix, parser_const)
        } else {
            format!("{}URI::{}.make_regexp({})", prefix, parser_const, args_src)
        };

        let msg = format!(
            "`{}` is obsolete and should not be used. Instead, use `{}`.",
            call_src, replacement
        );

        // Offense range: method name location
        let offense_start = if let Some(msg_loc) = node.message_loc() {
            msg_loc.start_offset()
        } else {
            node_start
        };
        let offense_end = if let Some(msg_loc) = node.message_loc() {
            msg_loc.end_offset()
        } else {
            node_end
        };

        let correction = Correction::replace(node_start, node_end, &replacement);

        let offense = self.ctx.offense_with_range(
            "Lint/UriRegexp",
            &msg,
            Severity::Warning,
            offense_start,
            offense_end,
        );
        self.offenses.push(offense.with_correction(correction));
    }
}

impl Visit<'_> for UriRegexpVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

impl Cop for UriRegexp {
    fn name(&self) -> &'static str {
        "Lint/UriRegexp"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = UriRegexpVisitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

crate::register_cop!("Lint/UriRegexp", |_cfg| {
    Some(Box::new(UriRegexp::new()))
});
