//! Style/RedundantCurrentDirectoryInPath
//!
//! Flags `require_relative` paths starting with `./` which is redundant.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{CallNode, Node};

const MSG: &str = "Remove the redundant current directory path.";

#[derive(Default)]
pub struct RedundantCurrentDirectoryInPath;

impl RedundantCurrentDirectoryInPath {
    pub fn new() -> Self {
        Self
    }
}

/// Return the length of the leading `\./+` match (bytes) in `s`, or 0.
fn single_prefix_len(s: &[u8]) -> usize {
    if s.first() != Some(&b'.') { return 0; }
    if s.get(1) != Some(&b'/') { return 0; }
    let mut i = 2;
    while s.get(i) == Some(&b'/') { i += 1; }
    i
}

/// Length of the maximally repeated `(\./+)+` prefix.
fn total_prefix_len(s: &[u8]) -> usize {
    let mut total = 0;
    loop {
        let n = single_prefix_len(&s[total..]);
        if n == 0 { break; }
        total += n;
    }
    total
}

impl Cop for RedundantCurrentDirectoryInPath {
    fn name(&self) -> &'static str {
        "Style/RedundantCurrentDirectoryInPath"
    }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node.receiver().is_some() {
            return vec![];
        }
        let method = node_name!(node);
        if method != "require_relative" {
            return vec![];
        }
        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let first = match args.arguments().iter().next() {
            Some(n) => n,
            None => return vec![],
        };

        // Extract the inner content range of the string literal (excluding quotes).
        // Supports StringNode (plain `'...'`, `"..."`, `%q(...)`, `%Q{...}`) and
        // InterpolatedStringNode (e.g., `"./path/#{x}"`).
        let (content_start, content_end, full_node_start, full_node_end) = match &first {
            Node::StringNode { .. } => {
                let s = first.as_string_node().unwrap();
                let open = s.opening_loc().map(|l| l.end_offset());
                let close = s.closing_loc().map(|l| l.start_offset());
                let node_start = s.location().start_offset();
                let node_end = s.location().end_offset();
                let cs = open.unwrap_or(s.content_loc().start_offset());
                let ce = close.unwrap_or(s.content_loc().end_offset());
                (cs, ce, node_start, node_end)
            }
            Node::InterpolatedStringNode { .. } => {
                let s = first.as_interpolated_string_node().unwrap();
                let open = s.opening_loc().map(|l| l.end_offset());
                let close = s.closing_loc().map(|l| l.start_offset());
                let node_start = s.location().start_offset();
                let node_end = s.location().end_offset();
                // Find the start of content: open end, or node_start + 1 as fallback.
                let cs = open.unwrap_or(node_start + 1);
                let ce = close.unwrap_or(node_end - 1);
                (cs, ce, node_start, node_end)
            }
            _ => return vec![],
        };

        let bytes = ctx.source.as_bytes();
        let content = &bytes[content_start..content_end];

        // Match first `\./+` occurrence — use RuboCop's `source.index(CURRENT_DIRECTORY_PREFIX)`
        // which searches within the argument's full source (including quotes).
        // But the offense column must match RuboCop's begin_pos which equals
        // `arg.source_range.begin_pos + source.index(./+)`. With a StringNode, the `'`/`"`
        // is at the node_start, and `./` starts at content_start (= node_start + 1).
        // So begin_pos = content_start. Length comes from `str_content.match(/\A\.\/+/)`.
        // That equals `single_prefix_len(content)`.
        let first_len = single_prefix_len(content);
        if first_len == 0 {
            // Maybe `./` appears elsewhere but not at start of content — RuboCop's
            // `source.index(./+)` still finds it, but `redundant_path_length` over
            // `str_content` is anchored, so returns nil → no offense.
            return vec![];
        }

        let begin = content_start;
        let end_offense = begin + first_len;

        // Autocorrect: remove the *entire* redundant leading `(\./+)+` prefix so a single
        // pass reaches the final form RuboCop gets via iterative correction.
        let total_len = total_prefix_len(content);
        let correction = Correction::delete(begin, begin + total_len);

        // Suppress offense when full node spans beyond what we handled (unused),
        // silences dead_code warnings for the locals we captured.
        let _ = (full_node_start, full_node_end);

        vec![ctx
            .offense_with_range(self.name(), MSG, self.severity(), begin, end_offense)
            .with_correction(correction)]
    }
}

crate::register_cop!("Style/RedundantCurrentDirectoryInPath", |_cfg| Some(Box::new(RedundantCurrentDirectoryInPath::new())));
