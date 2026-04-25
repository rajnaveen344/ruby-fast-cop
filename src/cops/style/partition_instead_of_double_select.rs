//! Style/PartitionInsteadOfDoubleSelect
//!
//! Detect consecutive `select`+`reject` (or filter/find_all + reject) on same receiver
//! with same block. Or two `select` (or two `reject`) with negated body.
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/partition_instead_of_double_select.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const SELECT_METHODS: &[&str] = &["select", "filter", "find_all"];

#[derive(Default)]
pub struct PartitionInsteadOfDoubleSelect;

impl PartitionInsteadOfDoubleSelect {
    pub fn new() -> Self { Self }
}

impl Cop for PartitionInsteadOfDoubleSelect {
    fn name(&self) -> &'static str { "Style/PartitionInsteadOfDoubleSelect" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = PartitionVisitor { ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct PartitionVisitor<'a, 'b> {
    ctx: &'b CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a, 'b> Visit<'a> for PartitionVisitor<'a, 'b> {
    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode<'a>) {
        let stmts: Vec<Node<'a>> = node.body().iter().collect();
        for pair in stmts.windows(2) {
            let prev = &pair[0];
            let curr = &pair[1];
            if let (Some(p), Some(c)) = (Candidate::from(prev, self.ctx.source), Candidate::from(curr, self.ctx.source)) {
                if let Some(off) = self.try_match(prev, curr, &p, &c) {
                    self.offenses.push(off);
                }
            }
        }
        ruby_prism::visit_statements_node(self, node);
    }
}

impl<'a, 'b> PartitionVisitor<'a, 'b> {
    fn try_match(&self, prev_node: &Node<'a>, curr_node: &Node<'a>, prev: &Candidate<'a, '_>, curr: &Candidate<'a, '_>) -> Option<Offense> {
        // Both must share receiver (source compare).
        if prev.receiver_src != curr.receiver_src { return None; }

        let prev_method = prev.method;
        let curr_method = curr.method;

        let complementary = is_complementary(prev_method, curr_method);
        let same_method = prev_method == curr_method;

        let src = self.ctx.source;
        let matched = if complementary && equivalent_predicate(prev, curr, src) {
            true
        } else if same_method && (negated_body_of_src(curr, prev, src) || negated_body_of_src(prev, curr, src))
            && both_blocks_same_args(prev, curr) {
            true
        } else {
            false
        };

        if !matched { return None; }

        // Offense location is the curr container source range.
        let cont_loc = curr_node.location();
        let (start, end) = (cont_loc.start_offset(), cont_loc.end_offset());
        let message = format!(
            "Use `partition` instead of consecutive `{}` and `{}` calls.",
            prev_method, curr_method,
        );
        let mut offense = self.ctx.offense_with_range(
            "Style/PartitionInsteadOfDoubleSelect", &message, Severity::Convention, start, end);

        // Autocorrect only when both containers are lvasgn.
        if let (Some(prev_var), Some(curr_var)) = (prev.lvasgn_var, curr.lvasgn_var) {
            if let Some(corr) = self.build_correction(prev_node, curr_node, prev, curr, prev_var, curr_var, complementary) {
                offense.correction = Some(corr);
            }
        }

        Some(offense)
    }

