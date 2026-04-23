//! ABC size calculator.
//!
//! Ports `lib/rubocop/cop/metrics/utils/abc_size_calculator.rb` plus the three
//! discount mixins (`iterating_block.rb`, `repeated_csend_discount.rb`,
//! `repeated_attribute_discount.rb`).
//!
//! Score = `Math.sqrt(A² + B² + C²).round(2)`.
//!
//! - **A**ssignments: lvar/ivar/cvar/gvar/const writes (and op-asgn variants),
//!   setter sends (`foo=`, `[]=`), `for`-loop hidden lvar, and capturing block
//!   parameters (names not starting with `_`).
//! - **B**ranches: `send`, `csend`, `yield`. Comparison sends (`==`, `<`, …)
//!   move to C instead. `csend` adds an extra C unless the same lvar's `&.`
//!   chain has already been seen (repeated-csend discount).
//! - **C**onditions: `if`, `while`, `until`, `for`, `csend` (above), block
//!   nodes whose enclosing send is a known iterating method (`.each`, `.map`,
//!   …), block-pass, `rescue`, `when`, `in`, `and`, `or`, and the `||=`/`&&=`
//!   operator-write variants. `if`/`case` with a real `else` keyword add 1 more.
//!
//! Repeated-attribute discount: when enabled, no-arg send chains on the same
//! receiver tree are deduplicated — e.g. `foo; self.foo` counts B=1 not 2.

use crate::helpers::method_complexity::is_iterating_method;
use crate::node_name;
use ruby_prism::{Node, Visit};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct AbcResult {
    pub assignment: u32,
    pub branch: u32,
    pub condition: u32,
    pub score: f64,
}

impl AbcResult {
    pub fn vector(&self) -> String {
        format!("<{}, {}, {}>", self.assignment, self.branch, self.condition)
    }
}

pub fn calculate(body: &Node, discount_repeated_attributes: bool) -> AbcResult {
    let mut calc = Calc {
        a: 0,
        b: 0,
        c: 0,
        repeated_csend: HashMap::new(),
        discount_repeated_attributes,
        attr_root: AttrTree::default(),
        lvar_attr_roots: HashMap::new(),
    };
    calc.visit(body);
    let raw = ((calc.a as f64).powi(2) + (calc.b as f64).powi(2) + (calc.c as f64).powi(2)).sqrt();
    let score = (raw * 100.0).round() / 100.0;
    AbcResult { assignment: calc.a, branch: calc.b, condition: calc.c, score }
}

#[derive(Debug, Default)]
struct AttrTree {
    children: HashMap<String, AttrTree>,
}

struct Calc {
    a: u32,
    b: u32,
    c: u32,
    repeated_csend: HashMap<String, usize>,
    discount_repeated_attributes: bool,
    attr_root: AttrTree,
    lvar_attr_roots: HashMap<String, AttrTree>,
}

fn is_comparison_method(name: &str) -> bool {
    matches!(name, "==" | "===" | "!=" | "<=" | ">=" | "<" | ">")
}

fn is_attribute_call(node: &ruby_prism::CallNode) -> bool {
    let no_args = node.arguments().map_or(true, |a| a.arguments().iter().count() == 0);
    let no_block = node.block().is_none();
    no_args && no_block
}

impl Calc {
    fn count_param(&mut self, name: &[u8]) {
        if !name.starts_with(b"_") { self.a += 1; }
    }

