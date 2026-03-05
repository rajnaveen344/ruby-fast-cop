//! Layout/EndAlignment - Checks whether the end keyword is aligned properly.
//!
//! This cop checks whether the end keywords are aligned properly for
//! if, unless, while, until, case, class, module, def, and begin.
//!
//! Three alignment styles are supported:
//! - `keyword`: align `end` with the keyword itself
//! - `variable`: align `end` with the LHS of an assignment (if present), otherwise keyword
//! - `start_of_line`: align `end` with the start of the line containing the keyword

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

/// Alignment style for EndAlignment cop
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndAlignmentStyle {
    Keyword,
    Variable,
    StartOfLine,
}

pub struct EndAlignment {
    style: EndAlignmentStyle,
}

impl EndAlignment {
    pub fn new(style: EndAlignmentStyle) -> Self {
        Self { style }
    }
}

impl Cop for EndAlignment {
    fn name(&self) -> &'static str {
        "Layout/EndAlignment"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = EndAlignmentVisitor {
            ctx,
            style: self.style,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct EndAlignmentVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: EndAlignmentStyle,
    offenses: Vec<Offense>,
}

/// Compute column (0-indexed) from a byte offset, skipping BOM character.
fn col_at_offset(source: &str, offset: usize) -> u32 {
    let mut col = 0u32;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            col = 0;
        } else if ch == '\u{FEFF}' {
            // Skip BOM character for column counting
        } else {
            col += 1;
        }
    }
    col
}

/// Compute line (1-indexed) from a byte offset.
fn line_at_offset(source: &str, offset: usize) -> u32 {
    let mut line = 1u32;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
        }
    }
    line
}

/// Get the byte offset of the start of the line containing the given offset.
fn line_start_offset(source: &str, offset: usize) -> usize {
    source[..offset]
        .rfind('\n')
        .map(|pos| pos + 1)
        .unwrap_or(0)
}

/// Get the column of the first non-whitespace, non-BOM character on the line.
fn first_non_ws_col(source: &str, offset: usize) -> u32 {
    let ls = line_start_offset(source, offset);
    let line_bytes = &source[ls..];
    let mut col = 0u32;
    for ch in line_bytes.chars() {
        if ch == '\n' {
            break;
        }
        if ch == '\u{FEFF}' {
            continue; // skip BOM
        }
        if ch.is_whitespace() {
            col += 1;
        } else {
            break;
        }
    }
    col
}

/// Get the text content of the source line up to (but not including) the newline.
fn get_line_text(source: &str, offset: usize) -> &str {
    let ls = line_start_offset(source, offset);
    let line_end = source[ls..]
        .find('\n')
        .map(|pos| ls + pos)
        .unwrap_or(source.len());
    &source[ls..line_end]
}

/// Check if the text before the keyword on the same line is only whitespace.
fn only_whitespace_before_keyword(source: &str, keyword_offset: usize) -> bool {
    let ls = line_start_offset(source, keyword_offset);
    let before = &source[ls..keyword_offset];
    before.chars().all(|c| c.is_whitespace() || c == '\u{FEFF}')
}

/// Detect if there's an assignment operator on the line before the keyword.
/// Returns true if `=`, `+=`, `-=`, `*=`, `/=`, `%=`, `**=`, `||=`, `&&=`,
/// `<<=`, `>>=`, `^=`, `|=`, `&=`, or `<<` is found before the keyword.
fn has_assignment_before_keyword(source: &str, keyword_offset: usize) -> bool {
    let ls = line_start_offset(source, keyword_offset);
    let before = &source[ls..keyword_offset];
    let bytes = before.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let ch = bytes[i];
        if ch == b'=' {
            let prev = if i > 0 { bytes[i - 1] } else { 0 };
            let next = if i + 1 < len { bytes[i + 1] } else { 0 };

            // Skip ==, =~, =>
            if next == b'=' || next == b'~' || next == b'>' {
                i += 2;
                continue;
            }
            // Skip !=, <=, >=
            if prev == b'!' || prev == b'<' || prev == b'>' {
                // But <=, >= are comparison, not assignment. However <= is already handled.
                // Actually: prev == '<' could be <<=, but that has = at end.
                // prev == '<' with current being '=' => this is the = in <=
                // But wait, we need to also accept: var = , +=, -=, etc.
                // If prev is an operator char that forms compound assignment:
                // +=, -=, *=, /=, %=, **=, ^=, |=, &=, ||=, &&=, <<=, >>=
                // The = we see now could be the = in <=, >=, !=
                if prev == b'!' {
                    i += 1;
                    continue;
                }
                // For <, > before =: could be <= (comparison) or <<= (assignment)
                // If prev is < and the one before is also <, it's <<=
                if prev == b'<' {
                    if i >= 2 && bytes[i - 2] == b'<' {
                        return true; // <<=
                    }
                    i += 1;
                    continue; // <=
                }
                if prev == b'>' {
                    if i >= 2 && bytes[i - 2] == b'>' {
                        return true; // >>=
                    }
                    i += 1;
                    continue; // >=
                }
            }
            // This is a valid assignment =
            return true;
        } else if ch == b'<' && i + 1 < len && bytes[i + 1] == b'<' {
            // << could be heredoc or append operator
            if i + 2 < len && bytes[i + 2] == b'=' {
                // <<= is handled by the = check
                i += 3;
                continue;
            }
            // Check if followed by space (likely << operator for append)
            let after = if i + 2 < len { bytes[i + 2] } else { 0 };
            if after == b' ' || after == b'\t' {
                return true; // var << expr
            }
            i += 2;
            continue;
        }

        // Skip string literals
        if ch == b'\'' || ch == b'"' {
            let quote = ch;
            i += 1;
            while i < len && bytes[i] != quote {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            if i < len {
                i += 1;
            }
            continue;
        }

        i += 1;
    }

    false
}

