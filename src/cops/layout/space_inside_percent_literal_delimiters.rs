//! Layout/SpaceInsidePercentLiteralDelimiters cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};

const MSG: &str = "Do not use spaces inside percent literal delimiters.";

pub struct SpaceInsidePercentLiteralDelimiters;

impl SpaceInsidePercentLiteralDelimiters {
    pub fn new() -> Self { Self }

    fn find_percent_literals(source: &str) -> Vec<(usize, usize)> {
        let bytes = source.as_bytes();
        let len = bytes.len();
        let mut results = Vec::new();
        let mut i = 0;

        while i < len {
            if bytes[i] == b'\'' || bytes[i] == b'"' {
                let quote = bytes[i];
                i += 1;
                while i < len && bytes[i] != quote {
                    if bytes[i] == b'\\' { i += 1; }
                    i += 1;
                }
                if i < len { i += 1; }
                continue;
            }
            if bytes[i] == b'#' {
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }

            if bytes[i] == b'%' && i + 1 < len {
                let type_char = bytes[i + 1];
                // Check for %w, %W, %i, %I, %x
                if matches!(type_char, b'w' | b'W' | b'i' | b'I' | b'x') {
                    if i + 2 < len {
                        let delim_byte = bytes[i + 2];
                        let (open, close) = match delim_byte {
                            b'(' => (b'(', b')'),
                            b'[' => (b'[', b']'),
                            b'{' => (b'{', b'}'),
                            other if !other.is_ascii_alphanumeric() && other != b' ' => {
                                (other, other)
                            }
                            _ => {
                                i += 1;
                                continue;
                            }
                        };

                        let content_start = i + 3; // right after opening delimiter
                        // Find matching close delimiter, handling nesting for paired delimiters
                        let mut j = content_start;
                        let mut depth = 1u32;
                        let paired = open != close;

                        while j < len && depth > 0 {
                            if bytes[j] == b'\\' {
                                j += 2; // skip escaped char
                                continue;
                            }
                            if paired {
                                if bytes[j] == open {
                                    depth += 1;
                                } else if bytes[j] == close {
                                    depth -= 1;
                                }
                            } else {
                                // Non-paired: same char opens and closes
                                if bytes[j] == close {
                                    depth -= 1;
                                }
                            }
                            if depth > 0 {
                                j += 1;
                            }
                        }

                        if depth == 0 {
                            let content_end = j; // position of closing delimiter
                            results.push((content_start, content_end));
                            i = j + 1;
                            continue;
                        }
                    }
                }
            }

            i += 1;
        }

        results
    }

    fn check_literal(
        &self,
        source: &str,
        content_start: usize,
        content_end: usize,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut offenses = Vec::new();

        if content_start >= content_end { return offenses; }

        let content = &source[content_start..content_end];

        if content.chars().all(|c| c.is_ascii_whitespace()) {
            if !content.is_empty() {
                let start_loc = Location::from_offsets(source, content_start, content_start);
                let location = if content.contains('\n') {
                    Location::new(start_loc.line, start_loc.column, start_loc.line, start_loc.column + 1)
                } else {
                    Location::from_offsets(source, content_start, content_end)
                };
                offenses.push(Offense::new(
                    "Layout/SpaceInsidePercentLiteralDelimiters", MSG, Severity::Convention, location, ctx.filename,
                ).with_correction(Correction::delete(content_start, content_end)));
            }
            return offenses;
        }

        if content.contains('\n') { return offenses; }

        let leading_space_count = content.bytes().take_while(|&b| b == b' ').count();
        if leading_space_count > 0 {
            let offense = ctx.offense_with_range(
                    "Layout/SpaceInsidePercentLiteralDelimiters",
                    MSG,
                    Severity::Convention,
                    content_start,
                    content_start + leading_space_count,
                )
                .with_correction(Correction::delete(
                    content_start,
                    content_start + leading_space_count,
                ));
            offenses.push(offense);
        }

        let content_bytes = content.as_bytes();
        let content_len = content_bytes.len();
        let mut trailing_space_count = 0usize;
        let mut k = content_len;
        while k > 0 && content_bytes[k - 1] == b' ' {
            k -= 1;
            trailing_space_count += 1;
        }

        if trailing_space_count > 0 {
            // Check if preceded by backslash - if so, don't count the last space
            // END_REGEX: /(?<!\\)( +)\z/
            // The negative lookbehind means: spaces at end that are NOT immediately preceded by \
            let trail_start = content_len - trailing_space_count;
            if trail_start > 0 && content_bytes[trail_start - 1] == b'\\' {
                // The space right after \ is part of the escaped space, skip it
                // But spaces AFTER that escaped space are still flagged
                if trailing_space_count > 1 {
                    let flag_start = content_start + trail_start + 1; // skip the `\ ` space
                    let flag_end = content_end;
                    let offense = ctx
                        .offense_with_range(
                            "Layout/SpaceInsidePercentLiteralDelimiters",
                            MSG,
                            Severity::Convention,
                            flag_start,
                            flag_end,
                        )
                        .with_correction(Correction::delete(flag_start, flag_end));
                    offenses.push(offense);
                }
                // If only 1 trailing space after \, it's part of the escape - no offense
            } else {
                let offense = ctx
                    .offense_with_range(
                        "Layout/SpaceInsidePercentLiteralDelimiters",
                        MSG,
                        Severity::Convention,
                        content_start + trail_start,
                        content_end,
                    )
                    .with_correction(Correction::delete(content_start + trail_start, content_end));
                offenses.push(offense);
            }
        }

        offenses
    }
}

impl Cop for SpaceInsidePercentLiteralDelimiters {
    fn name(&self) -> &'static str {
        "Layout/SpaceInsidePercentLiteralDelimiters"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        _node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut offenses = Vec::new();

        let literals = Self::find_percent_literals(ctx.source);
        for (content_start, content_end) in literals {
            offenses.extend(self.check_literal(ctx.source, content_start, content_end, ctx));
        }

        offenses
    }
}

crate::register_cop!("Layout/SpaceInsidePercentLiteralDelimiters", |_cfg| {
    Some(Box::new(SpaceInsidePercentLiteralDelimiters::new()))
});
