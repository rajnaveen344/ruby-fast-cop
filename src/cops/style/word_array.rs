//! Style/WordArray - Prefer %w or %W for arrays of word-like strings.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/word_array.rb
//! Mixin: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/mixin/percent_array.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/WordArray";
const PERCENT_MSG: &str = "Use `%w` or `%W` for an array of words.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    Percent,
    Brackets,
}

pub struct WordArray {
    style: EnforcedStyle,
    min_size: usize,
    word_regex: String,
    /// Preferred delimiter pair for %w/%W: e.g. "()" or "[]" or "{}" or "<>"
    preferred_delimiters: String,
}

impl Default for WordArray {
    fn default() -> Self {
        Self {
            style: EnforcedStyle::Percent,
            min_size: 2,
            word_regex: r"\A(?:\w|\w-\w|\n|\t)+\z".to_string(),
            preferred_delimiters: "()".to_string(),
        }
    }
}

impl WordArray {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(
        style: EnforcedStyle,
        min_size: usize,
        word_regex: String,
        preferred_delimiters: String,
    ) -> Self {
        Self {
            style,
            min_size,
            word_regex,
            preferred_delimiters,
        }
    }
}

impl Cop for WordArray {
    fn name(&self) -> &'static str {
        COP_NAME
    }
    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor {
            cop: self,
            ctx,
            parent_array_matrix_complex: vec![false],
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a, 'b> {
    cop: &'a WordArray,
    ctx: &'a CheckContext<'b>,
    /// Stack: for each ancestor ArrayNode, is it a "matrix of complex content"?
    parent_array_matrix_complex: Vec<bool>,
    offenses: Vec<Offense>,
}

impl<'a, 'b> Visit<'_> for Visitor<'a, 'b> {
    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        let elements: Vec<Node> = node.elements().iter().collect();

        // Compute matrix_of_complex_content for this array (used if its children are arrays)
        let is_matrix_complex =
            matrix_of_complex_content(&elements, self.ctx.source, &self.cop.word_regex);

        // Check this array for offense
        let offenses = self.cop.check_array_impl(
            node,
            &elements,
            self.ctx,
            *self.parent_array_matrix_complex.last().unwrap_or(&false),
        );
        self.offenses.extend(offenses);

        self.parent_array_matrix_complex.push(is_matrix_complex);
        ruby_prism::visit_array_node(self, node);
        self.parent_array_matrix_complex.pop();
    }
}

fn matrix_of_complex_content(elements: &[Node], source: &str, regex: &str) -> bool {
    if elements.is_empty() {
        return false;
    }
    if !elements
        .iter()
        .all(|e| matches!(e, Node::ArrayNode { .. }))
    {
        return false;
    }
    // Any subarray has complex content
    let re = match regex::Regex::new(regex) {
        Ok(r) => r,
        Err(_) => return true,
    };
    elements.iter().any(|sub| {
        let sub_arr = sub.as_array_node().unwrap();
        let sub_elems: Vec<Node> = sub_arr.elements().iter().collect();
        sub_elems.iter().any(|e| {
            let content = match string_content(e, source) {
                Some(c) => c,
                None => return true,
            };
            content.contains(' ') || !re.is_match(&content)
        })
    })
}

impl WordArray {
    fn check_array_impl(
        &self,
        node: &ruby_prism::ArrayNode,
        elements: &[Node],
        ctx: &CheckContext,
        within_matrix_complex: bool,
    ) -> Vec<Offense> {
        let is_percent = {
            let opening = node.opening_loc();
            match opening {
                Some(loc) => {
                    let s = &ctx.source[loc.start_offset()..loc.end_offset()];
                    s.starts_with("%w")
                        || s.starts_with("%W")
                        || s.starts_with("%i")
                        || s.starts_with("%I")
                }
                None => false,
            }
        };
        let is_percent_string = {
            let opening = node.opening_loc();
            match opening {
                Some(loc) => {
                    let s = &ctx.source[loc.start_offset()..loc.end_offset()];
                    s.starts_with("%w") || s.starts_with("%W")
                }
                None => false,
            }
        };

        let bracketed_of_str = !is_percent
            && elements
                .iter()
                .all(|e| matches!(e, Node::StringNode { .. } | Node::InterpolatedStringNode { .. }))
            && !elements.is_empty();

        if bracketed_of_str {
            if within_matrix_complex {
                return vec![];
            }
            return self.check_bracketed_string_array(node, elements, ctx);
        } else if is_percent_string {
            return self.check_percent_array(node, elements, ctx);
        }
        vec![]
    }

