//! Style/MutableConstant cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Freeze mutable objects assigned to constants.";

#[derive(Debug, Clone, PartialEq)]
pub enum EnforcedStyle {
    Literals,
    Strict,
}

pub struct MutableConstant {
    enforced_style: EnforcedStyle,
}

fn loc_start(node: &Node) -> usize { node.location().start_offset() }
fn loc_end(node: &Node) -> usize { node.location().end_offset() }

fn first_line_end(source: &str, start: usize, end: usize) -> usize {
    match source[start..end].find('\n') {
        Some(pos) => start + source[start..start + pos].trim_end().len(),
        None => end,
    }
}

fn is_const_named(node: &Node, target: &str) -> bool {
    match node {
        Node::ConstantReadNode { .. } => {
            node_name!(node.as_constant_read_node().unwrap()) == target
        }
        Node::ConstantPathNode { .. } => {
            let cp = node.as_constant_path_node().unwrap();
            let name = cp.name().map(|n| String::from_utf8_lossy(n.as_slice()).to_string()).unwrap_or_default();
            name == target && cp.parent().is_none()
        }
        _ => false,
    }
}

impl MutableConstant {
    pub fn new(style: EnforcedStyle) -> Self {
        Self { enforced_style: style }
    }

    fn parse_shareable_constant_value(line: &str) -> Option<String> {
        let trimmed = line.trim();
        if !trimmed.starts_with('#') { return None; }
        let content = trimmed[1..].trim();
        if let Some((key, val)) = content.split_once(':') {
            if key.trim() == "shareable_constant_value" {
                return Some(val.trim().to_string());
            }
        }
        None
    }

    fn shareable_constant_value_active(source: &str, line_number: u32, ruby_version: f64) -> bool {
        if ruby_version < 3.0 { return false; }
        let mut most_recent: Option<String> = None;
        for (i, line) in source.lines().enumerate() {
            if (i + 1) as u32 > line_number { break; }
            if let Some(value) = Self::parse_shareable_constant_value(line) {
                most_recent = Some(value);
            }
        }
        most_recent.map_or(false, |v| matches!(v.as_str(), "literal" | "experimental_everything" | "experimental_copy"))
    }