/// For "variable" style, detect if a keyword is a method argument or operator operand.
/// (e.g., `test case a when b`, `do_something case condition`, `variable + if`)
///
/// Returns false if the keyword is after a `;` separator (independent statement)
/// or if there's nothing before it.
fn is_in_call_or_operator_context(source: &str, keyword_offset: usize) -> bool {
    let ls = line_start_offset(source, keyword_offset);
    let before = &source[ls..keyword_offset];
    let trimmed = before.trim_end();

    if trimmed.is_empty() || trimmed.chars().all(|c| c.is_whitespace() || c == '\u{FEFF}') {
        return false;
    }

    // Check if the keyword follows a `;` - in that case it's an independent statement
    // Find the last `;` before the keyword
    if let Some(semi_pos) = before.rfind(';') {
        // Check if everything between the semicolon and keyword is whitespace
        let after_semi = &before[semi_pos + 1..];
        if after_semi.trim().is_empty() {
            return false; // Independent statement after ;
        }
    }

    // Also check for `(` immediately before (with optional whitespace) - still a call context
    // e.g., `puts(if test` or `format(\n  case`

    // If there's content before the keyword and it's NOT an assignment,
    // it's a call or operator context
    !has_assignment_before_keyword(source, keyword_offset)
}

impl<'a> EndAlignmentVisitor<'a> {
    fn check_end_alignment(
        &mut self,
        keyword: &str,
        keyword_offset: usize,
        end_offset: usize,
        end_end_offset: usize,
    ) {
        let source = self.ctx.source;
        let kw_line = line_at_offset(source, keyword_offset);
        let end_line = line_at_offset(source, end_offset);
        let end_col = col_at_offset(source, end_offset);

        // Skip if on same line (one-liners)
        if kw_line == end_line {
            return;
        }

        let kw_col = col_at_offset(source, keyword_offset);

        let expected_col = match self.style {
            EndAlignmentStyle::Keyword => kw_col,
            EndAlignmentStyle::Variable => {
                if only_whitespace_before_keyword(source, keyword_offset) {
                    // Keyword at start of line - align with keyword
                    kw_col
                } else if has_assignment_before_keyword(source, keyword_offset) {
                    // Assignment context - align with start of line (variable position)
                    first_non_ws_col(source, keyword_offset)
                } else if is_in_call_or_operator_context(source, keyword_offset) {
                    // Method call or operator context - align with start of line
                    first_non_ws_col(source, keyword_offset)
                } else {
                    kw_col
                }
            }
            EndAlignmentStyle::StartOfLine => {
                if only_whitespace_before_keyword(source, keyword_offset) {
                    // When keyword is at start of line, start_of_line = keyword col
                    // But actually, start_of_line means first non-ws on the line
                    first_non_ws_col(source, keyword_offset)
                } else {
                    first_non_ws_col(source, keyword_offset)
                }
            }
        };

        if end_col != expected_col {
            let align_target = self.build_align_target(keyword, keyword_offset, expected_col);
            let message = format!(
                "`end` at {}, {} is not aligned with `{}` at {}, {}.",
                end_line, end_col, align_target, kw_line, expected_col
            );

            let location = crate::offense::Location::from_offsets(source, end_offset, end_end_offset);
            self.offenses.push(Offense::new(
                "Layout/EndAlignment",
                message,
                Severity::Convention,
                location,
                self.ctx.filename,
            ));
        }
    }

