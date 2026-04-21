use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeginEndAlignmentStyle {
    StartOfLine,
    Begin,
}

pub struct BeginEndAlignment {
    style: BeginEndAlignmentStyle,
}

impl BeginEndAlignment {
    pub fn new(style: BeginEndAlignmentStyle) -> Self {
        Self { style }
    }
}

impl Cop for BeginEndAlignment {
    fn name(&self) -> &'static str {
        "Layout/BeginEndAlignment"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = BeginEndAlignmentVisitor {
            ctx,
            style: self.style,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct BeginEndAlignmentVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: BeginEndAlignmentStyle,
    offenses: Vec<Offense>,
}

impl<'a> BeginEndAlignmentVisitor<'a> {
    fn check_begin_alignment(&mut self, node: &ruby_prism::BeginNode) {
        let begin_loc = match node.begin_keyword_loc() {
            Some(loc) => loc,
            None => return,
        };
        let end_loc = match node.end_keyword_loc() {
            Some(loc) => loc,
            None => return,
        };

        let begin_off = begin_loc.start_offset();
        let end_off = end_loc.start_offset();

        // Same line — no check needed
        if self.ctx.same_line(begin_off, end_off) {
            return;
        }

        let end_col = self.ctx.col_of(end_off);

        let (expected_col, align_source) = match self.style {
            BeginEndAlignmentStyle::Begin => {
                let col = self.ctx.col_of(begin_off);
                (col, "begin".to_string())
            }
            BeginEndAlignmentStyle::StartOfLine => {
                let line_text = self.ctx.line_text(begin_off);
                let trimmed = line_text.trim_start();
                let indent = line_text.len() - trimmed.len();
                (indent, trimmed.trim_end().to_string())
            }
        };

        if end_col == expected_col {
            return;
        }

        let end_line = self.ctx.line_of(end_off);
        let align_line = self.ctx.line_of(begin_off);

        let message = format!(
            "`end` at {}, {} is not aligned with `{}` at {}, {}.",
            end_line, end_col, align_source, align_line, expected_col
        );

        let location =
            crate::offense::Location::from_offsets(self.ctx.source, end_off, end_loc.end_offset());
        self.offenses.push(Offense::new(
            "Layout/BeginEndAlignment",
            message,
            Severity::Convention,
            location,
            self.ctx.filename,
        ));
    }
}

impl Visit<'_> for BeginEndAlignmentVisitor<'_> {
    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        self.check_begin_alignment(node);
        ruby_prism::visit_begin_node(self, node);
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style_align_with: String,
}
impl Default for Cfg {
    fn default() -> Self { Self { enforced_style_align_with: "start_of_line".into() } }
}

crate::register_cop!("Layout/BeginEndAlignment", |cfg| {
    let c: Cfg = cfg.typed("Layout/BeginEndAlignment");
    let align_style = match c.enforced_style_align_with.as_str() {
        "begin" => BeginEndAlignmentStyle::Begin,
        _ => BeginEndAlignmentStyle::StartOfLine,
    };
    Some(Box::new(BeginEndAlignment::new(align_style)))
});
