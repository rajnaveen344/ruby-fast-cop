//! Style/HashEachMethods - Prefer `each_key`/`each_value` over `keys.each`/`values.each`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/hash_each_methods.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{CallNode, Node, Visit};

/// Array converter methods that produce arrays from hashes.
const ARRAY_CONVERTER_METHODS: &[&str] = &[
    "assoc", "chunk", "flatten", "rassoc", "sort", "sort_by", "to_a",
];

pub struct HashEachMethods {
    allowed_receivers: Vec<String>,
}

impl HashEachMethods {
    pub fn new() -> Self {
        Self {
            allowed_receivers: Vec::new(),
        }
    }

    pub fn with_config(allowed_receivers: Vec<String>) -> Self {
        Self { allowed_receivers }
    }

    fn src<'a>(source: &'a str, loc: &ruby_prism::Location) -> &'a str {
        &source[loc.start_offset()..loc.end_offset()]
    }

    fn is_allowed_receiver(&self, receiver: &Node, source: &str) -> bool {
        if self.allowed_receivers.is_empty() {
            return false;
        }
        let recv_src = Self::src(source, &receiver.location());

        self.allowed_receivers.iter().any(|allowed| {
            // Match full source text
            if recv_src == *allowed {
                return true;
            }
            // Match method name (for call nodes like `execute(sql)`)
            if let Some(call) = receiver.as_call_node() {
                let method_name = String::from_utf8_lossy(call.name().as_slice());
                if method_name == *allowed {
                    return true;
                }
            }
            // Match variable name (for lvar like `execute`)
            if let Some(lvar) = receiver.as_local_variable_read_node() {
                let var_name = String::from_utf8_lossy(lvar.name().as_slice());
                if var_name == *allowed {
                    return true;
                }
            }
            // Match constant path (for `Thread.current`)
            if let Some(call) = receiver.as_call_node() {
                if let Some(recv) = call.receiver() {
                    let full_src = Self::src(source, &receiver.location());
                    if full_src == *allowed {
                        return true;
                    }
                    // Check receiver.method pattern
                    let recv_src_inner = Self::src(source, &recv.location());
                    let method_name = String::from_utf8_lossy(call.name().as_slice());
                    let combined = format!("{}.{}", recv_src_inner, method_name);
                    if combined == *allowed {
                        return true;
                    }
                }
            }
            false
        })
    }

    /// Check if a preceding call is an array converter method.
    /// Handles both direct calls (foo.sort.each) and block calls (foo.sort_by { ... }.each).
    fn is_array_converter_preceding(call: &CallNode, source: &str) -> bool {
        let recv = match call.receiver() {
            Some(r) => r,
            None => return false,
        };

        // Direct call: foo.sort.each, foo.to_a.each
        if let Some(preceding_call) = recv.as_call_node() {
            let name = String::from_utf8_lossy(preceding_call.name().as_slice());
            if ARRAY_CONVERTER_METHODS.contains(&name.as_ref()) {
                return true;
            }
        }

        // Block call: foo.sort_by { ... }.each or foo.chunk { ... }.each
        // In Prism, the receiver is a BlockNode. We can't get the call from it,
        // but we can scan the source text before the block's opening delimiter
        // to find the method name.
        if let Some(_block) = recv.as_block_node() {
            let recv_src = &source[recv.location().start_offset()..recv.location().end_offset()];
            // Check if the source contains any array converter method name before `{` or `do`
            for method in ARRAY_CONVERTER_METHODS {
                // Look for `.method_name` pattern in the source before block open
                let pattern = format!(".{}", method);
                if recv_src.contains(&pattern) {
                    return true;
                }
                // Also check safe navigation
                let safe_pattern = format!("&.{}", method);
                if recv_src.contains(&safe_pattern) {
                    return true;
                }
            }
        }

        false
    }

    fn is_non_hash_literal(node: &Node) -> bool {
        matches!(
            node,
            Node::ArrayNode { .. }
                | Node::StringNode { .. }
                | Node::IntegerNode { .. }
                | Node::FloatNode { .. }
                | Node::SymbolNode { .. }
        )
    }

    /// Check if the root receiver of a call chain is a non-hash literal.
    fn root_is_non_hash_literal(node: &Node) -> bool {
        match node {
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                match call.receiver() {
                    Some(recv) => Self::root_is_non_hash_literal(&recv),
                    None => false, // No receiver — could be anything
                }
            }
            _ => Self::is_non_hash_literal(node),
        }
    }
}

