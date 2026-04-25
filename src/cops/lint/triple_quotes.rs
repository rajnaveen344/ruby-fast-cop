//! Lint/TripleQuotes cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Delimiting a string with multiple quotes has no effect, use a single quote instead.";

#[derive(Default)]
pub struct TripleQuotes;

impl TripleQuotes {
    pub fn new() -> Self { Self }
}

impl Cop for TripleQuotes {
    fn name(&self) -> &'static str { "Lint/TripleQuotes" }
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

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode) {
        let loc = node.location();
        let src = self.ctx.source;
        let start = loc.start_offset();
        let end = loc.end_offset();
        // Count leading quote marks (' or ") in source
        let bytes = src.as_bytes();
        let mut i = start;
        let mut lead_count = 0usize;
        while i < end && (bytes[i] == b'"' || bytes[i] == b'\'') {
            lead_count += 1;
            i += 1;
        }
        if lead_count < 3 {
            ruby_prism::visit_interpolated_string_node(self, node);
            return;
        }
        // Collect child str nodes with empty value
        let parts: Vec<_> = node.parts().iter().collect();
        let mut empty_indices: Vec<usize> = vec![];
        let mut total_strings = 0usize;
        for (idx, p) in parts.iter().enumerate() {
            if let Some(s) = p.as_string_node() {
                total_strings += 1;
                if s.unescaped().is_empty() {
                    empty_indices.push(idx);
                }
            }
        }
        if empty_indices.is_empty() {
            ruby_prism::visit_interpolated_string_node(self, node);
            return;
        }
        // If all children are empty str nodes, keep one (shift front)
        if empty_indices.len() == parts.len() && total_strings == parts.len() {
            empty_indices.remove(0);
        }
        if empty_indices.is_empty() {
            ruby_prism::visit_interpolated_string_node(self, node);
            return;
        }
        // Offense range: first-line span
        let mut line_end = start;
        while line_end < end && bytes[line_end] != b'\n' { line_end += 1; }
        let mut edits: Vec<Edit> = vec![];
        for idx in &empty_indices {
            let p = &parts[*idx];
            let ploc = p.location();
            edits.push(Edit {
                start_offset: ploc.start_offset(),
                end_offset: ploc.end_offset(),
                replacement: String::new(),
            });
        }
        let offense = self.ctx.offense_with_range(
            "Lint/TripleQuotes", MSG, Severity::Warning,
            start, line_end,
        ).with_correction(Correction { edits });
        self.out.push(offense);
        ruby_prism::visit_interpolated_string_node(self, node);
    }
}

crate::register_cop!("Lint/TripleQuotes", |_cfg| Some(Box::new(TripleQuotes::new())));
