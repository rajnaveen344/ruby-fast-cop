//! Style/SymbolArray - Prefer %i or %I for arrays of symbols.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/symbol_array.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/SymbolArray";
const PERCENT_MSG: &str = "Use `%i` or `%I` for an array of symbols.";
const DELIMITERS: &[char] = &['[', ']', '(', ')'];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle { Percent, Brackets }

pub struct SymbolArray {
    style: EnforcedStyle,
    min_size: usize,
    /// Preferred delimiters for `%i`/`%I`, e.g. "()" or "[]"
    preferred_delimiters: String,
}

impl Default for SymbolArray {
    fn default() -> Self { Self { style: EnforcedStyle::Percent, min_size: 2, preferred_delimiters: "()".to_string() } }
}

impl SymbolArray {
    pub fn new() -> Self { Self::default() }
    pub fn with_config(style: EnforcedStyle, min_size: usize, preferred_delimiters: String) -> Self {
        Self { style, min_size, preferred_delimiters }
    }
}

impl Cop for SymbolArray {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor { cop: self, ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a, 'b> {
    cop: &'a SymbolArray,
    ctx: &'a CheckContext<'b>,
    offenses: Vec<Offense>,
}

impl<'a, 'b> Visit<'_> for Visitor<'a, 'b> {
    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        let elements: Vec<Node> = node.elements().iter().collect();
        self.cop.check_array(node, &elements, self.ctx, &mut self.offenses);
        ruby_prism::visit_array_node(self, node);
    }
}

impl SymbolArray {
    fn check_array(
        &self,
        node: &ruby_prism::ArrayNode,
        elements: &[Node],
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
    ) {
        let (is_percent_symbol, is_percent) = opening_kind(node, ctx.source);

        // bracketed array of all-symbols?
        let all_symbols = !is_percent && !elements.is_empty()
            && elements.iter().all(|e| matches!(e,
                Node::SymbolNode { .. } | Node::InterpolatedSymbolNode { .. }
            ));

        if all_symbols {
            // check_bracketed_array
            if self.style != EnforcedStyle::Percent { return; }
            if elements.len() < self.min_size { return; }
            if complex_content(elements, ctx.source) { return; }
            let loc = node.location();
            let src_start = loc.start_offset();
            let src_end = loc.end_offset();
            let replacement = build_percent_array(elements, ctx.source, src_start, src_end, &self.preferred_delimiters);
            let correction = Correction::replace(src_start, src_end, replacement);
            offenses.push(ctx.offense(COP_NAME, PERCENT_MSG, Severity::Convention, &loc)
                .with_correction(correction));
        } else if is_percent_symbol {
            // check_percent_array
            if self.style == EnforcedStyle::Brackets || complex_content_in_percent(elements, ctx.source) {
                // Build bracketed rendering
                let bracketed = build_bracketed(elements, ctx.source);
                let has_newline = ctx.source[node.location().start_offset()..node.location().end_offset()].contains('\n');
                let src_start = node.location().start_offset();
                let src_end = node.location().end_offset();
                if has_newline {
                    let open = node.opening_loc().unwrap();
                    let msg = "Use an array literal `[...]` for an array of symbols.".to_string();
                    let replacement = build_bracketed_multiline(elements, ctx.source, src_start, src_end);
                    let correction = Correction::replace(src_start, src_end, replacement);
                    offenses.push(ctx.offense_with_range(COP_NAME, &msg, Severity::Convention, open.start_offset(), open.end_offset())
                        .with_correction(correction));
                } else {
                    let msg = format!("Use `{}` for an array of symbols.", bracketed);
                    let correction = Correction::replace(src_start, src_end, bracketed);
                    offenses.push(ctx.offense(COP_NAME, &msg, Severity::Convention, &node.location())
                        .with_correction(correction));
                }
            }
        }
    }
}

