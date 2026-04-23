//! Metrics/AbcSize cop.
//!
//! Ported from https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/metrics/abc_size.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::abc_size::{calculate, AbcResult};
use crate::helpers::allowed_methods::is_method_allowed;
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

pub struct AbcSize {
    max: f64,
    discount_repeated_attributes: bool,
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<String>,
}

impl AbcSize {
    pub fn with_config(
        max: f64,
        count_repeated_attributes: bool,
        allowed_methods: Vec<String>,
        allowed_patterns: Vec<String>,
    ) -> Self {
        Self {
            max,
            discount_repeated_attributes: !count_repeated_attributes,
            allowed_methods,
            allowed_patterns,
        }
    }
}

impl Cop for AbcSize {
    fn name(&self) -> &'static str { "Metrics/AbcSize" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut v = Visitor { cop: self, ctx, offenses: &mut offenses };
        v.visit(&result.node());
        offenses
    }
}

struct Visitor<'a> {
    cop: &'a AbcSize,
    ctx: &'a CheckContext<'a>,
    offenses: &'a mut Vec<Offense>,
}

impl Visitor<'_> {
    fn check(&mut self, method_name: &str, body: Option<Node>, header_start: usize, header_end: usize) {
        if is_method_allowed(&self.cop.allowed_methods, &self.cop.allowed_patterns, method_name, None) {
            return;
        }
        let body = match body { Some(b) => b, None => return };
        let result: AbcResult = calculate(&body, self.cop.discount_repeated_attributes);
        if result.score <= self.cop.max { return; }
        let msg = format!(
            "Assignment Branch Condition size for `{}` is too high. [{} {}/{}]",
            method_name,
            result.vector(),
            format_g(result.score),
            format_g(self.cop.max),
        );
        self.offenses.push(
            self.ctx.offense_with_range(
                "Metrics/AbcSize",
                &msg,
                Severity::Convention,
                header_start,
                header_end,
            ),
        );
    }
}

impl<'a> Visit<'a> for Visitor<'_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let name = node_name!(node).to_string();
        let loc = node.location();
        self.check(&name, node.body().map(Into::into), loc.start_offset(), loc.end_offset());
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if node.receiver().is_none() && node_name!(node) == "define_method" {
            if let Some(block) = node.block() {
                if let Some(bn) = block.as_block_node() {
                    let arg_name = node.arguments().and_then(|args| {
                        args.arguments().iter().next().and_then(|a| {
                            if let Some(sym) = a.as_symbol_node() {
                                Some(String::from_utf8_lossy(sym.unescaped().as_ref()).to_string())
                            } else if let Some(s) = a.as_string_node() {
                                Some(String::from_utf8_lossy(s.unescaped().as_ref()).to_string())
                            } else { None }
                        })
                    });
                    if let Some(name) = arg_name {
                        let loc = node.location();
                        self.check(&name, bn.body(), loc.start_offset(), loc.end_offset());
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

/// Ruby's `%.4g` — up to 4 significant digits, trailing zeros stripped.
fn format_g(v: f64) -> String {
    // Integer fast path.
    if v.fract() == 0.0 && v.abs() < 1e16 {
        let i = v as i64;
        if (i as f64) == v { return i.to_string(); }
    }
    // Compute 4-sig-digit representation.
    if v == 0.0 { return "0".to_string(); }
    let abs = v.abs();
    let exp = abs.log10().floor() as i32;
    let digits_after = (3 - exp).max(0) as usize;
    let formatted = format!("{:.*}", digits_after, v);
    // Strip trailing zeros and possibly the trailing decimal point.
    if formatted.contains('.') {
        let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_string()
    } else {
        formatted
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct AbcSizeCfg {
    max: f64,
    count_repeated_attributes: bool,
    allowed_methods: Vec<String>,
    ignored_methods: Vec<String>,
    excluded_methods: Vec<String>,
    allowed_patterns: Vec<String>,
    ignored_patterns: Vec<String>,
}

impl Default for AbcSizeCfg {
    fn default() -> Self {
        Self {
            max: 17.0,
            count_repeated_attributes: true,
            allowed_methods: Vec::new(),
            ignored_methods: Vec::new(),
            excluded_methods: Vec::new(),
            allowed_patterns: Vec::new(),
            ignored_patterns: Vec::new(),
        }
    }
}

crate::register_cop!("Metrics/AbcSize", |cfg| {
    let c: AbcSizeCfg = cfg.typed("Metrics/AbcSize");
    let mut allowed_methods = c.allowed_methods;
    allowed_methods.extend(c.ignored_methods);
    allowed_methods.extend(c.excluded_methods);
    let mut allowed_patterns = c.allowed_patterns;
    allowed_patterns.extend(c.ignored_patterns);
    Some(Box::new(AbcSize::with_config(
        c.max,
        c.count_repeated_attributes,
        allowed_methods,
        allowed_patterns,
    )))
});