/// Visitor to check for hash mutation (receiver[]=).
struct MutationChecker<'a> {
    receiver_src: &'a str,
    source: &'a str,
    mutated: bool,
}

impl Visit<'_> for MutationChecker<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if self.mutated {
            return;
        }
        let method = String::from_utf8_lossy(node.name().as_slice());
        if method == "[]=" {
            if let Some(recv) = node.receiver() {
                let recv_s = &self.source[recv.location().start_offset()..recv.location().end_offset()];
                if recv_s == self.receiver_src {
                    self.mutated = true;
                    return;
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

/// Visitor to find lvar reads matching a given name.
struct LvarFinder {
    name: String,
    found: bool,
}

impl Visit<'_> for LvarFinder {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        if !self.found {
            let var_name = String::from_utf8_lossy(node.name().as_slice());
            if var_name == self.name {
                self.found = true;
            }
        }
    }
}

fn lvar_used_in_body(name: &str, body: &Node) -> bool {
    let mut finder = LvarFinder {
        name: name.to_string(),
        found: false,
    };
    finder.visit(body);
    finder.found
}

/// Check if a block arg (possibly destructured) is unused in the body.
fn block_arg_unused(arg: &Node, body: &Node, source: &str) -> bool {
    match arg {
        Node::RequiredParameterNode { .. } => {
            let name = String::from_utf8_lossy(
                arg.as_required_parameter_node().unwrap().name().as_slice(),
            );
            !lvar_used_in_body(&name, body)
        }
        Node::RestParameterNode { .. } => {
            let rest = arg.as_rest_parameter_node().unwrap();
            if let Some(name_loc) = rest.name_loc() {
                let name = String::from_utf8_lossy(name_loc.as_slice());
                !lvar_used_in_body(&name, body)
            } else {
                true // anonymous rest param — unused
            }
        }
        Node::MultiTargetNode { .. } => {
            let mt = arg.as_multi_target_node().unwrap();
            // All parts must be unused
            for target in mt.lefts().iter() {
                if !block_arg_unused(&target, body, source) {
                    return false;
                }
            }
            if let Some(rest) = mt.rest() {
                if !block_arg_unused(&rest, body, source) {
                    return false;
                }
            }
            for target in mt.rights().iter() {
                if !block_arg_unused(&target, body, source) {
                    return false;
                }
            }
            true
        }
        Node::SplatNode { .. } => {
            if let Some(expr) = arg.as_splat_node().unwrap().expression() {
                block_arg_unused(&expr, body, source)
            } else {
                true
            }
        }
        _ => true,
    }
}

