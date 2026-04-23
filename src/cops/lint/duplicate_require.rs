//! Lint/DuplicateRequire cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/duplicate_require.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use crate::offense::Correction;
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct DuplicateRequire;

impl DuplicateRequire {
    pub fn new() -> Self { Self }
}

impl Cop for DuplicateRequire {
    fn name(&self) -> &'static str { "Lint/DuplicateRequire" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = RequireVisitor {
            ctx,
            offenses: Vec::new(),
            seen: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct RequireVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// (method_key, feature_arg) pairs seen so far
    seen: Vec<(String, String)>,
}

impl<'a> RequireVisitor<'a> {
    fn check_call(&mut self, node: &ruby_prism::CallNode) {
        let method = node_name!(node);
        if method != "require" && method != "require_relative" {
            return;
        }

        // receiver must be nil or Kernel
        let recv_ok = match node.receiver() {
            None => true,
            Some(r) => {
                // Allow Kernel.require
                match &r {
                    Node::ConstantReadNode { .. } => {
                        node_name!(r.as_constant_read_node().unwrap()) == "Kernel"
                    }
                    _ => false,
                }
            }
        };
        if !recv_ok {
            return;
        }

        // Must have exactly one string argument
        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return;
        }
        let feature = match arg_list[0].as_string_node() {
            Some(s) => {
                let loc = s.content_loc();
                self.ctx.src(loc.start_offset(), loc.end_offset()).to_string()
            }
            None => return,
        };

        let key = (method.to_string(), feature);

        if self.seen.contains(&key) {
            let loc = node.location();
            let msg = format!("Duplicate `{}` detected.", key.0);
            // Correction: remove entire line including leading whitespace + trailing newline
            let start = loc.start_offset();
            let end = loc.end_offset();
            let src_bytes = self.ctx.source.as_bytes();
            // Find line start (going back from node start)
            let line_start = self.ctx.line_start(start);
            // Include trailing newline
            let remove_end = if end < src_bytes.len() && src_bytes[end] == b'\n' {
                end + 1
            } else {
                end
            };
            let correction = Correction::delete(line_start, remove_end);
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/DuplicateRequire",
                &msg,
                Severity::Warning,
                start,
                end,
            ).with_correction(correction));
        } else {
            self.seen.push(key);
        }
    }
}

impl<'a> Visit<'_> for RequireVisitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Lint/DuplicateRequire", |_cfg| {
    Some(Box::new(DuplicateRequire::new()))
});
