//! Style/CommentedKeyword - no comments on same line as begin/class/def/end/module.
//!
//! Ported from:
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/commented_keyword.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

#[derive(Default)]
pub struct CommentedKeyword;

impl CommentedKeyword {
    pub fn new() -> Self {
        Self
    }
}

const KEYWORDS: &[&str] = &["begin", "class", "def", "end", "module"];

impl Cop for CommentedKeyword {
    fn name(&self) -> &'static str {
        "Style/CommentedKeyword"
    }
    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut offenses = Vec::new();

        for c in result.comments() {
            let loc = c.location();
            let cstart = loc.start_offset();
            let cend = loc.end_offset();
            let line_start = ctx.line_start(cstart);
            let line_end = ctx.source[cstart..]
                .find('\n')
                .map(|i| cstart + i)
                .unwrap_or(ctx.source.len());
            let line = &ctx.source[line_start..line_end];
            let comment_text = &ctx.source[cstart..cend];

            // Find which keyword matches (first word on line).
            let trimmed = line.trim_start();
            let matched_kw = KEYWORDS
                .iter()
                .find(|kw| trimmed.starts_with(*kw) && {
                    let after = &trimmed[kw.len()..];
                    after.chars().next().map_or(false, |ch| ch.is_whitespace())
                });
            let Some(kw) = matched_kw else { continue };
            let kw = *kw;

            // Find the "#" — it's at cstart. Must be after the keyword on this line.
            // The REGEXP in RuboCop: /(?<keyword>\S+).*#/ — the first non-ws word on
            // the line must not BE the comment (i.e., keyword is real code).
            // Already ensured above: trimmed starts with keyword.

            if is_allowed_comment(comment_text) {
                continue;
            }
            if rbs_inline_annotation(line, comment_text) {
                continue;
            }
            if steep_annotation(comment_text) {
                continue;
            }

            // Message uses first non-whitespace token BEFORE the comment as "keyword"
            // per RuboCop REGEXP = /(?<keyword>\S+).*#/ — that's first word of line.
            offenses.push(ctx.offense_with_range(
                "Style/CommentedKeyword",
                &format!(
                    "Do not place comments on the same line as the `{}` keyword.",
                    kw
                ),
                Severity::Convention,
                cstart,
                cend,
            ));
        }

        offenses
    }
}

fn is_allowed_comment(text: &str) -> bool {
    // :nodoc: / :yields:
    // Matches RuboCop: /#\s*:nodoc:/ etc.
    let after_hash = text.trim_start_matches('#').trim_start();
    if after_hash.starts_with(":nodoc:") || after_hash.starts_with(":yields:") {
        return true;
    }
    // rubocop:disable / rubocop:todo / rubocop :disable etc.
    // DirectiveComment regex accepts whitespace flexibility.
    is_rubocop_directive(text)
}

fn is_rubocop_directive(text: &str) -> bool {
    // Strip leading '#' and whitespace.
    let t = text.trim_start_matches('#').trim_start();
    // Must start with "rubocop"
    if !t.starts_with("rubocop") {
        return false;
    }
    let rest = &t["rubocop".len()..];
    // Skip optional whitespace, then ':', then optional ws, then disable|enable|todo
    let rest = rest.trim_start();
    let Some(rest) = rest.strip_prefix(':') else {
        return false;
    };
    let rest = rest.trim_start();
    rest.starts_with("disable")
        || rest.starts_with("enable")
        || rest.starts_with("todo")
}

fn rbs_inline_annotation(line: &str, comment: &str) -> bool {
    // SUBCLASS_DEFINITION: /\A\s*class\s+(\w|::)+\s*<\s*(\w|::)+/ → #[...]
    if is_subclass_definition(line) {
        // comment.text.start_with?(/#\[.+\]/)
        // Strip '#', must start with '[', contain chars, end with ']'
        if let Some(rest) = comment.strip_prefix('#') {
            if rest.starts_with('[') && rest.ends_with(']') && rest.len() > 2 {
                return true;
            }
        }
        return false;
    }
    // METHOD_OR_END_DEFINITIONS: /\A\s*(def\s|end)/ → #: ...
    let trimmed = line.trim_start();
    if trimmed.starts_with("def ")
        || trimmed.starts_with("def\t")
        || trimmed == "end"
        || trimmed.starts_with("end ")
        || trimmed.starts_with("end\t")
        || trimmed.starts_with("end#")
    {
        if comment.starts_with("#:") {
            return true;
        }
    }
    false
}

fn is_subclass_definition(line: &str) -> bool {
    // /\A\s*class\s+(\w|::)+\s*<\s*(\w|::)+/
    let t = line.trim_start();
    let Some(rest) = t.strip_prefix("class") else { return false };
    let rest = match rest.chars().next() {
        Some(c) if c.is_whitespace() => rest.trim_start(),
        _ => return false,
    };
    // consume one or more (\w | ::)
    let (consumed, rest) = consume_const_path(rest);
    if consumed == 0 {
        return false;
    }
    let rest = rest.trim_start();
    let Some(rest) = rest.strip_prefix('<') else { return false };
    let rest = rest.trim_start();
    let (c2, _) = consume_const_path(rest);
    c2 > 0
}

fn consume_const_path(s: &str) -> (usize, &str) {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_alphanumeric() || b == b'_' {
            i += 1;
        } else if i + 1 < bytes.len() && bytes[i] == b':' && bytes[i + 1] == b':' {
            i += 2;
        } else {
            break;
        }
    }
    (i, &s[i..])
}

fn steep_annotation(comment: &str) -> bool {
    // /#\ssteep:ignore(\s|\z)/
    let Some(rest) = comment.strip_prefix("# steep:ignore") else {
        return false;
    };
    rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace())
}

crate::register_cop!("Style/CommentedKeyword", |_cfg| {
    Some(Box::new(CommentedKeyword::new()))
});