    /// Walk a no-arg send chain backward, building [root_key, m1, m2, …].
    /// Returns None if any node in the chain isn't a recognized root or
    /// isn't a no-arg attribute call.
    fn collect_attribute_chain(&self, send: &ruby_prism::CallNode) -> Option<Vec<String>> {
        let mut chain: Vec<String> = vec![node_name!(send).to_string()];
        let mut cur = send.receiver();
        loop {
            let r = match cur { Some(n) => n, None => {
                chain.push("@root".into());
                chain.reverse();
                return Some(chain);
            }};
            if r.as_self_node().is_some() {
                chain.push("@root".into());
                chain.reverse();
                return Some(chain);
            }
            if let Some(lvr) = r.as_local_variable_read_node() {
                chain.push(format!("@lvar:{}", node_name!(lvr)));
                chain.reverse();
                return Some(chain);
            }
            if let Some(c) = r.as_constant_read_node() {
                chain.push(format!("@const:{}", node_name!(c)));
                chain.reverse();
                return Some(chain);
            }
            if let Some(iv) = r.as_instance_variable_read_node() {
                chain.push(format!("@ivar:{}", node_name!(iv)));
                chain.reverse();
                return Some(chain);
            }
            if let Some(cv) = r.as_class_variable_read_node() {
                chain.push(format!("@cvar:{}", node_name!(cv)));
                chain.reverse();
                return Some(chain);
            }
            if let Some(gv) = r.as_global_variable_read_node() {
                chain.push(format!("@gvar:{}", node_name!(gv)));
                chain.reverse();
                return Some(chain);
            }
            if let Some(inner) = r.as_call_node() {
                if !is_attribute_call(&inner) { return None; }
                chain.push(node_name!(inner).to_string());
                cur = inner.receiver();
                continue;
            }
            return None;
        }
    }

    fn discount_repeated_attribute(&mut self, send: &ruby_prism::CallNode) -> bool {
        if !is_attribute_call(send) { return false; }
        let chain = match self.collect_attribute_chain(send) {
            Some(c) => c,
            None => return false,
        };
        let (root, methods) = chain.split_first().unwrap();
        let tree: &mut AttrTree = if root == "@root" {
            &mut self.attr_root
        } else {
            self.lvar_attr_roots.entry(root.clone()).or_default()
        };
        let mut node = tree;
        let mut all_existed = true;
        for m in methods {
            if !node.children.contains_key(m) { all_existed = false; }
            node = node.children.entry(m.clone()).or_default();
        }
        all_existed
    }

    fn discount_for_repeated_csend(&mut self, node: &ruby_prism::CallNode) -> bool {
        let recv = match node.receiver() { Some(r) => r, None => return false };
        let lvr = match recv.as_local_variable_read_node() { Some(l) => l, None => return false };
        let var_name = node_name!(lvr).to_string();
        let key = node.location().start_offset();
        match self.repeated_csend.get(&var_name) {
            Some(seen) if *seen != key => true,
            Some(_) => false,
            None => { self.repeated_csend.insert(var_name, key); false }
        }
    }
}

