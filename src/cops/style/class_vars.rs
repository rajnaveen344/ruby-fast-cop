use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};

const MSG: &str = "Replace class var %s with a class instance var.";

#[derive(Default)]
pub struct ClassVars;

impl ClassVars {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for ClassVars {
    fn name(&self) -> &'static str {
        "Style/ClassVars"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_class_variable_write(
        &self,
        node: &ruby_prism::ClassVariableWriteNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let name_loc = node.name_loc();
        let name = String::from_utf8_lossy(node.name().as_slice());
        let msg = format!("Replace class var {} with a class instance var.", name);
        vec![ctx.offense(self.name(), &msg, self.severity(), &name_loc)]
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node) != "class_variable_set" {
            return vec![];
        }
        // Must have at least one argument (the class var name)
        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.is_empty() {
            return vec![];
        }
        let first = &arg_list[0];
        let first_src = &ctx.source[first.location().start_offset()..first.location().end_offset()];
        let msg = format!("Replace class var {} with a class instance var.", first_src);
        vec![ctx.offense_with_range(
            self.name(),
            &msg,
            self.severity(),
            first.location().start_offset(),
            first.location().end_offset(),
        )]
    }
}

crate::register_cop!("Style/ClassVars", |_cfg| Some(Box::new(ClassVars::new())));
