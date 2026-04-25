//! Style/SingleLineDoEndBlock cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Prefer multiline `do`...`end` block.";

pub struct SingleLineDoEndBlock {
    inspect_blocks: bool,
    line_length_max: usize,
}

impl SingleLineDoEndBlock {
    pub fn new(inspect_blocks: bool, line_length_max: usize) -> Self {
        Self { inspect_blocks, line_length_max }
    }
}

impl Cop for SingleLineDoEndBlock {
    fn name(&self) -> &'static str { "Style/SingleLineDoEndBlock" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();
        let mut v = V { ctx, cop: self, out: vec![] };
        v.visit(&tree);
        v.out
    }
}

struct V<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    cop: &'a SingleLineDoEndBlock,
    out: Vec<Offense>,
}

fn line_of(src: &str, off: usize) -> &str {
    let bytes = src.as_bytes();
    let mut s = off;
    while s > 0 && bytes[s - 1] != b'\n' { s -= 1; }
    let mut e = off;
    while e < bytes.len() && bytes[e] != b'\n' { e += 1; }
    &src[s..e]
}

fn opening_is_do(src: &str, opening_start: usize, opening_end: usize) -> bool {
    src.get(opening_start..opening_end).map(|s| s == "do").unwrap_or(false)
}

fn first_stmt_heredoc_close(body: &Option<ruby_prism::Node>) -> Option<usize> {
    let stmts = body.as_ref()?.as_statements_node()?;
    let first = stmts.body().iter().next()?;
    if let Some(s) = first.as_string_node() {
        let opening = s.opening_loc()?;
        let oloc = opening;
        let closing = s.closing_loc()?;
        // Heredoc opener starts with `<<`
        let _ = oloc;
        return Some(closing.end_offset());
    }
    if let Some(s) = first.as_interpolated_string_node() {
        let _opening = s.opening_loc()?;
        let closing = s.closing_loc()?;
        return Some(closing.end_offset());
    }
    None
}

fn is_heredoc_first(body: &Option<ruby_prism::Node>, src: &str) -> bool {
    let Some(stmts) = body.as_ref().and_then(|b| b.as_statements_node()) else { return false };
    let Some(first) = stmts.body().iter().next() else { return false };
    if let Some(s) = first.as_string_node() {
        if let Some(op) = s.opening_loc() {
            let bytes = src.as_bytes();
            let st = op.start_offset();
            return st + 1 < bytes.len() && bytes[st] == b'<' && bytes[st + 1] == b'<';
        }
    }
    if let Some(s) = first.as_interpolated_string_node() {
        if let Some(op) = s.opening_loc() {
            let bytes = src.as_bytes();
            let st = op.start_offset();
            return st + 1 < bytes.len() && bytes[st] == b'<' && bytes[st + 1] == b'<';
        }
    }
    false
}

impl<'a, 'b> V<'a, 'b> {
    fn handle(
        &mut self,
        node_loc_start: usize,
        node_loc_end: usize,
        opening_start: usize,
        opening_end: usize,
        closing_start: usize,
        closing_end: usize,
        params_end: Option<usize>,
        body: Option<ruby_prism::Node>,
        force_after_opening: bool,
    ) {
        let src = self.ctx.source;
        if !opening_is_do(src, opening_start, opening_end) { return; }

        // Single-line check via offsets: no `\n` between block start and end.
        if src.as_bytes()[node_loc_start..node_loc_end].contains(&b'\n') { return; }

        // Skip when RedundantLineBreak InspectBlocks=true & line fits LineLength.Max.
        if self.cop.inspect_blocks {
            let line = line_of(src, node_loc_start);
            if line.chars().count() <= self.cop.line_length_max { return; }
        }

        let insert1_at = if force_after_opening {
            opening_end
        } else {
            params_end.unwrap_or(opening_end)
        };

        let mk_insert = |off, s: String| Edit { start_offset: off, end_offset: off, replacement: s };
        let mk_delete = |s, e| Edit { start_offset: s, end_offset: e, replacement: String::new() };
        let mut edits = vec![mk_insert(insert1_at, "\n".to_string())];

        if is_heredoc_first(&body, src) {
            edits.push(mk_delete(closing_start, closing_end));
            if let Some(heredoc_close_end) = first_stmt_heredoc_close(&body) {
                let bytes = src.as_bytes();
                let prev_is_nl = heredoc_close_end > 0 && bytes[heredoc_close_end - 1] == b'\n';
                let need_trailing_nl = heredoc_close_end >= bytes.len()
                    || bytes[heredoc_close_end] != b'\n';
                let mut text = String::new();
                if !prev_is_nl { text.push('\n'); }
                text.push_str("end");
                if need_trailing_nl { text.push('\n'); }
                edits.push(mk_insert(heredoc_close_end, text));
            }
        } else {
            edits.push(mk_insert(closing_start, "\n".to_string()));
        }

        let off = self
            .ctx
            .offense_with_range("Style/SingleLineDoEndBlock", MSG, Severity::Convention,
                node_loc_start, node_loc_end)
            .with_correction(Correction { edits });
        self.out.push(off);
    }
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if let Some(block_node) = node.block() {
            if let Some(b) = block_node.as_block_node() {
                let opening = b.opening_loc();
                let closing = b.closing_loc();
                let params_end = b.parameters().and_then(|p| {
                    p.as_block_parameters_node().map(|bp| bp.location().end_offset())
                });
                let call_loc = node.location();
                self.handle(
                    call_loc.start_offset(), call_loc.end_offset(),
                    opening.start_offset(), opening.end_offset(),
                    closing.start_offset(), closing.end_offset(),
                    params_end,
                    b.body(),
                    false,
                );
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        let opening = node.opening_loc();
        let closing = node.closing_loc();
        // Lambda literal: `->(args) do ... end`. The LambdaNode's own location
        // typically begins at `do`; extend back via operator_loc to include `->`.
        let op_loc = node.operator_loc();
        let start = op_loc.start_offset();
        let end = closing.end_offset();
        self.handle(
            start, end,
            opening.start_offset(), opening.end_offset(),
            closing.start_offset(), closing.end_offset(),
            None,
            node.body(),
            true,
        );
        ruby_prism::visit_lambda_node(self, node);
    }
}

crate::register_cop!("Style/SingleLineDoEndBlock", |cfg| {
    let inspect_blocks = cfg.is_cop_enabled("Layout/RedundantLineBreak")
        && cfg
            .get_cop_config("Layout/RedundantLineBreak")
            .and_then(|c| c.raw.get("InspectBlocks"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
    let line_length_max = cfg
        .get_cop_config("Layout/LineLength")
        .and_then(|c| c.raw.get("Max"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(80);
    Some(Box::new(SingleLineDoEndBlock::new(inspect_blocks, line_length_max)))
});
