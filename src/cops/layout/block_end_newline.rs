//! Layout/BlockEndNewline
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/layout/block_end_newline.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use ruby_prism::Visit;

const COP_NAME: &str = "Layout/BlockEndNewline";

#[derive(Default)]
pub struct BlockEndNewline;

impl BlockEndNewline {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for BlockEndNewline {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = BlockEndVisitor {
            source: ctx.source,
            filename: ctx.filename,
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct BlockEndVisitor<'a> {
    source: &'a str,
    filename: &'a str,
    offenses: Vec<Offense>,
}

impl<'a> BlockEndVisitor<'a> {
    fn line_of(&self, offset: usize) -> usize {
        1 + self.source[..offset].bytes().filter(|&b| b == b'\n').count()
    }

    fn line_start(&self, offset: usize) -> usize {
        self.source[..offset].rfind('\n').map_or(0, |p| p + 1)
    }

    fn begins_own_line(&self, offset: usize) -> bool {
        let ls = self.line_start(offset);
        let prefix = &self.source[ls..offset];
        prefix.chars().all(|c| c == ' ' || c == '\t')
    }

    fn check_end(
        &mut self,
        block_start: usize,
        body_end: Option<usize>,
        end_loc_start: usize,
        end_loc_end: usize,
    ) {
        // Single line block: skip
        if self.line_of(block_start) == self.line_of(end_loc_start) {
            return;
        }
        // `end`/`}` already on its own line: ok
        if self.begins_own_line(end_loc_start) {
            return;
        }

        let range_start = match body_end {
            Some(e) => e,
            None => return,
        };
        let range_end = end_loc_end;

        // Skip if offense range starts with `;`
        let range_text = &self.source[range_start..range_end.min(self.source.len())];
        let trimmed = range_text.trim_start_matches(|c: char| c == ' ' || c == '\t' || c == '\n');
        if trimmed.starts_with(';') {
            return;
        }

        // Offense location is the `end`/`}` token itself
        let end_line = self.line_of(end_loc_start);
        let end_ls = self.line_start(end_loc_start);
        let end_col = end_loc_start - end_ls + 1; // 1-based for message
        let message = format!("Expression at {end_line}, {end_col} should be on its own line.");

        // Offense location = the `end` token
        let loc = Location::from_offsets(self.source, end_loc_start, end_loc_end);

        // Correction: depends on heredoc presence
        // Check if the offense range contains a heredoc call:
        // If the line with end_loc has `<<` before end_loc, the heredoc body follows
        let correction = if let Some(heredoc_end) = self.find_last_heredoc_end(end_loc_start, range_start) {
            // Heredoc correction: remove the ` }` portion and insert `\n}` after heredoc body
            let end_token = &self.source[end_loc_start..end_loc_end];
            Correction {
                edits: vec![
                    crate::offense::Edit {
                        start_offset: range_start,
                        end_offset: end_loc_end,
                        replacement: String::new(),
                    },
                    crate::offense::Edit {
                        start_offset: heredoc_end,
                        end_offset: heredoc_end,
                        replacement: format!("{end_token}\n"),
                    },
                ],
            }
        } else {
            // Normal correction: replace offense range with "\n<lstripped>"
            let replacement = format!("\n{trimmed}");
            Correction::replace(range_start, range_end, replacement)
        };

        self.offenses.push(
            Offense::new(COP_NAME, &message, Severity::Convention, loc, self.filename)
                .with_correction(correction),
        );
    }

    /// Find the end offset of the last heredoc body if the line containing `end_loc_start`
    /// has a heredoc call. Returns position after the final heredoc end marker.
    fn find_last_heredoc_end(&self, end_loc_start: usize, range_start: usize) -> Option<usize> {
        // Check if the line up to end_loc has a `<<` marker
        let end_line_start = self.line_start(end_loc_start);
        let end_line = &self.source[end_line_start..end_loc_start];
        if !end_line.contains("<<") {
            return None;
        }

        // Find the last heredoc delimiter marker in the source after end_loc_start
        // The heredoc body is between end_loc_start..
        // We scan for lines that look like heredoc end markers (just an identifier)
        // Actually: we find the overall end of the block's last child which includes heredoc
        // The body_end (range_start) is after the call on the same line as `}`,
        // but the heredoc body follows. We need to find the end of all heredoc bodies.

        // Scan from range_start forward for heredoc end markers
        // RuboCop uses Prism loc info; we'll scan source for the pattern
        // Simpler: find all `<<` heredoc markers on the end_loc line, then find their terminators

        let line_text = &self.source[end_line_start..end_loc_start];
        let mut heredoc_delimiters: Vec<String> = Vec::new();

        let mut scan = line_text;
        while let Some(pos) = scan.find("<<") {
            let rest = &scan[pos + 2..];
            let rest = rest.trim_start_matches(['-', '~']);
            let (inner, is_quoted) = if rest.starts_with('"') || rest.starts_with('\'') || rest.starts_with('`') {
                let q = &rest[..1];
                let inner = rest[1..].split(q).next().unwrap_or("");
                (inner, true)
            } else {
                let inner: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                let s = Box::leak(inner.into_boxed_str());
                (s as &str, false)
            };
            if !inner.is_empty() {
                let delim: String = inner.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                if !delim.is_empty() {
                    heredoc_delimiters.push(delim);
                }
            }
            // advance past this <<
            let advance = pos + 2;
            if advance >= scan.len() { break; }
            scan = &scan[advance..];
        }

        if heredoc_delimiters.is_empty() {
            return None;
        }

        // Scan lines after end_loc_start to find all heredoc end markers in order
        let after = &self.source[end_loc_start..];
        let mut last_end = None;
        let mut remaining_delims = heredoc_delimiters.clone();
        let lines: Vec<&str> = after.lines().collect();
        let mut byte_offset = end_loc_start;

        for line in &lines {
            let line_len = line.len() + 1; // +1 for newline
            let trimmed = line.trim();
            if let Some(pos) = remaining_delims.iter().position(|d| d == trimmed) {
                remaining_delims.remove(pos);
                last_end = Some(byte_offset + line_len);
                if remaining_delims.is_empty() {
                    break;
                }
            }
            byte_offset += line_len;
        }

        last_end
    }
}

impl<'a> Visit<'a> for BlockEndVisitor<'a> {
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'a>) {
        let block_start = node.location().start_offset();
        let end_loc = node.closing_loc();
        // body_end: prefer body end, fallback to params end, fallback to opening keyword end
        let body_end = node.body()
            .map(|b| b.location().end_offset())
            .or_else(|| node.parameters().map(|p| p.location().end_offset()))
            .or_else(|| Some(node.opening_loc().end_offset()));
        self.check_end(block_start, body_end, end_loc.start_offset(), end_loc.end_offset());
        ruby_prism::visit_block_node(self, node);
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'a>) {
        let block_start = node.location().start_offset();
        let end_loc = node.closing_loc();
        let body_end = node.body()
            .map(|b| b.location().end_offset())
            .or_else(|| node.parameters().and_then(|p| p.as_block_parameters_node().map(|bp| bp.location().end_offset())))
            .or_else(|| Some(node.opening_loc().end_offset()));
        self.check_end(block_start, body_end, end_loc.start_offset(), end_loc.end_offset());
        ruby_prism::visit_lambda_node(self, node);
    }
}

crate::register_cop!("Layout/BlockEndNewline", |_cfg| {
    Some(Box::new(BlockEndNewline::new()))
});
