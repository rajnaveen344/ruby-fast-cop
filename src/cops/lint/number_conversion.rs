//! Lint/NumberConversion - prefer kernel conversion methods over to_i/to_f/etc.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/lint/number_conversion.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use crate::node_name;
use regex::Regex;

const COP: &str = "Lint/NumberConversion";

const CONVERSION_METHODS: &[&str] = &["to_i", "to_f", "to_c", "to_r"];

pub struct NumberConversion {
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<Regex>,
    ignored_classes: Vec<String>,
}

impl NumberConversion {
    pub fn new(
        allowed_methods: Vec<String>,
        allowed_patterns: Vec<String>,
        ignored_classes: Vec<String>,
    ) -> Self {
        let pats: Vec<Regex> = allowed_patterns.iter().filter_map(|p| Regex::new(p).ok()).collect();
        Self { allowed_methods, allowed_patterns: pats, ignored_classes }
    }

    fn kernel_for(method: &str) -> &'static str {
        match method {
            "to_i" => "Integer",
            "to_f" => "Float",
            "to_c" => "Complex",
            "to_r" => "Rational",
            _ => "",
        }
    }

    fn replacement_call(method: &str, arg_src: &str) -> String {
        let kernel = Self::kernel_for(method);
        if method == "to_i" {
            format!("{}({}, 10)", kernel, arg_src)
        } else {
            format!("{}({})", kernel, arg_src)
        }
    }

    fn block_replacement(method: &str) -> String {
        let kernel = Self::kernel_for(method);
        if method == "to_i" {
            format!("{{ |i| {}(i, 10) }}", kernel)
        } else {
            format!("{{ |i| {}(i) }}", kernel)
        }
    }

    /// Walk down receiver chain to leftmost atom; if it's a ConstantReadNode/ConstantPathNode
    /// matching ignored_classes, return true.
    fn receiver_chain_in_ignored(&self, recv: &ruby_prism::Node) -> bool {
        let mut node_opt: Option<ruby_prism::Node> = Some(receiver_clone(recv));
        while let Some(n) = node_opt {
            if let Some(c) = n.as_constant_read_node() {
                let name = String::from_utf8_lossy(c.name().as_slice()).to_string();
                return self.ignored_classes.iter().any(|s| s == &name);
            }
            if let Some(cp) = n.as_constant_path_node() {
                // Use leftmost name of the path (e.g. ::Foo::Bar -> "Foo" if walking down)
                // Or just check the rightmost (top-most name): take name of the path itself.
                let name_id = cp.name();
                if let Some(name_id) = name_id {
                    let name = String::from_utf8_lossy(name_id.as_slice()).to_string();
                    if self.ignored_classes.iter().any(|s| s == &name) { return true; }
                }
                // walk further into parent
                if let Some(p) = cp.parent() { node_opt = Some(p); continue; }
                return false;
            }
            if let Some(call) = n.as_call_node() {
                if let Some(r) = call.receiver() { node_opt = Some(r); continue; }
                return false;
            }
            return false;
        }
        false
    }

    /// True if receiver is a numeric literal (or unary minus on one).
    fn receiver_is_literal_number(recv: &ruby_prism::Node) -> bool {
        if recv.as_integer_node().is_some() { return true; }
        if recv.as_float_node().is_some() { return true; }
        if recv.as_rational_node().is_some() { return true; }
        if recv.as_imaginary_node().is_some() { return true; }
        false
    }

    fn allowed_method_check(&self, recv: &ruby_prism::Node) -> bool {
        // If receiver is a CallNode, check its method name against allowed_methods/patterns.
        if let Some(c) = recv.as_call_node() {
            let m = node_name!(c);
            if self.allowed_methods.iter().any(|s| s == &m) { return true; }
            if self.allowed_patterns.iter().any(|r| r.is_match(&m)) { return true; }
        }
        false
    }
}

fn receiver_clone<'a>(_node: &ruby_prism::Node<'a>) -> ruby_prism::Node<'a> {
    // We avoid moving by re-resolving via parent chain; here we simply call node.location()
    // But we actually need an owned Node to walk. Since ruby_prism::Node is Copy in 1.9?
    // Let's try: just unsafe-transmute borrow to owned by relying on the fact that Node fields
    // are pointer + parser ref, both Copy.
    unsafe { std::ptr::read(_node as *const ruby_prism::Node) }
}