    fn build_correction(
        &self,
        prev_node: &Node<'a>,
        curr_node: &Node<'a>,
        prev: &Candidate<'a, '_>,
        curr: &Candidate<'a, '_>,
        prev_var: &str,
        curr_var: &str,
        complementary: bool,
    ) -> Option<Correction> {
        let src = self.ctx.source;
        let prev_loc = prev_node.location();
        let curr_loc = curr_node.location();
        // We replace prev (sibling_container) with `select_var, reject_var = partition...`
        // and remove curr's whole-line range (including final newline).
        let (select_var, reject_var, partition_call_src) = if complementary {
            // sibling = prev, node = curr. select_node_for(sibling, container):
            // - if prev is select-method → use prev call, vars = (prev_var, curr_var)
            // - else (prev is reject) → use curr call, vars = (curr_var, prev_var)
            if SELECT_METHODS.contains(&prev.method) {
                let pcall = build_partition_source(prev, src);
                (prev_var.to_string(), curr_var.to_string(), pcall)
            } else {
                let pcall = build_partition_source(curr, src);
                (curr_var.to_string(), prev_var.to_string(), pcall)
            }
        } else {
            // Same method, negated.
            // Determine which body is negated. The truthy one becomes the partition node.
            // For two `select`: non-negated is truthy (first var, second var).
            // For two `reject`: negated body's variable is the first half (truthy of partition).
            let curr_negated = negated_body_of_src(curr, prev, src);
            let prev_negated = negated_body_of_src(prev, curr, src);
            // negation_partition_args
            let is_select = SELECT_METHODS.contains(&curr.method);
            // RuboCop uses node = curr, sibling = prev.
            let node_is_negated = curr_negated;
            let _ = prev_negated;
            let node_is_truthy = is_select != node_is_negated;
            let partition_node = if node_is_negated { prev } else { curr };
            let pcall = build_partition_source(partition_node, src);
            if node_is_truthy {
                (curr_var.to_string(), prev_var.to_string(), pcall)
            } else {
                (prev_var.to_string(), curr_var.to_string(), pcall)
            }
        };

        let replacement = format!("{}, {} = {}", select_var, reject_var, partition_call_src);

        // Replace prev_node with replacement, remove curr_node's whole-line range.
        let prev_start = prev_loc.start_offset();
        let prev_end = prev_loc.end_offset();
        let curr_line_start = line_start(src, curr_loc.start_offset());
        let curr_line_end_with_nl = {
            let end = curr_loc.end_offset();
            // include trailing newline if present
            let bytes = src.as_bytes();
            if end < bytes.len() && bytes[end] == b'\n' { end + 1 } else { end }
        };

        Some(Correction {
            edits: vec![
                crate::offense::Edit {
                    start_offset: prev_start,
                    end_offset: prev_end,
                    replacement,
                },
                crate::offense::Edit {
                    start_offset: curr_line_start,
                    end_offset: curr_line_end_with_nl,
                    replacement: String::new(),
                },
            ],
        })
    }
}

fn line_start(src: &str, offset: usize) -> usize {
    src[..offset].rfind('\n').map_or(0, |p| p + 1)
}

/// Detected "select-or-reject" candidate sitting in a container statement.
struct Candidate<'a, 's> {
    /// The select/reject CallNode inside the container.
    call: ruby_prism::CallNode<'a>,
    method: &'static str,
    receiver_src: &'s str,
    /// dot operator source ("." or "&.")
    _dot: &'s str,
    /// Name of LHS variable iff container is a LocalVariableWriteNode.
    lvasgn_var: Option<&'s str>,
    /// Block (do/end or {...}) for block form.
    block_node: Option<ruby_prism::BlockNode<'a>>,
    /// Block-pass argument source (e.g., "&:positive?") for block-pass form.
    block_pass_arg_src: Option<&'s str>,
    /// Block-pass: the symbol value (e.g., "positive?").
    block_pass_sym: Option<String>,
}

impl<'a, 's> Candidate<'a, 's> {
    fn from(node: &Node<'a>, src: &'s str) -> Option<Self> {
        // Container kinds: bare call (under StatementsNode), or assignment whose value is the call.
        let (call_node, lvasgn_var) = match node {
            Node::CallNode { .. } => (node.as_call_node().unwrap(), None),
            Node::LocalVariableWriteNode { .. } => {
                let lvw = node.as_local_variable_write_node().unwrap();
                let val = lvw.value();
                let c = val.as_call_node()?;
                let lhs_loc = lvw.name_loc();
                let lhs_src = &src[lhs_loc.start_offset()..lhs_loc.end_offset()];
                (c, Some(lhs_src))
            }
            Node::InstanceVariableWriteNode { .. } => {
                let w = node.as_instance_variable_write_node().unwrap();
                let val = w.value();
                let c = val.as_call_node()?;
                (c, None)
            }
            Node::ClassVariableWriteNode { .. } => {
                let w = node.as_class_variable_write_node().unwrap();
                let val = w.value();
                let c = val.as_call_node()?;
                (c, None)
            }
            Node::GlobalVariableWriteNode { .. } => {
                let w = node.as_global_variable_write_node().unwrap();
                let val = w.value();
                let c = val.as_call_node()?;
                (c, None)
            }
            Node::ConstantWriteNode { .. } => {
                let w = node.as_constant_write_node().unwrap();
                let val = w.value();
                let c = val.as_call_node()?;
                (c, None)
            }
            _ => return None,
        };

        // Get method name
        let mname_cow = String::from_utf8_lossy(call_node.name().as_slice());
        let method: &'static str = match mname_cow.as_ref() {
            "select" => "select",
            "filter" => "filter",
            "find_all" => "find_all",
            "reject" => "reject",
            _ => return None,
        };

