//! Layout/EmptyLinesAroundBlockBody
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/empty_lines_around_block_body.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::empty_lines_around_body::{check, Style};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

pub use crate::helpers::empty_lines_around_body::Style as EmptyLinesAroundBlockBodyStyle;

pub struct EmptyLinesAroundBlockBody {
    style: Style,
}

impl EmptyLinesAroundBlockBody {
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
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'a>) {
        // adjusted_first_line = line of `{` / `do` (mirrors RuboCop's `send_node.last_line`).
        let opening_line = line_of(self.source, node.opening_loc().start_offset());
        let closing_line = last_line_of(self.source, node.closing_loc().end_offset());

        let body = node.body();
        self.offenses.extend(check(
            "Layout/EmptyLinesAroundBlockBody",
            self.severity,
            "block",
            self.style,
            opening_line,
            closing_line,
            body.as_ref(),
            self.source,
            self.ctx,
        ));

        ruby_prism::visit_block_node(self, node);
    }
}

impl Cop for EmptyLinesAroundBlockBody {
    fn name(&self) -> &'static str {
        "Layout/EmptyLinesAroundBlockBody"
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

crate::register_cop!("Layout/EmptyLinesAroundBlockBody", |cfg| {
    let c: Cfg = cfg.typed("Layout/EmptyLinesAroundBlockBody");
    let style = EmptyLinesAroundBlockBodyStyle::parse(&c.enforced_style);
    Some(Box::new(EmptyLinesAroundBlockBody::new(style)))
});
