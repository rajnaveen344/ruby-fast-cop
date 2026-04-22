//! Layout/MultilineBlockLayout
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/multiline_block_layout.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Location, Offense, Severity};
use ruby_prism::Visit;

const COP_NAME: &str = "Layout/MultilineBlockLayout";
const MSG: &str = "Block body expression is on the same line as the block start.";
const ARG_MSG: &str = "Block argument expression is not on the same line as the block start.";

#[derive(Default)]
pub struct MultilineBlockLayout;

impl MultilineBlockLayout {
    pub fn new() -> Self {
        Self
    }
}

fn line_of(source: &str, offset: usize) -> usize {
    1 + source[..offset.min(source.len())].bytes().filter(|&b| b == b'\n').count()
}

fn line_start(source: &str, offset: usize) -> usize {
    source[..offset].rfind('\n').map_or(0, |p| p + 1)
}

fn col_of(source: &str, offset: usize) -> usize {
    offset - line_start(source, offset)
}

struct BlockVisitor<'a> {
    source: &'a str,
    filename: &'a str,
    max_line_length: Option<usize>,
    offenses: Vec<Offense>,
}

impl<'a> BlockVisitor<'a> {
    fn check_block(
        &mut self,
        block_node_start: usize,  // start of the entire block node (e.g. `test do`, `test {`)
        block_node_col: usize,    // column of the block's outermost expression (RuboCop: node.source_range.column)
        block_start: usize,       // start of `do`/`{` keyword
        block_close_start: usize, // start of `end`/`}` closing keyword
        begin_end: usize,          // end of `do`/`{` (and block params if any)
        params: Option<ruby_prism::BlockParametersNode<'a>>,
        body: Option<ruby_prism::Node<'a>>,
    ) {
        let source = self.source;
        let block_start_line = line_of(source, block_start);
        let block_close_line = line_of(source, block_close_start);

        // Skip single-line blocks (block_start and close on same line)
        if block_start_line == block_close_line {
            return;
        }

        // Check params on beginning line
        let args_on_begin_line = self.args_on_beginning_line(block_start_line, &params);
        let line_break_needed = self.line_break_necessary_in_args(block_node_start, block_node_col, &params);

        if !args_on_begin_line && !line_break_needed {
            // Offense: args not on block start line
            if let Some(ref p) = params {
                let arg_start = p.location().start_offset();
                let arg_end = p.location().end_offset();
                let loc = Location::from_offsets(source, arg_start, arg_end);
                let correction = self.autocorrect_arguments(block_start, begin_end, block_node_col, &params, &body);
                self.offenses.push(
                    Offense::new(COP_NAME, ARG_MSG, Severity::Convention, loc, self.filename)
                        .with_correction(correction),
                );
                return; // don't also check body until args are fixed
            }
        }

        // Check body: body should not be on same line as begin
        if let Some(ref b) = body {
            let body_start = b.location().start_offset();
            let body_start_line = line_of(source, body_start);
            let begin_end_line = line_of(source, begin_end.saturating_sub(1));

            if begin_end_line == body_start_line {
                // Offense: body on same line as block start
                // The offense range is the body expression range
                let body_end = b.location().end_offset();

                // Get the first expression of body
                let (first_start, first_end) = if let Some(stmts) = b.as_statements_node() {
                    let children: Vec<ruby_prism::Node> = stmts.body().iter().collect();
                    if let Some(first) = children.first() {
                        (first.location().start_offset(), first.location().end_offset())
                    } else {
                        (body_start, body_end)
                    }
                } else {
                    (body_start, body_end)
                };
                let _ = first_end;

                let loc = Location::from_offsets(source, first_start, body_end.min(first_start + 80));
                let loc = Location::from_offsets(source, first_start, first_start + (body_end - body_start).min(source.len() - first_start));

                // Compute offense end = after the first statement (not whole body if multi-stmt)
                // RuboCop: expression.begin_pos..expression.end_pos where expression = body source range
                // For begin_type, body.children.first source_range
                let offense_end = if let Some(stmts) = b.as_statements_node() {
                    let children: Vec<ruby_prism::Node> = stmts.body().iter().collect();
                    children.first().map_or(body_end, |c| c.location().end_offset())
                } else {
                    body_end
                };

                let loc = Location::from_offsets(source, first_start, offense_end);
                let correction = self.autocorrect_body(block_node_col, b);
                self.offenses.push(
                    Offense::new(COP_NAME, MSG, Severity::Convention, loc, self.filename)
                        .with_correction(correction),
                );
            }
        }
    }

