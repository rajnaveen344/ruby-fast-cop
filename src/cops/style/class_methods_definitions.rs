//! Style/ClassMethodsDefinitions cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::Node;

const MSG_SCLASS: &str = "Do not define public methods within class << self.";
const MSG_DEF_SELF: &str = "Use `class << self` to define a class method.";

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    #[default]
    DefSelf,
    SelfClass,
}

pub struct ClassMethodsDefinitions {
    style: EnforcedStyle,
}

impl ClassMethodsDefinitions {
    pub fn new() -> Self {
        Self { style: EnforcedStyle::DefSelf }
    }

    pub fn with_style(style: EnforcedStyle) -> Self {
        Self { style }
    }

    fn class_elements<'a>(body: Option<Node<'a>>) -> Vec<Node<'a>> {
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

    /// Compute current visibility for each child via siblings sweep. Returns
    /// "public"/"private"/"protected" for the target node.
    fn node_visibility<'a>(elements: &[Node<'a>], target: &Node<'a>) -> &'static str {
        let mut current = "public";
        for el in elements {
            if let Some(call) = el.as_call_node() {
                if call.receiver().is_none() && call.arguments().is_none() && call.block().is_none() {
                    let m = node_name!(call);
                    match m.as_ref() {
                        "private" => current = "private",
                        "protected" => current = "protected",
                        "public" => current = "public",
                        _ => {}
                    }
                }
            }
            if el.location().start_offset() == target.location().start_offset() {
                return match current {
                    "private" => "private",
                    "protected" => "protected",
                    _ => "public",
                };
            }
        }
        "public"
    }

    /// Instance defs only (no `def self.x`). Mirrors RuboCop `def_type?` (not defs_type).
    fn def_nodes<'a>(sclass: &ruby_prism::SingletonClassNode<'a>) -> Vec<Node<'a>> {
        let body = match sclass.body() {
            Some(b) => b,
            None => return vec![],
        };
        let is_instance_def = |n: &Node| -> bool {
            n.as_def_node().map(|d| d.receiver().is_none()).unwrap_or(false)
        };
        // Single DefNode body
        if is_instance_def(&body) {
            return vec![body];
        }
        // StatementsNode children, only instance defs
        if let Some(stmts) = body.as_statements_node() {
            return stmts
                .body()
                .iter()
                .filter(|n| is_instance_def(n))
                .collect();
        }
        vec![]
    }
}

impl Default for ClassMethodsDefinitions {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for ClassMethodsDefinitions {
    fn name(&self) -> &'static str {
        "Style/ClassMethodsDefinitions"
    }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        if self.style != EnforcedStyle::SelfClass {
            return vec![];
        }
        // Receiver must be self
        if !matches!(node.receiver(), Some(Node::SelfNode { .. })) {
            return vec![];
        }
        // Range = `def self.name` — `def` + ` ` + `self.` + name
        let loc = node.location();
        let name_loc = node.name_loc();
        let start = loc.start_offset();
        let end = name_loc.end_offset();
        vec![ctx.offense_with_range(self.name(), MSG_DEF_SELF, Severity::Convention, start, end)]
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if self.style != EnforcedStyle::DefSelf {
            return vec![];
        }
        use ruby_prism::Visit;
        let mut visitor = SClassVisitor { ctx, cop: self, offenses: vec![] };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct SClassVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a ClassMethodsDefinitions,
    offenses: Vec<Offense>,
}

impl<'a> ruby_prism::Visit<'_> for SClassVisitor<'a> {
    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        // Only `class << self`
        if matches!(node.expression(), Node::SelfNode { .. }) {
            let defs = ClassMethodsDefinitions::def_nodes(node);
            if !defs.is_empty() {
                let elements = ClassMethodsDefinitions::class_elements(node.body());
                let all_public = defs.iter().all(|d| {
                    ClassMethodsDefinitions::node_visibility(&elements, d) == "public"
                });
                if all_public {
                    // Range = `class << self` (sclass class_keyword + expression)
                    // sclass has class_keyword_loc and expression()
                    let kw = node.class_keyword_loc();
                    let expr = node.expression();
                    let start = kw.start_offset();
                    let end = expr.location().end_offset();
                    self.offenses.push(self.ctx.offense_with_range(
                        self.cop.name(),
                        MSG_SCLASS,
                        Severity::Convention,
                        start,
                        end,
                    ));
                }
            }
        }
        ruby_prism::visit_singleton_class_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: Option<String>,
}

crate::register_cop!("Style/ClassMethodsDefinitions", |cfg| {
    let c: Cfg = cfg.typed("Style/ClassMethodsDefinitions");
    let style = match c.enforced_style.as_deref() {
        Some("self_class") => EnforcedStyle::SelfClass,
        _ => EnforcedStyle::DefSelf,
    };
    Some(Box::new(ClassMethodsDefinitions::with_style(style)))
});