impl<'a> Visit<'a> for Calc {
    // ── Assignments ─────────────────────────────────────────────────────────
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        ruby_prism::visit_local_variable_write_node(self, node);
        let name = node_name!(node).to_string();
        // Reset csend tracking on lvar write.
        self.repeated_csend.remove(&name);
        // Reset attribute trie for that lvar.
        if self.discount_repeated_attributes {
            self.lvar_attr_roots.remove(&name);
        }
        if !name.starts_with('_') { self.a += 1; }
    }
    fn visit_instance_variable_write_node(&mut self, n: &ruby_prism::InstanceVariableWriteNode) {
        ruby_prism::visit_instance_variable_write_node(self, n); self.a += 1;
    }
    fn visit_class_variable_write_node(&mut self, n: &ruby_prism::ClassVariableWriteNode) {
        ruby_prism::visit_class_variable_write_node(self, n); self.a += 1;
    }
    fn visit_global_variable_write_node(&mut self, n: &ruby_prism::GlobalVariableWriteNode) {
        ruby_prism::visit_global_variable_write_node(self, n); self.a += 1;
    }
    fn visit_constant_write_node(&mut self, n: &ruby_prism::ConstantWriteNode) {
        ruby_prism::visit_constant_write_node(self, n); self.a += 1;
    }
    fn visit_constant_path_write_node(&mut self, n: &ruby_prism::ConstantPathWriteNode) {
        ruby_prism::visit_constant_path_write_node(self, n); self.a += 1;
    }
    fn visit_local_variable_operator_write_node(&mut self, n: &ruby_prism::LocalVariableOperatorWriteNode) {
        ruby_prism::visit_local_variable_operator_write_node(self, n); self.a += 1;
    }
    fn visit_instance_variable_operator_write_node(&mut self, n: &ruby_prism::InstanceVariableOperatorWriteNode) {
        ruby_prism::visit_instance_variable_operator_write_node(self, n); self.a += 1;
    }
    fn visit_class_variable_operator_write_node(&mut self, n: &ruby_prism::ClassVariableOperatorWriteNode) {
        ruby_prism::visit_class_variable_operator_write_node(self, n); self.a += 1;
    }
    fn visit_global_variable_operator_write_node(&mut self, n: &ruby_prism::GlobalVariableOperatorWriteNode) {
        ruby_prism::visit_global_variable_operator_write_node(self, n); self.a += 1;
    }
    fn visit_constant_operator_write_node(&mut self, n: &ruby_prism::ConstantOperatorWriteNode) {
        ruby_prism::visit_constant_operator_write_node(self, n); self.a += 1;
    }
    fn visit_constant_path_operator_write_node(&mut self, n: &ruby_prism::ConstantPathOperatorWriteNode) {
        ruby_prism::visit_constant_path_operator_write_node(self, n); self.a += 1;
    }

    // Multi-assign — count each non-setter target once.
    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        ruby_prism::visit_multi_write_node(self, node);
        let mut count = |n: &Node| {
            match n {
                Node::CallTargetNode { .. } | Node::IndexTargetNode { .. } => {}
                _ => { self.a += 1; }
            }
        };
        for t in node.lefts().iter() { count(&t); }
        if let Some(rest) = node.rest() { count(&rest); }
        for t in node.rights().iter() { count(&t); }
    }

    // ||=, &&= on any target → count as condition (like ||/&&) and as A.
    fn visit_local_variable_or_write_node(&mut self, n: &ruby_prism::LocalVariableOrWriteNode) {
        ruby_prism::visit_local_variable_or_write_node(self, n);
        let name = node_name!(n).to_string();
        self.repeated_csend.remove(&name);
        self.c += 1; if !name.starts_with('_') { self.a += 1; }
    }
    fn visit_local_variable_and_write_node(&mut self, n: &ruby_prism::LocalVariableAndWriteNode) {
        ruby_prism::visit_local_variable_and_write_node(self, n);
        let name = node_name!(n).to_string();
        self.repeated_csend.remove(&name);
        self.c += 1; if !name.starts_with('_') { self.a += 1; }
    }
    fn visit_instance_variable_or_write_node(&mut self, n: &ruby_prism::InstanceVariableOrWriteNode) {
        ruby_prism::visit_instance_variable_or_write_node(self, n); self.c += 1; self.a += 1;
    }
    fn visit_instance_variable_and_write_node(&mut self, n: &ruby_prism::InstanceVariableAndWriteNode) {
        ruby_prism::visit_instance_variable_and_write_node(self, n); self.c += 1; self.a += 1;
    }
    fn visit_class_variable_or_write_node(&mut self, n: &ruby_prism::ClassVariableOrWriteNode) {
        ruby_prism::visit_class_variable_or_write_node(self, n); self.c += 1; self.a += 1;
    }
    fn visit_class_variable_and_write_node(&mut self, n: &ruby_prism::ClassVariableAndWriteNode) {
        ruby_prism::visit_class_variable_and_write_node(self, n); self.c += 1; self.a += 1;
    }
    fn visit_global_variable_or_write_node(&mut self, n: &ruby_prism::GlobalVariableOrWriteNode) {
        ruby_prism::visit_global_variable_or_write_node(self, n); self.c += 1; self.a += 1;
    }
    fn visit_global_variable_and_write_node(&mut self, n: &ruby_prism::GlobalVariableAndWriteNode) {
        ruby_prism::visit_global_variable_and_write_node(self, n); self.c += 1; self.a += 1;
    }
    fn visit_constant_or_write_node(&mut self, n: &ruby_prism::ConstantOrWriteNode) {
        ruby_prism::visit_constant_or_write_node(self, n); self.c += 1; self.a += 1;
    }
    fn visit_constant_and_write_node(&mut self, n: &ruby_prism::ConstantAndWriteNode) {
        ruby_prism::visit_constant_and_write_node(self, n); self.c += 1; self.a += 1;
    }
    fn visit_constant_path_or_write_node(&mut self, n: &ruby_prism::ConstantPathOrWriteNode) {
        ruby_prism::visit_constant_path_or_write_node(self, n); self.c += 1; self.a += 1;
    }
    fn visit_constant_path_and_write_node(&mut self, n: &ruby_prism::ConstantPathAndWriteNode) {
        ruby_prism::visit_constant_path_and_write_node(self, n); self.c += 1; self.a += 1;
    }
    fn visit_call_or_write_node(&mut self, n: &ruby_prism::CallOrWriteNode) {
        ruby_prism::visit_call_or_write_node(self, n); self.c += 1; self.a += 1;
    }
    fn visit_call_and_write_node(&mut self, n: &ruby_prism::CallAndWriteNode) {
        ruby_prism::visit_call_and_write_node(self, n); self.c += 1; self.a += 1;
    }
    fn visit_index_or_write_node(&mut self, n: &ruby_prism::IndexOrWriteNode) {
        ruby_prism::visit_index_or_write_node(self, n); self.c += 1; self.a += 1;
    }
    fn visit_index_and_write_node(&mut self, n: &ruby_prism::IndexAndWriteNode) {
        ruby_prism::visit_index_and_write_node(self, n); self.c += 1; self.a += 1;
    }
    fn visit_call_operator_write_node(&mut self, n: &ruby_prism::CallOperatorWriteNode) {
        ruby_prism::visit_call_operator_write_node(self, n); self.a += 1;
    }
    fn visit_index_operator_write_node(&mut self, n: &ruby_prism::IndexOperatorWriteNode) {
        ruby_prism::visit_index_operator_write_node(self, n); self.a += 1;
    }

    // ── Conditions ──────────────────────────────────────────────────────────
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        ruby_prism::visit_if_node(self, node);
        // Real-else bonus.
        let sub = node.subsequent();
        let real_else = matches!(&sub, Some(Node::ElseNode { .. }))
            && sub.as_ref().unwrap().as_else_node().unwrap().else_keyword_loc().as_slice() == b"else";
        if real_else { self.c += 1; }
        self.c += 1;
    }
    fn visit_unless_node(&mut self, n: &ruby_prism::UnlessNode) {
        ruby_prism::visit_unless_node(self, n); self.c += 1;
    }
    fn visit_while_node(&mut self, n: &ruby_prism::WhileNode) {
        ruby_prism::visit_while_node(self, n); self.c += 1;
    }
    fn visit_until_node(&mut self, n: &ruby_prism::UntilNode) {
        ruby_prism::visit_until_node(self, n); self.c += 1;
    }
    fn visit_for_node(&mut self, n: &ruby_prism::ForNode) {
        ruby_prism::visit_for_node(self, n);
        // For-loop binds an lvar (its `index`) → A; loop itself → C.
        self.a += 1; self.c += 1;
    }
    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        ruby_prism::visit_case_node(self, node);
        if node.else_clause().is_some() { self.c += 1; }
        self.c += 1;
    }
    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        ruby_prism::visit_case_match_node(self, node);
        if node.else_clause().is_some() { self.c += 1; }
        self.c += 1;
    }
    fn visit_when_node(&mut self, n: &ruby_prism::WhenNode) {
        ruby_prism::visit_when_node(self, n); self.c += 1;
    }
    fn visit_in_node(&mut self, n: &ruby_prism::InNode) {
        ruby_prism::visit_in_node(self, n); self.c += 1;
    }
    fn visit_and_node(&mut self, n: &ruby_prism::AndNode) {
        ruby_prism::visit_and_node(self, n); self.c += 1;
    }
    fn visit_or_node(&mut self, n: &ruby_prism::OrNode) {
        ruby_prism::visit_or_node(self, n); self.c += 1;
    }
    fn visit_rescue_node(&mut self, n: &ruby_prism::RescueNode) {
        ruby_prism::visit_rescue_node(self, n); self.c += 1;
    }

    // ── Branches & block-as-condition ──────────────────────────────────────
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Recurse first so children-counts (incl. csend tracking inside chain)
        // happen before we evaluate this send.
        ruby_prism::visit_call_node(self, node);

        let name = node_name!(node);

        // Setter sends: count A here (RuboCop's setter_method?). Setters return
        // before the branch path? No — RuboCop's `assignment?` returns true for
        // setters AND `branch?` is also true (send is always a branch). Both
        // counts apply.
        let is_setter = name.ends_with('=') && !is_comparison_method(&name) && name != "<=" && name != ">=";
        if is_setter { self.a += 1; }

        // Branch / comparison
        if is_comparison_method(&name) {
            self.c += 1;
        } else {
            self.b += 1;
            if node.is_safe_navigation() && !self.discount_for_repeated_csend(node) {
                self.c += 1;
            }
        }

        // Repeated-attribute discount: undo +B (and +C if csend) when applicable.
        if self.discount_repeated_attributes
            && !is_comparison_method(&name)
            && self.discount_repeated_attribute(node)
        {
            self.b -= 1;
            if node.is_safe_navigation() { self.c -= 1; }
        }

        // Update receiver-tree for setter sends (`self.foo = x` → invalidate `foo`).
        if self.discount_repeated_attributes && is_setter {
            if let Some(stripped) = name.strip_suffix('=') {
                if stripped != "[]" {
                    if let Some(recv) = node.receiver() {
                        if recv.as_self_node().is_some() {
                            self.attr_root.children.remove(stripped);
                        } else if let Some(lvr) = recv.as_local_variable_read_node() {
                            let lname = node_name!(lvr).to_string();
                            self.lvar_attr_roots.entry(lname).or_default()
                                .children.remove(stripped);
                        }
                    }
                }
            }
        }

        // Block-as-condition: known iterating method with a do/end block.
        if let Some(block) = node.block() {
            if matches!(&block, Node::BlockNode { .. }) && is_iterating_method(&name) {
                self.c += 1;
            }
        }
    }

    fn visit_yield_node(&mut self, n: &ruby_prism::YieldNode) {
        ruby_prism::visit_yield_node(self, n); self.b += 1;
    }

    // ── Block parameters → A (capturing only) ──────────────────────────────
    fn visit_required_parameter_node(&mut self, n: &ruby_prism::RequiredParameterNode) {
        self.count_param(node_name!(n).as_bytes());
        ruby_prism::visit_required_parameter_node(self, n);
    }
    fn visit_optional_parameter_node(&mut self, n: &ruby_prism::OptionalParameterNode) {
        self.count_param(node_name!(n).as_bytes());
        ruby_prism::visit_optional_parameter_node(self, n);
    }
    fn visit_rest_parameter_node(&mut self, n: &ruby_prism::RestParameterNode) {
        if let Some(name) = n.name() { self.count_param(name.as_slice()); }
        ruby_prism::visit_rest_parameter_node(self, n);
    }
    fn visit_keyword_rest_parameter_node(&mut self, n: &ruby_prism::KeywordRestParameterNode) {
        if let Some(name) = n.name() { self.count_param(name.as_slice()); }
        ruby_prism::visit_keyword_rest_parameter_node(self, n);
    }
    fn visit_required_keyword_parameter_node(&mut self, n: &ruby_prism::RequiredKeywordParameterNode) {
        self.count_param(node_name!(n).as_bytes());
        ruby_prism::visit_required_keyword_parameter_node(self, n);
    }
    fn visit_optional_keyword_parameter_node(&mut self, n: &ruby_prism::OptionalKeywordParameterNode) {
        self.count_param(node_name!(n).as_bytes());
        ruby_prism::visit_optional_keyword_parameter_node(self, n);
    }
    fn visit_block_parameter_node(&mut self, n: &ruby_prism::BlockParameterNode) {
        if let Some(name) = n.name() { self.count_param(name.as_slice()); }
        ruby_prism::visit_block_parameter_node(self, n);
    }
}
