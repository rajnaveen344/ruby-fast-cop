//! Lint/RedundantTypeConversion cop

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

const CONVERSION_METHODS: &[&str] = &[
    "to_s", "to_sym", "to_i", "to_f", "to_r", "to_c", "to_a", "to_h", "to_set", "to_d",
];

fn is_typed_method_for(conversion: &str, method: &str) -> bool {
    if conversion == "to_s" {
        matches!(method, "inspect" | "to_json")
    } else {
        false
    }
}

#[derive(Default)]
pub struct RedundantTypeConversion;

impl RedundantTypeConversion {
    pub fn new() -> Self {
        Self
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
        let method_name = node_name!(node);
        let method_str = method_name.as_ref();
        if !CONVERSION_METHODS.contains(&method_str) { return; }
        let receiver = match node.receiver() { Some(r) => r, None => return };

        if (method_str == "to_h" || method_str == "to_set") && self.has_block_or_block_pass(node) { return; }
        if let Some(args) = node.arguments() {
            if args.arguments().iter().count() > 0 { return; }
        }

        if !check_receiver_redundant(method_str, &receiver, self.ctx) { return; }
        let msg_loc = match node.message_loc() { Some(loc) => loc, None => return };

        let offense = self.ctx.offense_with_range(
            "Lint/RedundantTypeConversion",
            &format!("Redundant `{}` detected.", method_str),
            Severity::Warning,
            msg_loc.start_offset(),
            msg_loc.end_offset(),
        );

        if let Some(dot) = node.call_operator_loc() {
            let remove_end = node.closing_loc().map_or(msg_loc.end_offset(), |c| c.end_offset());
            self.offenses.push(offense.with_correction(Correction::delete(dot.start_offset(), remove_end)));
        } else {
            self.offenses.push(offense);
        }
    }

    fn has_block_or_block_pass(&self, node: &ruby_prism::CallNode) -> bool {
        node.block().is_some()
            || node.arguments().map_or(false, |args|
                args.arguments().iter().any(|a| matches!(a, ruby_prism::Node::BlockArgumentNode { .. })))
    }
}

impl Visit<'_> for RedundantTypeConversionVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

fn check_receiver_redundant(method: &str, receiver: &ruby_prism::Node, ctx: &CheckContext) -> bool {
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

fn is_constructor_receiver(method: &str, receiver: &ruby_prism::Node, ctx: &CheckContext) -> bool {
    match receiver {
        ruby_prism::Node::CallNode { .. } => {
            let call = receiver.as_call_node().unwrap();
            // Check for exception: false
            if constructor_suppresses_exceptions(&call) {
                return false;
            }

            let recv_method = node_name!(call);
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

fn is_kernel_receiver(receiver: &Option<ruby_prism::Node>) -> bool {
    match receiver {
        None => true,
        Some(node) => is_constant_named(node, "Kernel"),
    }
}

fn is_constant_named(node: &ruby_prism::Node, name: &str) -> bool {
    match node {
        ruby_prism::Node::ConstantReadNode { .. } => {
            let c = node.as_constant_read_node().unwrap();
            let const_name = node_name!(c);
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

fn is_chained_conversion(method: &str, receiver: &ruby_prism::Node) -> bool {
    receiver.as_call_node().map_or(false, |call|
        node_name!(call) == method)
}

fn is_chained_to_typed_method(method: &str, receiver: &ruby_prism::Node) -> bool {
    receiver.as_call_node().map_or(false, |call|
        is_typed_method_for(method, &node_name!(call)))
}
