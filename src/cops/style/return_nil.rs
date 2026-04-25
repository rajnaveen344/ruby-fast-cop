//! Style/ReturnNil — enforces consistency between `return` and `return nil`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    Return,
    ReturnNil,
}

pub struct ReturnNil {
    style: Style,
}

impl Default for ReturnNil {
    fn default() -> Self {
        Self { style: Style::Return }
    }
}

impl ReturnNil {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_style(s: &str) -> Self {
        let style = if s == "return_nil" { Style::ReturnNil } else { Style::Return };
        Self { style }
    }
}

impl Cop for ReturnNil {
    fn name(&self) -> &'static str {
        "Style/ReturnNil"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor { ctx, offenses: Vec::new(), style: self.style };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    style: Style,
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_return_node(&mut self, node: &ruby_prism::ReturnNode) {
        let args = node.arguments();
        let arg_list: Vec<Node> = match args.as_ref() {
            Some(a) => a.arguments().iter().collect(),
            None => Vec::new(),
        };

        let is_return_nil =
            arg_list.len() == 1 && matches!(arg_list[0], Node::NilNode { .. });
        let is_bare_return = arg_list.is_empty();

        let (flag, replacement, msg) = match self.style {
            Style::Return => {
                if !is_return_nil {
                    return;
                }
                (true, "return", "Use `return` instead of `return nil`.")
            }
            Style::ReturnNil => {
                if !is_bare_return {
                    return;
                }
                (true, "return nil", "Use `return nil` instead of `return`.")
            }
        };

        if !flag {
            return;
        }

        let nloc = node.location();
        let start = nloc.start_offset();
        let end = nloc.end_offset();
        let off = self
            .ctx
            .offense_with_range("Style/ReturnNil", msg, Severity::Convention, start, end)
            .with_correction(Correction::replace(start, end, replacement));
        self.offenses.push(off);
        ruby_prism::visit_return_node(self, node);
    }
}

crate::register_cop!("Style/ReturnNil", |cfg| {
    let style = cfg
        .get_cop_config("Style/ReturnNil")
        .and_then(|c| c.raw.get("EnforcedStyle"))
        .and_then(|v| v.as_str())
        .unwrap_or("return")
        .to_string();
    Some(Box::new(ReturnNil::with_style(&style)))
});