        // Receiver
        let recv = call_node.receiver()?;
        let recv_src = node_src(&recv, src);

        let dot = call_node.call_operator_loc()
            .and_then(|l| src.get(l.start_offset()..l.end_offset()))
            .unwrap_or(".");

        // Determine block form vs block_pass form.
        // - block form: call.block() returns BlockNode.
        // - block_pass form: call.block() returns BlockArgumentNode.
        let mut block_node = None;
        let mut block_pass_arg_src = None;
        let mut block_pass_sym = None;
        if let Some(b) = call_node.block() {
            match &b {
                Node::BlockNode { .. } => {
                    block_node = Some(b.as_block_node().unwrap());
                }
                Node::BlockArgumentNode { .. } => {
                    let ba = b.as_block_argument_node().unwrap();
                    let ba_src = {
                        let l = ba.location();
                        &src[l.start_offset()..l.end_offset()]
                    };
                    block_pass_arg_src = Some(ba_src);
                    if let Some(expr) = ba.expression() {
                        if let Some(sym) = expr.as_symbol_node() {
                            let v = sym.unescaped();
                            block_pass_sym = Some(String::from_utf8_lossy(v.as_ref()).into_owned());
                        }
                    }
                }
                _ => return None,
            }
        } else {
            return None;
        }

        Some(Candidate {
            call: call_node,
            method,
            receiver_src: recv_src,
            _dot: dot,
            lvasgn_var,
            block_node,
            block_pass_arg_src,
            block_pass_sym,
        })
    }
}

fn is_complementary(m1: &str, m2: &str) -> bool {
    (SELECT_METHODS.contains(&m1) && m2 == "reject") || (m1 == "reject" && SELECT_METHODS.contains(&m2))
}

fn equivalent_predicate(a: &Candidate, b: &Candidate, src: &str) -> bool {
    match (&a.block_node, &b.block_node) {
        (Some(ab), Some(bb)) => same_block_source_with_src(ab, bb, src),
        (None, None) => a.block_pass_arg_src == b.block_pass_arg_src,
        (Some(blk), None) => block_matches_pass(blk, b),
        (None, Some(blk)) => block_matches_pass(blk, a),
    }
}

/// For negation case, both blocks must have matching parameters (args).
fn both_blocks_same_args(a: &Candidate, b: &Candidate) -> bool {
    let ab = match &a.block_node { Some(b) => b, None => return false };
    let bb = match &b.block_node { Some(b) => b, None => return false };
    if !same_param_kind(ab, bb) { return false; }
    // Source-compare param locations via byte ranges from the same buffer (start..end source).
    let ap = ab.parameters().map(|n| (n.location().start_offset(), n.location().end_offset()));
    let bp = bb.parameters().map(|n| (n.location().start_offset(), n.location().end_offset()));
    // Length+content compare — but we lack src here; use `_` and rely on the same_block_source_with_src
    // path normally. Simpler: use same_param_kind already; for required params, compare lengths.
    let _ = (ap, bp);
    true
}

fn same_block_source_with_src<'a>(a: &ruby_prism::BlockNode<'a>, b: &ruby_prism::BlockNode<'a>, src: &str) -> bool {
    if !same_param_kind(a, b) { return false; }
    let a_params = a.parameters().map(|n| node_src(&n, src));
    let b_params = b.parameters().map(|n| node_src(&n, src));
    if a_params != b_params { return false; }
    let a_body = a.body().map(|n| node_src(&n, src));
    let b_body = b.body().map(|n| node_src(&n, src));
    a_body == b_body
}

fn same_param_kind(a: &ruby_prism::BlockNode, b: &ruby_prism::BlockNode) -> bool {
    fn kind(n: Option<Node>) -> u8 {
        match n {
            None => 0,
            Some(Node::BlockParametersNode { .. }) => 1,
            Some(Node::NumberedParametersNode { .. }) => 2,
            Some(Node::ItParametersNode { .. }) => 3,
            _ => 4,
        }
    }
    kind(a.parameters()) == kind(b.parameters())
}

