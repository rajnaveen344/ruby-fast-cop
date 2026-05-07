//! Lint/SafeNavigationChain cop
//!
//! Checks for safe navigation operator (`&.`) followed by a regular method call (`.`).
//! e.g., `x&.foo.bar` — if `x` is nil, `x&.foo` returns nil, then `.bar` raises NoMethodError.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;
use std::collections::{HashMap, HashSet};

const COMPARISON_METHODS: &[&str] = &["<", ">", "<=", ">=", "==", "!=", "<=>", "===", "=~"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParentKind {
    /// `&&`, `||`
    Logical,
    /// Parent is itself a comparison call (`a == foo`)
    Comparison,
    /// Parent is array literal
    Array,
    /// Parent is hash assoc (pair) value
    AssocValue,
    Other,
}

const MSG: &str = "Do not chain ordinary method call after safe navigation operator.";

/// Methods that exist on NilClass (safe to call after &.)
const NIL_METHODS: &[&str] = &[
    "&", "^", "|", "===", "=~", ">>", "<<",
    "inspect", "to_a", "to_c", "to_f", "to_i", "to_r", "to_s",
    "nil?", "to_h", "is_a?", "kind_of?", "respond_to?",
    "class", "clone", "dup", "freeze", "frozen?", "hash",
    "object_id", "equal?", "eql?", "instance_of?",
    "instance_variable_get", "instance_variable_set", "instance_variable_defined?",
    "instance_variables", "method", "methods",
    "private_methods", "protected_methods", "public_methods",
    "public_send", "send", "tap", "then", "yield_self",
    "define_singleton_method", "display", "enum_for", "to_enum",
    "extend", "singleton_class", "singleton_method", "singleton_methods",
    "taint", "tainted?", "untaint", "trust", "untrust", "untrusted?",
    "!",  "!=", "!~", "==",
    "__id__", "__send__",
    // stdlib additions
    "to_d",
];

const PLUS_MINUS_METHODS: &[&str] = &["+@", "-@"];

pub struct SafeNavigationChain {
    allowed_methods: HashSet<String>,
}

impl Default for SafeNavigationChain {
    fn default() -> Self {
        Self {
            allowed_methods: HashSet::new(),
        }
    }
}

impl SafeNavigationChain {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_allowed_methods(allowed: Vec<String>) -> Self {
        Self {
            allowed_methods: allowed.into_iter().collect(),
        }
    }

    fn is_safe_nav(call: &ruby_prism::CallNode, source: &str) -> bool {
        if let Some(op_loc) = call.call_operator_loc() {
            &source[op_loc.start_offset()..op_loc.end_offset()] == "&."
        } else {
            false
        }
    }

    fn is_nil_method(&self, method: &str) -> bool {
        NIL_METHODS.contains(&method) || self.allowed_methods.contains(method)
    }

    /// Check if a node (or its receiver chain) contains a safe navigation call.
    fn node_has_safe_nav(node: &Node, source: &str) -> bool {
        match node {
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                if Self::is_safe_nav(&call, source) {
                    return true;
                }
                if let Some(recv) = call.receiver() {
                    return Self::node_has_safe_nav(&recv, source);
                }
                false
            }
            // Blocks: `x&.select { ... }` — the block wraps around the call
            Node::BlockNode { .. } => {
                // In Prism, blocks as receivers don't exist directly.
                // But `x&.select { |x| foo(x) }.bar` parses as:
                // CallNode(.bar, receiver=BlockNode(CallNode(&.select, receiver=x)))
                // So we need to check BlockNode structure — but BlockNode doesn't have
                // a `.call()` method. We check its children.
                false
            }
            _ => false,
        }
    }

    /// Get the root receiver of a call chain (the leftmost node).
    fn root_receiver_src(node: &Node, source: &str) -> String {
        match node {
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                if let Some(recv) = call.receiver() {
                    Self::root_receiver_src(&recv, source)
                } else {
                    let loc = node.location();
                    source[loc.start_offset()..loc.end_offset()].to_string()
                }
            }
            _ => {
                let loc = node.location();
                source[loc.start_offset()..loc.end_offset()].to_string()
            }
        }
    }
}

impl Cop for SafeNavigationChain {
    fn name(&self) -> &'static str {
        "Lint/SafeNavigationChain"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        // Pre-pass: build parent kind map for every CallNode start offset.
        let mut parent_map: HashMap<(usize, usize), ParentKind> = HashMap::new();
        let mut pp = ParentPass {
            source: ctx.source,
            parent_map: &mut parent_map,
        };
        use ruby_prism::Visit;
        pp.visit_program_node(node);

