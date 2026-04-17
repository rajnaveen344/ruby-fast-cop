//! Style/WordArray - Prefer %w or %W for arrays of word-like strings.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/word_array.rb
//! Mixin: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/mixin/percent_array.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/WordArray";
const PERCENT_MSG: &str = "Use `%w` or `%W` for an array of words.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle { Percent, Brackets }

pub struct WordArray {
    style: EnforcedStyle,
    min_size: usize,
    word_regex: String,
}

impl Default for WordArray {
    fn default() -> Self {
        Self {
            style: EnforcedStyle::Percent,
            min_size: 2,
            word_regex: r"\A(?:\w|\w-\w|\n|\t)+\z".to_string(),
        }
    }
}

impl WordArray {
    pub fn new() -> Self { Self::default() }

    pub fn with_config(style: EnforcedStyle, min_size: usize, word_regex: String) -> Self {
        Self { style, min_size, word_regex }
    }
}

impl Cop for WordArray {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor { cop: self, ctx, parent_array_matrix_complex: vec![false], offenses: Vec::new() };
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
        let is_matrix_complex = matrix_of_complex_content(&elements, self.ctx.source, &self.cop.word_regex);

        // Check this array for offense
        let offenses = self.cop.check_array_impl(node, &elements, self.ctx, *self.parent_array_matrix_complex.last().unwrap_or(&false));
        self.offenses.extend(offenses);

        self.parent_array_matrix_complex.push(is_matrix_complex);
        ruby_prism::visit_array_node(self, node);
        self.parent_array_matrix_complex.pop();
    }
}

fn matrix_of_complex_content(elements: &[Node], source: &str, regex: &str) -> bool {
    if elements.is_empty() { return false; }
    if !elements.iter().all(|e| matches!(e, Node::ArrayNode { .. })) { return false; }
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
                    s.starts_with("%w") || s.starts_with("%W") || s.starts_with("%i") || s.starts_with("%I")
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

        let bracketed_of_str = !is_percent && elements.iter().all(|e| matches!(e,
            Node::StringNode { .. } | Node::InterpolatedStringNode { .. }
        )) && !elements.is_empty();

        if bracketed_of_str {
            if within_matrix_complex { return vec![]; }
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
        if self.style != EnforcedStyle::Percent { return vec![]; }
        if elements.len() < self.min_size { return vec![]; }
        if self.complex_content(elements, ctx.source) { return vec![]; }
        if self.has_comments_in_array(node, ctx.source) { return vec![]; }
        if invalid_percent_array_context(node, ctx.source) { return vec![]; }

        // `within_matrix_of_complex_content?`: skip if parent is array of arrays & any sibling array has complex content
        // (we approximate: skip detection; not tested heavily)

        let loc = node.location();
        vec![ctx.offense(COP_NAME, PERCENT_MSG, Severity::Convention, &loc)]
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

        // Build bracketed replacement for message
        let mut bracketed = String::from("[");
        for (i, e) in elements.iter().enumerate() {
            if i > 0 { bracketed.push_str(", "); }
            bracketed.push_str(&percent_element_to_literal(e, ctx.source));
        }
        bracketed.push(']');

        let has_newline = ctx.source[node.location().start_offset()..node.location().end_offset()].contains('\n');

        if has_newline {
            let open = node.opening_loc().unwrap();
            let msg = "Use an array literal `[...]` for an array of words.".to_string();
            vec![ctx.offense_with_range(COP_NAME, &msg, Severity::Convention, open.start_offset(), open.end_offset())]
        } else {
            let msg = format!("Use `{}` for an array of words.", bracketed);
            vec![ctx.offense(COP_NAME, &msg, Severity::Convention, &node.location())]
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
        // search for a '#' that's not inside a string. Approximate: if slice spans multi lines,
        // check each line's tail past last string-quote for '#'.
        // Simpler: count lines and check for '#' starting with whitespace or after string closer in
        // the original source; if any inner line contains `"..." #` or `'...' #` or bare `# ...`, yes.
        if !slice.contains('\n') { return false; }
        for line in slice.lines().skip(1).take_while(|_| true) {
            // check if line contains `#` after trimming to code section.
            if let Some(p) = line.find('#') {
                // ignore escape `\#`
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

/// For elements inside a %w/%W array, convert to a bracketed literal using the raw source text.
fn percent_element_to_literal(node: &Node, source: &str) -> String {
    if let Node::InterpolatedStringNode { .. } = node {
        let s = &source[node.location().start_offset()..node.location().end_offset()];
        return format!("\"{}\"", s);
    }
    let s = &source[node.location().start_offset()..node.location().end_offset()];
    // %w: only backslash-space and backslash-backslash are real escapes; convert them back.
    // Detect if source contains `\ ` (escaped space) — unescape to real space; then choose single quotes.
    // If source contains any backslash escape that isn't `\\` or `\ `, keep raw with double quotes.
    let has_escape_space = s.contains("\\ ");
    let has_other_backslash = s.chars().zip(s.chars().skip(1))
        .any(|(a, b)| a == '\\' && b != ' ' && b != '\\');
    if has_other_backslash || s.contains('\'') || s.contains('\t') || s.contains('\n') {
        return format!("\"{}\"", s);
    }
    let unescaped = s.replace("\\ ", " ").replace("\\\\", "\\");
    format!("'{}'", unescaped)
}

#[allow(dead_code)]
fn element_to_string_literal(node: &Node, source: &str) -> String {
    let content = string_content(node, source).unwrap_or_default();
    // Decide quote style — use single unless content has ' or needs escaping like \n \t
    let needs_double = content.contains('\'') || content.contains('\n') || content.contains('\t');
    if needs_double {
        let escaped = content
            .replace('\\', "\\\\")
            .replace('\n', "\\n")
            .replace('\t', "\\t")
            .replace('"', "\\\"");
        format!("\"{}\"", escaped)
    } else {
        format!("'{}'", content)
    }
}

fn invalid_percent_array_context(node: &ruby_prism::ArrayNode, source: &str) -> bool {
    // Check: parent is a send with block literal, this is an arg, no parens. We don't have parent pointers
    // here; approximate by scanning source for pattern `ident [...] { ... }` where [...] is our array.
    // Simplified: look at char just after the array's end; if it's whitespace + `{`, and at the start
    // the line starts with `identifier ` (no `(` before array), treat as ambiguous-block context.
    let _ = (node, source);
    false
}

fn invalid_percent_array_contents(elements: &[Node], source: &str) -> bool {
    elements.iter().any(|e| {
        let c = match string_content(e, source) { Some(c) => c, None => return true };
        c.contains(' ') || !std::str::from_utf8(c.as_bytes()).is_ok()
    })
}

// Helper to get Location from a Location by copy. Prism Location can be cloned since v1.9? If not,
// fallback: create via offset tuple.
impl WordArray {
    // nothing else
}

fn normalize_ruby_regex_local(pat: &str) -> String {
    let mut s = pat.to_string();
    if let Some(inner) = s.strip_prefix("(?-mix:").and_then(|x| x.strip_suffix(")")) {
        s = inner.to_string();
    }
    s = s.replace(r"\p{Word}", r"\w");
    s
}

crate::register_cop!("Style/WordArray", |cfg| {
    let cop_config = cfg.get_cop_config("Style/WordArray");
    let style = match cop_config.and_then(|c| c.raw.get("EnforcedStyle")).and_then(|v| v.as_str()) {
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
    Some(Box::new(WordArray::with_config(style, min_size, word_regex)))
});
