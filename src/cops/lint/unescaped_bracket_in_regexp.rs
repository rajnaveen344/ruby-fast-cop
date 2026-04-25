//! Lint/UnescapedBracketInRegexp - flag unescaped `]` in regexp outside char classes.
//!
//! Mirrors `RuboCop::Cop::Lint::UnescapedBracketInRegexp`. Skips:
//!   - first character of pattern
//!   - `]` inside a character class
//!   - escaped `\]`
//!   - interpolated strings inside `Regexp.new` / `Regexp.compile`
//!
//! Emulates the Ruby warning:
//!   warning: regular expression has ']' without escape

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct UnescapedBracketInRegexp;

impl UnescapedBracketInRegexp {
    pub fn new() -> Self { Self }
}

const MSG: &str = "Regular expression has `]` without escape.";

impl Cop for UnescapedBracketInRegexp {
    fn name(&self) -> &'static str { "Lint/UnescapedBracketInRegexp" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut v = V { ctx, out: vec![] };
        v.visit(&result.node());
        v.out
    }
}

struct V<'a, 'b> { ctx: &'a CheckContext<'b>, out: Vec<Offense> }

impl<'a, 'b> V<'a, 'b> {
    /// Scan regexp content (between delimiters or inside string for Regexp.new).
    /// `content_start`/`content_end` are absolute byte offsets in source.
    /// `string_start_zero`: true if this content represents the *whole* pattern (so
    /// position 0 in the pattern is "first character" and gets skipped).
    fn scan(&mut self, content_start: usize, content_end: usize) {
        let bytes = self.ctx.source.as_bytes();
        let mut i = content_start;
        let mut cc_depth: usize = 0;
        let mut pattern_pos: usize = 0; // index within the pattern itself

        while i < content_end {
            let b = bytes[i];

            // Skip escapes: `\X` consumes 2 bytes (or 1+utf8 for high bytes).
            if b == b'\\' && i + 1 < content_end {
                let next = bytes[i + 1];
                let extra = if next < 0x80 {
                    1
                } else {
                    let s = &self.ctx.source[i + 1..content_end];
                    s.chars().next().map(|c| c.len_utf8()).unwrap_or(1)
                };
                i += 1 + extra;
                pattern_pos += 1 + extra;
                continue;
            }

            if b == b'[' {
                cc_depth += 1;
                i += 1;
                pattern_pos += 1;
                continue;
            }

            if b == b']' {
                if cc_depth > 0 {
                    cc_depth -= 1;
                } else if pattern_pos != 0 {
                    // Unescaped `]` outside char class, not first char.
                    self.out.push(
                        self.ctx.offense_with_range(
                            "Lint/UnescapedBracketInRegexp",
                            MSG,
                            Severity::Warning,
                            i, i + 1,
                        ).with_correction(Correction::replace(i, i + 1, "\\]")),
                    );
                }
                i += 1;
                pattern_pos += 1;
                continue;
            }

            // Step over single byte / utf8 character.
            let step = if b < 0x80 {
                1
            } else {
                let s = &self.ctx.source[i..content_end];
                s.chars().next().map(|c| c.len_utf8()).unwrap_or(1)
            };
            i += step;
            pattern_pos += step;
        }
    }

    fn handle_call(&mut self, node: &ruby_prism::CallNode) {
        let method_name = String::from_utf8_lossy(node.name().as_slice()).into_owned();
        if method_name != "new" && method_name != "compile" { return; }
        // Receiver must be `Regexp` or `::Regexp`.
        let Some(recv) = node.receiver() else { return };
        let is_regexp = match recv {
            Node::ConstantReadNode { .. } => {
                String::from_utf8_lossy(recv.as_constant_read_node().unwrap().name().as_slice()) == "Regexp"
            }
            Node::ConstantPathNode { .. } => {
                let cp = recv.as_constant_path_node().unwrap();
                cp.parent().is_none()
                    && String::from_utf8_lossy(cp.name().unwrap().as_slice()) == "Regexp"
            }
            _ => false,
        };
        if !is_regexp { return; }

        let Some(args) = node.arguments() else { return };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.is_empty() { return; }
        let first = &arg_list[0];

        // Skip if any argument contains interpolation (dstr).
        if has_dstr(first) { return; }

        // Need a plain StringNode for first arg.
        let Some(s) = first.as_string_node() else { return };
        let content_loc = s.content_loc();
        self.scan(content_loc.start_offset(), content_loc.end_offset());
    }
}

fn has_dstr(node: &Node) -> bool {
    if matches!(node, Node::InterpolatedStringNode { .. } | Node::InterpolatedSymbolNode { .. }) {
        return true;
    }
    // Walk descendants looking for InterpolatedStringNode.
    struct Finder { found: bool }
    impl Visit<'_> for Finder {
        fn visit_interpolated_string_node(&mut self, _: &ruby_prism::InterpolatedStringNode) {
            self.found = true;
        }
    }
    let mut f = Finder { found: false };
    f.visit(node);
    f.found
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_regular_expression_node(&mut self, node: &ruby_prism::RegularExpressionNode) {
        let c = node.content_loc();
        self.scan(c.start_offset(), c.end_offset());
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.handle_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Lint/UnescapedBracketInRegexp", |_cfg| Some(Box::new(UnescapedBracketInRegexp::new())));
