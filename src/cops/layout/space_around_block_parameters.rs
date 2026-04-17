//! Layout/SpaceAroundBlockParameters
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/space_around_block_parameters.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    NoSpace,
    Space,
}

pub struct SpaceAroundBlockParameters {
    style: Style,
}

impl Default for SpaceAroundBlockParameters {
    fn default() -> Self {
        Self { style: Style::NoSpace }
    }
}

impl SpaceAroundBlockParameters {
    pub fn new(style: Style) -> Self { Self { style } }
}

impl Cop for SpaceAroundBlockParameters {
    fn name(&self) -> &'static str { "Layout/SpaceAroundBlockParameters" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor { ctx, style: self.style, offenses: Vec::new() };
        v.visit_program_node(node);
        // Dedupe by (line, column, last_column): matches RuboCop's offense dedup
        // where two different check paths produce the same range.
        let mut seen = std::collections::HashSet::new();
        v.offenses.retain(|o| {
            let key = (o.location.line, o.location.column, o.location.last_column);
            seen.insert(key)
        });
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: Style,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn push_offense(&mut self, begin: usize, end: usize, msg: &str, replacement: &str) {
        let loc = Location::from_offsets(self.ctx.source, begin, end);
        let correction = Correction::replace(begin, end, replacement.to_string());
        self.offenses.push(
            Offense::new(
                "Layout/SpaceAroundBlockParameters",
                msg,
                Severity::Convention,
                loc,
                self.ctx.filename,
            )
            .with_correction(correction),
        );
    }

    fn check_space_missing(&mut self, pos: usize, msg: &str) {
        // Zero-width range at `pos` to signal missing space (expect_offense ^ is 1 col).
        let loc = Location::from_offsets(self.ctx.source, pos, pos);
        self.offenses.push(
            Offense::new(
                "Layout/SpaceAroundBlockParameters",
                msg,
                Severity::Convention,
                loc,
                self.ctx.filename,
            )
            .with_correction(Correction::insert(pos, " ")),
        );
    }

    fn handle_params(&mut self, bp_node: &ruby_prism::BlockParametersNode, body: Option<Node<'_>>) {
        let (Some(opening), Some(closing)) = (bp_node.opening_loc(), bp_node.closing_loc()) else {
            return;
        };
        let opening_end = opening.end_offset();
        let closing_begin = closing.start_offset();

        let args = collect_args(bp_node);
        if args.is_empty() { return; }

        let first_begin = args.first().unwrap().location().start_offset();
        let last_end_raw = args.last().unwrap().location().end_offset();
        // Include trailing comma if present between last arg and closing pipe.
        let last_end = include_trailing_comma(self.ctx.source, last_end_raw, closing_begin);

        // Inside pipes.
        match self.style {
            Style::NoSpace => {
                // Space before first: (opening_end, first_begin) non-empty
                if first_begin > opening_end && has_only_spaces(self.ctx.source, opening_end, first_begin) {
                    self.push_offense(opening_end, first_begin, "Space before first block parameter detected.", "");
                }
                // Space after last: (last_end, closing_begin) non-empty
                if closing_begin > last_end && has_only_spaces(self.ctx.source, last_end, closing_begin) {
                    self.push_offense(last_end, closing_begin, "Space after last block parameter detected.", "");
                }
            }
            Style::Space => {
                let first_arg_loc = args.first().unwrap().location();
                let last_arg_loc = args.last().unwrap().location();
                // Must have EXACTLY one space before first.
                if first_begin == opening_end {
                    // Missing — offense target = first arg's source_range.
                    let loc = Location::from_offsets(
                        self.ctx.source,
                        first_arg_loc.start_offset(),
                        first_arg_loc.end_offset(),
                    );
                    self.offenses.push(
                        Offense::new(
                            "Layout/SpaceAroundBlockParameters",
                            "Space before first block parameter missing.",
                            Severity::Convention,
                            loc,
                            self.ctx.filename,
                        )
                        .with_correction(Correction::insert(opening_end, " ")),
                    );
                } else if first_begin > opening_end + 1
                    && has_only_spaces(self.ctx.source, opening_end, first_begin)
                {
                    // Extra: flag [opening_end, first_begin - 1). Length = n_spaces - 1.
                    self.push_offense(
                        opening_end,
                        first_begin - 1,
                        "Extra space before first block parameter detected.",
                        "",
                    );
                }
                // Must have EXACTLY one space after last.
                if last_end == closing_begin {
                    // Missing — offense target = last arg's source_range.
                    let loc = Location::from_offsets(
                        self.ctx.source,
                        last_arg_loc.start_offset(),
                        last_arg_loc.end_offset(),
                    );
                    self.offenses.push(
                        Offense::new(
                            "Layout/SpaceAroundBlockParameters",
                            "Space after last block parameter missing.",
                            Severity::Convention,
                            loc,
                            self.ctx.filename,
                        )
                        .with_correction(Correction::insert(last_arg_loc.end_offset(), " ")),
                    );
                } else if closing_begin > last_end + 1
                    && has_only_spaces(self.ctx.source, last_end, closing_begin)
                {
                    // Extra: flag [last_end + 1, closing_begin). Length = n_spaces - 1.
                    self.push_offense(
                        last_end + 1,
                        closing_begin,
                        "Extra space after last block parameter detected.",
                        "",
                    );
                }
            }
        }

        // After closing pipe (space required before body).
        if let Some(body_node) = body {
            let closing_end = closing.end_offset();
            let closing_begin_p = closing.start_offset();
            let body_begin = body_node.location().start_offset();
            if closing_end == body_begin && closing_is_pipe(self.ctx.source, closing_begin_p) {
                // Offense target = the closing pipe itself.
                let loc = Location::from_offsets(self.ctx.source, closing_begin_p, closing_end);
                self.offenses.push(
                    Offense::new(
                        "Layout/SpaceAroundBlockParameters",
                        "Space after closing `|` missing.",
                        Severity::Convention,
                        loc,
                        self.ctx.filename,
                    )
                    .with_correction(Correction::insert(closing_end, " ")),
                );
            }
        }

        // Check each arg for extra space before.
        for arg in &args {
            self.check_arg_recursive(arg, opening_end);
        }
    }

