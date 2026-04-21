//! Style/SymbolArray - Prefer %i or %I for arrays of symbols.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/symbol_array.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/SymbolArray";
const PERCENT_MSG: &str = "Use `%i` or `%I` for an array of symbols.";
const DELIMITERS: &[char] = &['[', ']', '(', ')'];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle { Percent, Brackets }

pub struct SymbolArray {
    style: EnforcedStyle,
    min_size: usize,
}

impl Default for SymbolArray {
    fn default() -> Self { Self { style: EnforcedStyle::Percent, min_size: 2 } }
}

impl SymbolArray {
    pub fn new() -> Self { Self::default() }
    pub fn with_config(style: EnforcedStyle, min_size: usize) -> Self {
        Self { style, min_size }
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
            offenses.push(ctx.offense(COP_NAME, PERCENT_MSG, Severity::Convention, &loc));
        } else if is_percent_symbol {
            // check_percent_array
            if self.style == EnforcedStyle::Brackets || complex_content_in_percent(elements, ctx.source) {
                // Build bracketed rendering
                let bracketed = build_bracketed(elements, ctx.source);
                let has_newline = ctx.source[node.location().start_offset()..node.location().end_offset()].contains('\n');
                if has_newline {
                    let open = node.opening_loc().unwrap();
                    let msg = "Use an array literal `[...]` for an array of symbols.".to_string();
                    offenses.push(ctx.offense_with_range(COP_NAME, &msg, Severity::Convention, open.start_offset(), open.end_offset()));
                } else {
                    let msg = format!("Use `{}` for an array of symbols.", bracketed);
                    offenses.push(ctx.offense(COP_NAME, &msg, Severity::Convention, &node.location()));
                }
            }
        }
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
    Some(Box::new(SymbolArray::with_config(style, c.min_size)))
});
