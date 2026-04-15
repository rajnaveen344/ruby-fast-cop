use crate::cops::{CheckContext, Cop};
use crate::helpers::trailing_comma::{
    self, effective_end_offset, find_trailing_comma, trailing_comma_search_start,
};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

pub use crate::helpers::trailing_comma::EnforcedStyleForMultiline;

pub struct TrailingCommaInArguments {
    style: EnforcedStyleForMultiline,
}

impl TrailingCommaInArguments {
    pub fn new(style: EnforcedStyleForMultiline) -> Self {
        Self { style }
    }

    fn check_call_node(
        &self,
        node: &ruby_prism::CallNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let source = ctx.source;

        let arguments = match node.arguments() {
            Some(args) => args,
            None => return vec![],
        };

        let args: Vec<Node> = arguments.arguments().iter().collect();
        if args.is_empty() {
            return vec![];
        }

        let (open_offset, close_offset) = match self.find_delimiters(node, source) {
            Some(pair) => pair,
            None => return vec![],
        };

        let last_arg = &args[args.len() - 1];
        let last_is_block_pass = matches!(last_arg, Node::BlockArgumentNode { .. });

        let is_multiline = !ctx.same_line(open_offset, close_offset);

        let search_start = trailing_comma_search_start(last_arg, source);
        let trailing_comma_pos = find_trailing_comma(source, search_start, close_offset);

        if is_multiline {
            self.check_multiline(
                ctx,
                &args,
                last_arg,
                last_is_block_pass,
                close_offset,
                trailing_comma_pos,
                source,
            )
        } else {
            self.check_single_line(ctx, trailing_comma_pos)
        }
    }

    fn find_delimiters(
        &self,
        node: &ruby_prism::CallNode,
        source: &str,
    ) -> Option<(usize, usize)> {
        if let (Some(open_loc), Some(close_loc)) = (node.opening_loc(), node.closing_loc()) {
            let open_offset = open_loc.start_offset();
            let close_offset = close_loc.start_offset();
            let open_char = source.as_bytes().get(open_offset).copied();
            if matches!(open_char, Some(b'(') | Some(b'[')) {
                return Some((open_offset, close_offset));
            }
        }
        None
    }

    fn check_single_line(
        &self,
        ctx: &CheckContext,
        trailing_comma_pos: Option<usize>,
    ) -> Vec<Offense> {
        if let Some(comma_offset) = trailing_comma_pos {
            let msg = self.single_line_message();
            let offense = ctx
                .offense_with_range(
                    self.name(),
                    msg,
                    Severity::Convention,
                    comma_offset,
                    comma_offset + 1,
                )
                .with_correction(Correction::delete(comma_offset, comma_offset + 1));
            vec![offense]
        } else {
            vec![]
        }
    }

    fn check_multiline(
        &self,
        ctx: &CheckContext,
        args: &[Node],
        last_arg: &Node,
        last_is_block_pass: bool,
        close_offset: usize,
        trailing_comma_pos: Option<usize>,
        source: &str,
    ) -> Vec<Offense> {
        match self.style {
            EnforcedStyleForMultiline::NoComma => {
                if let Some(comma_offset) = trailing_comma_pos {
                    let msg = "Avoid comma after the last parameter of a method call.";
                    let offense = ctx
                        .offense_with_range(
                            self.name(),
                            msg,
                            Severity::Convention,
                            comma_offset,
                            comma_offset + 1,
                        )
                        .with_correction(Correction::delete(
                            comma_offset,
                            comma_offset + 1,
                        ));
                    vec![offense]
                } else {
                    vec![]
                }
            }
            EnforcedStyleForMultiline::Comma => self.check_multiline_comma_style(
                ctx,
                args,
                last_arg,
                last_is_block_pass,
                close_offset,
                trailing_comma_pos,
                source,
                false,
            ),
            EnforcedStyleForMultiline::ConsistentComma => self.check_multiline_comma_style(
                ctx,
                args,
                last_arg,
                last_is_block_pass,
                close_offset,
                trailing_comma_pos,
                source,
                true,
            ),
            EnforcedStyleForMultiline::DiffComma => self.check_multiline_diff_comma_style(
                ctx,
                args,
                last_arg,
                last_is_block_pass,
                close_offset,
                trailing_comma_pos,
                source,
            ),
        }
    }