    fn check_bracketed_string_array(
        &self,
        node: &ruby_prism::ArrayNode,
        elements: &[Node],
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        if self.style != EnforcedStyle::Percent {
            return vec![];
        }
        if elements.len() < self.min_size {
            return vec![];
        }
        if self.complex_content(elements, ctx.source) {
            return vec![];
        }
        if self.has_comments_in_array(node, ctx.source) {
            return vec![];
        }
        if invalid_percent_array_context(node, ctx.source) {
            return vec![];
        }

        let loc = node.location();
        let mut off = ctx.offense(COP_NAME, PERCENT_MSG, Severity::Convention, &loc);
        if let Some(c) =
            build_percent_correction(ctx.source, node, elements, &self.preferred_delimiters)
        {
            off = off.with_correction(c);
        }
        vec![off]
    }

    fn check_percent_array(
        &self,
        node: &ruby_prism::ArrayNode,
        elements: &[Node],
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        if self.style != EnforcedStyle::Brackets {
            // In `percent` mode, percent arrays containing invalid content (spaces or bad encoding) are flagged.
            if !invalid_percent_array_contents(elements, ctx.source) {
                return vec![];
            }
        }

        // Build bracketed replacement for message and correction
        let bracketed_single = build_bracketed_replacement_single_line(elements, ctx.source);
        let has_newline = ctx.source[node.location().start_offset()..node.location().end_offset()]
            .contains('\n');

        if has_newline {
            let open = node.opening_loc().unwrap();
            let msg = "Use an array literal `[...]` for an array of words.".to_string();
            let mut off = ctx.offense_with_range(
                COP_NAME,
                &msg,
                Severity::Convention,
                open.start_offset(),
                open.end_offset(),
            );
            let correction = build_brackets_correction(ctx.source, node, elements);
            off = off.with_correction(correction);
            vec![off]
        } else {
            let msg = format!("Use `{}` for an array of words.", bracketed_single);
            let mut off = ctx.offense(COP_NAME, &msg, Severity::Convention, &node.location());
            let correction = build_brackets_correction(ctx.source, node, elements);
            off = off.with_correction(correction);
            vec![off]
        }
    }

    fn complex_content(&self, elements: &[Node], source: &str) -> bool {
        // Mirror RuboCop: regex must match, no spaces, no interpolation.
        let re = match regex::Regex::new(&self.word_regex) {
            Ok(r) => r,
            Err(_) => return true,
        };
        elements.iter().any(|e| {
            let content = match string_content(e, source) {
                Some(c) => c,
                None => return true, // non-str content = complex
            };
            content.contains(' ') || !re.is_match(&content)
        })
    }

    fn has_comments_in_array(&self, node: &ruby_prism::ArrayNode, source: &str) -> bool {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let slice = &source[start..end];
        if !slice.contains('\n') {
            return false;
        }
        for line in slice.lines().skip(1) {
            if let Some(p) = line.find('#') {
                let before = &line[..p];
                let q_count = before.chars().filter(|&c| c == '\'' || c == '"').count();
                if q_count % 2 == 0 {
                    return true;
                }
            }
        }
        false
    }
}

/// Get the raw source text between the quotes of a string node (no unescaping).
fn string_raw_content<'a>(node: &Node, source: &'a str) -> Option<&'a str> {
    match node {
        Node::StringNode { .. } => {
            let s = node.as_string_node().unwrap();
            let open_end = s.opening_loc()?.end_offset();
            let close_start = s.closing_loc()?.start_offset();
            Some(&source[open_end..close_start])
        }
        _ => None,
    }
}

fn string_content(node: &Node, source: &str) -> Option<String> {
    match node {
        Node::StringNode { .. } => {
            let s = node.as_string_node().unwrap();
            let bytes = s.unescaped();
            std::str::from_utf8(bytes).ok().map(|s| s.to_string())
        }
        Node::InterpolatedStringNode { .. } => {
            let s = node.as_interpolated_string_node().unwrap();
            let parts: Vec<_> = s.parts().iter().collect();
            // Only allow non-interpolated parts
            let mut out = String::new();
            for p in parts {
                match p {
                    Node::StringNode { .. } => {
                        let sn = p.as_string_node().unwrap();
                        let bytes = sn.unescaped();
                        out.push_str(std::str::from_utf8(bytes).ok()?);
                    }
                    _ => return None,
                }
            }
            Some(out)
        }
        _ => {
            let _ = source;
            None
        }
    }
}

