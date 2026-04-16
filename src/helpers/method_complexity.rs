//! Shared helper for Metrics/CyclomaticComplexity and Metrics/PerceivedComplexity.
//!
//! Mirrors RuboCop's `MethodComplexity` mixin — walks method body counting
//! decision-point nodes, honours AllowedMethods/AllowedPatterns, and handles
//! `define_method` blocks with known iterating blocks.

use crate::cops::CheckContext;
use crate::helpers::allowed_methods::is_method_allowed;
use crate::node_name;
use crate::offense::{Location, Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashMap;

/// Methods treated as iterating blocks (ary.map {...}, each {...}).
/// Mirrors RuboCop's `KNOWN_ITERATING_METHODS` from
/// `lib/rubocop/cop/mixin/utils/iterating_block.rb`.
pub const KNOWN_ITERATING_METHODS: &[&str] = &[
    // Enumerable
    "all?", "any?", "chain", "chunk", "chunk_while", "collect", "collect_concat",
    "count", "cycle", "detect", "drop", "drop_while", "each", "each_cons",
    "each_entry", "each_slice", "each_with_index", "each_with_object", "entries",
    "filter", "filter_map", "find", "find_all", "find_index", "flat_map", "grep",
    "grep_v", "group_by", "inject", "lazy", "map", "max", "max_by", "min",
    "min_by", "minmax", "minmax_by", "none?", "one?", "partition", "reduce",
    "reject", "reverse_each", "select", "slice_after", "slice_before",
    "slice_when", "sort", "sort_by", "sum", "take", "take_while", "tally",
    "to_h", "uniq", "zip",
    // Enumerator
    "with_index", "with_object",
    // Array
    "bsearch", "bsearch_index", "collect!", "combination", "d_permutation",
    "delete_if", "each_index", "keep_if", "map!", "permutation", "product",
    "reject!", "repeat", "repeated_combination", "select!", "sort!",
    // Hash
    "each_key", "each_pair", "each_value", "fetch", "fetch_values", "has_key?",
    "merge", "merge!", "transform_keys", "transform_keys!", "transform_values",
    "transform_values!",
];

pub fn is_iterating_method(name: &str) -> bool {
    KNOWN_ITERATING_METHODS.contains(&name)
}

/// What kind of complexity the cop measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComplexityKind {
    Cyclomatic,
    Perceived,
}

pub struct MethodComplexityConfig {
    pub kind: ComplexityKind,
    pub cop_name: &'static str,
    pub msg_template: &'static str,
    pub max: usize,
    pub allowed_methods: Vec<String>,
    pub allowed_patterns: Vec<String>,
}

/// Run the complexity analysis and append offenses.
pub fn check_program(ctx: &CheckContext, cfg: &MethodComplexityConfig, offenses: &mut Vec<Offense>) {
    let result = ruby_prism::parse(ctx.source.as_bytes());
    let mut v = ComplexityVisitor { ctx, cfg, offenses };
    v.visit(&result.node());
}

struct ComplexityVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cfg: &'a MethodComplexityConfig,
    offenses: &'a mut Vec<Offense>,
}

impl ComplexityVisitor<'_> {
    fn maybe_check(&mut self, name: &str, body: Option<Node>, header_start: usize, header_end: usize) {
        if is_method_allowed(&self.cfg.allowed_methods, &self.cfg.allowed_patterns, name, None) { return; }
        let body = match body { Some(b) => b, None => return };
        let score = self.score_body(&body);
        // RuboCop compares complexity.ceil-style — scores are integers here.
        if score <= self.cfg.max { return; }
        let msg = self.cfg.msg_template
            .replace("{method}", name)
            .replace("{complexity}", &score.to_string())
            .replace("{max}", &self.cfg.max.to_string());
        let loc = Location::from_offsets(self.ctx.source, header_start, header_end);
        self.offenses.push(Offense::new(self.cfg.cop_name, msg, Severity::Convention, loc, self.ctx.filename));
    }

    fn score_body(&self, body: &Node) -> usize {
        let mut scorer = ScoreVisitor { kind: self.cfg.kind, score: 1, csend_seen: HashMap::new() };
        scorer.visit(body);
        scorer.score
    }
}

