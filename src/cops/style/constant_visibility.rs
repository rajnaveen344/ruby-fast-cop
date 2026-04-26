//! Style/ConstantVisibility cop
//!
//! Constants in classes/modules should have explicit visibility.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct ConstantVisibility {
    ignore_modules: bool,
}

impl ConstantVisibility {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(ignore_modules: bool) -> Self {
        Self { ignore_modules }
    }

    /// Iterate top-level statements in a class/module body. If body is a
    /// StatementsNode, yields its children; if a single statement, yields one.
    fn body_statements<'a>(body: Option<Node<'a>>) -> Vec<Node<'a>> {
        let body = match body {
            Some(b) => b,
            None => return vec![],
        };
        if let Some(stmts) = body.as_statements_node() {
            stmts.body().iter().collect()
        } else {
            vec![body]
        }
    }

    /// Whether `name` appears in a sibling `public_constant`/`private_constant`
    /// call as a symbol argument.
    fn has_visibility_decl(siblings: &[Node], name: &str) -> bool {
        for sibling in siblings {
            let call = match sibling.as_call_node() {
                Some(c) => c,
                None => continue,
            };
            if call.receiver().is_some() {
                continue;
            }
            let m = node_name!(call);
            if m != "public_constant" && m != "private_constant" {
                continue;
            }
            let args = match call.arguments() {
                Some(a) => a,
                None => continue,
            };
            for arg in args.arguments().iter() {
                // Splat → unknown contents, treat as covering (skip).
                if matches!(arg, Node::SplatNode { .. }) {
                    return true;
                }
                if let Node::SymbolNode { .. } = arg {
                    let s = arg.as_symbol_node().unwrap();
                    let value_str = String::from_utf8_lossy(s.unescaped().as_ref()).into_owned();
                    if value_str == name {
                        return true;
                    }
                }
                if let Node::StringNode { .. } = arg {
                    let s = arg.as_string_node().unwrap();
                    let value_str = String::from_utf8_lossy(s.unescaped().as_ref()).into_owned();
                    if value_str == name {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Whether the RHS of `MyClass = X` constructs a class/module via Class.new/Module.new/Struct.new/Data.define.
    fn is_class_constructor(rhs: &Node) -> bool {
        let call = match rhs.as_call_node() {
            Some(c) => c,
            None => return false,
        };
        let recv = match call.receiver() {
            Some(r) => r,
            None => return false,
        };
        let recv_name = match &recv {
            Node::ConstantReadNode { .. } => {
                let c = recv.as_constant_read_node().unwrap();
                String::from_utf8_lossy(c.name().as_slice()).into_owned()
            }
            _ => return false,
        };
        let method = node_name!(call);
        matches!(
            (recv_name.as_str(), method.as_ref()),
            ("Class", "new") | ("Module", "new") | ("Struct", "new") | ("Data", "define")
        )
    }

    fn check_body(&self, ctx: &CheckContext, body: Option<Node>) -> Vec<Offense> {
        let siblings = Self::body_statements(body);
        let mut offenses = Vec::new();
        for stmt in &siblings {
            let cw = match stmt.as_constant_write_node() {
                Some(c) => c,
                None => continue,
            };
            let const_name = String::from_utf8_lossy(cw.name().as_slice()).into_owned();
            if Self::has_visibility_decl(&siblings, &const_name) {
                continue;
            }
            if self.ignore_modules {
                if let Some(rhs) = Some(cw.value()) {
                    if Self::is_class_constructor(&rhs) {
                        continue;
                    }
                }
            }
            // Offense range: whole `BAR = value`
            let nloc = cw.location();
            let msg = format!(
                "Explicitly make `{}` public or private using either `#public_constant` or `#private_constant`.",
                const_name
            );
            offenses.push(ctx.offense_with_range(
                "Style/ConstantVisibility",
                &msg,
                Severity::Convention,
                nloc.start_offset(),
                nloc.end_offset(),
            ));
        }
        offenses
    }
}

impl Cop for ConstantVisibility {
    fn name(&self) -> &'static str {
        "Style/ConstantVisibility"
    }

    fn check_class(&self, node: &ruby_prism::ClassNode, ctx: &CheckContext) -> Vec<Offense> {
        self.check_body(ctx, node.body())
    }

    fn check_module(&self, node: &ruby_prism::ModuleNode, ctx: &CheckContext) -> Vec<Offense> {
        self.check_body(ctx, node.body())
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    ignore_modules: bool,
}

crate::register_cop!("Style/ConstantVisibility", |cfg| {
    let c: Cfg = cfg.typed("Style/ConstantVisibility");
    Some(Box::new(ConstantVisibility::with_config(c.ignore_modules)))
});