/// Get the raw source text of a %w/%W element (the token between delimiters, without quotes).
fn percent_element_raw_source<'a>(node: &Node, source: &'a str) -> &'a str {
    &source[node.location().start_offset()..node.location().end_offset()]
}

/// For elements inside a %w/%W array, convert to a bracketed literal using the raw source text.
fn percent_element_to_literal(node: &Node, source: &str) -> String {
    if let Node::InterpolatedStringNode { .. } = node {
        let s = percent_element_raw_source(node, source);
        return format!("\"{}\"", s);
    }
    let s = percent_element_raw_source(node, source);
    // Check if source contains actual escape characters (control chars, real \n, \t)
    // vs apparent ones (backslash followed by letter in single-quoted string)
    let has_control = s.chars().any(|c| c.is_control());
    let has_real_escape = has_control;

    // Check for `\` followed by non-space, non-backslash
    let has_backslash_escape = {
        let bytes = s.as_bytes();
        let mut found = false;
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == b'\\' {
                let next = bytes[i + 1];
                if next != b' ' && next != b'\\' {
                    found = true;
                    break;
                }
                i += 2;
            } else {
                i += 1;
            }
        }
        found
    };

    if has_real_escape {
        // Encode control chars as unicode escapes
        let mut out = String::new();
        for ch in s.chars() {
            if ch.is_control() {
                out.push_str(&format!("\\u{:04X}", ch as u32));
            } else {
                out.push(ch);
            }
        }
        return format!("\"{}\"", out);
    }

    if has_backslash_escape || s.contains('\'') {
        // Use double quotes, keep content as-is (it has backslash escapes already)
        return format!("\"{}\"", s);
    }

    // Unescape `\ ` → ` ` and `\\` → `\`
    let unescaped = s.replace("\\ ", " ").replace("\\\\", "\\");
    format!("'{}'", unescaped)
}

/// Build single-line bracketed replacement string like `['a', 'b', 'c']`
fn build_bracketed_replacement_single_line(elements: &[Node], source: &str) -> String {
    let mut bracketed = String::from("[");
    for (i, e) in elements.iter().enumerate() {
        if i > 0 {
            bracketed.push_str(", ");
        }
        bracketed.push_str(&percent_element_to_literal(e, source));
    }
    bracketed.push(']');
    bracketed
}

/// Build correction: %w/%W array → bracketed array
fn build_brackets_correction(
    source: &str,
    node: &ruby_prism::ArrayNode,
    elements: &[Node],
) -> Correction {
    let arr_start = node.location().start_offset();
    let arr_end = node.location().end_offset();
    let arr_src = &source[arr_start..arr_end];
    let has_newline = arr_src.contains('\n');

    let replacement = if has_newline {
        // Preserve line structure: map each element to its source line, keep indentation
        // e.g. %w(\n  foo\n  bar\n) → [\n  'foo',\n  'bar'\n]
        build_brackets_multiline(source, node, elements)
    } else {
        build_bracketed_replacement_single_line(elements, source)
    };

    Correction::replace(arr_start, arr_end, &replacement)
}

