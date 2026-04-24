//! Style/RedundantArgument cop
//!
//! Checks for a redundant argument passed to certain methods (configured via `Methods`).
//! Example: `"foo".chomp("\n")` -> `"foo".chomp` because `"\n"` is the default.
//!
//! Ported from `lib/rubocop/cop/style/redundant_argument.rb`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;
use std::collections::HashMap;

const NO_RECEIVER_METHODS: &[&str] = &["exit", "exit!"];

#[derive(Default)]
pub struct RedundantArgument {
    /// Map of method name → expected default-argument source text (Ruby-inspected form).
    /// e.g. `chomp` -> `"\n"` (the `.inspect` form: with double quotes + escapes).
    methods: HashMap<String, String>,
}

impl RedundantArgument {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from a serde_yaml::Value mapping. Values are "inspected" to match
    /// Ruby's `.inspect` form (strings wrapped in double quotes with escapes,
    /// numbers as decimal, booleans as lowercased, nil as "nil").
    pub fn with_methods(methods: HashMap<String, String>) -> Self {
        Self { methods }
    }

    /// Inspect a yaml value to produce Ruby-like "inspected" source form.
    pub fn inspect_yaml_value(v: &serde_yaml::Value) -> Option<String> {
        match v {
            serde_yaml::Value::String(s) => Some(ruby_string_inspect(s)),
            serde_yaml::Value::Number(n) => Some(n.to_string()),
            serde_yaml::Value::Bool(b) => Some(b.to_string()),
            serde_yaml::Value::Null => Some("nil".to_string()),
            _ => None,
        }
    }
}

/// Produce a Ruby `String#inspect` equivalent: double-quoted with control chars escaped.
fn ruby_string_inspect(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            '\x07' => out.push_str("\\a"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            '\x0b' => out.push_str("\\v"),
            '\x1b' => out.push_str("\\e"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                out.push_str(&format!("\\x{:02X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Check if the source form of the argument matches the configured redundant form.
fn argument_matches(arg_source: &str, redundant: &str) -> bool {
    // Direct match (already inspected form, e.g. `"\n"` or `2` or `true`).
    if arg_source == redundant {
        return true;
    }
    // Also try: treat arg as a raw literal (if redundant is a number/bool/nil,
    // arg source would be the same). For strings, convert arg's source to inspect form.
    // Strings can be single-quoted or double-quoted in Ruby.
    // RuboCop logic: if value responds_to(:value) → use node.value, else source.
    // Then if value is AST::Node → use source, else if exclude_cntrl_character? → inspect, else to_s.
    // Essentially: strings are compared in inspect form.
    // Our redundant is always inspect form.
    false
}

/// Normalize an argument node's source to inspect form for comparison.
/// Returns None if we can't determine (e.g., dynamic string).
fn normalize_arg_for_match<'a>(arg_source: &'a str, node: &Node, source: &str) -> Option<String> {
    let _ = arg_source;
    let _ = source;
    match node {
        Node::StringNode { .. } => {
            let sn = node.as_string_node().unwrap();
            let bytes = sn.unescaped();
            let s = std::str::from_utf8(bytes).ok()?;
            Some(ruby_string_inspect(s))
        }
        Node::IntegerNode { .. } | Node::FloatNode { .. } | Node::TrueNode { .. } | Node::NilNode { .. } | Node::FalseNode { .. } | Node::SymbolNode { .. } => {
            // For non-string literals, source text directly matches (e.g. `0`, `true`, `nil`).
            let l = node.location();
            Some(source[l.start_offset()..l.end_offset()].to_string())
        }
        _ => None,
    }
}

impl Cop for RedundantArgument {
    fn name(&self) -> &'static str {
        "Style/RedundantArgument"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);

        // Must have receiver unless in NO_RECEIVER_METHODS.
        let has_receiver = node.receiver().is_some();
        if !has_receiver && !NO_RECEIVER_METHODS.contains(&method.as_ref()) {
            return vec![];
        }

        // Exactly one argument
        let Some(args_node) = node.arguments() else {
            return vec![];
        };
        let args: Vec<_> = args_node.arguments().iter().collect();
        if args.len() != 1 {
            return vec![];
        }

        // Lookup redundant-arg source form
        let Some(redundant) = self.methods.get(method.as_ref()) else {
            return vec![];
        };

        let arg = &args[0];
        let arg_loc = arg.location();
        let arg_source = &ctx.source[arg_loc.start_offset()..arg_loc.end_offset()];

        // Compute normalized form for matching.
        let matched = if let Some(norm) = normalize_arg_for_match(arg_source, arg, ctx.source) {
            norm == *redundant
        } else {
            argument_matches(arg_source, redundant)
        };

        if !matched {
            return vec![];
        }

        // Offense range: if parenthesized: from `(` to `)` inclusive. Else surrounding
        // whitespace around arg source range.
        let (range_start, range_end) = if let (Some(open), Some(close)) =
            (node.opening_loc(), node.closing_loc())
        {
            (open.start_offset(), close.end_offset())
        } else {
            // No parens — include leading whitespace
            let bytes = ctx.source.as_bytes();
            let mut s = arg_loc.start_offset();
            while s > 0 && (bytes[s - 1] == b' ' || bytes[s - 1] == b'\t') {
                s -= 1;
            }
            (s, arg_loc.end_offset())
        };

        let msg = format!(
            "Argument {} is redundant because it is implied by default.",
            arg_source
        );
        let offense = ctx
            .offense_with_range(self.name(), &msg, self.severity(), range_start, range_end)
            .with_correction(Correction::delete(range_start, range_end));
        vec![offense]
    }
}

crate::register_cop!("Style/RedundantArgument", |cfg| {
    let cop_config = cfg.get_cop_config("Style/RedundantArgument");
    let mut methods: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Some(v) = cop_config.and_then(|c| c.raw.get("Methods")) {
        if let Some(map) = v.as_mapping() {
            for (k, val) in map.iter() {
                if let Some(key_str) = k.as_str() {
                    if let Some(inspected) = RedundantArgument::inspect_yaml_value(val) {
                        methods.insert(key_str.to_string(), inspected);
                    }
                }
            }
        }
    }
    Some(Box::new(RedundantArgument::with_methods(methods)))
});