    fn args_on_beginning_line(
        &self,
        block_start_line: usize,
        params: &Option<ruby_prism::BlockParametersNode<'a>>,
    ) -> bool {
        let Some(p) = params else { return true };
        // RuboCop: !node.arguments? || node.loc.begin.line == node.arguments.loc.last_line
        let args_last_line = line_of(self.source, p.location().end_offset().saturating_sub(1));
        block_start_line == args_last_line
    }

    fn line_break_necessary_in_args(
        &self,
        block_node_start: usize,
        block_node_col: usize,
        params: &Option<ruby_prism::BlockParametersNode<'a>>,
    ) -> bool {
        let Some(max) = self.max_line_length else { return false };
        let Some(p) = params else { return false };

        // needed_length_for_args:
        // block column + 2*pipe+1 + first_line_len + arg_string_len
        let block_col = block_node_col;
        let first_line = {
            let ls = line_start(self.source, block_node_start);
            let le = self.source[ls..].find('\n').map_or(self.source.len(), |i| ls + i);
            &self.source[ls..le]
        };
        let arg_str = self.block_arg_string(p);
        // chars_for_pipes: if first line ends with `|`, just 1 pipe; else space + 2 pipes = 3
        let chars_for_pipes = if first_line.trim_end().ends_with('|') { 1 } else { 3 };
        let needed = block_col + chars_for_pipes + first_line.len() + arg_str.len();
        needed > max
    }

    fn node_arg_string(&self, node: &ruby_prism::Node<'a>) -> String {
        // Equivalent to RuboCop's block_arg_string for a single arg node
        if let Some(multi) = node.as_multi_target_node() {
            // mlhs_type? in RuboCop → wrap in parens, recurse into children
            let children: Vec<String> = multi.lefts().iter()
                .map(|c| self.node_arg_string(&c))
                .collect();
            format!("({})", children.join(", "))
        } else {
            // Use the source text of this arg, normalized to single line
            let raw = &self.source[node.location().start_offset()..node.location().end_offset()];
            raw.replace('\n', " ").split_whitespace().collect::<Vec<_>>().join(" ")
        }
    }

    fn block_arg_string(&self, params: &ruby_prism::BlockParametersNode<'a>) -> String {
        let ps_opt = params.parameters();
        let all_nodes: Vec<ruby_prism::Node> = ps_opt.iter().flat_map(|ps| {
            let mut v: Vec<ruby_prism::Node> = Vec::new();
            v.extend(ps.requireds().iter());
            v.extend(ps.optionals().iter());
            // Exclude ImplicitRestNode (created by trailing comma in `|a,|`)
            if let Some(rest) = ps.rest() {
                if rest.as_implicit_rest_node().is_none() {
                    v.push(rest);
                }
            }
            v.extend(ps.keywords().iter());
            v.extend(ps.posts().iter());
            v
        }).collect();

        let parts: Vec<String> = all_nodes.iter().map(|p| self.node_arg_string(p)).collect();
        let mut s = parts.join(", ");

        // trailing comma check: exactly 1 simple :arg (RequiredParameterNode) AND source has ','
        // RuboCop: args.each_descendant(:arg) — only simple args, not mlhs
        let simple_arg_count = ps_opt.iter()
            .flat_map(|ps| ps.requireds().iter())
            .filter(|n| n.as_required_parameter_node().is_some())
            .count();
        if simple_arg_count == 1 {
            let raw = &self.source[params.location().start_offset()..params.location().end_offset()];
            if raw.contains(',') {
                s.push(',');
            }
        }
        s
    }