/// Build multiline bracketed replacement preserving indentation from %w array
fn build_brackets_multiline(
    source: &str,
    node: &ruby_prism::ArrayNode,
    elements: &[Node],
) -> String {
    let arr_start = node.location().start_offset();
    let arr_src = &source[arr_start..node.location().end_offset()];

    // Find the indentation used in the original %w array
    // by looking at the first element's line indent
    let mut result = String::from("[");

    for (i, e) in elements.iter().enumerate() {
        // Find the line this element starts on
        let elem_start = e.location().start_offset();
        let line_start = source[..elem_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let indent: String = source[line_start..elem_start]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();

        result.push('\n');
        result.push_str(&indent);
        result.push_str(&percent_element_to_literal(e, source));
        // Add comma for all but last element
        if i < elements.len() - 1 {
            result.push(',');
        }
    }

    // Closing bracket: find indent of the closing delimiter line
    let close_line_start = source[..node.location().end_offset()]
        .rfind('\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    let close_indent: String = source[close_line_start..node.location().end_offset()]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();

    result.push('\n');
    result.push_str(&close_indent);
    result.push(']');

    let _ = arr_src;
    result
}

/// Get delimiter pair from a string like "()" → ('(', ')')
fn get_delimiters(delimiters: &str) -> (char, char) {
    let mut chars = delimiters.chars();
    let open = chars.next().unwrap_or('(');
    let close = chars.next().unwrap_or(')');
    (open, close)
}

/// Render raw source (between quotes) for use inside a %w array.
/// Only escapes delimiter chars; does NOT re-escape backslash sequences.
/// When `skip_delim_escape` is true (paired delimiters with balanced count),
/// delimiter chars pass through unescaped — mirrors RuboCop's
/// `substitute_escaped_delimiters` behaviour for `[]`, `()`, `{}`, `<>`.
fn render_raw_for_percent(raw: &str, open_delim: char, close_delim: char) -> String {
    let skip = paired_delim_balanced(raw, open_delim, close_delim);
    let mut rendered = String::with_capacity(raw.len() + 4);
    for ch in raw.chars() {
        if !skip && (ch == open_delim || ch == close_delim) {
            rendered.push('\\');
        }
        rendered.push(ch);
    }
    rendered
}

/// True when `open_delim != close_delim` (paired) AND the content has equal
/// counts of each — RuboCop treats this as a "balanced" word and skips
/// delimiter escaping. Same-char delimiters (`||`, `!!`) always need escapes.
fn paired_delim_balanced(content: &str, open_delim: char, close_delim: char) -> bool {
    if open_delim == close_delim {
        return false;
    }
    let opens = content.chars().filter(|&c| c == open_delim).count();
    let closes = content.chars().filter(|&c| c == close_delim).count();
    opens == closes
}

/// Render a string content for use inside a %w/%W array.
/// Handles escaping of delimiter chars and non-ASCII.
fn render_for_percent(
    content: &str,
    open_delim: char,
    close_delim: char,
    needs_w_capital: &mut bool,
) -> String {
    let skip_delim = paired_delim_balanced(content, open_delim, close_delim);
    let mut rendered = String::with_capacity(content.len() + 4);
    for ch in content.chars() {
        match ch {
            '\n' => {
                rendered.push_str("\\n");
                *needs_w_capital = true;
            }
            '\t' => {
                rendered.push_str("\\t");
                *needs_w_capital = true;
            }
            '\r' => {
                rendered.push_str("\\r");
                *needs_w_capital = true;
            }
            '\\' => {
                rendered.push_str("\\\\");
                *needs_w_capital = true;
            }
            c if c.is_control() => {
                rendered.push_str(&format!("\\u{:04X}", c as u32));
                *needs_w_capital = true;
            }
            c if !c.is_ascii() => {
                // Non-ASCII: output as-is (RuboCop preserves Unicode chars in %w)
                rendered.push(c);
            }
            c if !skip_delim && (c == open_delim || c == close_delim) => {
                // Escape delimiter characters
                rendered.push('\\');
                rendered.push(c);
            }
            _ => rendered.push(ch),
        }
    }
    rendered
}

/// Build multiline %w body that preserves source line structure.
/// Each element keeps its original line grouping.
fn build_percent_body_line_preserving(
    source: &str,
    elements: &[Node],
    open_delim: char,
    close_delim: char,
    needs_w_capital: &mut bool,
) -> String {
    if elements.is_empty() {
        return String::new();
    }

    // Group elements by source line
    let mut lines: Vec<Vec<String>> = Vec::new();
    let mut current_line_num = usize::MAX;
    let mut current_group: Vec<String> = Vec::new();

    for e in elements {
        let elem_start = e.location().start_offset();
        let line_num = count_lines_to(&source[..elem_start]);

        let content = match string_content(e, source) {
            Some(c) => c,
            None => continue,
        };

        let rendered = render_for_percent(&content, open_delim, close_delim, needs_w_capital);

        if line_num != current_line_num && current_line_num != usize::MAX {
            lines.push(current_group.clone());
            current_group = Vec::new();
        }
        current_line_num = line_num;
        current_group.push(rendered);
    }
    if !current_group.is_empty() {
        lines.push(current_group);
    }

    // Join groups with newlines, elements within group with spaces
    lines
        .iter()
        .map(|g| g.join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Count 1-based line number from source prefix
fn count_lines_to(prefix: &str) -> usize {
    prefix.chars().filter(|&c| c == '\n').count() + 1
}

/// Build `[...]` → `%w(...)` / `%W(...)` correction.
fn build_percent_correction(
    source: &str,
    node: &ruby_prism::ArrayNode,
    elements: &[Node],
    preferred_delimiters: &str,
) -> Option<Correction> {
    let (open_delim, close_delim) = get_delimiters(preferred_delimiters);

    let arr_start = node.location().start_offset();
    let arr_end = node.location().end_offset();
    let arr_src = &source[arr_start..arr_end];

    // Check if newlines appear BETWEEN elements (not just inside string content)
    // Compare end-line of element[i] with start-line of element[i+1].
    // If they're on the same line, the newline is inside element[i]'s content.
    let has_inter_element_newlines = elements.windows(2).any(|pair| {
        let end_line_a = count_lines_to(&source[..pair[0].location().end_offset()]);
        let start_line_b = count_lines_to(&source[..pair[1].location().start_offset()]);
        start_line_b > end_line_a
    });

    // Check if opening bracket is on a separate line from first element
    // (determines if we need leading/trailing newline in %w output)
    let bracket_on_own_line = if let Some(first) = elements.first() {
        let open_line = count_lines_to(&source[..arr_start]);
        let first_line = count_lines_to(&source[..first.location().start_offset()]);
        first_line > open_line
    } else {
        false
    };

    // Build element representations
    // First check: are all elements simple StringNodes?
    let all_simple = elements
        .iter()
        .all(|e| matches!(e, Node::StringNode { .. }));
    if !all_simple {
        return None;
    }

    let mut needs_w_capital = false;

    let body = if has_inter_element_newlines {
        // Preserve line structure
        build_percent_body_line_preserving(
            source,
            elements,
            open_delim,
            close_delim,
            &mut needs_w_capital,
        )
    } else {
        let mut parts: Vec<String> = Vec::with_capacity(elements.len());
        for e in elements {
            let content = string_content(e, source)?;
            // Check if unescaped content has actual control chars (real \n, \t, etc.)
            let has_real_control = content.chars().any(|c| c.is_control());
            if has_real_control {
                // Use unescaped content, re-escape for %W
                let rendered = render_for_percent(&content, open_delim, close_delim, &mut needs_w_capital);
                parts.push(rendered);
            } else if let Some(raw) = string_raw_content(e, source) {
                // Use raw source — preserves \t, \n etc. as-is (no double-escaping)
                // But still check if delimiters need escaping
                let rendered = render_raw_for_percent(raw, open_delim, close_delim);
                parts.push(rendered);
            } else {
                let rendered = render_for_percent(&content, open_delim, close_delim, &mut needs_w_capital);
                parts.push(rendered);
            }
        }
        parts.join(" ")
    };

    let prefix = if needs_w_capital { "%W" } else { "%w" };

    let replacement = if has_inter_element_newlines && bracket_on_own_line {
        format!("{}{}\n{}\n{}", prefix, open_delim, body, close_delim)
    } else {
        format!("{}{}{}{}", prefix, open_delim, body, close_delim)
    };
    Some(Correction::replace(arr_start, arr_end, &replacement))
}

fn invalid_percent_array_context(node: &ruby_prism::ArrayNode, source: &str) -> bool {
    let _ = (node, source);
    false
}

fn invalid_percent_array_contents(elements: &[Node], source: &str) -> bool {
    elements.iter().any(|e| {
        let c = match string_content(e, source) {
            Some(c) => c,
            None => return true,
        };
        c.contains(' ') || !std::str::from_utf8(c.as_bytes()).is_ok()
    })
}

impl WordArray {
    // nothing else
}

fn normalize_ruby_regex_local(pat: &str) -> String {
    let mut s = pat.to_string();
    if let Some(inner) = s
        .strip_prefix("(?-mix:")
        .and_then(|x| x.strip_suffix(")"))
    {
        s = inner.to_string();
    }
    s = s.replace(r"\p{Word}", r"\w");
    s
}

crate::register_cop!("Style/WordArray", |cfg| {
    let cop_config = cfg.get_cop_config("Style/WordArray");
    let style = match cop_config
        .and_then(|c| c.raw.get("EnforcedStyle"))
        .and_then(|v| v.as_str())
    {
        Some("brackets") => EnforcedStyle::Brackets,
        _ => EnforcedStyle::Percent,
    };
    let min_size = cop_config
        .and_then(|c| c.raw.get("MinSize"))
        .and_then(|v| v.as_u64())
        .unwrap_or(2) as usize;
    let word_regex = cop_config
        .and_then(|c| c.raw.get("WordRegex"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| r"\A(?:\w|\w-\w|\n|\t)+\z".into());
    let word_regex = normalize_ruby_regex_local(&word_regex);

    // Read preferred delimiters from Style/PercentLiteralDelimiters config
    let preferred_delimiters = cfg
        .get_cop_config("Style/PercentLiteralDelimiters")
        .and_then(|c| c.raw.get("PreferredDelimiters"))
        .and_then(|v| v.get("default"))
        .and_then(|v| v.as_str())
        .unwrap_or("()")
        .to_string();

    Some(Box::new(WordArray::with_config(
        style,
        min_size,
        word_regex,
        preferred_delimiters,
    )))
});
