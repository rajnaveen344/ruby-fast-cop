//! Lint/UselessConstantScoping cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Useless `private` access modifier for constant scope.";

#[derive(Default)]
pub struct UselessConstantScoping;

impl UselessConstantScoping {
    pub fn new() -> Self { Self }
}

impl Cop for UselessConstantScoping {
    fn name(&self) -> &'static str { "Lint/UselessConstantScoping" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();
        let mut v = V { ctx, out: vec![] };
        v.visit(&tree);
        v.out
    }
}

struct V<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    out: Vec<Offense>,
}

impl<'a, 'b> V<'a, 'b> {
    fn process_body(&mut self, body: &ruby_prism::StatementsNode) {
        let stmts: Vec<Node> = body.body().iter().collect();
        let mut last_bare: Option<&'static str> = None;
        for (i, stmt) in stmts.iter().enumerate() {
            if let Some(call) = stmt.as_call_node() {
                if call.receiver().is_none() && call.arguments().is_none() && call.block().is_none() {
                    let n = node_name!(&call);
                    let nstr = n.as_ref();
                    if nstr == "private" || nstr == "public" || nstr == "protected" {
                        last_bare = Some(match nstr {
                            "private" => "private",
                            "public" => "public",
                            _ => "protected",
                        });
                        continue;
                    }
                }
            }
            if last_bare == Some("private") {
                let cname = if let Some(cw) = stmt.as_constant_write_node() {
                    Some(String::from_utf8_lossy(cw.name().as_slice()).into_owned())
                } else {
                    None
                };
                if let Some(name) = cname {
                    let mut protected = false;
                    for later in &stmts[i+1..] {
                        if let Some(call) = later.as_call_node() {
                            if call.receiver().is_none() && node_name!(&call).as_ref() == "private_constant" {
                                if let Some(args) = call.arguments() {
                                    for a in args.arguments().iter() {
                                        let av = if let Some(s) = a.as_symbol_node() {
                                            s.value_loc().map(|v| self.ctx.source[v.start_offset()..v.end_offset()].to_string())
                                        } else if let Some(s) = a.as_string_node() {
                                            let v = s.content_loc();
                                            Some(self.ctx.source[v.start_offset()..v.end_offset()].to_string())
                                        } else {
                                            None
                                        };
                                        if av.as_deref() == Some(name.as_str()) {
                                            protected = true;
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        if protected { break; }
                    }
                    if !protected {
                        let loc = stmt.location();
                        self.out.push(self.ctx.offense_with_range(
                            "Lint/UselessConstantScoping", MSG, Severity::Warning,
                            loc.start_offset(), loc.end_offset(),
                        ));
                    }
                }
            }
        }
    }
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        if let Some(body) = node.body() {
            if let Some(s) = body.as_statements_node() {
                self.process_body(&s);
            }
        }
        ruby_prism::visit_class_node(self, node);
    }
    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        if let Some(body) = node.body() {
            if let Some(s) = body.as_statements_node() {
                self.process_body(&s);
            }
        }
        ruby_prism::visit_module_node(self, node);
    }
    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        if let Some(body) = node.body() {
            if let Some(s) = body.as_statements_node() {
                self.process_body(&s);
            }
        }
        ruby_prism::visit_singleton_class_node(self, node);
    }
}

crate::register_cop!("Lint/UselessConstantScoping", |_cfg| Some(Box::new(UselessConstantScoping::new())));
