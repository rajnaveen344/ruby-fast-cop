//! Layout/SpaceInsideBlockBraces
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/space_inside_block_braces.rb
//!
//! Checks that block braces have or don't have surrounding space inside them
//! depending on configuration. For blocks taking parameters, also checks that
//! the left brace has or doesn't have trailing space depending on configuration.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

const COP_NAME: &str = "Layout/SpaceInsideBlockBraces";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceInsideBlockBracesStyle {
    Space,
    NoSpace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockEmptyBracesStyle {
    Space,
    NoSpace,
}

pub struct SpaceInsideBlockBraces {
    style: SpaceInsideBlockBracesStyle,
    empty_style: BlockEmptyBracesStyle,
    space_before_block_parameters: bool,
}

impl Default for SpaceInsideBlockBraces {
    fn default() -> Self {
        Self {
            style: SpaceInsideBlockBracesStyle::Space,
            empty_style: BlockEmptyBracesStyle::NoSpace,
            space_before_block_parameters: true,
        }
    }
}

impl SpaceInsideBlockBraces {
    pub fn new(
        style: SpaceInsideBlockBracesStyle,
        empty_style: BlockEmptyBracesStyle,
        space_before_block_parameters: bool,
    ) -> Self {
        Self {
            style,
            empty_style,
            space_before_block_parameters,
        }
    }
}

impl Cop for SpaceInsideBlockBraces {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = Visitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a SpaceInsideBlockBraces,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn check_block(
        &mut self,
        body: Option<ruby_prism::Node<'a>>,
        params_first_byte: Option<usize>,
        is_pipe_delim: bool,
        left_brace: usize,
        right_brace: usize,
    ) {
        let source = self.ctx.source;
        let bytes = source.as_bytes();

        // Only handle `{...}` blocks (not do/end).
        if bytes.get(left_brace) != Some(&b'{') || bytes.get(right_brace) != Some(&b'}') {
            return;
        }

        let inner_start = left_brace + 1;
        let inner_end = right_brace;

        // Multi-line empty braces — skip (matches RuboCop's note about issue 7363).
        if body.is_none() && source[inner_start..inner_end].contains('\n') {
            return;
        }

        if inner_start == inner_end {
            // Adjacent braces: `{}` — pass full `{}` range.
            self.check_empty_braces(left_brace, right_brace + 1, true);
            return;
        }

        let inner = &source[inner_start..inner_end];
        let has_non_ws = inner.bytes().any(|b| !b.is_ascii_whitespace());

        if !has_non_ws {
            // Whitespace only: `{ }` / `{   }` — range is the inner whitespace.
            self.check_empty_braces(inner_start, inner_end, false);
            return;
        }

        // Has contents: check left+right brace spacing
        self.check_left_brace(inner, left_brace, params_first_byte, is_pipe_delim);
        self.check_right_brace(left_brace, right_brace);
    }

    fn check_empty_braces(&mut self, begin: usize, end: usize, adjacent: bool) {
        if adjacent {
            // `{}`: only emit if style is `space`. Range covers the full `{}`.
            if self.cop.empty_style == BlockEmptyBracesStyle::Space {
                self.push_offense(begin, end, "Space missing inside empty braces.");
            }
        } else {
            // `{ }` / `{  }`: only emit if style is `no_space`. Range is inner whitespace.
            if self.cop.empty_style == BlockEmptyBracesStyle::NoSpace {
                self.push_offense(begin, end, "Space inside empty braces detected.");
            }
        }
    }

    fn check_left_brace(
        &mut self,
        inner: &str,
        left_brace: usize,
        params_first_byte: Option<usize>,
        is_pipe_delim: bool,
    ) {
        // Does inner start with non-space? If so, no leading space inside `{`
        let starts_with_non_space = inner.as_bytes().first().map_or(false, |&b| !b.is_ascii_whitespace());
        if starts_with_non_space {
            // No space inside left brace
            self.no_space_inside_left_brace(left_brace, params_first_byte, is_pipe_delim);
        } else {
            // Space inside left brace
            self.space_inside_left_brace(left_brace, params_first_byte, is_pipe_delim);
        }
    }

    fn no_space_inside_left_brace(
        &mut self,
        left_brace: usize,
        params_first_byte: Option<usize>,
        is_pipe_delim: bool,
    ) {
        if is_pipe_delim {
            // `{|x|...` — pipe right after `{`
            if let Some(pipe_pos) = params_first_byte {
                if left_brace + 1 == pipe_pos && self.cop.space_before_block_parameters {
                    // Always emits regardless of EnforcedStyle (overrides per docs)
                    self.push_offense(left_brace, pipe_pos + 1, "Space between { and | missing.");
                }
            }
        } else {
            // `{puts...` — non-space immediately after `{`
            self.no_space(left_brace + 1, left_brace + 2, "Space missing inside {.");
        }
    }

    fn space_inside_left_brace(
        &mut self,
        left_brace: usize,
        params_first_byte: Option<usize>,
        is_pipe_delim: bool,
    ) {
        if is_pipe_delim {
            if let Some(pipe_pos) = params_first_byte {
                // We have spaces between `{` and `|`
                if !self.cop.space_before_block_parameters {
                    self.push_offense(left_brace + 1, pipe_pos, "Space between { and | detected.");
                }
            }
        } else {
            // Spaces between `{` and the first content byte
            let source = self.ctx.source;
            let bytes = source.as_bytes();
            let mut i = left_brace + 1;
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
            // Range covers the spaces (one position right of `{` to where content starts)
            self.space(left_brace + 2, i + 1, "Space inside { detected.");
        }
    }

    fn check_right_brace(&mut self, left_brace: usize, right_brace: usize) {
        let source = self.ctx.source;
        let bytes = source.as_bytes();
        let single_line = !source[left_brace..right_brace].contains('\n');
        let inner = &source[left_brace + 1..right_brace];

        if single_line {
            // Translation of RuboCop's `check_right_brace` single_line branch:
            // `single_line && /\S$/.match?(inner)` → no_space offense at `}`.
            if inner.bytes().last().map_or(false, |b| !b.is_ascii_whitespace()) {
                self.no_space(right_brace, right_brace + 1, "Space missing inside }.");
                return;
            }
        }

        // Multi-line OR single-line with trailing whitespace: check alignment.
        // `column` = RuboCop's `node.source_range.column`, i.e. the column where
        // the enclosing expression begins. Approximate via the first non-whitespace
        // column on the line containing the left brace.
        let left_line_start = source[..left_brace]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        let column = source[left_line_start..left_brace]
            .bytes()
            .take_while(|&b| b == b' ' || b == b'\t')
            .count();

        // Last line of inner = bytes from last newline to right_brace
        let last_line_start = source[..right_brace]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(left_brace + 1);
        let last_line = &source[last_line_start..right_brace];
        let last_line_spaces = last_line
            .bytes()
            .take_while(|&b| b == b' ' || b == b'\t')
            .count();
        let right_col = self.ctx.col_of(right_brace);
        let is_multiline = !single_line;

        // `aligned_braces?`: column == right_brace.column || column == inner_last_space_count(inner)
        if is_multiline && (column == right_col || column == last_line_spaces) {
            return;
        }

        // `space_inside_right_brace`: compute the space range to flag.
        // Ruby reference (see RuboCop v1.85.0 layout/space_inside_block_braces.rb):
        //   brace_with_space = range_with_surrounding_space(right_brace, side: :left)
        //   begin_pos = brace_with_space.begin_pos
        //   end_pos   = brace_with_space.end_pos - 1   # == right_brace.begin_pos
        //   begin_pos = end_pos - (right_brace.column - column)   if crosses line
        //   if inner.end_with?(']')
        //     end_pos -= 1
        //     begin_pos = end_pos - (inner_last_space_count(inner) - column)
        //   end
        //   space(begin_pos, end_pos, ...)
        let mut brace_with_space_begin = right_brace;
        // range_with_surrounding_space walks back over `[ \t]*` then a single `\n`.
        while brace_with_space_begin > 0 {
            let c = bytes[brace_with_space_begin - 1];
            if c == b' ' || c == b'\t' {
                brace_with_space_begin -= 1;
            } else {
                break;
            }
        }
        if brace_with_space_begin > 0 && bytes[brace_with_space_begin - 1] == b'\n' {
            brace_with_space_begin -= 1;
        }

        let brace_with_space_end = right_brace + 1; // `}.end_pos`
        let crosses_line = source[brace_with_space_begin..brace_with_space_end].contains('\n');

        let mut begin_pos = brace_with_space_begin;
        let mut end_pos = brace_with_space_end - 1; // = right_brace (`}.begin_pos`)

        if crosses_line {
            // Ruby: `begin_pos = end_pos - (right_brace.column - column)`
            begin_pos = end_pos.saturating_sub(right_col.saturating_sub(column));
        }

        if inner.ends_with(']') {
            end_pos = end_pos.saturating_sub(1);
            let delta = last_line_spaces.saturating_sub(column);
            begin_pos = end_pos.saturating_sub(delta);
        }

        if begin_pos < end_pos {
            self.space_emit(begin_pos, end_pos, "Space inside } detected.");
        }
    }

    fn space_emit(&mut self, begin: usize, end: usize, msg: &'static str) {
        // Same guard as `space()` but without the `begin-1, end-1` RuboCop
        // off-by-one adjustment that applied in the old single-line path.
        if self.cop.style == SpaceInsideBlockBracesStyle::NoSpace {
            self.push_offense(begin, end, msg);
        }
    }

    fn no_space(&mut self, begin: usize, end: usize, msg: &'static str) {
        if self.cop.style == SpaceInsideBlockBracesStyle::Space {
            self.push_offense(begin, end, msg);
        }
    }

    fn space(&mut self, begin: usize, end: usize, msg: &'static str) {
        if self.cop.style == SpaceInsideBlockBracesStyle::NoSpace {
            self.push_offense(begin.saturating_sub(1), end.saturating_sub(1), msg);
        }
    }

    fn push_offense(&mut self, begin: usize, end: usize, msg: &'static str) {
        if begin > end {
            return;
        }
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME,
            msg,
            Severity::Convention,
            begin,
            end,
        ));
    }
}

