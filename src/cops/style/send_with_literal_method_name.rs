//! Style/SendWithLiteralMethodName - flag `obj.public_send(:foo)` → `obj.foo`.
//!
//! Ported from `lib/rubocop/cop/style/send_with_literal_method_name.rb`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::Node;

const RESERVED_WORDS: &[&str] = &[
    "BEGIN", "END", "alias", "and", "begin", "break", "case", "class", "def",
    "defined?", "do", "else", "elsif", "end", "ensure", "false", "for", "if",
    "in", "module", "next", "nil", "not", "or", "redo", "rescue", "retry",
    "return", "self", "super", "then", "true", "undef", "unless", "until",
    "when", "while", "yield",
];

#[derive(Default)]
pub struct SendWithLiteralMethodName {
    allow_send: bool,
}

impl SendWithLiteralMethodName {
    pub fn with_allow_send(allow_send: bool) -> Self {
        Self { allow_send }
    }
}

impl Cop for SendWithLiteralMethodName {
    fn name(&self) -> &'static str { "Style/SendWithLiteralMethodName" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if !matches!(method.as_ref(), "public_send" | "send" | "__send__") {
            return vec![];
        }
        // AllowSend: when true, only flag public_send.
        if self.allow_send && method.as_ref() != "public_send" {
            return vec![];
        }

        let args_node = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let args: Vec<_> = args_node.arguments().iter().collect();
        if args.is_empty() { return vec![]; }

        let first = &args[0];
        let method_name = match extract_literal_name(first) {
            Some(n) => n,
            None => return vec![],
        };

        if !is_valid_method_name(&method_name) { return vec![]; }
        if RESERVED_WORDS.contains(&method_name.as_str()) { return vec![]; }

        // Selector start to end of full call node (matches RuboCop offense_range)
        let sel_loc = match node.message_loc() {
            Some(l) => l,
            None => return vec![],
        };
        let sel_start = sel_loc.start_offset();
        let node_loc = node.location();
        let node_end = node_loc.end_offset();

        let msg = format!("Use `{}` method call directly instead.", method_name);

        let mut edits: Vec<Edit> = Vec::new();
        if args.len() == 1 {
            // Replace selector..node_end with method_name
            edits.push(Edit {
                start_offset: sel_start,
                end_offset: node_end,
                replacement: method_name.clone(),
            });
        } else {
            // Replace just the selector with method_name
            edits.push(Edit {
                start_offset: sel_start,
                end_offset: sel_loc.end_offset(),
                replacement: method_name.clone(),
            });
            // Remove from first arg start up to second arg start (deletes :foo + comma + space)
            let first_loc = first.location();
            let second_loc = args[1].location();
            edits.push(Edit {
                start_offset: first_loc.start_offset(),
                end_offset: second_loc.start_offset(),
                replacement: String::new(),
            });
        }

        let offense = ctx
            .offense_with_range(self.name(), &msg, self.severity(), sel_start, node_end)
            .with_correction(Correction { edits });
        vec![offense]
    }
}

fn extract_literal_name(n: &Node) -> Option<String> {
    match n {
        Node::SymbolNode { .. } => {
            let s = n.as_symbol_node().unwrap();
            let bytes = s.unescaped();
            std::str::from_utf8(bytes).ok().map(|x| x.to_string())
        }
        Node::StringNode { .. } => {
            let s = n.as_string_node().unwrap();
            let bytes = s.unescaped();
            std::str::from_utf8(bytes).ok().map(|x| x.to_string())
        }
        _ => None,
    }
}

fn is_valid_method_name(name: &str) -> bool {
    let mut chars = name.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    if !(first.is_ascii_alphabetic() || first == '_') { return false; }
    let mut last = first;
    for c in chars {
        last = c;
        if !(c.is_ascii_alphanumeric() || c == '_' || c == '!' || c == '?') {
            return false;
        }
    }
    // `!`/`?` only allowed at the end
    let middle: String = name.chars().take(name.chars().count() - 1).collect();
    if middle.contains('!') || middle.contains('?') { return false; }
    let _ = last;
    true
}

crate::register_cop!("Style/SendWithLiteralMethodName", |cfg| {
    let allow_send = cfg
        .get_cop_config("Style/SendWithLiteralMethodName")
        .and_then(|c| c.raw.get("AllowSend").and_then(|v| v.as_bool()))
        .unwrap_or(true);
    Some(Box::new(SendWithLiteralMethodName::with_allow_send(allow_send)))
});
