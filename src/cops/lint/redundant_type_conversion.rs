//! Lint/RedundantTypeConversion - Checks for redundant uses of type conversion methods.
//!
//! Detects calls like `"foo".to_s`, `42.to_i`, `[].to_a`, etc. where the receiver is already
//! the target type. Also detects chained same-conversion (`foo.to_s.to_s`) and conversion
//! after typed methods (`foo.inspect.to_s`).
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/redundant_type_conversion.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

const CONVERSION_METHODS: &[&str] = &[
    "to_s", "to_sym", "to_i", "to_f", "to_r", "to_c", "to_a", "to_h", "to_set", "to_d",
];

/// Methods that are expected to return a specific type, making a further conversion redundant.
fn is_typed_method_for(conversion: &str, method: &str) -> bool {
    if conversion == "to_s" {
        matches!(method, "inspect" | "to_json")
    } else {
        false
    }
}

pub struct RedundantTypeConversion;

impl RedundantTypeConversion {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RedundantTypeConversion {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for RedundantTypeConversion {
    fn name(&self) -> &'static str {
        "Lint/RedundantTypeConversion"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = RedundantTypeConversionVisitor {
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct RedundantTypeConversionVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> RedundantTypeConversionVisitor<'a> {
    fn check_call(&mut self, node: &ruby_prism::CallNode) {
        let method_name = String::from_utf8_lossy(node.name().as_slice());
        let method_str = method_name.as_ref();

        // Only handle our conversion methods
        if !CONVERSION_METHODS.contains(&method_str) {
            return;
        }

        // Must have a receiver (bare `to_s` is not flagged)
        let receiver = match node.receiver() {
            Some(r) => r,
            None => return,
        };

        // For to_h and to_set: don't flag if there's a block
        if method_str == "to_h" || method_str == "to_set" {
            if self.has_block_or_block_pass(node) {
                return;
            }
        }

        // The node must not have non-empty arguments
        // `foo.to_s(2)` is a base conversion, not redundant
        // But `foo.to_s()` IS redundant (empty args)
        if let Some(args) = node.arguments() {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if !arg_list.is_empty() {
                return;
            }
        }

        // Check if this is a "redundant" conversion
        let is_redundant = check_receiver_redundant(method_str, &receiver, self.ctx);

        if !is_redundant {
            return;
        }

        // Offense location: the method name (message_loc)
        let msg_loc = match node.message_loc() {
            Some(loc) => loc,
            None => return,
        };

        let message = format!("Redundant `{}` detected.", method_str);

        let offense = self.ctx.offense_with_range(
            "Lint/RedundantTypeConversion",
            &message,
            Severity::Warning,
            msg_loc.start_offset(),
            msg_loc.end_offset(),
        );

        // Autocorrect: remove from the dot/&. through the end of the call (including parens)
        if let Some(dot) = node.call_operator_loc() {
            let remove_start = dot.start_offset();
            let remove_end = if let Some(closing) = node.closing_loc() {
                closing.end_offset()
            } else {
                msg_loc.end_offset()
            };
            let correction = Correction::delete(remove_start, remove_end);
            self.offenses.push(offense.with_correction(correction));
        } else {
            self.offenses.push(offense);
        }
    }

    /// Check if to_h/to_set has a block attached.
    fn has_block_or_block_pass(&self, node: &ruby_prism::CallNode) -> bool {
        // Check for block_pass argument (&:baz)
        if let Some(args) = node.arguments() {
            for arg in args.arguments().iter() {
                if matches!(arg, ruby_prism::Node::BlockArgumentNode { .. }) {
                    return true;
                }
            }
        }

        // Check if the CallNode has a block
        if node.block().is_some() {
            return true;
        }

        false
    }
}

impl Visit<'_> for RedundantTypeConversionVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

/// Check if the receiver makes this conversion redundant.
/// Recursively unwraps parentheses.
fn check_receiver_redundant(method: &str, receiver: &ruby_prism::Node, ctx: &CheckContext) -> bool {
    // Unwrap parentheses first
    match receiver {
        ruby_prism::Node::ParenthesesNode { .. } => {
            let paren = receiver.as_parentheses_node().unwrap();
            if let Some(body) = paren.body() {
                match &body {
                    ruby_prism::Node::StatementsNode { .. } => {
                        let stmts = body.as_statements_node().unwrap();
                        let body_nodes: Vec<_> = stmts.body().iter().collect();
                        if body_nodes.len() == 1 {
                            return check_receiver_redundant(method, &body_nodes[0], ctx);
                        }
                    }
                    _ => return check_receiver_redundant(method, &body, ctx),
                }
            }
            false
        }
        _ => {
            is_literal_receiver(method, receiver)
                || is_constructor_receiver(method, receiver, ctx)
                || is_chained_conversion(method, receiver)
                || is_chained_to_typed_method(method, receiver)
        }
    }
}

/// Check if the receiver is a literal of the same type as the conversion.
fn is_literal_receiver(method: &str, receiver: &ruby_prism::Node) -> bool {
    match method {
        "to_s" => matches!(
            receiver,
            ruby_prism::Node::StringNode { .. }
                | ruby_prism::Node::InterpolatedStringNode { .. }
        ),
        "to_sym" => matches!(
            receiver,
            ruby_prism::Node::SymbolNode { .. }
                | ruby_prism::Node::InterpolatedSymbolNode { .. }
        ),
        "to_i" => matches!(receiver, ruby_prism::Node::IntegerNode { .. }),
        "to_f" => matches!(receiver, ruby_prism::Node::FloatNode { .. }),
        "to_r" => matches!(receiver, ruby_prism::Node::RationalNode { .. }),
        "to_c" => matches!(receiver, ruby_prism::Node::ImaginaryNode { .. }),
        "to_a" => matches!(receiver, ruby_prism::Node::ArrayNode { .. }),
        "to_h" => matches!(receiver, ruby_prism::Node::HashNode { .. }),
        _ => false,
    }
}

/// Check if the receiver is a constructor producing the same type.
fn is_constructor_receiver(method: &str, receiver: &ruby_prism::Node, ctx: &CheckContext) -> bool {
    match receiver {
        ruby_prism::Node::CallNode { .. } => {
            let call = receiver.as_call_node().unwrap();
            // Check for exception: false
            if constructor_suppresses_exceptions(&call) {
                return false;
            }

            let recv_method = String::from_utf8_lossy(call.name().as_slice());
            let recv_method_str = recv_method.as_ref();

            // Check for Kernel methods: String(), Integer(), Float(), etc.
            if call.receiver().is_none() || is_kernel_receiver(&call.receiver()) {
                return is_kernel_constructor_for(method, recv_method_str);
            }

            // Check for Class.new or Class[] patterns
            if let Some(class_recv) = call.receiver() {
                if is_class_constructor_for(method, recv_method_str, &class_recv) {
                    return true;
                }
            }

            false
        }
        // Hash.new { |k,v| ... } - a BlockNode wrapping Hash.new call
        ruby_prism::Node::BlockNode { .. } => {
            if method != "to_h" {
                return false;
            }
            let block = receiver.as_block_node().unwrap();
            // In Prism, look at the source to determine the call target
            let loc = block.location();
            let source_text = &ctx.source[loc.start_offset()..loc.end_offset()];
            let trimmed = source_text.trim_start();
            if trimmed.starts_with("Hash.new") || trimmed.starts_with("::Hash.new") {
                return true;
            }
            false
        }
        _ => false,
    }
}

/// Check if the call has `exception: false` in its arguments.
fn constructor_suppresses_exceptions(call: &ruby_prism::CallNode) -> bool {
    if let Some(args) = call.arguments() {
        for arg in args.arguments().iter() {
            if let ruby_prism::Node::KeywordHashNode { .. } = &arg {
                let kw_hash = arg.as_keyword_hash_node().unwrap();
                for elem in kw_hash.elements().iter() {
                    if let ruby_prism::Node::AssocNode { .. } = &elem {
                        let assoc = elem.as_assoc_node().unwrap();
                        let key = assoc.key();
                        let value = assoc.value();
                        if let ruby_prism::Node::SymbolNode { .. } = &key {
                            let sym = key.as_symbol_node().unwrap();
                            let sym_name =
                                String::from_utf8_lossy(sym.unescaped().as_ref());
                            if sym_name.as_ref() == "exception" {
                                if let ruby_prism::Node::FalseNode { .. } = &value {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

/// Check if the receiver is `Kernel`, `::Kernel`, or nil (bare call).
fn is_kernel_receiver(receiver: &Option<ruby_prism::Node>) -> bool {
    match receiver {
        None => true,
        Some(node) => is_constant_named(node, "Kernel"),
    }
}

/// Check if a node is a constant with the given name, possibly qualified (::Name, etc.).
fn is_constant_named(node: &ruby_prism::Node, name: &str) -> bool {
    match node {
        ruby_prism::Node::ConstantReadNode { .. } => {
            let c = node.as_constant_read_node().unwrap();
            let const_name = String::from_utf8_lossy(c.name().as_slice());
            const_name.as_ref() == name
        }
        ruby_prism::Node::ConstantPathNode { .. } => {
            let path = node.as_constant_path_node().unwrap();
            let child_name = path
                .name()
                .map(|n| String::from_utf8_lossy(n.as_slice()).to_string())
                .unwrap_or_default();
            if child_name != name {
                return false;
            }
            match path.parent() {
                None => true, // ::Name (root-scoped)
                Some(parent) => is_constant_named(&parent, "Kernel"),
            }
        }
        _ => false,
    }
}

/// Check if a Kernel method name matches the conversion method.
fn is_kernel_constructor_for(conversion_method: &str, kernel_method: &str) -> bool {
    match conversion_method {
        "to_s" => kernel_method == "String",
        "to_i" => kernel_method == "Integer",
        "to_f" => kernel_method == "Float",
        "to_d" => kernel_method == "BigDecimal",
        "to_r" => kernel_method == "Rational",
        "to_c" => kernel_method == "Complex",
        "to_a" => kernel_method == "Array",
        "to_h" => kernel_method == "Hash",
        _ => false,
    }
}

/// Check if a Class.method matches a constructor for the conversion.
fn is_class_constructor_for(
    conversion_method: &str,
    call_method: &str,
    class_node: &ruby_prism::Node,
) -> bool {
    match conversion_method {
        "to_s" => call_method == "new" && is_constant_named(class_node, "String"),
        "to_a" => {
            (call_method == "new" || call_method == "[]")
                && is_constant_named(class_node, "Array")
        }
        "to_h" => {
            (call_method == "new" || call_method == "[]")
                && is_constant_named(class_node, "Hash")
        }
        "to_set" => {
            (call_method == "new" || call_method == "[]")
                && is_constant_named(class_node, "Set")
        }
        _ => false,
    }
}

/// Check if the conversion is chained on the same conversion method.
fn is_chained_conversion(method: &str, receiver: &ruby_prism::Node) -> bool {
    match receiver {
        ruby_prism::Node::CallNode { .. } => {
            let call = receiver.as_call_node().unwrap();
            let recv_method = String::from_utf8_lossy(call.name().as_slice());
            recv_method.as_ref() == method
        }
        _ => false,
    }
}

/// Check if the conversion is chained on a typed method.
fn is_chained_to_typed_method(method: &str, receiver: &ruby_prism::Node) -> bool {
    match receiver {
        ruby_prism::Node::CallNode { .. } => {
            let call = receiver.as_call_node().unwrap();
            let recv_method = String::from_utf8_lossy(call.name().as_slice());
            is_typed_method_for(method, recv_method.as_ref())
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check_source_with_cops;

    fn check(source: &str) -> Vec<Offense> {
        let cops: Vec<Box<dyn crate::cops::Cop>> =
            vec![Box::new(RedundantTypeConversion::new())];
        check_source_with_cops(source, "test.rb", &cops)
    }

    #[test]
    fn detects_string_to_s() {
        let offenses = check("'string'.to_s");
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("to_s"));
    }

    #[test]
    fn allows_integer_to_s() {
        let offenses = check("1.to_s");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn detects_integer_to_i() {
        let offenses = check("42.to_i");
        assert_eq!(offenses.len(), 1);
        assert!(offenses[0].message.contains("to_i"));
    }

    #[test]
    fn detects_chained_to_s() {
        let offenses = check("foo.to_s.to_s");
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn allows_foo_to_s() {
        let offenses = check("foo.to_s");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn allows_self_to_s() {
        let offenses = check("self.to_s");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn allows_bare_to_s() {
        let offenses = check("to_s");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn detects_inspect_to_s() {
        let offenses = check("foo.inspect.to_s");
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn allows_exception_false() {
        let offenses = check("Integer(\"number\", exception: false).to_i");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn detects_integer_constructor() {
        let offenses = check("Integer(42).to_i");
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn detects_array_literal_to_a() {
        let offenses = check("[1, 2, 3].to_a");
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn detects_hash_literal_to_h() {
        let offenses = check("{ foo: bar }.to_h");
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn allows_hash_to_h_with_block() {
        let offenses = check("{ key: value }.to_h { |key, value| [foo(key), bar(value)] }");
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn detects_set_new_to_set() {
        let offenses = check("Set.new([1, 2, 3]).to_set");
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn allows_set_to_set_with_block() {
        let offenses = check("Set[1, 2, 3].to_set { |item| foo(item) }");
        assert_eq!(offenses.len(), 0);
    }
}
