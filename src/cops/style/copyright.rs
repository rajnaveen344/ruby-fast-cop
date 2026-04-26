//! Style/Copyright - Checks that a copyright notice was given in each source file.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/copyright.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};
use regex::Regex;

#[derive(Default)]
pub struct Copyright {
    notice: String,
    autocorrect_notice: String,
}

impl Copyright {
    pub fn new(notice: String, autocorrect_notice: String) -> Self {
        Self { notice, autocorrect_notice }
    }

    /// Build a regex from the notice. RuboCop joins lines (stripped) before
    /// constructing the regex.
    fn notice_regex(&self) -> Option<Regex> {
        let joined: String = self.notice.lines().map(|l| l.trim()).collect::<Vec<_>>().join("");
        Regex::new(&joined).ok()
    }
}

impl Cop for Copyright {
    fn name(&self) -> &'static str { "Style/Copyright" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if self.notice.is_empty() {
            return vec![];
        }
        let regex = match self.notice_regex() {
            Some(r) => r,
            None => return vec![],
        };

        let result = ruby_prism::parse(ctx.source.as_bytes());

        // Collect leading contiguous comment tokens (only those that appear
        // before any non-comment token). For `=begin...=end` block comments,
        // Prism returns a single comment with the whole block text.
        let mut comments: Vec<(usize, usize, String)> = Vec::new();
        for c in result.comments() {
            let loc = c.location();
            let text = ctx.source[loc.start_offset()..loc.end_offset()].to_string();
            comments.push((loc.start_offset(), loc.end_offset(), text));
        }
        comments.sort_by_key(|c| c.0);

        // Build the multiline notice string by accumulating leading-only
        // comments (those that appear before any code).
        let first_code_offset = first_code_offset(ctx.source, &result);

        let mut multiline_notice = String::new();
        let comment_re = Regex::new(r"^# *").unwrap();
        for (start, _end, text) in &comments {
            if first_code_offset.map_or(false, |fc| *start >= fc) {
                break;
            }
            // Strip leading "# " or block-comment markers.
            let stripped = if text.starts_with("=begin") {
                // Block comment: drop the =begin/=end lines, keep middle content.
                let mut lines: Vec<&str> = text.lines().collect();
                if !lines.is_empty() && lines[0].starts_with("=begin") {
                    lines.remove(0);
                }
                if !lines.is_empty() && lines.last().map_or(false, |l| l.starts_with("=end")) {
                    lines.pop();
                }
                lines.join("\n")
            } else {
                comment_re.replace(text, "").to_string()
            };
            multiline_notice.push_str(&stripped);
            // RuboCop's loop breaks early when the regex matches the single
            // comment text — emulating that, we also early-out.
            if regex.is_match(text) {
                return vec![];
            }
        }

        if regex.is_match(&multiline_notice) {
            return vec![];
        }

        // Offense at start-of-file (line 1, col 0..0 → widened to 1).
        let message = format!("Include a copyright notice matching /{}/ before any code.", self.notice);
        let location = Location::from_offsets(ctx.source, 0, 0);
        let mut offense = Offense::new(
            "Style/Copyright",
            message,
            Severity::Convention,
            location,
            ctx.filename,
        );

        // Build correction if AutocorrectNotice is valid.
        if !self.autocorrect_notice.is_empty() {
            let stripped_autocorrect = strip_leading_hashes(&self.autocorrect_notice);
            if regex.is_match(&stripped_autocorrect) {
                // Insert at offset just after shebang/encoding tokens.
                let insert_offset = insertion_offset(ctx.source, &comments);
                let text = format!("{}\n", self.autocorrect_notice);
                offense = offense.with_correction(Correction::insert(insert_offset, text));
            }
        }
        vec![offense]
    }
}

/// Strip leading `# ` from each line.
fn strip_leading_hashes(s: &str) -> String {
    let re = Regex::new(r"(?m)^# *").unwrap();
    re.replace_all(s, "").to_string()
}

/// Find the byte offset of the first non-comment, non-magic token. Used to
/// determine the cutoff for "leading" comments.
fn first_code_offset(_source: &str, result: &ruby_prism::ParseResult) -> Option<usize> {
    let node = result.node();
    let prog = node.as_program_node()?;
    let stmts = prog.statements();
    stmts.body().iter().next().map(|n| n.location().start_offset())
}

/// Compute the offset to insert the notice at, after shebang/encoding.
fn insertion_offset(source: &str, comments: &[(usize, usize, String)]) -> usize {
    let mut idx = 0;
    // Skip shebang token if first comment is `#!...`
    if let Some((s, e, t)) = comments.get(idx) {
        if t.starts_with("#!") {
            idx += 1;
            // Insertion will be after the newline of this line.
            let _ = (s, e);
        }
    }
    // Skip encoding token if next comment is `# encoding: ...` or `coding: ...`
    let enc_re = Regex::new(r"\A#.*coding\s?[:=]\s?(?i:utf)-8").unwrap();
    if let Some((_, _, t)) = comments.get(idx) {
        if enc_re.is_match(t) {
            idx += 1;
        }
    }
    // Insert at the start of `comments[idx]` if present, else at offset 0.
    if let Some((s, _, _)) = comments.get(idx) {
        *s
    } else if idx == 0 {
        0
    } else {
        // No more comments; insert after the last skipped comment's line ending.
        let (_, e, _) = &comments[idx - 1];
        // Find newline after `e`
        let bytes = source.as_bytes();
        let mut i = *e;
        while i < bytes.len() && bytes[i] != b'\n' {
            i += 1;
        }
        if i < bytes.len() {
            i + 1
        } else {
            i
        }
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    notice: Option<String>,
    autocorrect_notice: Option<String>,
}

crate::register_cop!("Style/Copyright", |cfg| {
    let c: Cfg = cfg.typed("Style/Copyright");
    Some(Box::new(Copyright::new(
        c.notice.unwrap_or_default(),
        c.autocorrect_notice.unwrap_or_default(),
    )))
});