impl Cop for NumberConversion {
    fn name(&self) -> &'static str { COP }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // 1. Direct conversion call: receiver.to_X (no args)
        let method = node_name!(node);
        let m_str: &str = &method;
        if CONVERSION_METHODS.contains(&m_str) {
            // Skip if any arguments
            if node.arguments().is_some() { return vec![]; }
            if node.block().is_some() { return vec![]; }
            let receiver = match node.receiver() { Some(r) => r, None => return vec![] };

            // Skip number literals
            if Self::receiver_is_literal_number(&receiver) { return vec![]; }
            // Skip kernel numeric constructors: Integer(...), Float(...), Complex(...), Rational(...)
            if let Some(c) = receiver.as_call_node() {
                if c.receiver().is_none() {
                    let n = node_name!(c);
                    if n == "Integer" || n == "Float" || n == "Complex" || n == "Rational" {
                        return vec![];
                    }
                }
                // Skip if receiver is itself a conversion method on something — RuboCop emits
                // only the inner offense for `var.to_i.to_f` (one offense, not two).
                let n = node_name!(c);
                let n_str: &str = &n;
                if CONVERSION_METHODS.contains(&n_str) { return vec![]; }
            }
            // Skip ignored classes
            if self.receiver_chain_in_ignored(&receiver) { return vec![]; }
            // Skip allowed methods (the receiver method name)
            if self.allowed_method_check(&receiver) { return vec![]; }

            let recv_loc = receiver.location();
            let recv_src = ctx.source[recv_loc.start_offset()..recv_loc.end_offset()].to_string();

            let start = node.location().start_offset();
            let end = node.location().end_offset();
            let orig_src = ctx.source[start..end].to_string();
            let replacement = Self::replacement_call(&method, &recv_src);
            // For message use original source but normalized: replace `&.` or `.` separator;
            // RuboCop's message uses the original source with `.` (not `&.`).
            let orig_msg_src = orig_src.replace("&.", ".");
            let message = format!(
                "Replace unsafe number conversion with number class parsing, instead of using `{}`, use stricter `{}`.",
                orig_msg_src, replacement
            );
            let mut off = ctx.offense_with_range(COP, &message, Severity::Warning, start, end);
            off.correction = Some(Correction::replace(start, end, &replacement));
            return vec![off];
        }

        // 2. Symbol form &:to_X passed as block argument: foo.map(&:to_i)
        if let Some(blk) = node.block() {
            if let Some(ba) = blk.as_block_argument_node() {
                if let Some(expr) = ba.expression() {
                    if let Some(sym) = expr.as_symbol_node() {
                        if let Some(value_loc) = sym.value_loc() {
                            let sym_name = std::str::from_utf8(value_loc.as_slice()).unwrap_or("");
                            if CONVERSION_METHODS.contains(&sym_name) {
                                return self.emit_block_form(node, ctx, sym_name, "&:");
                            }
                        }
                    }
                }
            }
        }

        // 3. try(:to_X) or send(:to_X) — symbol as positional arg.
        let m_outer = node_name!(node);
        if m_outer == "try" || m_outer == "send" {
            if let Some(args) = node.arguments() {
                let arg_list: Vec<_> = args.arguments().iter().collect();
                if arg_list.len() == 1 {
                    if let Some(sym) = arg_list[0].as_symbol_node() {
                        if let Some(value_loc) = sym.value_loc() {
                            let sym_name = std::str::from_utf8(value_loc.as_slice()).unwrap_or("");
                            if CONVERSION_METHODS.contains(&sym_name) {
                                return self.emit_block_form(node, ctx, sym_name, ":");
                            }
                        }
                    }
                }
            }
        }

        vec![]
    }
}

impl NumberConversion {
    fn emit_block_form(
        &self,
        outer: &ruby_prism::CallNode,
        ctx: &CheckContext,
        sym_name: &str,
        prefix: &str,
    ) -> Vec<Offense> {
        let start = outer.location().start_offset();
        let end = outer.location().end_offset();
        let block_repl = Self::block_replacement(sym_name);
        let orig_arg = format!("{}{}", prefix, sym_name);
        let message = format!(
            "Replace unsafe number conversion with number class parsing, instead of using `{}`, use stricter `{}`.",
            orig_arg, block_repl
        );

        // Build correction: replace the arguments list (including parens or surrounding space)
        // with " { |i| Kernel(i[, 10]) }".
        // Strategy: replace from end of message_loc through end of outer with block form.
        let msg_loc = match outer.message_loc() { Some(l) => l, None => return vec![] };
        let after_msg = msg_loc.end_offset();
        let correction = Correction::replace(after_msg, end, format!(" {}", block_repl));

        let mut off = ctx.offense_with_range(COP, &message, Severity::Warning, start, end);
        off.correction = Some(correction);
        vec![off]
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allowed_methods: Option<Vec<String>>,
    allowed_patterns: Option<Vec<String>>,
    ignored_classes: Option<Vec<String>>,
}

crate::register_cop!("Lint/NumberConversion", |cfg| {
    let c: Cfg = cfg.typed("Lint/NumberConversion");
    Some(Box::new(NumberConversion::new(
        c.allowed_methods.unwrap_or_default(),
        c.allowed_patterns.unwrap_or_default(),
        c.ignored_classes.unwrap_or_default(),
    )))
});