/// Build `%i(...)` or `%I(...)` replacement, preserving multiline structure.
fn build_percent_array(
    elements: &[Node],
    source: &str,
    array_start: usize,
    array_end: usize,
    preferred_delimiters: &str,
) -> String {
    let open_ch = preferred_delimiters.chars().next().unwrap_or('(');
    let close_ch = preferred_delimiters.chars().nth(1).unwrap_or(')');
    let has_newline = has_newline_between_elements(elements, source, array_start, array_end);

    // Determine if %I needed (any element has escape sequences in unescaped content)
    let needs_capital = elements.iter().any(|e| {
        if let Some(sym) = e.as_symbol_node() {
            let unesc = String::from_utf8_lossy(sym.unescaped()).to_string();
            let raw_src = &source[e.location().start_offset()..e.location().end_offset()];
            // needs %I if content has \n, \t, \r, \\ that differ from raw
            unesc.contains('\n') || unesc.contains('\t') || unesc.contains('\r') ||
            // or if source has a quoted sym with escape: `:"..."` or `:'\n'`
            (raw_src.starts_with(":\"") || raw_src.starts_with(":'"))
                && (raw_src.contains("\\n") || raw_src.contains("\\t") || raw_src.contains("\\r"))
        } else {
            false
        }
    });
    let prefix = if needs_capital { "%I" } else { "%i" };

    if !has_newline {
        // Single-line: %i(one two three)
        let parts: Vec<String> = elements.iter().map(|e| symbol_percent_content(e, source)).collect();
        format!("{}{}{}{}",  prefix, open_ch, parts.join(" "), close_ch)
    } else {
        // Multiline: preserve line structure.
        // Find which source line each element starts on (relative to array_start).
        // Then group by line, join with space within a line, join with \n between lines.
        // Opening bracket might be on its own line (like `[\n  :foo,\n  :bar\n]`).

        // Get all element start lines (absolute lines in source).
        if elements.is_empty() {
            return format!("{}{}{}",  prefix, open_ch, close_ch);
        }

        // Check if opening bracket is on its own line (nothing after `[` except whitespace/newline).
        let bracket_line_src = {
            let line_start = source[..array_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
            let line_end = source[array_start..].find('\n').map(|p| array_start + p).unwrap_or(source.len());
            &source[line_start..line_end]
        };
        let after_bracket = &bracket_line_src[bracket_line_src.find('[').map(|p| p + 1).unwrap_or(0)..];
        let bracket_on_own_line = after_bracket.trim().is_empty();

        let elem_lines: Vec<usize> = elements.iter().map(|e| {
            let off = e.location().start_offset();
            source[..off].chars().filter(|&c| c == '\n').count()
        }).collect();

        // Closing bracket line
        let close_off = array_end - 1; // last char should be `]`
        let close_line = source[..close_off].chars().filter(|&c| c == '\n').count();

        // Check if closing bracket on its own line
        let close_line_src = {
            let line_start = source[..close_off].rfind('\n').map(|p| p + 1).unwrap_or(0);
            &source[line_start..]
        };
        let close_on_own_line = close_line_src.trim_start().starts_with(']');

        if bracket_on_own_line {
            // Format: %i[\n  foo\n  bar\n  baz\n]
            // Each element on its own line with same indentation.
            let mut result = format!("{}{}", prefix, open_ch);
            let mut prev_line = elem_lines[0].wrapping_sub(1);
            for (i, e) in elements.iter().enumerate() {
                let el = elem_lines[i];
                if el != prev_line {
                    // new line — get the indentation of this element
                    let off = e.location().start_offset();
                    let line_start = source[..off].rfind('\n').map(|p| p + 1).unwrap_or(0);
                    let indent = &source[line_start..off];
                    result.push('\n');
                    result.push_str(indent);
                } else {
                    result.push(' ');
                }
                result.push_str(&symbol_percent_content(e, source));
                prev_line = el;
            }
            if close_on_own_line {
                // add newline + indentation of close bracket
                let line_start = source[..close_off].rfind('\n').map(|p| p + 1).unwrap_or(0);
                let indent = &source[line_start..close_off];
                result.push('\n');
                result.push_str(indent);
            }
            result.push(close_ch);
            result
        } else {
            // Opening bracket on same line as first element(s).
            // Format: %i(one\ntwo three\nfour)
            let mut result = format!("{}{}", prefix, open_ch);
            let mut prev_line = usize::MAX;
            for (i, e) in elements.iter().enumerate() {
                let el = elem_lines[i];
                if prev_line == usize::MAX {
                    // first element
                } else if el != prev_line {
                    result.push('\n');
                } else {
                    result.push(' ');
                }
                result.push_str(&symbol_percent_content(e, source));
                prev_line = el;
            }
            result.push(close_ch);
            result
        }
    }
}

/// Build `[...]` replacement from multiline `%i[...]` or `%I[...]`.
/// Preserves each element on its own line, adds `:` prefix and `,` (except last).
fn build_bracketed_multiline(
    elements: &[Node],
    source: &str,
    array_start: usize,
    array_end: usize,
) -> String {
    // Get the original percent array source to understand structure.
    let array_src = &source[array_start..array_end];

    // Find opening/closing delimiter position.
    // Opening: `%i[` or `%I[` etc — opening_loc covers just `%i[`.
    // We need to figure out if elements are on separate lines or same line.
    if elements.is_empty() {
        return "[]".to_string();
    }

    // Check if there's a newline between opener and first element.
    let first_off = elements[0].location().start_offset();
    let open_end = {
        // First non-whitespace byte after array start is the end of `%i[`
        let src_prefix = &source[array_start..first_off];
        array_start + src_prefix.len()
    };
    let _ = open_end;
    let between_open_and_first = &source[array_start + 3..first_off]; // `%i[` is 3 chars
    let opener_on_own_line = between_open_and_first.contains('\n');

    if opener_on_own_line {
        // Format: [\n  :one,\n  :two\n]
        let mut result = String::from("[");
        let last_idx = elements.len() - 1;
        for (i, e) in elements.iter().enumerate() {
            let off = e.location().start_offset();
            let line_start = source[..off].rfind('\n').map(|p| p + 1).unwrap_or(0);
            let indent = &source[line_start..off];
            result.push('\n');
            result.push_str(indent);
            result.push_str(&symbol_literal_for(e, source));
            if i < last_idx {
                result.push(',');
            }
        }
        // closing bracket: find indentation of `)`
        let close_off = array_end - 1;
        let line_start = source[..close_off].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let indent = &source[line_start..close_off];
        result.push('\n');
        result.push_str(indent);
        result.push(']');
        result
    } else {
        // Single-line
        build_bracketed(elements, source)
    }
}

/// Get the percent-array content for a single symbol element.
/// For `%i`: just the raw identifier. For escapes, use re-escaped form.
fn symbol_percent_content(node: &Node, source: &str) -> String {
    if let Some(sym) = node.as_symbol_node() {
        let unescaped = String::from_utf8_lossy(sym.unescaped()).to_string();
        // Re-escape \n \t \r \\ for use inside %i/%I
        if unescaped.contains('\n') || unescaped.contains('\t') || unescaped.contains('\r') {
            let escaped = unescaped
                .replace('\\', "\\\\")
                .replace('\n', "\\n")
                .replace('\t', "\\t")
                .replace('\r', "\\r");
            return escaped;
        }
        return unescaped;
    }
    if let Some(_dsym) = node.as_interpolated_symbol_node() {
        // Use source text between `: and the closing quote — strip outer colon+quotes
        let s = &source[node.location().start_offset()..node.location().end_offset()];
        return strip_dsym_for_percent(s);
    }
    String::new()
}

/// Check if there's a newline between elements (not within an element's own source).
fn has_newline_between_elements(elements: &[Node], source: &str, array_start: usize, array_end: usize) -> bool {
    if elements.is_empty() {
        return source[array_start..array_end].contains('\n');
    }
    // Between array start and first element
    let first_start = elements[0].location().start_offset();
    if source[array_start..first_start].contains('\n') { return true; }
    // Between each pair of consecutive elements
    for w in elements.windows(2) {
        let gap_start = w[0].location().end_offset();
        let gap_end = w[1].location().start_offset();
        if source[gap_start..gap_end].contains('\n') { return true; }
    }
    // Between last element and closing bracket
    let last_end = elements[elements.len()-1].location().end_offset();
    if source[last_end..array_end].contains('\n') { return true; }
    false
}

fn strip_dsym_for_percent(s: &str) -> String {
    // `:"..."` → strip `:"` and `"`; `:'...'` → strip `:'` and `'`
    let s = s.trim_start_matches(':');
    if s.starts_with('"') {
        s.trim_start_matches('"').trim_end_matches('"').to_string()
    } else if s.starts_with('\'') {
        s.trim_start_matches('\'').trim_end_matches('\'').to_string()
    } else {
        s.to_string()
    }
}

fn opening_kind(node: &ruby_prism::ArrayNode, source: &str) -> (bool, bool) {
    // returns (is_percent_symbol, is_percent_any)
    match node.opening_loc() {
        Some(loc) => {
            let s = &source[loc.start_offset()..loc.end_offset()];
            let sym = s.starts_with("%i") || s.starts_with("%I");
            let any = sym || s.starts_with("%w") || s.starts_with("%W");
            (sym, any)
        }
        None => (false, false),
    }
}

/// Complex content for bracketed array of symbols: any sym has space, or delimiters
/// outside balanced pairs.
fn complex_content(elements: &[Node], source: &str) -> bool {
    elements.iter().any(|e| {
        // Source of the sym element (e.g. `:foo` for bracketed, `foo` for %i)
        let src = &source[e.location().start_offset()..e.location().end_offset()];
        // A symbol like `:[`, `:]`, `:(`, `:)` (or in %i: `[`, `]`, `(`, `)`) is allowed.
        if DELIMITERS.iter().any(|d| src == &format!(":{}", d)) { return false; }
        if src.len() == 1 && DELIMITERS.iter().any(|d| src == &d.to_string()) { return false; }

        let content = symbol_content(e, source);
        let without_balanced = strip_balanced_delims(&content);
        content.contains(' ') || DELIMITERS.iter().any(|d| without_balanced.contains(*d))
    })
}

/// For percent array (`%i[...]`), check if the children need to be converted back to brackets.
/// Mirrors `invalid_percent_array_contents?` = `complex_content?(node)`.
fn complex_content_in_percent(elements: &[Node], source: &str) -> bool {
    complex_content(elements, source)
}

/// Extract symbol content (without leading `:`).
fn symbol_content(node: &Node, source: &str) -> String {
    if let Some(sym) = node.as_symbol_node() {
        // unescaped
        let bytes = sym.unescaped();
        return String::from_utf8_lossy(bytes).to_string();
    }
    if let Some(dsym) = node.as_interpolated_symbol_node() {
        // Use source text between opening and closing, strip `:` if present
        let s = &source[dsym.location().start_offset()..dsym.location().end_offset()];
        return s.trim_start_matches(':').trim_matches('"').trim_matches('\'').to_string();
    }
    String::new()
}

fn strip_balanced_delims(s: &str) -> String {
    // Remove `[...]` and `(...)` where inner has no whitespace or nested delims — per RuboCop regex:
    // /(\[[^\s\[\]]*\])|(\([^\s()]*\))/
    let re = regex::Regex::new(r"(\[[^\s\[\]]*\])|(\([^\s()]*\))").unwrap();
    re.replace_all(s, "").to_string()
}

/// Build bracketed replacement from %i/%I children.
fn build_bracketed(elements: &[Node], source: &str) -> String {
    if elements.is_empty() { return "[]".to_string(); }
    let mut parts = Vec::with_capacity(elements.len());
    for e in elements {
        parts.push(symbol_literal_for(e, source));
    }
    format!("[{}]", parts.join(", "))
}

fn symbol_literal_for(node: &Node, source: &str) -> String {
    if let Node::InterpolatedSymbolNode { .. } = node {
        // `:"..."` style, pass source verbatim but ensure leading `:"` and trailing `"`.
        let s = &source[node.location().start_offset()..node.location().end_offset()];
        return format!(":\"{}\"", strip_dsym_quotes(s));
    }
    let content = symbol_content(node, source);
    to_symbol_literal(&content)
}

fn strip_dsym_quotes(s: &str) -> String {
    // strip leading `:` and outer quotes if present (from colon-variant dsym)
    let s = s.trim_start_matches(':');
    let s = s.trim_start_matches('"').trim_end_matches('"');
    s.to_string()
}

fn to_symbol_literal(content: &str) -> String {
    if symbol_without_quote(content) {
        format!(":{}", content)
    } else {
        // Quote with single quotes, escaping as needed
        let needs_double = content.contains('\'') || content.contains('\n') || content.contains('\t');
        if needs_double {
            let escaped = content.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\t', "\\t");
            format!(":\"{}\"", escaped)
        } else {
            format!(":'{}'", content)
        }
    }
}

fn symbol_without_quote(s: &str) -> bool {
    // method name
    if regex::Regex::new(r"^[a-zA-Z_]\w*[!?]?$").unwrap().is_match(s) { return true; }
    // @/@@var
    if regex::Regex::new(r"^@@?[a-zA-Z_]\w*$").unwrap().is_match(s) { return true; }
    // $var
    if regex::Regex::new(r"^\$[1-9]\d*$").unwrap().is_match(s) { return true; }
    if regex::Regex::new(r"^\$[a-zA-Z_]\w*$").unwrap().is_match(s) { return true; }
    // Redefinable operators - simpler: exact match
    const OPS: &[&str] = &[
        "|", "^", "&", "<=>", "==", "===", "=~", ">", ">=", "<", "<=", "<<", ">>",
        "+", "-", "*", "/", "%", "**", "~", "+@", "-@", "[]", "[]=", "`", "!", "!=", "!~",
    ];
    OPS.contains(&s)
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style: String, min_size: usize }
impl Default for Cfg {
    fn default() -> Self { Self { enforced_style: String::new(), min_size: 2 } }
}

crate::register_cop!("Style/SymbolArray", |cfg| {
    let c: Cfg = cfg.typed("Style/SymbolArray");
    let style = match c.enforced_style.as_str() {
        "brackets" => EnforcedStyle::Brackets,
        _ => EnforcedStyle::Percent,
    };
    // Read preferred delimiters for %i/%I from Style/PercentLiteralDelimiters cross-cop config.
    let preferred_delimiters = {
        let pld = cfg.get_cop_config("Style/PercentLiteralDelimiters");
        let delims = pld.and_then(|c| c.raw.get("PreferredDelimiters"));
        let v = delims.and_then(|d| d.get("%i").or_else(|| d.get("default")));
        v.and_then(|v| v.as_str()).unwrap_or("()").to_string()
    };
    Some(Box::new(SymbolArray::with_config(style, c.min_size, preferred_delimiters)))
});