impl Visit<'_> for ComplexityVisitor<'_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let name = node_name!(node).to_string();
        let loc = node.location();
        // RuboCop uses node.source_range — whole def...end. Location::from_offsets clamps
        // last_column to end-of-first-line for multiline ranges, matching expect_offense.
        let body = node.body();
        self.maybe_check(&name, body, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // define_method :name do ... end
        if node.receiver().is_none() && node_name!(node) == "define_method" {
            if let Some(block) = node.block() {
                if let Some(block_node) = block.as_block_node() {
                    // First arg must be sym/str literal
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
                        let body = block_node.body();
                        let call_loc = node.location();
                        // Whole `define_method :name do ... end` range; Location clamps to first line.
                        self.maybe_check(&name, body, call_loc.start_offset(), call_loc.end_offset());
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

struct ScoreVisitor {
    kind: ComplexityKind,
    score: usize,
    /// Repeated `&.` on same untouched local var → only count first
    csend_seen: HashMap<String, bool>,
}

impl ScoreVisitor {
    fn add(&mut self, n: usize) { self.score += n; }
}

impl Visit<'_> for ScoreVisitor {
    // Reset csend tracking on local var assignment
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let name = node_name!(node).to_string();
        self.csend_seen.remove(&name);
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode) {
        let name = node_name!(node).to_string();
        self.csend_seen.remove(&name);
        self.add(1); // `foo &&= x` counts like `&&`
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        let name = node_name!(node).to_string();
        self.csend_seen.remove(&name);
        self.add(1); // `foo ||= x` counts like `||`
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        let name = node_name!(node).to_string();
        self.csend_seen.remove(&name);
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        match self.kind {
            ComplexityKind::Cyclomatic => self.add(1),
            ComplexityKind::Perceived => {
                // RuboCop: `node.else? && !node.elsif?` → 2, else 1.
                // `else?` = has explicit `else` keyword. Ternary's ElseNode uses `:` not `else`, so
                // `else?` returns false for ternaries. `elsif?` = subsequent is another IfNode.
                let sub = node.subsequent();
                let has_real_else = match &sub {
                    Some(Node::ElseNode { .. }) => {
                        let en = sub.as_ref().unwrap().as_else_node().unwrap();
                        en.else_keyword_loc().as_slice() == b"else"
                    }
                    _ => false,
                };
                let is_elsif_follow = matches!(&sub, Some(Node::IfNode { .. }));
                if has_real_else && !is_elsif_follow { self.add(2); } else { self.add(1); }
            }
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        // UnlessNode parses as an `if` in RuboCop's AST with inverted cond; counts same as if.
        self.add(1);
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        self.add(1);
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        self.add(1);
        ruby_prism::visit_until_node(self, node);
    }

    fn visit_for_node(&mut self, node: &ruby_prism::ForNode) {
        self.add(1);
        ruby_prism::visit_for_node(self, node);
    }

    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode) {
        self.add(1);
        ruby_prism::visit_rescue_node(self, node);
    }

    fn visit_when_node(&mut self, node: &ruby_prism::WhenNode) {
        // Cyclomatic counts every when; Perceived counts cases differently (in visit_case_node).
        if matches!(self.kind, ComplexityKind::Cyclomatic) {
            self.add(1);
        }
        ruby_prism::visit_when_node(self, node);
    }

    fn visit_in_node(&mut self, node: &ruby_prism::InNode) {
        // `in_pattern` maps to Prism's InNode inside CaseMatchNode — counts both modes.
        self.add(1);
        ruby_prism::visit_in_node(self, node);
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        if matches!(self.kind, ComplexityKind::Perceived) {
            let whens = node.conditions().iter().count();
            let has_else = node.else_clause().is_some();
            let nb_branches = whens + if has_else { 1 } else { 0 };
            if node.predicate().is_none() {
                self.add(nb_branches);
            } else {
                // (nb_branches * 0.2 + 0.8).round as usize
                let raw = (nb_branches as f64) * 0.2 + 0.8;
                self.add(raw.round() as usize);
            }
        }
        ruby_prism::visit_case_node(self, node);
    }

    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        // `in_pattern` counting happens via visit_in_node; nothing special here for either kind.
        ruby_prism::visit_case_match_node(self, node);
    }

    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        self.add(1);
        ruby_prism::visit_and_node(self, node);
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        self.add(1);
        ruby_prism::visit_or_node(self, node);
    }

    // Instance/class/global/constant/call/index ||= and &&= — count like ||/&&.
    fn visit_instance_variable_or_write_node(&mut self, n: &ruby_prism::InstanceVariableOrWriteNode) {
        self.add(1); ruby_prism::visit_instance_variable_or_write_node(self, n);
    }
    fn visit_instance_variable_and_write_node(&mut self, n: &ruby_prism::InstanceVariableAndWriteNode) {
        self.add(1); ruby_prism::visit_instance_variable_and_write_node(self, n);
    }
    fn visit_class_variable_or_write_node(&mut self, n: &ruby_prism::ClassVariableOrWriteNode) {
        self.add(1); ruby_prism::visit_class_variable_or_write_node(self, n);
    }
    fn visit_class_variable_and_write_node(&mut self, n: &ruby_prism::ClassVariableAndWriteNode) {
        self.add(1); ruby_prism::visit_class_variable_and_write_node(self, n);
    }
    fn visit_global_variable_or_write_node(&mut self, n: &ruby_prism::GlobalVariableOrWriteNode) {
        self.add(1); ruby_prism::visit_global_variable_or_write_node(self, n);
    }
    fn visit_global_variable_and_write_node(&mut self, n: &ruby_prism::GlobalVariableAndWriteNode) {
        self.add(1); ruby_prism::visit_global_variable_and_write_node(self, n);
    }
    fn visit_constant_or_write_node(&mut self, n: &ruby_prism::ConstantOrWriteNode) {
        self.add(1); ruby_prism::visit_constant_or_write_node(self, n);
    }
    fn visit_constant_and_write_node(&mut self, n: &ruby_prism::ConstantAndWriteNode) {
        self.add(1); ruby_prism::visit_constant_and_write_node(self, n);
    }
    fn visit_constant_path_or_write_node(&mut self, n: &ruby_prism::ConstantPathOrWriteNode) {
        self.add(1); ruby_prism::visit_constant_path_or_write_node(self, n);
    }
    fn visit_constant_path_and_write_node(&mut self, n: &ruby_prism::ConstantPathAndWriteNode) {
        self.add(1); ruby_prism::visit_constant_path_and_write_node(self, n);
    }
    fn visit_call_or_write_node(&mut self, n: &ruby_prism::CallOrWriteNode) {
        self.add(1); ruby_prism::visit_call_or_write_node(self, n);
    }
    fn visit_call_and_write_node(&mut self, n: &ruby_prism::CallAndWriteNode) {
        self.add(1); ruby_prism::visit_call_and_write_node(self, n);
    }
    fn visit_index_or_write_node(&mut self, n: &ruby_prism::IndexOrWriteNode) {
        self.add(1); ruby_prism::visit_index_or_write_node(self, n);
    }
    fn visit_index_and_write_node(&mut self, n: &ruby_prism::IndexAndWriteNode) {
        self.add(1); ruby_prism::visit_index_and_write_node(self, n);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Safe navigation (&.): csend = +1 unless repeated on same untouched local.
        if node.is_safe_navigation() {
            // Check repeated &. on same lvar
            if let Some(recv) = node.receiver() {
                if let Some(lvr) = recv.as_local_variable_read_node() {
                    let name = node_name!(lvr).to_string();
                    if !self.csend_seen.contains_key(&name) {
                        self.csend_seen.insert(name, true);
                        self.add(1);
                    }
                    // else: repeated, skip
                } else {
                    self.add(1);
                }
            } else {
                self.add(1);
            }
        }

        // Block attached: count if known iterating method.
        if let Some(block) = node.block() {
            match &block {
                Node::BlockNode { .. } => {
                    if is_iterating_method(&node_name!(node)) {
                        self.add(1);
                    }
                    // Recurse into block body
                }
                Node::BlockArgumentNode { .. } => {
                    // `.map(&:to_s)` — block_pass. Cyclomatic counts this as +1 always.
                    if matches!(self.kind, ComplexityKind::Cyclomatic) {
                        self.add(1);
                    }
                }
                _ => {}
            }
        }

        ruby_prism::visit_call_node(self, node);
    }
}

// Separate local-var or-write scoring is above; this is the proper impl used by
// the trait via override. The no-op `visit_local_variable_or_write_node_count`
// above is just a name-only placeholder Rust accepts since no trait method has
// that signature.
