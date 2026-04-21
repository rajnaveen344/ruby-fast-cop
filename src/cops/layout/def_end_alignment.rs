use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefEndAlignmentStyle {
    StartOfLine,
    Def,
}

pub struct DefEndAlignment {
    style: DefEndAlignmentStyle,
}

impl DefEndAlignment {
    pub fn new(style: DefEndAlignmentStyle) -> Self {
        Self { style }
    }
}

impl Cop for DefEndAlignment {
    fn name(&self) -> &'static str {
        "Layout/DefEndAlignment"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = DefEndAlignmentVisitor {
            ctx,
            style: self.style,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct DefEndAlignmentVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: DefEndAlignmentStyle,
    offenses: Vec<Offense>,
}

/// Check if a CallNode is a def_modifier (e.g., `private def foo`).
/// In Prism, this appears as a CallNode whose arguments contain a DefNode.
fn is_def_modifier(call: &ruby_prism::CallNode) -> bool {
    if call.receiver().is_some() {
        return false;
    }
    if let Some(args) = call.arguments() {
        for arg in args.arguments().iter() {
            if arg.as_def_node().is_some() {
                return true;
            }
        }
    }
    false
}

impl<'a> DefEndAlignmentVisitor<'a> {
    fn emit_offense(
        &mut self,
        end_off: usize,
        end_end_off: usize,
        expected_col: usize,
        align_source: &str,
        align_line: usize,
    ) {
        let end_col = self.ctx.col_of(end_off);
        if end_col == expected_col {
            return;
        }
        let end_line = self.ctx.line_of(end_off);
        let message = format!(
            "`end` at {}, {} is not aligned with `{}` at {}, {}.",
            end_line, end_col, align_source, align_line, expected_col
        );
        let location =
            crate::offense::Location::from_offsets(self.ctx.source, end_off, end_end_off);
        self.offenses.push(Offense::new(
            "Layout/DefEndAlignment",
            message,
            Severity::Convention,
            location,
            self.ctx.filename,
        ));
    }

    fn check_def_end(&mut self, def_node: &ruby_prism::DefNode) {
        let end_loc = match def_node.end_keyword_loc() {
            Some(loc) => loc,
            None => return,
        };

        let def_off = def_node.def_keyword_loc().start_offset();
        let end_off = end_loc.start_offset();

        if self.ctx.same_line(def_off, end_off) {
            return;
        }

        let (expected_col, align_source) = match self.style {
            DefEndAlignmentStyle::Def => (self.ctx.col_of(def_off), "def".to_string()),
            DefEndAlignmentStyle::StartOfLine => (self.ctx.indentation_of(def_off), "def".to_string()),
        };

        self.emit_offense(end_off, end_loc.end_offset(), expected_col, &align_source, self.ctx.line_of(def_off));
    }

    fn check_send_def_modifier(&mut self, call: &ruby_prism::CallNode) {
        if !is_def_modifier(call) {
            return;
        }

        let args = match call.arguments() {
            Some(a) => a,
            None => return,
        };

        for arg in args.arguments().iter() {
            if let Some(def_node) = arg.as_def_node() {
                let end_loc = match def_node.end_keyword_loc() {
                    Some(loc) => loc,
                    None => return,
                };

                let end_off = end_loc.start_offset();
                let def_kw_off = def_node.def_keyword_loc().start_offset();

                if self.ctx.same_line(def_kw_off, end_off) {
                    return;
                }

                let call_off = call.location().start_offset();
                let (expected_col, align_source, align_line) = match self.style {
                    DefEndAlignmentStyle::Def => {
                        (self.ctx.col_of(def_kw_off), "def".to_string(), self.ctx.line_of(def_kw_off))
                    }
                    DefEndAlignmentStyle::StartOfLine => {
                        let def_kw_end = def_node.def_keyword_loc().end_offset();
                        (self.ctx.indentation_of(call_off), self.ctx.src(call_off, def_kw_end).to_string(), self.ctx.line_of(call_off))
                    }
                };

                self.emit_offense(end_off, end_loc.end_offset(), expected_col, &align_source, align_line);
            }
        }
    }
}

impl Visit<'_> for DefEndAlignmentVisitor<'_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        // Only check if not already handled by a send modifier parent.
        // We check: is the def on its own line (not preceded by a modifier call)?
        // Prism structures `private def foo...end` as:
        //   CallNode(name="private", arguments=[DefNode])
        // So when we visit the DefNode directly, we check if it begins its line.
        // If it doesn't, it might be handled by the send visitor — but we still
        // need to handle plain `def` here.
        // The simple approach: check if the def is the first token on its line.
        let def_off = node.def_keyword_loc().start_offset();
        if self.ctx.begins_its_line(def_off) {
            self.check_def_end(node);
        }
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_send_def_modifier(node);
        ruby_prism::visit_call_node(self, node);
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

crate::register_cop!("Layout/DefEndAlignment", |cfg| {
    let c: Cfg = cfg.typed("Layout/DefEndAlignment");
    let align_style = match c.enforced_style_align_with.as_str() {
        "def" => DefEndAlignmentStyle::Def,
        _ => DefEndAlignmentStyle::StartOfLine,
    };
    Some(Box::new(DefEndAlignment::new(align_style)))
});