    fn check_arg_recursive(&mut self, arg: &Node, opening_end: usize) {
        // Recurse into MultiTarget first.
        if let Some(mt) = arg.as_multi_target_node() {
            for sub in mt.lefts().iter() {
                self.check_arg_recursive(&sub, opening_end);
            }
            if let Some(rest) = mt.rest() {
                self.check_arg_recursive(&rest, opening_end);
            }
            for sub in mt.rights().iter() {
                self.check_arg_recursive(&sub, opening_end);
            }
        }

        let arg_begin = arg.location().start_offset();

        // Scan backwards for spaces/tabs until non-ws.
        let bytes = self.ctx.source.as_bytes();
        let mut i = arg_begin;
        while i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') {
            i -= 1;
        }
        if i == arg_begin { return; } // no leading space
        // Skip if newline just before (multiline block args).
        if i > 0 && (bytes[i - 1] == b'\n' || bytes[i - 1] == b'\r') { return; }
        // RuboCop: range [i, arg_begin - 1). Length = leading_ws_count - 1.
        // If only 1 leading space, arg_begin - 1 == i, so no offense.
        let space_start = i;
        let space_end = arg_begin - 1;
        if space_end <= space_start { return; }

        // When leading whitespace runs all the way back to the opening pipe, the
        // "Space before first block parameter" check already generates an offense
        // whose correction covers this entire zone. Emit the offense (for parity)
        // with a correction that matches the wider delete, so apply_corrections'
        // overlap skip leaves the correct result.
        let correction = if i == opening_end {
            Correction::delete(opening_end, arg_begin)
        } else {
            Correction::delete(space_start, space_end)
        };
        let loc = Location::from_offsets(self.ctx.source, space_start, space_end);
        self.offenses.push(
            Offense::new(
                "Layout/SpaceAroundBlockParameters",
                "Extra space before block parameter detected.",
                Severity::Convention,
                loc,
                self.ctx.filename,
            )
            .with_correction(correction),
        );
    }
}

fn closing_is_pipe(source: &str, pos: usize) -> bool {
    source.as_bytes().get(pos).copied() == Some(b'|')
}

fn include_trailing_comma(source: &str, last_end: usize, closing_begin: usize) -> usize {
    // If between last_end and closing_begin there's a comma (possibly surrounded by ws),
    // return position just after the comma.
    let bytes = source.as_bytes();
    let mut i = last_end;
    while i < closing_begin && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i < closing_begin && bytes[i] == b',' {
        return i + 1;
    }
    last_end
}

fn has_only_spaces(source: &str, start: usize, end: usize) -> bool {
    if start >= end { return false; }
    source.as_bytes()[start..end].iter().all(|&b| b == b' ' || b == b'\t')
}

fn collect_args<'a>(bp_node: &ruby_prism::BlockParametersNode<'a>) -> Vec<Node<'a>> {
    let mut out = Vec::new();
    if let Some(params) = bp_node.parameters() {
        for n in params.requireds().iter() { out.push(n); }
        for n in params.optionals().iter() { out.push(n); }
        if let Some(rest) = params.rest() {
            // ImplicitRestNode is the phantom rest from a trailing comma (`|x,|`).
            // It has zero-width presence — skip to avoid false "last arg".
            if rest.as_implicit_rest_node().is_none() {
                out.push(rest);
            }
        }
        for n in params.posts().iter() { out.push(n); }
        for n in params.keywords().iter() { out.push(n); }
        if let Some(kr) = params.keyword_rest() { out.push(kr); }
        if let Some(bl) = params.block() { out.push(bl.as_node()); }
    }
    for n in bp_node.locals().iter() { out.push(n); }
    out.sort_by_key(|n| n.location().start_offset());
    out
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        if let Some(params) = node.parameters() {
            if let Some(bp) = params.as_block_parameters_node() {
                self.handle_params(&bp, node.body());
            }
        }
        ruby_prism::visit_block_node(self, node);
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        if let Some(params) = node.parameters() {
            if let Some(bp) = params.as_block_parameters_node() {
                self.handle_params(&bp, node.body());
            }
        }
        ruby_prism::visit_lambda_node(self, node);
    }
}

crate::register_cop!("Layout/SpaceAroundBlockParameters", |cfg| {
    let style = cfg
        .get_cop_config("Layout/SpaceAroundBlockParameters")
        .and_then(|c| c.raw.get("EnforcedStyleInsidePipes"))
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "space" => Style::Space,
            _ => Style::NoSpace,
        })
        .unwrap_or(Style::NoSpace);
    Some(Box::new(SpaceAroundBlockParameters::new(style)))
});