    fn build_align_target(&self, keyword: &str, keyword_offset: usize, _expected_col: u32) -> String {
        let source = self.ctx.source;
        match self.style {
            EndAlignmentStyle::Keyword => keyword.to_string(),
            EndAlignmentStyle::Variable => {
                if only_whitespace_before_keyword(source, keyword_offset) {
                    // Standalone keyword
                    keyword.to_string()
                } else {
                    let ls = line_start_offset(source, keyword_offset);
                    let before = &source[ls..keyword_offset];

                    // Check if keyword follows a `;` (independent statement)
                    if let Some(semi_pos) = before.rfind(';') {
                        let after_semi = &before[semi_pos + 1..];
                        if after_semi.trim().is_empty() {
                            // Independent statement - use just the keyword
                            return keyword.to_string();
                        }
                    }

                    // Something before keyword (assignment, call, operator) -
                    // include text from first non-ws to end of keyword
                    let first_nw = source[ls..].chars()
                        .position(|c| !c.is_whitespace() && c != '\u{FEFF}')
                        .unwrap_or(0);
                    let text = &source[ls + first_nw..keyword_offset + keyword.len()];
                    text.to_string()
                }
            }
            EndAlignmentStyle::StartOfLine => {
                // Get the whole line content (trimmed)
                let line_text = get_line_text(source, keyword_offset);
                let trimmed = line_text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}');
                trimmed.trim_end().to_string()
            }
        }
    }
}

impl Visit<'_> for EndAlignmentVisitor<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        if let Some(kw_loc) = node.if_keyword_loc() {
            if let Some(end_loc) = node.end_keyword_loc() {
                let kw_text = std::str::from_utf8(kw_loc.as_slice()).unwrap_or("if");
                // Only check top-level if, not elsif
                if kw_text == "if" {
                    self.check_end_alignment(
                        "if",
                        kw_loc.start_offset(),
                        end_loc.start_offset(),
                        end_loc.end_offset(),
                    );
                }
            }
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        if let Some(end_loc) = node.end_keyword_loc() {
            let kw_loc = node.keyword_loc();
            self.check_end_alignment(
                "unless",
                kw_loc.start_offset(),
                end_loc.start_offset(),
                end_loc.end_offset(),
            );
        }
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        if let Some(end_loc) = node.closing_loc() {
            let kw_loc = node.keyword_loc();
            self.check_end_alignment(
                "while",
                kw_loc.start_offset(),
                end_loc.start_offset(),
                end_loc.end_offset(),
            );
        }
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        if let Some(end_loc) = node.closing_loc() {
            let kw_loc = node.keyword_loc();
            self.check_end_alignment(
                "until",
                kw_loc.start_offset(),
                end_loc.start_offset(),
                end_loc.end_offset(),
            );
        }
        ruby_prism::visit_until_node(self, node);
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        let kw_loc = node.case_keyword_loc();
        let end_loc = node.end_keyword_loc();
        self.check_end_alignment(
            "case",
            kw_loc.start_offset(),
            end_loc.start_offset(),
            end_loc.end_offset(),
        );
        ruby_prism::visit_case_node(self, node);
    }

    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        let kw_loc = node.case_keyword_loc();
        let end_loc = node.end_keyword_loc();
        self.check_end_alignment(
            "case",
            kw_loc.start_offset(),
            end_loc.start_offset(),
            end_loc.end_offset(),
        );
        ruby_prism::visit_case_match_node(self, node);
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        let kw_loc = node.class_keyword_loc();
        let end_loc = node.end_keyword_loc();
        self.check_end_alignment(
            "class",
            kw_loc.start_offset(),
            end_loc.start_offset(),
            end_loc.end_offset(),
        );
        ruby_prism::visit_class_node(self, node);
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        let kw_loc = node.class_keyword_loc();
        let end_loc = node.end_keyword_loc();
        self.check_end_alignment(
            "class",
            kw_loc.start_offset(),
            end_loc.start_offset(),
            end_loc.end_offset(),
        );
        ruby_prism::visit_singleton_class_node(self, node);
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        let kw_loc = node.module_keyword_loc();
        let end_loc = node.end_keyword_loc();
        self.check_end_alignment(
            "module",
            kw_loc.start_offset(),
            end_loc.start_offset(),
            end_loc.end_offset(),
        );
        ruby_prism::visit_module_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        if let Some(end_loc) = node.end_keyword_loc() {
            let kw_loc = node.def_keyword_loc();
            self.check_end_alignment(
                "def",
                kw_loc.start_offset(),
                end_loc.start_offset(),
                end_loc.end_offset(),
            );
        }
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        if let Some(kw_loc) = node.begin_keyword_loc() {
            if let Some(end_loc) = node.end_keyword_loc() {
                self.check_end_alignment(
                    "begin",
                    kw_loc.start_offset(),
                    end_loc.start_offset(),
                    end_loc.end_offset(),
                );
            }
        }
        ruby_prism::visit_begin_node(self, node);
    }

    fn visit_for_node(&mut self, node: &ruby_prism::ForNode) {
        let kw_loc = node.for_keyword_loc();
        let end_loc = node.end_keyword_loc();
        self.check_end_alignment(
            "for",
            kw_loc.start_offset(),
            end_loc.start_offset(),
            end_loc.end_offset(),
        );
        ruby_prism::visit_for_node(self, node);
    }
}
