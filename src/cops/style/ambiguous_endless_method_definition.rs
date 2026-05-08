//! Style/AmbiguousEndlessMethodDefinition
//!
//! Flags endless method definitions inside ambiguous lower-precedence
//! operations: `and`, `or`, or modifier `if`/`unless`/`while`/`until`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

fn is_endless_def(n: &Node) -> bool {
    n.as_def_node().map(|d| d.equal_loc().is_some()).unwrap_or(false)
}

/// Build correction: replace `def foo = body` with `def foo\n  body\nend`
fn build_correction(def_node: &ruby_prism::DefNode, source: &str) -> Option<Correction> {
    let body = def_node.body()?;
    let name_loc = def_node.name_loc();
    let def_start = def_node.location().start_offset();
    let def_end = def_node.location().end_offset();

    let method_name = &source[name_loc.start_offset()..name_loc.end_offset()];

    // Arguments: if params exist, include them including parens
    let args_src = if let Some(params) = def_node.parameters() {
        let loc = params.location();
        // Check if there are parens around params
        let paren_open = def_node.lparen_loc();
        let paren_close = def_node.rparen_loc();
        if let (Some(open), Some(close)) = (paren_open, paren_close) {
            source[open.start_offset()..close.end_offset()].to_string()
        } else {
            format!(" {}", &source[loc.start_offset()..loc.end_offset()])
        }
    } else {
        String::new()
    };

    let body_src = &source[body.location().start_offset()..body.location().end_offset()];
    let replacement = format!("def {}{}\n  {}\nend", method_name, args_src, body_src);

    Some(Correction {
        edits: vec![Edit {
            start_offset: def_start,
            end_offset: def_end,
            replacement,
        }],
    })
}

struct V<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> V<'a> {
    fn flag_with_def(&mut self, op_start: usize, op_end: usize, keyword: &str, def_node: &ruby_prism::DefNode) {
        let msg = format!("Avoid using `{}` statements with endless methods.", keyword);
        let correction = build_correction(def_node, self.ctx.source);
        let offense = self.ctx.offense_with_range(
            "Style/AmbiguousEndlessMethodDefinition",
            &msg, Severity::Convention, op_start, op_end,
        );
        self.offenses.push(if let Some(c) = correction {
            offense.with_correction(c)
        } else {
            offense
        });
    }
}

impl<'a> Visit<'_> for V<'a> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        // modifier form: no end_keyword_loc
        if node.end_keyword_loc().is_none() {
            if let Some(stmts) = node.statements() {
                for stmt in stmts.body().iter() {
                    if let Some(def_node) = stmt.as_def_node() {
                        if def_node.equal_loc().is_some() {
                            let loc = node.location();
                            self.flag_with_def(loc.start_offset(), loc.end_offset(), "if", &def_node);
                            break;
                        }
                    }
                }
            }
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        if node.end_keyword_loc().is_none() {
            if let Some(stmts) = node.statements() {
                for stmt in stmts.body().iter() {
                    if let Some(def_node) = stmt.as_def_node() {
                        if def_node.equal_loc().is_some() {
                            let loc = node.location();
                            self.flag_with_def(loc.start_offset(), loc.end_offset(), "unless", &def_node);
                            break;
                        }
                    }
                }
            }
        }
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        if node.closing_loc().is_none() {
            if let Some(stmts) = node.statements() {
                for stmt in stmts.body().iter() {
                    if let Some(def_node) = stmt.as_def_node() {
                        if def_node.equal_loc().is_some() {
                            let loc = node.location();
                            self.flag_with_def(loc.start_offset(), loc.end_offset(), "while", &def_node);
                            break;
                        }
                    }
                }
            }
        }
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        if node.closing_loc().is_none() {
            if let Some(stmts) = node.statements() {
                for stmt in stmts.body().iter() {
                    if let Some(def_node) = stmt.as_def_node() {
                        if def_node.equal_loc().is_some() {
                            let loc = node.location();
                            self.flag_with_def(loc.start_offset(), loc.end_offset(), "until", &def_node);
                            break;
                        }
                    }
                }
            }
        }
        ruby_prism::visit_until_node(self, node);
    }

    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        let left = node.left();
        if let Some(def_node) = left.as_def_node() {
            if def_node.equal_loc().is_some() {
                let loc = node.location();
                self.flag_with_def(loc.start_offset(), loc.end_offset(), "and", &def_node);
            }
        }
        ruby_prism::visit_and_node(self, node);
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        let left = node.left();
        if let Some(def_node) = left.as_def_node() {
            if def_node.equal_loc().is_some() {
                let loc = node.location();
                self.flag_with_def(loc.start_offset(), loc.end_offset(), "or", &def_node);
            }
        }
        ruby_prism::visit_or_node(self, node);
    }
}

#[derive(Default)]
pub struct AmbiguousEndlessMethodDefinition;

impl AmbiguousEndlessMethodDefinition {
    pub fn new() -> Self { Self }
}

impl Cop for AmbiguousEndlessMethodDefinition {
    fn name(&self) -> &'static str { "Style/AmbiguousEndlessMethodDefinition" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(3, 0) { return vec![] }
        let mut v = V { ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

crate::register_cop!("Style/AmbiguousEndlessMethodDefinition", |_cfg| {
    Some(Box::new(AmbiguousEndlessMethodDefinition::new()))
});