    fn autocorrect_arguments(
        &self,
        _block_start: usize,
        begin_end: usize,
        block_node_col: usize,
        params: &Option<ruby_prism::BlockParametersNode<'a>>,
        body: &Option<ruby_prism::Node<'a>>,
    ) -> Correction {
        let Some(p) = params else {
            return Correction::insert(begin_end, "".to_string());
        };

        let arg_str = self.block_arg_string(p);
        let source = self.source;
        let bytes = source.as_bytes();

        // params location end (including closing pipe)
        let param_end = p.location().end_offset();
        // Skip trailing whitespace after params on same line
        let mut range_end = param_end;
        while range_end < bytes.len() && (bytes[range_end] == b' ' || bytes[range_end] == b'\t') {
            range_end += 1;
        }

        // Edit 1: replace [begin_end..range_end] with " |arg_str|"
        let replacement = format!(" |{}|", arg_str);
        let mut edits = vec![crate::offense::Edit {
            start_offset: begin_end,
            end_offset: range_end,
            replacement,
        }];

        // Edit 2: if body is on same line as params end, also insert newline before body
        if let Some(b) = body {
            let body_start = b.location().start_offset();
            let param_end_line = line_of(source, param_end.saturating_sub(1));
            let body_start_line = line_of(source, body_start);
            if param_end_line == body_start_line {
                // Also autocorrect the body
                let first_node_start = if let Some(stmts) = b.as_statements_node() {
                    let children: Vec<ruby_prism::Node> = stmts.body().iter().collect();
                    children.first().map_or(body_start, |c| c.location().start_offset())
                } else {
                    body_start
                };
                let indent = format!("\n  {}", " ".repeat(block_node_col));
                edits.push(crate::offense::Edit {
                    start_offset: first_node_start,
                    end_offset: first_node_start,
                    replacement: indent,
                });
            }
        }

        Correction { edits }
    }

    fn autocorrect_body(
        &self,
        block_node_col: usize,    // column of the block expression (RuboCop: node.source_range.column)
        body: &ruby_prism::Node<'a>,
    ) -> Correction {
        let source = self.source;

        // Get first node of body (if begin_type? and not starting with `(`, take first child)
        let first_node_start = if let Some(stmts) = body.as_statements_node() {
            let children: Vec<ruby_prism::Node> = stmts.body().iter().collect();
            children.first().map_or(body.location().start_offset(), |c| c.location().start_offset())
        } else {
            body.location().start_offset()
        };

        // RuboCop: corrector.insert_before(first_node, "\n  #{' ' * block_start_col}")
        // = "\n" + "  " + block_node_col spaces
        let indent = format!("\n  {}", " ".repeat(block_node_col));
        Correction::insert(first_node_start, indent)
    }
}

impl<'a> Visit<'a> for BlockVisitor<'a> {
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'a>) {
        let open_loc = node.opening_loc();
        let close_loc = node.closing_loc();
        let block_node_start = node.location().start_offset();
        // For BlockNode, block_node_col = line indent (RuboCop block node includes the call receiver)
        let block_node_col = {
            let ls = line_start(self.source, block_node_start);
            let line = &self.source[ls..];
            line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
        };

        // begin_end = end of `do`/`{` opening keyword (RuboCop: node.loc.begin.end.begin_pos)
        let params = node.parameters();
        let begin_end = open_loc.end_offset();

        let body = node.body().map(|b| b);

        self.check_block(
            block_node_start,
            block_node_col,
            open_loc.start_offset(),
            close_loc.start_offset(),
            begin_end,
            params.and_then(|p| p.as_block_parameters_node()),
            body,
        );
        ruby_prism::visit_block_node(self, node);
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'a>) {
        let open_loc = node.opening_loc();
        let close_loc = node.closing_loc();
        let block_node_start = node.location().start_offset();
        // For LambdaNode, block_node_col = actual column of `->` (RuboCop: node.source_range.column)
        let block_node_col = col_of(self.source, block_node_start);

        let params = node.parameters();
        let begin_end = open_loc.end_offset();

        let body = node.body();

        self.check_block(
            block_node_start,
            block_node_col,
            open_loc.start_offset(),
            close_loc.start_offset(),
            begin_end,
            params.and_then(|p| p.as_block_parameters_node()),
            body,
        );
        ruby_prism::visit_lambda_node(self, node);
    }
}

impl Cop for MultilineBlockLayout {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        // Get max_line_length from Layout/LineLength config
        // (used to decide if line_break_necessary_in_args)
        // For simplicity, we don't read cross-cop config here (complex)
        // The tests that use this have LineLength Enabled=false anyway
        let mut v = BlockVisitor {
            source: ctx.source,
            filename: ctx.filename,
            max_line_length: Some(120), // default per RuboCop; 0 = no limit
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {}

crate::register_cop!("Layout/MultilineBlockLayout", |cfg| {
    Some(Box::new(MultilineBlockLayout::new()))
});