struct HashEachVisitor<'a> {
    cop: &'a HashEachMethods,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> HashEachVisitor<'a> {
    /// Handle `foo.keys.each { ... }` and `foo.values.each { ... }` patterns.
    fn check_kv_each(&mut self, call: &CallNode, block: &ruby_prism::BlockNode) {
        let each_method = String::from_utf8_lossy(call.name().as_slice());
        if each_method != "each" {
            return;
        }

        let kv_call = match call.receiver() {
            Some(recv) => match recv.as_call_node() {
                Some(c) => c,
                None => return,
            },
            None => return,
        };

        let kv_method = String::from_utf8_lossy(kv_call.name().as_slice());
        if kv_method != "keys" && kv_method != "values" {
            return;
        }

        let parent_receiver = match kv_call.receiver() {
            Some(r) => r,
            None => return,
        };

        if self.cop.is_allowed_receiver(&parent_receiver, self.ctx.source) {
            return;
        }

        // Check root receiver is not a non-hash literal
        if HashEachMethods::root_is_non_hash_literal(&parent_receiver) {
            return;
        }

        // Check for hash mutation in block body
        let receiver_src = &self.ctx.source
            [parent_receiver.location().start_offset()..parent_receiver.location().end_offset()];
        if let Some(body) = block.body() {
            let mut checker = MutationChecker {
                receiver_src,
                source: self.ctx.source,
                mutated: false,
            };
            checker.visit(&body);
            if checker.mutated {
                return;
            }
        }

        let prefer = if kv_method == "keys" {
            "each_key"
        } else {
            "each_value"
        };

        let kv_msg_loc = match kv_call.message_loc() {
            Some(loc) => loc,
            None => return,
        };
        let each_msg_loc = match call.message_loc() {
            Some(loc) => loc,
            None => return,
        };

        let offense_start = kv_msg_loc.start_offset();
        let offense_end = each_msg_loc.end_offset();
        let current = &self.ctx.source[offense_start..offense_end];

        let message = format!("Use `{}` instead of `{}`.", prefer, current);

        let offense = self.ctx.offense_with_range(
            "Style/HashEachMethods",
            &message,
            Severity::Convention,
            offense_start,
            offense_end,
        );

        let recv_end = parent_receiver.location().end_offset();
        let dot_src = &self.ctx.source[recv_end..kv_msg_loc.start_offset()];
        let replacement = format!("{}{}", dot_src, prefer);
        let correction = Correction::replace(recv_end, offense_end, &replacement);

        self.offenses.push(offense.with_correction(correction));
    }

    /// Handle `foo.keys.each(&:bar)` with block_pass.
    fn check_kv_each_block_pass(&mut self, call: &CallNode) {
        let each_method = String::from_utf8_lossy(call.name().as_slice());
        if each_method != "each" {
            return;
        }

        // Must have a block_pass argument (not a block node)
        let block = match call.block() {
            Some(b) => b,
            None => return,
        };
        if block.as_block_argument_node().is_none() {
            return; // It's a block node, not block_pass
        }

        let kv_call = match call.receiver() {
            Some(recv) => match recv.as_call_node() {
                Some(c) => c,
                None => return,
            },
            None => return,
        };

        let kv_method = String::from_utf8_lossy(kv_call.name().as_slice());
        if kv_method != "keys" && kv_method != "values" {
            return;
        }

        let parent_receiver = match kv_call.receiver() {
            Some(r) => r,
            None => return,
        };

        if self.cop.is_allowed_receiver(&parent_receiver, self.ctx.source) {
            return;
        }

        let prefer = if kv_method == "keys" {
            "each_key"
        } else {
            "each_value"
        };

        let kv_msg_loc = match kv_call.message_loc() {
            Some(loc) => loc,
            None => return,
        };
        let each_msg_loc = match call.message_loc() {
            Some(loc) => loc,
            None => return,
        };

        let offense_start = kv_msg_loc.start_offset();
        let offense_end = each_msg_loc.end_offset();
        let current = &self.ctx.source[offense_start..offense_end];

        let message = format!("Use `{}` instead of `{}`.", prefer, current);
        let offense = self.ctx.offense_with_range(
            "Style/HashEachMethods",
            &message,
            Severity::Convention,
            offense_start,
            offense_end,
        );

        let recv_end = parent_receiver.location().end_offset();
        let dot_src = &self.ctx.source[recv_end..kv_msg_loc.start_offset()];
        let replacement = format!("{}{}", dot_src, prefer);
        let correction = Correction::replace(recv_end, offense_end, &replacement);

        self.offenses.push(offense.with_correction(correction));
    }

    /// Handle `foo.each { |k, unused_v| ... }` pattern.
    fn check_each_unused_args(&mut self, call: &CallNode, block: &ruby_prism::BlockNode) {
        let each_method = String::from_utf8_lossy(call.name().as_slice());
        if each_method != "each" {
            return;
        }

        let receiver = match call.receiver() {
            Some(r) => r,
            None => return,
        };

        // Skip if receiver is keys/values (handled by check_kv_each)
        if let Some(recv_call) = receiver.as_call_node() {
            let method = String::from_utf8_lossy(recv_call.name().as_slice());
            if method == "keys" || method == "values" {
                return;
            }
        }

        // Check for array converter methods
        if HashEachMethods::is_array_converter_preceding(call, self.ctx.source) {
            return;
        }

        // Check root receiver
        if HashEachMethods::root_is_non_hash_literal(&receiver) {
            return;
        }

        // Must have block parameters
        let params = match block.parameters() {
            Some(p) => p,
            None => return,
        };

        let block_params = match params.as_block_parameters_node() {
            Some(bp) => bp,
            None => return,
        };

        let param_node = match block_params.parameters() {
            Some(p) => p,
            None => return,
        };

        let requireds: Vec<Node> = param_node.requireds().iter().collect();
        let posts: Vec<Node> = param_node.posts().iter().collect();
        let optionals: Vec<Node> = param_node.optionals().iter().collect();
        let rest = param_node.rest();

        if !optionals.is_empty() {
            return;
        }

        // Collect all params in order: requireds, rest, posts
        let mut all_params: Vec<&Node> = Vec::new();
        for r in &requireds {
            all_params.push(r);
        }
        if let Some(ref rest_node) = rest {
            all_params.push(rest_node);
        }
        for p in &posts {
            all_params.push(p);
        }

        // Need exactly 2 parameters
        if all_params.len() != 2 {
            return;
        }

        let key_node = all_params[0];
        let value_node = all_params[1];

        // For single arg like |(k, v)| (parenthesized), skip
        // This is when requireds has 1 element with MultiTargetNode and no rest/posts
        if requireds.len() == 1 && rest.is_none() && posts.is_empty() {
            return;
        }

        // Both parenthesized destructured — skip
        if matches!(key_node, Node::MultiTargetNode { .. })
            && matches!(value_node, Node::MultiTargetNode { .. })
        {
            return;
        }

        let body = match block.body() {
            Some(b) => b,
            None => return, // Empty body — both unused
        };

        let key_unused = block_arg_unused(key_node, &body, self.ctx.source);
        let value_unused = block_arg_unused(value_node, &body, self.ctx.source);

        if (key_unused && value_unused) || (!key_unused && !value_unused) {
            return;
        }

        // Offense range covers the entire expression (from receiver start to block end)
        let expr_start = call.location().start_offset();
        let expr_end = block.location().end_offset();

        let key_src = &self.ctx.source
            [key_node.location().start_offset()..key_node.location().end_offset()];
        let value_src = &self.ctx.source
            [value_node.location().start_offset()..value_node.location().end_offset()];

        let each_msg_loc = call.message_loc().unwrap();

        if value_unused && !key_unused {
            let message = format!(
                "Use `each_key` instead of `each` and remove the unused `{}` block argument.",
                value_src
            );

            let offense = self.ctx.offense_with_range(
                "Style/HashEachMethods",
                &message,
                Severity::Convention,
                expr_start,
                expr_end,
            );

            let key_end = key_node.location().end_offset();
            let value_end = value_node.location().end_offset();

            let edits = vec![
                crate::offense::Edit {
                    start_offset: each_msg_loc.start_offset(),
                    end_offset: each_msg_loc.end_offset(),
                    replacement: "each_key".to_string(),
                },
                crate::offense::Edit {
                    start_offset: key_end,
                    end_offset: value_end,
                    replacement: String::new(),
                },
            ];

            self.offenses
                .push(offense.with_correction(crate::offense::Correction { edits }));
        } else if key_unused && !value_unused {
            let message = format!(
                "Use `each_value` instead of `each` and remove the unused `{}` block argument.",
                key_src
            );

            let offense = self.ctx.offense_with_range(
                "Style/HashEachMethods",
                &message,
                Severity::Convention,
                expr_start,
                expr_end,
            );

            let key_start = key_node.location().start_offset();
            let value_start = value_node.location().start_offset();

            let edits = vec![
                crate::offense::Edit {
                    start_offset: each_msg_loc.start_offset(),
                    end_offset: each_msg_loc.end_offset(),
                    replacement: "each_value".to_string(),
                },
                crate::offense::Edit {
                    start_offset: key_start,
                    end_offset: value_start,
                    replacement: String::new(),
                },
            ];

            self.offenses
                .push(offense.with_correction(crate::offense::Correction { edits }));
        }
    }
}

impl Visit<'_> for HashEachVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Check for keys.each(&:bar) / values.each(&:bar) (block_pass)
        self.check_kv_each_block_pass(node);

        // Check if this call has a block attached
        if let Some(block_ref) = node.block() {
            if let Some(block) = block_ref.as_block_node() {
                self.check_kv_each(node, &block);
                self.check_each_unused_args(node, &block);
            }
        }

        ruby_prism::visit_call_node(self, node);
    }
}

impl Cop for HashEachMethods {
    fn name(&self) -> &'static str {
        "Style/HashEachMethods"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = HashEachVisitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

crate::register_cop!("Style/HashEachMethods", |cfg| {
    let cop_config = cfg.get_cop_config("Style/HashEachMethods");
    let allowed_receivers: Vec<String> = cop_config
        .and_then(|c| c.raw.get("AllowedReceivers"))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    Some(Box::new(HashEachMethods::with_config(allowed_receivers)))
});
