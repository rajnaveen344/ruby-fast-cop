use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct ClassMethods;

impl ClassMethods {
    pub fn new() -> Self {
        Self
    }

    /// Check body of a class/module for defs nodes with matching receiver
    fn check_body(
        &self,
        class_name_src: &str,
        class_name_bytes: &[u8],
        body: Option<Node>,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let body = match body {
            Some(b) => b,
            None => return vec![],
        };

        let mut offenses = Vec::new();

        match &body {
            Node::DefNode { .. } => {
                let def = body.as_def_node().unwrap();
                if let Some(recv) = def.receiver() {
                    if self.receiver_matches(&recv, class_name_bytes) {
                        offenses.push(self.make_offense(def, class_name_src, ctx));
                    }
                }
            }
            Node::StatementsNode { .. } => {
                let stmts = body.as_statements_node().unwrap();
                for child in stmts.body().iter() {
                    if let Node::DefNode { .. } = &child {
                        let def = child.as_def_node().unwrap();
                        if let Some(recv) = def.receiver() {
                            if self.receiver_matches(&recv, class_name_bytes) {
                                offenses.push(self.make_offense(def, class_name_src, ctx));
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        offenses
    }

    fn receiver_matches(&self, recv: &Node, class_name_bytes: &[u8]) -> bool {
        match recv {
            Node::SelfNode { .. } => false,
            Node::ConstantReadNode { .. } => {
                let c = recv.as_constant_read_node().unwrap();
                c.name().as_slice() == class_name_bytes
            }
            _ => false,
        }
    }

    fn make_offense(
        &self,
        def: ruby_prism::DefNode,
        class_name_src: &str,
        ctx: &CheckContext,
    ) -> Offense {
        let recv = def.receiver().unwrap();
        // Offense is receiver's name location (just the constant name portion)
        let recv_start = recv.location().start_offset();
        let recv_end = recv.location().end_offset();
        let method_name = node_name!(def);
        let msg = format!(
            "Use `self.{}` instead of `{}.{}`.",
            method_name, class_name_src, method_name
        );
        ctx.offense_with_range(self.name(), &msg, self.severity(), recv_start, recv_end)
    }
}

impl Cop for ClassMethods {
    fn name(&self) -> &'static str {
        "Style/ClassMethods"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_class(&self, node: &ruby_prism::ClassNode, ctx: &CheckContext) -> Vec<Offense> {
        let name_bytes = node.name().as_slice().to_vec();
        let const_path = node.constant_path();
        let name_src = &ctx.source[const_path.location().start_offset()..const_path.location().end_offset()];
        self.check_body(name_src, &name_bytes, node.body(), ctx)
    }

    fn check_module(&self, node: &ruby_prism::ModuleNode, ctx: &CheckContext) -> Vec<Offense> {
        let name_bytes = node.name().as_slice().to_vec();
        let const_path = node.constant_path();
        let name_src = &ctx.source[const_path.location().start_offset()..const_path.location().end_offset()];
        self.check_body(name_src, &name_bytes, node.body(), ctx)
    }
}

crate::register_cop!("Style/ClassMethods", |_cfg| Some(Box::new(ClassMethods::new())));