    fn check_multiline_comma_style(
        &self,
        ctx: &CheckContext,
        args: &[Node],
        last_arg: &Node,
        last_is_block_pass: bool,
        close_offset: usize,
        trailing_comma_pos: Option<usize>,
        source: &str,
        consistent: bool,
    ) -> Vec<Offense> {
        if last_is_block_pass {
            return vec![];
        }

        let last_end = effective_end_offset(last_arg, source);
        let close_on_same_line = ctx.same_line(last_end, close_offset);

        if !consistent {
            if close_on_same_line {
                return vec![];
            }
            if has_multiple_items_on_same_line(args, ctx) {
                return vec![];
            }
            if args.len() == 1 {
                return vec![];
            }
        } else {
            if args.len() == 1
                && !is_multiline_single_arg_needing_comma(last_arg, ctx, close_offset)
            {
                return vec![];
            }
            if close_on_same_line && matches!(last_arg, Node::HashNode { .. }) {
                return vec![];
            }
        }

        if trailing_comma_pos.is_some() {
            vec![]
        } else {
            self.missing_comma_offense(ctx, last_arg, source)
        }
    }

    fn check_multiline_diff_comma_style(
        &self,
        ctx: &CheckContext,
        args: &[Node],
        last_arg: &Node,
        last_is_block_pass: bool,
        close_offset: usize,
        trailing_comma_pos: Option<usize>,
        source: &str,
    ) -> Vec<Offense> {
        if last_is_block_pass {
            return vec![];
        }

        let last_end = effective_end_offset(last_arg, source);
        let close_on_same_line = ctx.same_line(last_end, close_offset);

        if close_on_same_line {
            if let Some(comma_offset) = trailing_comma_pos {
                let msg = "Avoid comma after the last parameter of a method call, unless that item immediately precedes a newline.";
                let offense = ctx
                    .offense_with_range(
                        self.name(),
                        msg,
                        Severity::Convention,
                        comma_offset,
                        comma_offset + 1,
                    )
                    .with_correction(Correction::delete(comma_offset, comma_offset + 1));
                vec![offense]
            } else {
                vec![]
            }
        } else {
            if trailing_comma_pos.is_some() {
                return vec![];
            }
            if has_multiple_items_on_same_line(args, ctx) {
                return vec![];
            }
            if args.len() == 1 {
                return vec![];
            }
            self.missing_comma_offense(ctx, last_arg, source)
        }
    }

    fn missing_comma_offense(
        &self,
        ctx: &CheckContext,
        last_arg: &Node,
        source: &str,
    ) -> Vec<Offense> {
        let msg = "Put a comma after the last parameter of a multiline method call.";
        let (offense_start, offense_end) = trailing_comma::offense_range(last_arg, source);
        let insert_pos = trailing_comma::insertion_point_for_comma(last_arg, source);
        let offense = ctx
            .offense_with_range(
                self.name(),
                msg,
                Severity::Convention,
                offense_start,
                offense_end,
            )
            .with_correction(Correction::insert(insert_pos, ","));
        vec![offense]
    }

    fn single_line_message(&self) -> &'static str {
        match self.style {
            EnforcedStyleForMultiline::NoComma => {
                "Avoid comma after the last parameter of a method call."
            }
            EnforcedStyleForMultiline::Comma => {
                "Avoid comma after the last parameter of a method call, unless each item is on its own line."
            }
            EnforcedStyleForMultiline::ConsistentComma => {
                "Avoid comma after the last parameter of a method call, unless items are split onto multiple lines."
            }
            EnforcedStyleForMultiline::DiffComma => {
                "Avoid comma after the last parameter of a method call, unless that item immediately precedes a newline."
            }
        }
    }
}

impl Cop for TrailingCommaInArguments {
    fn name(&self) -> &'static str {
        "Style/TrailingCommaInArguments"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        self.check_call_node(node, ctx)
    }
}

// ─── Arg-specific same-line helpers ────────────────────────────────────

fn has_multiple_items_on_same_line(args: &[Node], ctx: &CheckContext) -> bool {
    let mut all_lines: Vec<usize> = Vec::new();
    for arg in args {
        match arg {
            Node::KeywordHashNode { .. } => {
                let kh = arg.as_keyword_hash_node().unwrap();
                for elem in kh.elements().iter() {
                    all_lines.push(ctx.line_of(elem.location().start_offset()));
                }
            }
            _ => {
                all_lines.push(ctx.line_of(arg.location().start_offset()));
            }
        }
    }
    for i in 1..all_lines.len() {
        if all_lines[i] == all_lines[i - 1] {
            return true;
        }
    }
    false
}

fn is_multiline_single_arg_needing_comma(
    arg: &Node,
    ctx: &CheckContext,
    close_offset: usize,
) -> bool {
    match arg {
        Node::KeywordHashNode { .. } => {
            let kh = arg.as_keyword_hash_node().unwrap();
            let elements: Vec<Node> = kh.elements().iter().collect();
            if elements.len() >= 2 {
                return !ctx.same_line(
                    elements[0].location().start_offset(),
                    elements.last().unwrap().location().start_offset(),
                );
            }
            false
        }
        Node::HashNode { .. } => false,
        _ => !ctx.same_line(arg.location().end_offset(), close_offset),
    }
}
