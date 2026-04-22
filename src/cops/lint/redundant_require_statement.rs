//! Lint/RedundantRequireStatement - Remove unnecessary require statements.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Remove unnecessary `require` statement.";

#[derive(Default)]
pub struct RedundantRequireStatement;

impl RedundantRequireStatement {
    pub fn new() -> Self { Self }
}

impl Cop for RedundantRequireStatement {
    fn name(&self) -> &'static str { "Lint/RedundantRequireStatement" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl Visit<'_> for Visitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice());
        if method == "require" && node.receiver().is_none() {
            if let Some(args) = node.arguments() {
                let arg_list: Vec<_> = args.arguments().iter().collect();
                if let Some(first) = arg_list.first() {
                    if let Some(str_node) = first.as_string_node() {
                        let feature = String::from_utf8_lossy(str_node.unescaped());
                        if self.is_redundant_feature(feature.as_ref()) {
                            let loc = node.location();
                            self.offenses.push(self.ctx.offense_with_range(
                                "Lint/RedundantRequireStatement",
                                MSG,
                                Severity::Warning,
                                loc.start_offset(),
                                loc.end_offset(),
                            ));
                        }
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

impl<'a> Visitor<'a> {
    fn is_redundant_feature(&self, feature: &str) -> bool {
        let ver = self.ctx.target_ruby_version;
        feature == "enumerator"
            || (ver >= 2.1 && feature == "thread")
            || (ver >= 2.2 && (feature == "rational" || feature == "complex"))
            || (ver >= 2.7 && feature == "ruby2_keywords")
            || (ver >= 3.1 && feature == "fiber")
            || (ver >= 3.2 && feature == "set")
            || (ver >= 4.0 && feature == "pathname")
    }
}

crate::register_cop!("Lint/RedundantRequireStatement", |_cfg| Some(Box::new(RedundantRequireStatement::new())));
