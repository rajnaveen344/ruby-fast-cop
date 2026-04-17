use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

/// Layout/CaseIndentation — checks how `when`/`in` are indented relative to `case`/`end`.
pub struct CaseIndentation {
    style: String,        // "case" (default) or "end"
    indent_one_step: bool,
    indentation_width: Option<usize>, // cop-specific override; falls back to IndentationWidth.Width (default 2)
    layout_indent_width: usize,       // Layout/IndentationWidth.Width
}

impl CaseIndentation {
    pub fn new() -> Self {
        Self {
            style: "case".to_string(),
            indent_one_step: false,
            indentation_width: None,
            layout_indent_width: 2,
        }
    }

    pub fn with_config(
        style: String,
        indent_one_step: bool,
        indentation_width: Option<usize>,
        layout_indent_width: usize,
    ) -> Self {
        Self {
            style,
            indent_one_step,
            indentation_width,
            layout_indent_width,
        }
    }
}

impl Default for CaseIndentation {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for CaseIndentation {
    fn name(&self) -> &'static str {
        "Layout/CaseIndentation"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = CaseVisitor {
            ctx,
            style: self.style.as_str(),
            indent_one_step: self.indent_one_step,
            indent_width: self.indentation_width.unwrap_or(self.layout_indent_width),
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct CaseVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: &'a str,
    indent_one_step: bool,
    indent_width: usize,
    offenses: Vec<Offense>,
}

impl<'a> CaseVisitor<'a> {
    fn check_case(
        &mut self,
        case_kw: &ruby_prism::Location,
        end_kw: &ruby_prism::Location,
        branches: Vec<(String, usize, usize)>,
        else_line: Option<usize>,
        last_cond_begin_line: Option<usize>,
    ) {
        let case_line = self.ctx.line_of(case_kw.start_offset());
        let end_line = self.ctx.line_of(end_kw.start_offset());

        // Single-line case: skip
        if case_line == end_line {
            return;
        }

        // enforced_style_end and end/last conditional same line — skip
        if self.style == "end" {
            let last_cond_line = else_line.or(last_cond_begin_line);
            if let Some(lcl) = last_cond_line {
                if lcl == end_line {
                    return;
                }
            }
        }

        let base_col = match self.style {
            "end" => self.ctx.col_of(end_kw.start_offset()),
            _ => self.ctx.col_of(case_kw.start_offset()),
        };

        let indent_delta = if self.indent_one_step { self.indent_width } else { 0 };
        let expected_col = base_col + indent_delta;

        for (branch_type, kw_start, kw_end) in branches {
            let kw_col = self.ctx.col_of(kw_start);
            if kw_col != expected_col {
                let depth = if self.indent_one_step {
                    "one step more than"
                } else {
                    "as deep as"
                };
                let message = format!(
                    "Indent `{}` {} `{}`.",
                    branch_type, depth, self.style
                );
                let location = crate::offense::Location::from_offsets(
                    self.ctx.source,
                    kw_start,
                    kw_end,
                );
                self.offenses.push(Offense::new(
                    "Layout/CaseIndentation",
                    message,
                    Severity::Convention,
                    location,
                    self.ctx.filename,
                ));
            }
        }
    }
}

impl Visit<'_> for CaseVisitor<'_> {
    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        let case_kw = node.case_keyword_loc();
        let end_kw = node.end_keyword_loc();

        let mut branches = Vec::new();
        let mut last_cond_begin_line: Option<usize> = None;
        for when in node.conditions().iter() {
            if let Some(w) = when.as_when_node() {
                let kw = w.keyword_loc();
                let (s, e) = (kw.start_offset(), kw.end_offset());
                branches.push(("when".to_string(), s, e));
                last_cond_begin_line = Some(self.ctx.line_of(s));
            }
        }

        let else_line = node
            .else_clause()
            .map(|e| self.ctx.line_of(e.else_keyword_loc().start_offset()));

        self.check_case(&case_kw, &end_kw, branches, else_line, last_cond_begin_line);
        ruby_prism::visit_case_node(self, node);
    }

    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        let case_kw = node.case_keyword_loc();
        let end_kw = node.end_keyword_loc();

        let mut branches = Vec::new();
        let mut last_cond_begin_line: Option<usize> = None;
        for inp in node.conditions().iter() {
            if let Some(p) = inp.as_in_node() {
                let kw = p.in_loc();
                let (s, e) = (kw.start_offset(), kw.end_offset());
                branches.push(("in".to_string(), s, e));
                last_cond_begin_line = Some(self.ctx.line_of(s));
            }
        }

        let else_line = node
            .else_clause()
            .map(|e| self.ctx.line_of(e.else_keyword_loc().start_offset()));

        self.check_case(&case_kw, &end_kw, branches, else_line, last_cond_begin_line);
        ruby_prism::visit_case_match_node(self, node);
    }
}

crate::register_cop!("Layout/CaseIndentation", |cfg| {
    let cop_config = cfg.get_cop_config("Layout/CaseIndentation");
    let style = cop_config
        .and_then(|c| c.raw.get("EnforcedStyle"))
        .and_then(|v| v.as_str())
        .unwrap_or("case")
        .to_string();
    let indent_one_step = cop_config
        .and_then(|c| c.raw.get("IndentOneStep"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let indent_width = cop_config
        .and_then(|c| c.raw.get("IndentationWidth"))
        .and_then(|v| v.as_i64())
        .map(|v| v as usize);
    let layout_iw = cfg
        .get_cop_config("Layout/IndentationWidth")
        .and_then(|c| c.raw.get("Width"))
        .and_then(|v| v.as_i64())
        .map(|v| v as usize)
        .unwrap_or(2);
    Some(Box::new(CaseIndentation::with_config(style, indent_one_step, indent_width, layout_iw)))
});