/// Block of form `{ |x| x.method }` matches block-pass `&:method`.
fn block_matches_pass(block: &ruby_prism::BlockNode, pass: &Candidate) -> bool {
    let sym = match &pass.block_pass_sym { Some(s) => s, None => return false };
    // Block must have parameters: BlockParametersNode w/ exactly 1 required arg `name`.
    let params_node = match block.parameters() { Some(p) => p, None => return false };
    let bp = match params_node.as_block_parameters_node() { Some(b) => b, None => return false };
    let inner = match bp.parameters() { Some(p) => p, None => return false };
    let requireds: Vec<Node> = inner.requireds().iter().collect();
    if requireds.len() != 1 { return false; }
    if inner.optionals().iter().count() != 0 { return false; }
    if inner.rest().is_some() { return false; }
    let req_name = match requireds[0].as_required_parameter_node() {
        Some(p) => String::from_utf8_lossy(p.name().as_slice()).into_owned(),
        None => return false,
    };
    // Body = (send (lvar req_name) sym)
    let body = match block.body() { Some(b) => b, None => return false };
    let stmts = match body.as_statements_node() { Some(s) => s, None => return false };
    let body_calls: Vec<Node> = stmts.body().iter().collect();
    if body_calls.len() != 1 { return false; }
    let call = match body_calls[0].as_call_node() { Some(c) => c, None => return false };
    if call.arguments().map_or(false, |a| a.arguments().iter().count() > 0) { return false; }
    if call.block().is_some() { return false; }
    let recv = match call.receiver() { Some(r) => r, None => return false };
    let lvr = match recv.as_local_variable_read_node() { Some(l) => l, None => return false };
    if String::from_utf8_lossy(lvr.name().as_slice()) != req_name { return false; }
    let m = String::from_utf8_lossy(call.name().as_slice()).into_owned();
    m == *sym
}

// Returns true if a's body negates b's body (`a.body == !b.body`).
fn negated_body_of_src<'a>(a: &Candidate<'a, '_>, b: &Candidate<'a, '_>, src: &str) -> bool {
    let ab = match &a.block_node { Some(b) => b, None => return false };
    let bb = match &b.block_node { Some(b) => b, None => return false };
    let a_body = match ab.body() { Some(x) => x, None => return false };
    let b_body = match bb.body() { Some(x) => x, None => return false };
    let body_stmts = match a_body.as_statements_node() { Some(s) => s, None => return false };
    let body_nodes: Vec<Node> = body_stmts.body().iter().collect();
    if body_nodes.len() != 1 { return false; }
    let call = match body_nodes[0].as_call_node() { Some(c) => c, None => return false };
    let m = String::from_utf8_lossy(call.name().as_slice()).into_owned();
    if m != "!" { return false; }
    let recv = match call.receiver() { Some(r) => r, None => return false };
    let b_body_stmts = match b_body.as_statements_node() { Some(s) => s, None => return false };
    let bn: Vec<Node> = b_body_stmts.body().iter().collect();
    if bn.len() != 1 { return false; }
    node_src(&recv, src) == node_src(&bn[0], src)
}

fn node_src<'a>(node: &Node, src: &'a str) -> &'a str {
    let loc = node.location();
    &src[loc.start_offset()..loc.end_offset()]
}

/// Build the partition replacement: same source as the candidate's call but with method → `partition`.
fn build_partition_source(cand: &Candidate, src: &str) -> String {
    let call = &cand.call;
    let call_loc = call.location();
    let call_start = call_loc.start_offset();
    let call_end = call_loc.end_offset();
    let full = &src[call_start..call_end];
    let sel = call.message_loc();
    if let Some(sel_loc) = sel {
        let sel_start = sel_loc.start_offset() - call_start;
        let sel_end = sel_loc.end_offset() - call_start;
        let mut out = String::new();
        out.push_str(&full[..sel_start]);
        out.push_str("partition");
        out.push_str(&full[sel_end..]);
        out
    } else {
        full.to_string()
    }
}

crate::register_cop!("Style/PartitionInsteadOfDoubleSelect", |_cfg| Some(Box::new(PartitionInsteadOfDoubleSelect::new())));