        let mut visitor = SafeNavChainVisitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
            pending_outer_dots: Vec::new(),
            parent_map,
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct SafeNavChainVisitor<'a> {
    cop: &'a SafeNavigationChain,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// Dot offsets of outer plain-dot calls that chain after the innermost safe-nav call.
    /// When we flag the innermost plain-dot call, we attach ALL of these as additional
    /// correction edits so the single-pass correction converts the entire chain.
    pending_outer_dots: Vec<usize>,
    /// (CallNode start, CallNode end) → parent kind. Tuple key disambiguates a
    /// CallNode from its leftmost-receiver subcall (which shares start_offset).
    parent_map: HashMap<(usize, usize), ParentKind>,
}

/// Pre-pass walker building a parent-kind map for CallNodes.
struct ParentPass<'a> {
    source: &'a str,
    parent_map: &'a mut HashMap<(usize, usize), ParentKind>,
}

impl<'a> ParentPass<'a> {
    fn op_src<'b>(&'b self, loc: &ruby_prism::Location) -> &'b str {
        &self.source[loc.start_offset()..loc.end_offset()]
    }
}

impl<'a> ruby_prism::Visit<'_> for ParentPass<'a> {
    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        // RuboCop's `logical_operator?` is true only for `&&`/`||`, not `and`/`or`.
        let kind = if self.op_src(&node.operator_loc()) == "&&" {
            ParentKind::Logical
        } else {
            ParentKind::Other
        };
        record_child_call(self.parent_map, &node.left(), kind);
        record_child_call(self.parent_map, &node.right(), kind);
        ruby_prism::visit_and_node(self, node);
    }
    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        let kind = if self.op_src(&node.operator_loc()) == "||" {
            ParentKind::Logical
        } else {
            ParentKind::Other
        };
        record_child_call(self.parent_map, &node.left(), kind);
        record_child_call(self.parent_map, &node.right(), kind);
        ruby_prism::visit_or_node(self, node);
    }
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // If this call is itself a comparison, mark its receiver and arguments.
        let method = String::from_utf8_lossy(node.name().as_slice()).to_string();
        if COMPARISON_METHODS.contains(&method.as_str()) {
            if let Some(recv) = node.receiver() {
                record_child_call(self.parent_map, &recv, ParentKind::Comparison);
            }
            if let Some(args) = node.arguments() {
                for a in args.arguments().iter() {
                    record_child_call(self.parent_map, &a, ParentKind::Comparison);
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        for e in node.elements().iter() {
            record_child_call(self.parent_map, &e, ParentKind::Array);
        }
        ruby_prism::visit_array_node(self, node);
    }
    fn visit_assoc_node(&mut self, node: &ruby_prism::AssocNode) {
        record_child_call(self.parent_map, &node.value(), ParentKind::AssocValue);
        ruby_prism::visit_assoc_node(self, node);
    }
}

fn record_child_call(map: &mut HashMap<(usize, usize), ParentKind>, child: &Node, kind: ParentKind) {
    if let Node::CallNode { .. } = child {
        let loc = child.location();
        map.insert((loc.start_offset(), loc.end_offset()), kind);
    }
}

impl<'a> SafeNavChainVisitor<'a> {
    /// Check if a CallNode is a regular (non-safe-nav) call chained after a safe-nav receiver.
    fn check_send(&mut self, node: &ruby_prism::CallNode) {
        // This node must be a regular `.` call (not `&.`) or an operator call without dot
        if SafeNavigationChain::is_safe_nav(node, self.ctx.source) {
            // Safe-nav call: reset pending chain (a new chain may start here)
            self.pending_outer_dots.clear();
            return;
        }

        // Must have a receiver to be a chain
        let receiver = match node.receiver() {
            Some(r) => r,
            None => {
                self.pending_outer_dots.clear();
                return;
            }
        };

        let method = node_name!(node);

        // Skip +@ and -@ (unary operators)
        if PLUS_MINUS_METHODS.contains(&method.as_ref()) {
            self.pending_outer_dots.clear();
            return;
        }

        // Skip nil methods and allowed methods
        if self.cop.is_nil_method(&method) {
            self.pending_outer_dots.clear();
            return;
        }

        // Check if the IMMEDIATE receiver is a safe-nav call (csend) or a block on a csend.
        // RuboCop only flags `(send (csend ...) ...)` or `(send (block (csend ...) ...) ...)`.
        // It does NOT flag `(send (send (csend ...) ...) ...)` — only the first `.` after `&.`.
        let recv_has_safe_nav = match &receiver {
            Node::CallNode { .. } => {
                let recv_call = receiver.as_call_node().unwrap();
                SafeNavigationChain::is_safe_nav(&recv_call, self.ctx.source)
            }
            // For block as receiver: `x&.select { ... }.bar` — check if the block's call uses &.
            _ => self.check_block_receiver_has_safe_nav(&receiver),
        };

        // Check if receiver chain (transitively) contains a safe-nav call.
        // If so, this node is a continuation of a chain and its dot should also get `&`.
        let recv_chain_has_safe_nav = if !recv_has_safe_nav {
            SafeNavigationChain::node_has_safe_nav(&receiver, self.ctx.source)
        } else {
            false
        };

        if recv_chain_has_safe_nav {
            // This node is an outer chain member (e.g., `.baz` in `x&.foo.bar.baz`).
            // Accumulate its dot offset so the inner offense can include it in the correction.
            if let Some(dot_loc) = node.call_operator_loc() {
                self.pending_outer_dots.push(dot_loc.start_offset());
            }
            return;
        }

        if !recv_has_safe_nav {
            self.pending_outer_dots.clear();
            return;
        }

        // Determine offense range:
        // From dot (or end of receiver) to end of this call
        let start = if let Some(dot_loc) = node.call_operator_loc() {
            dot_loc.start_offset()
        } else {
            // Operator method (no dot): offense starts after receiver ends
            receiver.location().end_offset()
        };
        let end = node.location().end_offset();

        // Build correction: insert `&` before this dot AND all accumulated outer dots,
        // OR rewrite bracket-method into explicit `&.[](...)` form.
        let is_bracket = method.as_ref() == "[]" || method.as_ref() == "[]=";
        let mut correction_edits = vec![];

        if is_bracket {
            // Replace from end of safe-nav receiver through end of this node with
            //   &.METHOD(args)
            // RuboCop computes:
            //   offense_range = recv_end .. node_end
            //   args_src = node.arguments.map(&:source).join(", ")
            //   replacement = "&.METHOD(args_src)"
            let recv_end = receiver.location().end_offset();
            let node_end = node.location().end_offset();
            let args_src = if let Some(args_node) = node.arguments() {
                args_node
                    .arguments()
                    .iter()
                    .map(|a| {
                        let loc = a.location();
                        self.ctx.source[loc.start_offset()..loc.end_offset()].to_string()
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            } else {
                String::new()
            };
            let replacement = format!("&.{}({})", method, args_src);
            correction_edits.push(crate::offense::Edit {
                start_offset: recv_end,
                end_offset: node_end,
                replacement,
            });
            // Bracket case: do NOT promote outer pending dots (`.to_s` stays plain).
            self.pending_outer_dots.clear();
        } else {
            // Insert `&` before this dot
            if let Some(dot_loc) = node.call_operator_loc() {
                correction_edits.push(crate::offense::Edit {
                    start_offset: dot_loc.start_offset(),
                    end_offset: dot_loc.start_offset(),
                    replacement: "&".to_string(),
                });
            } else {
                // Operator method (no dot): insert `&.` after receiver to produce `recv&. op rhs`
                let recv_end = receiver.location().end_offset();
                correction_edits.push(crate::offense::Edit {
                    start_offset: recv_end,
                    end_offset: recv_end,
                    replacement: "&.".to_string(),
                });
            }
            // Insert `&` before all outer chain dots (accumulated from outer calls visited first)
            for &dot_start in &self.pending_outer_dots {
                correction_edits.push(crate::offense::Edit {
                    start_offset: dot_start,
                    end_offset: dot_start,
                    replacement: "&".to_string(),
                });
            }
            self.pending_outer_dots.clear();

            // Paren-wrap when:
            //   (a) operator-without-dot inside array or assoc-value
            //   (b) comparison method whose parent is logical (and/or) or another comparison
            let no_dot = node.call_operator_loc().is_none();
            let n_loc = node.location();
            let parent_kind = self.parent_map.get(&(n_loc.start_offset(), n_loc.end_offset())).copied().unwrap_or(ParentKind::Other);
            let is_comparison = COMPARISON_METHODS.contains(&method.as_ref());
            let needs_paren = (no_dot && (parent_kind == ParentKind::Array || parent_kind == ParentKind::AssocValue))
                || (is_comparison && (parent_kind == ParentKind::Logical || parent_kind == ParentKind::Comparison));
            if needs_paren {
                let n_start = node.location().start_offset();
                let n_end = node.location().end_offset();
                correction_edits.push(crate::offense::Edit {
                    start_offset: n_start,
                    end_offset: n_start,
                    replacement: "(".to_string(),
                });
                correction_edits.push(crate::offense::Edit {
                    start_offset: n_end,
                    end_offset: n_end,
                    replacement: ")".to_string(),
                });
            }
        }

        let correction = if correction_edits.is_empty() {
            None
        } else {
            Some(Correction { edits: correction_edits })
        };

        let offense = self.ctx.offense_with_range(self.cop.name(), MSG, self.cop.severity(), start, end);
        let offense = if let Some(c) = correction {
            offense.with_correction(c)
        } else {
            offense
        };
        self.offenses.push(offense);
    }

    /// Check if a block-type receiver has safe navigation in its call.
    /// In Prism, `x&.select { ... }.bar` — the block is the receiver of `.bar`.
    /// We check if the source of the block-receiver contains `&.` at the top level.
    /// This is a heuristic; in Prism the block node wraps the call.
    fn check_block_receiver_has_safe_nav(&self, node: &Node) -> bool {
        match node {
            Node::BlockNode { .. } => {
                // BlockNode doesn't expose its call directly in Prism.
                // But the source of the block starts with the call, so check source.
                let loc = node.location();
                let recv_src = &self.ctx.source[loc.start_offset()..loc.end_offset()];
                // Check for `&.` — but make sure we don't match nested &. in the block body.
                // The call is at the beginning of the block, so just check the prefix.
                // A simple heuristic: check if `&.` appears before the first `{` or `do`.
                let brace_pos = recv_src.find('{').or_else(|| {
                    // Find `do` keyword position
                    recv_src.find(" do\n").or_else(|| recv_src.find(" do "))
                });
                let check_range = brace_pos.unwrap_or(recv_src.len());
                recv_src[..check_range].contains("&.")
            }
            _ => false,
        }
    }
}

impl ruby_prism::Visit<'_> for SafeNavChainVisitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_send(node);
        ruby_prism::visit_call_node(self, node);
    }

    // Handle the `&&` special case:
    // `x&.foo.bar && x&.foo.baz` — only report .bar, not .baz
    // We need to check AndNode: if RHS is a call chain with safe nav, and receiver matches LHS, skip RHS.
    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        // Process LHS normally
        use ruby_prism::Visit;
        self.visit(&node.left());

        // For RHS: check if it should be suppressed
        let rhs = node.right();
        if self.should_suppress_and_rhs(&node.left(), &rhs) {
            // Don't visit RHS — suppress offense
            // But we still need to recurse into RHS children that aren't the top-level call
            // Actually, we suppress the top-level send on RHS only. Inner calls still checked.
            // This is complex. RuboCop's approach: `require_safe_navigation?` returns false for
            // the RHS call when parent is `&&` and lhs.receiver == rhs.receiver.
            // In practice, the simplest approach: mark offenses, then remove offenses from RHS
            // when they match the && pattern.
            // For now, just skip visiting RHS entirely.
            return;
        }

        self.visit(&rhs);
    }
}