    fn frozen_string_literal_value(source: &str) -> Option<bool> {
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            if !trimmed.starts_with('#') { break; }
            let content = trimmed[1..].trim();
            if content.starts_with("-*-") && content.ends_with("-*-") {
                let inner = content[3..content.len() - 3].trim();
                for part in inner.split(';') {
                    if let Some((key, val)) = part.trim().split_once(':') {
                        if key.trim().to_lowercase().replace(['-', '_'], "") == "frozenstringliteral" {
                            return Some(val.trim().eq_ignore_ascii_case("true"));
                        }
                    }
                }
                continue;
            }
            if let Some((key, val)) = content.split_once(':') {
                if key.trim().to_lowercase().replace(['-', '_'], "") == "frozenstringliteral" {
                    return Some(val.trim().eq_ignore_ascii_case("true"));
                }
            }
        }
        None
    }

    fn paren_wraps_range(node: &Node) -> bool {
        let paren = match node.as_parentheses_node() { Some(p) => p, None => return false };
        let body = match paren.body() { Some(b) => b, None => return false };
        if let Some(stmts) = body.as_statements_node() {
            let items: Vec<_> = stmts.body().iter().collect();
            if items.len() == 1 { return matches!(items[0], Node::RangeNode { .. }); }
        }
        matches!(body, Node::RangeNode { .. })
    }

    fn is_mutable_literal(node: &Node, ruby_version: f64) -> bool {
        match node {
            Node::ArrayNode { .. } | Node::HashNode { .. } | Node::StringNode { .. }
            | Node::InterpolatedStringNode { .. } | Node::XStringNode { .. }
            | Node::InterpolatedXStringNode { .. } | Node::SplatNode { .. } => true,
            Node::RegularExpressionNode { .. } | Node::InterpolatedRegularExpressionNode { .. }
            | Node::RangeNode { .. } => ruby_version < 3.0,
            Node::ParenthesesNode { .. } => Self::paren_wraps_range(node) && ruby_version < 3.0,
            _ => false,
        }
    }

    fn is_immutable_literal(node: &Node, ruby_version: f64) -> bool {
        match node {
            Node::IntegerNode { .. } | Node::FloatNode { .. } | Node::RationalNode { .. }
            | Node::ImaginaryNode { .. } | Node::SymbolNode { .. }
            | Node::InterpolatedSymbolNode { .. } | Node::NilNode { .. }
            | Node::TrueNode { .. } | Node::FalseNode { .. } => true,
            Node::RegularExpressionNode { .. } | Node::InterpolatedRegularExpressionNode { .. }
            | Node::RangeNode { .. } => ruby_version >= 3.0,
            Node::ParenthesesNode { .. } => Self::paren_wraps_range(node) && ruby_version >= 3.0,
            _ => false,
        }
    }

    fn is_frozen(node: &Node) -> bool {
        node.as_call_node().map_or(false, |call| {
            node_name!(call) == "freeze"
        })
    }

    fn operation_produces_immutable_object(node: &Node) -> bool {
        match node {
            Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. } => true,
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let method_name = node_name!(call);
                if method_name == "freeze" { return true; }
                if method_name == "new" {
                    if let Some(receiver) = call.receiver() {
                        if is_const_named(&receiver, "Struct") { return true; }
                    }
                }
                if matches!(method_name.as_ref(), "count" | "length" | "size") { return true; }
                if matches!(method_name.as_ref(), "+" | "-" | "*" | "**" | "/" | "%" | "<<" | "==" | "===" | "!=" | "<=" | ">=" | "<" | ">") {
                    if call.receiver().as_ref().map_or(false, |r| matches!(r, Node::IntegerNode { .. } | Node::FloatNode { .. })) { return true; }
                    if let Some(args) = call.arguments() {
                        if args.arguments().iter().any(|a| matches!(a, Node::IntegerNode { .. } | Node::FloatNode { .. })) { return true; }
                    }
                    if matches!(method_name.as_ref(), "==" | "===" | "!=" | "<=" | ">=" | "<" | ">") { return true; }
                }
                if method_name == "[]" {
                    return call.receiver().as_ref().map_or(false, |r| is_const_named(r, "ENV"));
                }
                false
            }
            Node::OrNode { .. } => {
                let left = node.as_or_node().unwrap().left();
                if let Some(call) = left.as_call_node() {
                    if node_name!(call) == "[]" {
                        return call.receiver().as_ref().map_or(false, |r| is_const_named(r, "ENV"));
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn has_real_interpolation(node: &Node) -> bool {
        match node {
            Node::InterpolatedStringNode { .. } => {
                node.as_interpolated_string_node().unwrap().parts().iter()
                    .any(|part| matches!(part, Node::EmbeddedStatementsNode { .. }) || Self::has_real_interpolation(&part))
            }
            Node::InterpolatedXStringNode { .. } => {
                node.as_interpolated_x_string_node().unwrap().parts().iter()
                    .any(|part| matches!(part, Node::EmbeddedStatementsNode { .. }))
            }
            Node::InterpolatedRegularExpressionNode { .. } => {
                node.as_interpolated_regular_expression_node().unwrap().parts().iter()
                    .any(|part| matches!(part, Node::EmbeddedStatementsNode { .. }))
            }
            _ => false,
        }
    }

    fn is_heredoc(node: &Node, source: &str) -> bool {
        matches!(node, Node::InterpolatedStringNode { .. } | Node::StringNode { .. })
            && source[loc_start(node)..].starts_with("<<")
    }

    fn is_string_concat(node: &Node, source: &str) -> bool {
        let (start, end) = (loc_start(node), loc_end(node));
        if start >= end || end > source.len() { return false; }
        if !matches!(node, Node::InterpolatedStringNode { .. } | Node::StringNode { .. }) { return false; }
        let text = &source[start..end];
        if text.contains("\\\n") { return true; }
        let mut found_quote = false;
        let mut found_space = false;
        for ch in text.chars() {
            if found_quote {
                if ch == ' ' || ch == '\t' { found_space = true; }
                else if found_space && (ch == '\'' || ch == '"') { return true; }
                else { found_quote = false; found_space = false; }
            }
            if ch == '\'' || ch == '"' { found_quote = true; found_space = false; }
        }
        false
    }

    fn check_value(&self, value: &Node, ctx: &CheckContext) -> Option<Offense> {
        let rv = ctx.target_ruby_version;
        let should_flag = match &self.enforced_style {
            EnforcedStyle::Literals => {
                let is_mutable = Self::is_mutable_literal(value, rv);
                let is_range_in_parens = Self::paren_wraps_range(value) && rv <= 2.7;
                is_mutable || is_range_in_parens
            }
            EnforcedStyle::Strict => {
                !Self::is_immutable_literal(value, rv) && !Self::operation_produces_immutable_object(value)
            }
        };
        if !should_flag { return None; }
        if self.is_frozen_by_magic_comment(value, ctx) { return None; }

        let (start, end) = (loc_start(value), loc_end(value));
        let offense_end = first_line_end(ctx.source, start, end);
        let offense = ctx.offense_with_range("Style/MutableConstant", MSG, Severity::Convention, start, offense_end);
        Some(offense.with_correction(self.build_correction(value, ctx.source, start, end)))
    }

    fn is_frozen_by_magic_comment(&self, value: &Node, ctx: &CheckContext) -> bool {
        let rv = ctx.target_ruby_version;
        match value {
            Node::StringNode { .. } | Node::InterpolatedStringNode { .. } => {
                let has_interp = Self::has_real_interpolation(value);
                let is_heredoc = Self::is_heredoc(value, ctx.source);
                let is_concat = Self::is_string_concat(value, ctx.source);

                if is_heredoc || is_concat {
                    if rv >= 3.0 && has_interp { return false; }
                    return Self::frozen_string_literal_value(ctx.source) == Some(true);
                }
                if has_interp {
                    if rv >= 3.0 { return false; }
                    return Self::frozen_string_literal_value(ctx.source) == Some(true);
                }
                false
            }
            _ => false,
        }
    }

    fn build_correction(&self, node: &Node, source: &str, start: usize, end: usize) -> Correction {
        if let Some(c) = self.correct_splat(node, source, start, end) { return c; }
        if node.as_array_node().is_some() {
            let src = &source[start..end];
            if !src.starts_with('[') && !src.starts_with('%') {
                return Correction::replace(start, end, format!("[{}].freeze", src));
            }
        }
        if self.requires_parentheses(node) {
            return Correction::replace(start, end, format!("({}).freeze", &source[start..end]));
        }
        Correction::insert(end, ".freeze")
    }

    fn correct_splat(&self, node: &Node, source: &str, start: usize, end: usize) -> Option<Correction> {
        let arr = node.as_array_node()?;
        let elements: Vec<_> = arr.elements().iter().collect();
        if elements.len() != 1 { return None; }
        let splat = elements[0].as_splat_node()?;
        let inner = splat.expression()?;
        let (is, ie) = (loc_start(&inner), loc_end(&inner));
        let inner_src = &source[is..ie];
        if matches!(inner, Node::RangeNode { .. }) {
            Some(Correction::replace(start, end, format!("({}).to_a.freeze", inner_src)))
        } else if Self::paren_wraps_range(&inner) {
            Some(Correction::replace(start, end, format!("{}.to_a.freeze", inner_src)))
        } else {
            None
        }
    }

    fn requires_parentheses(&self, node: &Node) -> bool {
        match node {
            Node::RangeNode { .. } => true,
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let method_name = node_name!(call);
                call.call_operator_loc().is_none()
                    && matches!(method_name.as_ref(), "+" | "-" | "*" | "**" | "/" | "%" | "<<" | ">>")
            }
            _ => false,
        }
    }
}

struct MutableConstantVisitor<'a> {
    cop: &'a MutableConstant,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    in_def: bool,
}

