//! Layout/EmptyLinesAroundModuleBody
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/empty_lines_around_module_body.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::empty_lines_around_body::{check, Style};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

pub use crate::helpers::empty_lines_around_body::Style as EmptyLinesAroundModuleBodyStyle;

pub struct EmptyLinesAroundModuleBody {
    style: Style,
}

impl EmptyLinesAroundModuleBody {
    pub fn new(style: Style) -> Self {
        Self { style }
    }
}

fn line_of(source: &str, offset: usize) -> usize {
    1 + source.as_bytes()[..offset].iter().filter(|&&b| b == b'\n').count()
}

fn last_line_of(source: &str, end: usize) -> usize {
    let last_byte = if end > 0 { end - 1 } else { 0 };
    1 + source.as_bytes()[..=last_byte].iter().filter(|&&b| b == b'\n').count()
}

struct Visitor<'a> {
    source: &'a str,
    style: Style,
    severity: Severity,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'a> for Visitor<'a> {
    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode<'a>) {
        let first_line = line_of(self.source, node.location().start_offset());
        let last_line = last_line_of(self.source, node.location().end_offset());
        let body = node.body();
        self.offenses.extend(check(
            "Layout/EmptyLinesAroundModuleBody",
            self.severity,
            "module",
            self.style,
            first_line,
            last_line,
            body.as_ref(),
            self.source,
            self.ctx,
        ));
        ruby_prism::visit_module_node(self, node);
    }
}

impl Cop for EmptyLinesAroundModuleBody {
    fn name(&self) -> &'static str {
        "Layout/EmptyLinesAroundModuleBody"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor {
            source: ctx.source,
            style: self.style,
            severity: self.severity(),
            ctx,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
}
impl Default for Cfg {
    fn default() -> Self { Self { enforced_style: "no_empty_lines".into() } }
}

crate::register_cop!("Layout/EmptyLinesAroundModuleBody", |cfg| {
    let c: Cfg = cfg.typed("Layout/EmptyLinesAroundModuleBody");
    let style = Style::parse(&c.enforced_style);
    Some(Box::new(EmptyLinesAroundModuleBody::new(style)))
});