impl<'a> SafeNavChainVisitor<'a> {
    fn should_suppress_and_rhs(&self, lhs: &Node, rhs: &Node) -> bool {
        // Check if both LHS and RHS are call chains, and the root receiver of the
        // safe-nav portion matches between them.
        let lhs_root = self.get_safe_nav_root_receiver(lhs);
        let rhs_root = self.get_safe_nav_root_receiver(rhs);

        if let (Some(l), Some(r)) = (lhs_root, rhs_root) {
            l == r
        } else {
            false
        }
    }

    /// Get the root receiver of the safe-nav call in a chain.
    fn get_safe_nav_root_receiver(&self, node: &Node) -> Option<String> {
        match node {
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                if let Some(recv) = call.receiver() {
                    // Check if this call or any receiver uses safe nav
                    if SafeNavigationChain::node_has_safe_nav(node, self.ctx.source) {
                        return Some(SafeNavigationChain::root_receiver_src(node, self.ctx.source));
                    }
                }
                None
            }
            _ => None,
        }
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allowed_methods: Vec<String>,
}

crate::register_cop!("Lint/SafeNavigationChain", |cfg| {
    let c: Cfg = cfg.typed("Lint/SafeNavigationChain");
    Some(Box::new(SafeNavigationChain::with_allowed_methods(c.allowed_methods)))
});
