//! Style/CommentAnnotation - annotation keywords like TODO/FIXME formatted correctly.
//!
//! Ported from:
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/comment_annotation.rb
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/mixin/annotation_comment.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use regex::Regex;

const DEFAULT_KEYWORDS: &[&str] = &["TODO", "FIXME", "OPTIMIZE", "HACK", "REVIEW"];

pub struct CommentAnnotation {
    keywords: Vec<String>,
    require_colon: bool,
}

impl Default for CommentAnnotation {
    fn default() -> Self {
        Self {
            keywords: DEFAULT_KEYWORDS.iter().map(|s| s.to_string()).collect(),
            require_colon: true,
        }
    }
}

impl CommentAnnotation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(keywords: Vec<String>, require_colon: bool) -> Self {
        Self {
            keywords,
            require_colon,
        }
    }

    fn build_regex(&self) -> Regex {
        let mut sorted: Vec<&str> = self.keywords.iter().map(|s| s.as_str()).collect();
        sorted.sort_by_key(|s| std::cmp::Reverse(s.len()));
        let alt = sorted.iter().map(|k| regex::escape(k)).collect::<Vec<_>>().join("|");
        // ^(# ?)(\b KEYWORDS \b)(\s*:)?(\s+)?(\S+)?  , case-insensitive
        let pat = format!(r"(?i)^(# ?)(\b(?:{})\b)(\s*:)?(\s+)?(\S+)?", alt);
        Regex::new(&pat).unwrap()
    }
}

impl Cop for CommentAnnotation {
    fn name(&self) -> &'static str {
        "Style/CommentAnnotation"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut offenses = Vec::new();

        let comments: Vec<_> = result.comments().collect();
        let lines: Vec<usize> = comments
            .iter()
            .map(|c| ctx.line_of(c.location().start_offset()))
            .collect();

        let re = self.build_regex();

        for (i, c) in comments.iter().enumerate() {
            let loc = c.location();
            let cstart = loc.start_offset();
            let cend = loc.end_offset();
            let comment_text = &ctx.source[cstart..cend];

            if !comment_text.starts_with('#') {
                continue;
            }

            // Inline comment: preceded by code on same line.
            let line_start = ctx.line_start(cstart);
            let line_prefix = &ctx.source[line_start..cstart];
            let is_inline = line_prefix.chars().any(|ch| !ch.is_whitespace());

            // First in a comment block (or inline).
            let is_first_in_block = if is_inline {
                true
            } else {
                i == 0 || lines[i - 1] < lines[i] - 1 || {
                    let prev = &comments[i - 1];
                    let prev_line_start = ctx.line_start(prev.location().start_offset());
                    let prev_prefix = &ctx.source[prev_line_start..prev.location().start_offset()];
                    prev_prefix.chars().any(|ch| !ch.is_whitespace())
                }
            };
            if !is_first_in_block {
                continue;
            }

            let Some(caps) = re.captures(comment_text) else {
                continue;
            };
            let margin = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let keyword = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            let colon = caps.get(3).map(|m| m.as_str());
            let space = caps.get(4).map(|m| m.as_str());
            let note = caps.get(5).map(|m| m.as_str());

            // keyword_appearance?
            if !(colon.is_some() || space.is_some()) {
                continue;
            }
            // just_keyword_of_sentence?: `Optimize if you want.`
            let is_capitalized = keyword.chars().next().map_or(false, |c| c.is_uppercase())
                && keyword.chars().skip(1).all(|c| !c.is_uppercase());
            if is_capitalized && colon.is_none() && space.is_some() && note.is_some() {
                continue;
            }

            // correct?
            let is_upper = keyword.chars().all(|c| !c.is_lowercase());
            let has_colon = colon.is_some();
            let correct = keyword == keyword.to_uppercase()
                && space.is_some()
                && note.is_some()
                && is_upper
                && (has_colon == self.require_colon);

            if correct {
                continue;
            }

            // Range: margin.length..margin.length + (keyword + colon + space).length
            let prefix_len =
                keyword.len() + colon.map(|s| s.len()).unwrap_or(0) + space.map(|s| s.len()).unwrap_or(0);
            let range_start = cstart + margin.len();
            let range_end = range_start + prefix_len;

            let (message, correction) = if note.is_none() {
                let msg = format!(
                    "Annotation comment, with keyword `{}`, is missing a note.",
                    keyword
                );
                (msg, None)
            } else {
                let msg = if self.require_colon {
                    format!(
                        "Annotation keywords like `{}` should be all upper case, followed by a colon, and a space, then a note describing the problem.",
                        keyword
                    )
                } else {
                    format!(
                        "Annotation keywords like `{}` should be all upper case, followed by a space, then a note describing the problem.",
                        keyword
                    )
                };
                let upper_kw = keyword.to_uppercase();
                let replacement = if self.require_colon {
                    format!("{}: ", upper_kw)
                } else {
                    format!("{} ", upper_kw)
                };
                (
                    msg,
                    Some(Correction::replace(range_start, range_end, replacement)),
                )
            };

            let mut offense = ctx.offense_with_range(
                self.name(),
                &message,
                self.severity(),
                range_start,
                range_end,
            );
            if let Some(c) = correction {
                offense = offense.with_correction(c);
            }
            offenses.push(offense);
        }

        offenses
    }
}