impl<'a> MutableConstantVisitor<'a> {
    fn check_assignment(&mut self, value: &Node, line_number: u32) {
        if self.in_def || MutableConstant::is_frozen(value) { return; }
        if MutableConstant::shareable_constant_value_active(self.ctx.source, line_number, self.ctx.target_ruby_version) { return; }
        if let Some(o) = self.cop.check_value(value, self.ctx) {
            self.offenses.push(o);
        }
    }
}

impl Visit<'_> for MutableConstantVisitor<'_> {
    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        let value = node.value();
        let loc = crate::offense::Location::from_offsets(
            self.ctx.source,
            node.location().start_offset(),
            node.location().end_offset(),
        );
        self.check_assignment(&value, loc.line);
        ruby_prism::visit_constant_write_node(self, node);
    }

    fn visit_constant_or_write_node(&mut self, node: &ruby_prism::ConstantOrWriteNode) {
        let value = node.value();
        let loc = crate::offense::Location::from_offsets(
            self.ctx.source,
            node.location().start_offset(),
            node.location().end_offset(),
        );
        self.check_assignment(&value, loc.line);
        ruby_prism::visit_constant_or_write_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let was_in_def = self.in_def;
        self.in_def = true;
        ruby_prism::visit_def_node(self, node);
        self.in_def = was_in_def;
    }
}

impl Cop for MutableConstant {
    fn name(&self) -> &'static str {
        "Style/MutableConstant"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = MutableConstantVisitor { cop: self, ctx, offenses: Vec::new(), in_def: false };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

crate::register_cop!("Style/MutableConstant", |cfg| {
    let cop_config = cfg.get_cop_config("Style/MutableConstant");
    let style = cop_config
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "strict" => EnforcedStyle::Strict,
            _ => EnforcedStyle::Literals,
        })
        .unwrap_or(EnforcedStyle::Literals);
    Some(Box::new(MutableConstant::new(style)))
});
