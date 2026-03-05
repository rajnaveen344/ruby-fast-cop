//! Style/FrozenStringLiteralComment cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};

#[derive(Debug, Clone, PartialEq)]
pub enum EnforcedStyle {
    Always,
    AlwaysTrue,
    Never,
}

pub struct FrozenStringLiteralComment {
    enforced_style: EnforcedStyle,
}

impl FrozenStringLiteralComment {
    pub fn new(style: EnforcedStyle) -> Self {
        Self { enforced_style: style }
    }

    fn find_frozen_key_value(content: &str) -> Option<String> {
        content.split(';').find_map(|part| {
            let (key, val) = part.trim().split_once(':')?;
            if key.trim().to_lowercase().replace(['-', '_'], "") == "frozenstringliteral" {
                Some(val.trim().to_string())
            } else { None }
        })
    }

    fn parse_frozen_comment_value(line: &str) -> Option<String> {
        let content = line.trim().strip_prefix('#')?.trim();
        if content.starts_with("-*-") && content.ends_with("-*-") {
            return Self::find_frozen_key_value(&content[3..content.len() - 3]);
        }
        Self::find_frozen_key_value(content)
    }

    fn is_shebang(line: &str) -> bool { line.starts_with("#!") }

    fn is_encoding_comment(line: &str) -> bool {
        let content = match line.trim().strip_prefix('#') { Some(c) => c.trim(), None => return false };
        if content.starts_with("-*-") && content.ends_with("-*-") {
            return content[3..content.len() - 3].trim().split(';').any(|part|
                part.trim().split_once(':').map_or(false, |(key, _)|
                    matches!(key.trim().to_lowercase().as_str(), "encoding" | "coding")));
        }
        content.contains("encoding:") || content.contains("coding:")
    }

    fn insertion_point(source: &str) -> (usize, usize, bool) {
        let mut offset = 0;
        let mut last_magic_line_end = 0;
        let mut has_any_magic = false;

        for (i, line) in source.lines().enumerate() {
            let next_offset = offset + line.len() + if offset + line.len() < source.len() { 1 } else { 0 };
            if (i == 0 && Self::is_shebang(line)) || Self::is_encoding_comment(line) {
                has_any_magic = true;
                last_magic_line_end = next_offset;
            } else if !line.trim().is_empty() && !line.trim().starts_with('#') {
                break;
            }
            offset = next_offset;
        }

        let insert_at = if has_any_magic { last_magic_line_end } else { 0 };
        let mut replace_end = insert_at;
        while replace_end < source.len() && source.as_bytes()[replace_end] == b'\n' {
            replace_end += 1;
        }
        (insert_at, replace_end, replace_end > insert_at)
    }

    fn frozen_comment_byte_range(source: &str, line_idx: usize) -> (usize, usize) {
        let mut offset = 0;
        for (i, line) in source.lines().enumerate() {
            let next_offset = offset + line.len() + if offset + line.len() < source.len() { 1 } else { 0 };
            if i == line_idx {
                let mut end = next_offset;
                if end < source.len() {
                    let rest = &source[end..];
                    if rest.starts_with("\r\n") { end += 2; }
                    else if rest.starts_with('\n') { end += 1; }
                }
                return (offset, end);
            }
            offset = next_offset;
        }
        (offset, offset)
    }

    fn find_frozen_comment_in_magic_area(source: &str) -> Option<(usize, String, String)> {
        for (i, line) in source.lines().enumerate() {
            if let Some(value) = Self::parse_frozen_comment_value(line) {
                return Some((i, line.to_string(), value));
            }
            let trimmed = line.trim();
            if trimmed.is_empty() || Self::is_shebang(line) || Self::is_encoding_comment(line) || trimmed.starts_with('#') {
                continue;
            }
            break;
        }
        None
    }

    fn make_missing_offense(&self, ctx: &CheckContext, message: &str) -> Offense {
        let (insert_offset, replace_end, had_blank_lines) = Self::insertion_point(ctx.source);
        let insert_text = if had_blank_lines { "# frozen_string_literal: true\n\n" } else { "# frozen_string_literal: true\n" };
        Offense::new(self.name(), message, self.severity(), Location::new(1, 0, 1, 1), ctx.filename)
            .with_correction(Correction::replace(insert_offset, replace_end, insert_text))
    }
}

impl Cop for FrozenStringLiteralComment {
    fn name(&self) -> &'static str { "Style/FrozenStringLiteralComment" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if ctx.source.trim().is_empty() || !ctx.ruby_version_at_least(2, 3) {
            return vec![];
        }

        match self.enforced_style {
            EnforcedStyle::Always => {
                if let Some((_, _, value)) = Self::find_frozen_comment_in_magic_area(ctx.source) {
                    if value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false") {
                        return vec![];
                    }
                }
                vec![self.make_missing_offense(ctx, "Missing frozen string literal comment.")]
            }
            EnforcedStyle::AlwaysTrue => {
                if let Some((line_idx, line_text, value)) = Self::find_frozen_comment_in_magic_area(ctx.source) {
                    if value.eq_ignore_ascii_case("true") { return vec![]; }

                    let (start, _end) = Self::frozen_comment_byte_range(ctx.source, line_idx);
                    let line_end = start + line_text.len();
                    let line_end_nl = if line_end < ctx.source.len() { line_end + 1 } else { line_end };

                    let replacement = if line_text.trim().contains("-*-") {
                        let trimmed = line_text.trim();
                        let inner = &trimmed[trimmed.find("-*-").unwrap() + 3..trimmed.rfind("-*-").unwrap()].trim();
                        let parts: Vec<String> = inner.split(';').filter_map(|part| {
                            let pt = part.trim();
                            if pt.is_empty() { return None; }
                            if let Some((key, _)) = pt.split_once(':') {
                                if key.trim().to_lowercase().replace(['-', '_'], "") == "frozenstringliteral" {
                                    return Some(format!("{}: true", key.trim()));
                                }
                            }
                            Some(pt.to_string())
                        }).collect();
                        format!("# -*- {} -*-\n", parts.join("; "))
                    } else {
                        "# frozen_string_literal: true\n".to_string()
                    };
                    let line_num = (line_idx + 1) as u32;
                    return vec![Offense::new(
                        self.name(), "Frozen string literal comment must be set to `true`.", self.severity(),
                        Location::new(line_num, 0, line_num, line_text.chars().count() as u32), ctx.filename,
                    ).with_correction(Correction::replace(start, line_end_nl, replacement))];
                }
                vec![self.make_missing_offense(ctx, "Missing magic comment `# frozen_string_literal: true`.")]
            }
            EnforcedStyle::Never => {
                if let Some((line_idx, line_text, _)) = Self::find_frozen_comment_in_magic_area(ctx.source) {
                    let (start, end) = Self::frozen_comment_byte_range(ctx.source, line_idx);
                    let line_num = (line_idx + 1) as u32;
                    return vec![Offense::new(
                        self.name(), "Unnecessary frozen string literal comment.", self.severity(),
                        Location::new(line_num, 0, line_num, line_text.chars().count() as u32), ctx.filename,
                    ).with_correction(Correction::delete(start, end))];
                }
                vec![]
            }
        }
    }
}
