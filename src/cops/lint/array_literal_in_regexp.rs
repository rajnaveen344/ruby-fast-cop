//! Lint/ArrayLiteralInRegexp - flag `/#{[a, b, c]}/` interpolations.
//!
//! Inside a regexp, an interpolated array becomes its `to_s` form
//! (`["a", "b"]`) which is rarely intended. RuboCop suggests a
//! character class or alternation depending on element lengths.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct ArrayLiteralInRegexp;

impl ArrayLiteralInRegexp {
    pub fn new() -> Self { Self }
}

const MSG_CHAR: &str = "Use a character class instead of interpolating an array in a regexp.";
const MSG_ALT: &str = "Use alternation instead of interpolating an array in a regexp.";
const MSG_UNK: &str = "Use alternation or a character class instead of interpolating an array in a regexp.";

impl Cop for ArrayLiteralInRegexp {
    fn name(&self) -> &'static str { "Lint/ArrayLiteralInRegexp" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut v = V { ctx, out: vec![] };
        v.visit(&result.node());
        v.out
    }
}

struct V<'a, 'b> { ctx: &'a CheckContext<'b>, out: Vec<Offense> }


fn chars_count(s: &str) -> usize {
    s.chars().count()
}

/// Mimic Ruby `Regexp.escape`. Escapes regex metachars.
fn regexp_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '.' | '\\' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$' | '#' | '-' => {
                out.push('\\');
                out.push(ch);
            }
            ' ' => { out.push('\\'); out.push(' '); }
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\x0c' => out.push_str("\\f"),
            '\x0b' => out.push_str("\\v"),
            _ => out.push(ch),
        }
    }
    out
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_interpolated_regular_expression_node(
        &mut self,
        node: &ruby_prism::InterpolatedRegularExpressionNode,
    ) {
        for part in node.parts().iter() {
            if let Node::EmbeddedStatementsNode { .. } = part {
                let es = part.as_embedded_statements_node().unwrap();
                self.handle_interpolation_with_source(&es);
            }
        }
    }
}

impl<'a, 'b> V<'a, 'b> {
    fn handle_interpolation_with_source(&mut self, es: &ruby_prism::EmbeddedStatementsNode) {
        let Some(stmts) = es.statements() else { return };
        let body: Vec<_> = stmts.body().iter().collect();
        if body.len() != 1 { return; }
        let last = &body[0];
        let Some(arr) = last.as_array_node() else { return };

        let elements: Vec<Node> = arr.elements().iter().collect();
        let es_loc = es.location();

        let mut values: Vec<String> = Vec::with_capacity(elements.len());
        let mut all_literal = true;
        for el in &elements {
            match self.literal_value_src(el) {
                Some(v) => values.push(v),
                None => { all_literal = false; break; }
            }
        }

        if !all_literal {
            self.out.push(self.ctx.offense_with_range(
                "Lint/ArrayLiteralInRegexp",
                MSG_UNK,
                Severity::Warning,
                es_loc.start_offset(), es_loc.end_offset(),
            ));
            return;
        }

        let is_char_class = values.iter().all(|v| chars_count(v) == 1);
        let escaped: Vec<String> = values.iter().map(|v| regexp_escape(v)).collect();
        let (msg, replacement) = if is_char_class {
            (MSG_CHAR, format!("[{}]", escaped.join("")))
        } else {
            (MSG_ALT, format!("(?:{})", escaped.join("|")))
        };

        self.out.push(
            self.ctx.offense_with_range(
                "Lint/ArrayLiteralInRegexp",
                msg,
                Severity::Warning,
                es_loc.start_offset(), es_loc.end_offset(),
            ).with_correction(Correction::replace(es_loc.start_offset(), es_loc.end_offset(), replacement)),
        );
    }

    fn literal_value_src(&self, node: &Node) -> Option<String> {
        match node {
            Node::StringNode { .. } => {
                let s = node.as_string_node().unwrap();
                let bytes = s.unescaped();
                std::str::from_utf8(bytes).ok().map(|s| s.to_string())
            }
            Node::SymbolNode { .. } => {
                let s = node.as_symbol_node().unwrap();
                let bytes = s.unescaped();
                std::str::from_utf8(bytes).ok().map(|s| s.to_string())
            }
            Node::IntegerNode { .. } | Node::FloatNode { .. } => {
                let l = node.location();
                Some(self.ctx.source[l.start_offset()..l.end_offset()].to_string())
            }
            Node::TrueNode { .. } => Some("true".to_string()),
            Node::FalseNode { .. } => Some("false".to_string()),
            Node::NilNode { .. } => Some("nil".to_string()),
            _ => None,
        }
    }
}

crate::register_cop!("Lint/ArrayLiteralInRegexp", |_cfg| Some(Box::new(ArrayLiteralInRegexp::new())));