impl<'a> Visit<'a> for Visitor<'a> {
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'a>) {
        self.check_block_node(node);
        ruby_prism::visit_block_node(self, node);
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'a>) {
        // -> { ... } also gets checked
        let open = node.opening_loc();
        let close = node.closing_loc();
        let body = node.body();
        // Only handle brace-form lambdas
        if self.ctx.source.as_bytes().get(open.start_offset()) == Some(&b'{') {
            self.check_block(body, None, false, open.start_offset(), close.start_offset());
        }
        ruby_prism::visit_lambda_node(self, node);
    }
}

impl<'a> Visitor<'a> {
    fn check_block_node(&mut self, node: &ruby_prism::BlockNode<'a>) {
        let open = node.opening_loc();
        let close = node.closing_loc();
        let bytes = self.ctx.source.as_bytes();
        if bytes.get(open.start_offset()) != Some(&b'{') {
            return; // do/end form
        }

        // Detect block parameters and pipe delimiter by scanning the source
        // between `{` and the first non-whitespace byte. ParametersNode location
        // can sometimes precede `{` (e.g. numblock/itblock), so source-scan instead.
        let after_brace = open.end_offset();
        let close_off = close.start_offset();
        let (params_first_byte, is_pipe_delim) = if after_brace < close_off {
            let between = &self.ctx.source[after_brace..close_off];
            let trimmed = between.trim_start();
            if trimmed.starts_with('|') {
                let pipe_pos = after_brace + (between.len() - trimmed.len());
                (Some(pipe_pos), true)
            } else if node.parameters().is_some() {
                let first = after_brace + (between.len() - trimmed.len());
                (Some(first), false)
            } else {
                (None, false)
            }
        } else {
            (None, false)
        };

        self.check_block(
            node.body(),
            params_first_byte,
            is_pipe_delim,
            open.start_offset(),
            close.start_offset(),
        );
    }
}

crate::register_cop!("Layout/SpaceInsideBlockBraces", |cfg| {
    let cop_config = cfg.get_cop_config("Layout/SpaceInsideBlockBraces");
    let style = cop_config
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "no_space" => SpaceInsideBlockBracesStyle::NoSpace,
            _ => SpaceInsideBlockBracesStyle::Space,
        })
        .unwrap_or(SpaceInsideBlockBracesStyle::Space);
    let raw_empty = cop_config
        .and_then(|c| c.raw.get("EnforcedStyleForEmptyBraces"))
        .and_then(|v| v.as_str());
    let empty_style = match raw_empty {
        Some("space") => BlockEmptyBracesStyle::Space,
        Some("no_space") | None => BlockEmptyBracesStyle::NoSpace,
        Some(_) => return None,
    };
    let space_before_params = cop_config
        .and_then(|c| c.raw.get("SpaceBeforeBlockParameters"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    Some(Box::new(SpaceInsideBlockBraces::new(style, empty_style, space_before_params)))
});
