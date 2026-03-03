//! Layout/LeadingCommentSpace - Checks whether comments have a leading space after the #.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/leading_comment_space.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Location, Offense, Severity};

pub struct LeadingCommentSpace {
    allow_doxygen_comment_style: bool,
    allow_gemfile_ruby_comment: bool,
    allow_rbs_inline_annotation: bool,
    allow_steep_annotation: bool,
}

impl LeadingCommentSpace {
    pub fn new() -> Self {
        Self {
            allow_doxygen_comment_style: false,
            allow_gemfile_ruby_comment: false,
            allow_rbs_inline_annotation: false,
            allow_steep_annotation: false,
        }
    }

    pub fn with_config(
        allow_doxygen_comment_style: bool,
        allow_gemfile_ruby_comment: bool,
        allow_rbs_inline_annotation: bool,
        allow_steep_annotation: bool,
    ) -> Self {
        Self {
            allow_doxygen_comment_style,
            allow_gemfile_ruby_comment,
            allow_rbs_inline_annotation,
            allow_steep_annotation,
        }
    }

    /// Check if line starts a =begin doc comment
    fn is_doc_comment_start(line: &str) -> bool {
        line.starts_with("=begin")
    }

    /// Check if line ends a =begin doc comment
    fn is_doc_comment_end(line: &str) -> bool {
        line.starts_with("=end")
    }

    /// Find the comment portion of a line (the # and everything after).
    /// Returns (byte_start, comment_text) where byte_start is the byte offset of `#`.
    /// Returns None if no comment found on this line.
    fn find_comment(line: &str) -> Option<(usize, &str)> {
        // Simple approach: find # that's not inside a string
        // We use a basic state machine to track string context
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '#' => {
                    let byte_pos = line.char_indices().nth(i).map(|(pos, _)| pos).unwrap_or(0);
                    return Some((byte_pos, &line[byte_pos..]));
                }
                '\'' => {
                    // Skip single-quoted string
                    i += 1;
                    while i < chars.len() && chars[i] != '\'' {
                        if chars[i] == '\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                '"' => {
                    // Skip double-quoted string
                    i += 1;
                    while i < chars.len() && chars[i] != '"' {
                        if chars[i] == '\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// Check if a comment is exempt from the leading space rule.
    fn is_exempt_comment(
        &self,
        comment_text: &str,
        line_index: usize,
        is_first_on_line: bool,
        filename: &str,
    ) -> bool {
        let after_hash = &comment_text[1..]; // Everything after the #

        // Empty comment: just "#"
        if after_hash.is_empty() {
            return true;
        }

        // Already has a space
        if after_hash.starts_with(' ') || after_hash.starts_with('\t') {
            return true;
        }

        // Multiple # (e.g., "####", "######")
        if after_hash.starts_with('#') {
            return true;
        }

        // Sprockets directive: #=
        if after_hash.starts_with('=') {
            return true;
        }

        // RDoc toggle comments: #++ and #--
        if after_hash == "++" || after_hash == "--" {
            return true;
        }

        // Shebang on first line: #!/... or #! ... (multiline shebangs)
        if line_index == 0 && after_hash.starts_with('!') {
            return true;
        }

        // Multiline shebang continuation: #! on lines after a line-0 shebang
        // (handled by checking if line_index > 0 && starts with #!)
        // Actually, shebangs on first line exempt ALL #! lines that follow
        // But we need to know if line 0 was a shebang. We handle this in the caller.

        // Doxygen comments: #* or #**
        if self.allow_doxygen_comment_style && after_hash.starts_with('*') {
            return true;
        }

        // RBS inline annotations: #:, #|, #[
        if self.allow_rbs_inline_annotation
            && (after_hash.starts_with(':')
                || after_hash.starts_with('|')
                || after_hash.starts_with('['))
        {
            return true;
        }

        // Steep annotations: #$ or #: (Steep uses #: for type annotations)
        if self.allow_steep_annotation
            && (after_hash.starts_with('$') || after_hash.starts_with(':'))
        {
            return true;
        }

        // Gemfile ruby comment: #ruby= or #ruby-
        // Only allowed in files named "Gemfile"
        if self.allow_gemfile_ruby_comment
            && is_first_on_line
            && (after_hash.starts_with("ruby=") || after_hash.starts_with("ruby-"))
        {
            let fname = std::path::Path::new(filename)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("");
            if fname == "Gemfile" {
                return true;
            }
        }

        false
    }
}

impl Cop for LeadingCommentSpace {
    fn name(&self) -> &'static str {
        "Layout/LeadingCommentSpace"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let mut in_doc_comment = false;
        let mut first_line_is_shebang = false;
        let mut byte_offset: usize = 0;

        for (line_index, line) in ctx.source.lines().enumerate() {
            let line_byte_offset = byte_offset;
            byte_offset += line.len();
            if byte_offset < ctx.source.len() {
                byte_offset += 1; // skip '\n'
            }
            // Track =begin/=end doc comments
            if !in_doc_comment && Self::is_doc_comment_start(line) {
                in_doc_comment = true;
                continue;
            }
            if in_doc_comment {
                if Self::is_doc_comment_end(line) {
                    in_doc_comment = false;
                }
                continue;
            }

            // Track if first line is a shebang
            if line_index == 0 {
                let trimmed = line.trim_start();
                if trimmed.starts_with("#!") {
                    first_line_is_shebang = true;
                    continue; // Shebangs are always exempt
                }
            }

            // Check for shebang continuation (lines starting with #! after a shebang first line)
            if first_line_is_shebang && line_index > 0 {
                let trimmed = line.trim_start();
                if trimmed.starts_with("#!") {
                    // This is part of a multiline shebang - exempt
                    continue;
                }
            }

            // Find comment on this line
            if let Some((byte_start, comment_text)) = Self::find_comment(line) {
                // Check if this is a standalone comment (# at start of trimmed line)
                let char_start = line[..byte_start].chars().count();
                let is_first_on_line = line[..byte_start].trim().is_empty();

                // Check if comment needs a space
                if comment_text.len() > 1 {
                    let after_hash = &comment_text[1..];

                    // Check exemptions
                    if self.is_exempt_comment(comment_text, line_index, is_first_on_line, ctx.filename) {
                        continue;
                    }

                    // config.ru first-line exemption: #\ on first line
                    if line_index == 0 && after_hash.starts_with('\\') {
                        // This is only exempt for config.ru files
                        // Since test runner uses "test.rb", check filename
                        if ctx.filename.ends_with("config.ru") {
                            continue;
                        }
                    }

                    // Offense: missing space after #
                    let comment_char_len = comment_text.chars().count();
                    let line_num = (line_index + 1) as u32;

                    // Correction: insert space after the #
                    // byte_start is the byte offset of # within the line
                    let hash_abs_byte = line_byte_offset + byte_start;
                    let correction = Correction::insert(hash_abs_byte + 1, " ");

                    offenses.push(Offense::new(
                        self.name(),
                        "Missing space after `#`.",
                        self.severity(),
                        Location::new(
                            line_num,
                            char_start as u32,
                            line_num,
                            (char_start + comment_char_len) as u32,
                        ),
                        ctx.filename,
                    )
                    .with_correction(correction));
                }
            }
        }

        offenses
    }
}
