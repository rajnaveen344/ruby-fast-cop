//! Style/MapCompactWithConditionalBlock - prefer `select`/`reject` over map/filter_map with conditional block.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/map_compact_with_conditional_block.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{BlockNode, CallNode, Node};

#[derive(Default)]
pub struct MapCompactWithConditionalBlock;

impl MapCompactWithConditionalBlock {
    pub fn new() -> Self { Self }
}

fn block_param_name(block: &BlockNode) -> Option<String> {
    let params = block.parameters()?;
    match &params {
        Node::BlockParametersNode { .. } => {
            let bp = params.as_block_parameters_node()?;
            let inner = bp.parameters()?;
            let reqs: Vec<_> = inner.requireds().iter().collect();
            if reqs.len() != 1 { return None; }
            let rp = reqs[0].as_required_parameter_node()?;
            Some(node_name!(rp).into_owned())
        }
        _ => None,
    }
}

fn is_lvar_ref(node: &Node, name: &str) -> bool {
    match node {
        Node::LocalVariableReadNode { .. } => {
            node_name!(node.as_local_variable_read_node().unwrap()) == name
        }
        Node::CallNode { .. } => {
            let c = node.as_call_node().unwrap();
            node_name!(c) == name && c.receiver().is_none() && c.arguments().is_none()
        }
        _ => false,
    }
}

fn is_nil_literal(node: &Node) -> bool {
    matches!(node, Node::NilNode { .. })
}

/// `next` with no args, or `next nil`.
fn is_bare_next_or_nil(node: &Node) -> bool {
    if let Node::NextNode { .. } = node {
        let n = node.as_next_node().unwrap();
        match n.arguments() {
            None => return true,
            Some(args) => {
                let list: Vec<_> = args.arguments().iter().collect();
                if list.is_empty() { return true; }
                if list.len() == 1 && is_nil_literal(&list[0]) { return true; }
            }
        }
    }
    is_nil_literal(node)
}

/// `next item` — next with single lvar matching pname.
fn is_next_with_param(node: &Node, pname: &str) -> bool {
    let n = match node.as_next_node() { Some(n) => n, None => return false };
    let args = match n.arguments() { Some(a) => a, None => return false };
    let list: Vec<_> = args.arguments().iter().collect();
    if list.len() != 1 { return false; }
    is_lvar_ref(&list[0], pname)
}

/// Classify a branch expression as one of:
///   Param (returns `item`), Nil (returns nil or bare next or next nil), NextItem (next item), Other
#[derive(Debug, PartialEq, Eq)]
enum BranchKind { Param, Nil, NextItem, Other }

fn classify(node: &Node, pname: &str) -> BranchKind {
    if is_lvar_ref(node, pname) { return BranchKind::Param; }
    if is_next_with_param(node, pname) { return BranchKind::NextItem; }
    if is_bare_next_or_nil(node) { return BranchKind::Nil; }
    BranchKind::Other
}

/// Analyze an if/unless node inside a block body. Returns (condition_source, truthy_returns_item).
/// `truthy_returns_item` means: when the condition is truthy, the block effectively keeps the item.
fn analyze_if<'a>(if_node: &ruby_prism::IfNode<'a>, pname: &str, source: &'a str)
    -> Option<(String, bool)>
{
    // No elsif: check consequent has no else-is-if chain
    // We'll handle both if and unless via wrapper.
    let cond = if_node.predicate();
    let cond_src = source.get(cond.location().start_offset()..cond.location().end_offset())?.to_string();

    let if_branch = if_node.statements()?;
    let if_stmts: Vec<_> = if_branch.body().iter().collect();
    if if_stmts.len() != 1 { return None; }
    let if_cls = classify(&if_stmts[0], pname);

    let else_cls = if let Some(else_clause) = if_node.subsequent() {
        // ElseNode expected — reject ElsifNode (chained if-elsif)
        match &else_clause {
            Node::ElseNode { .. } => {
                let eln = else_clause.as_else_node().unwrap();
                let st = eln.statements()?;
                let lst: Vec<_> = st.body().iter().collect();
                if lst.len() != 1 { return None; }
                classify(&lst[0], pname)
            }
            _ => return None, // elsif — skip
        }
    } else {
        // No else branch → treat as Nil
        BranchKind::Nil
    };

    // Match shapes:
    // if_cls=Param,    else=Nil        → truthy=true (select)
    // if_cls=Nil,      else=Param      → truthy=false (reject)
    // if_cls=NextItem, else=Nil        → truthy=true
    // if_cls=Nil,      else=NextItem   → truthy=false
    let truthy = match (&if_cls, &else_cls) {
        (BranchKind::Param, BranchKind::Nil) => true,
        (BranchKind::NextItem, BranchKind::Nil) => true,
        (BranchKind::Nil, BranchKind::Param) => false,
        (BranchKind::Nil, BranchKind::NextItem) => false,
        _ => return None,
    };
    Some((cond_src, truthy))
}

/// Detect modifier-form `item if cond` or `item unless cond`.
/// Returns (condition_src, truthy_returns_item).
fn analyze_modifier<'a>(stmt: &Node<'a>, pname: &str, source: &'a str) -> Option<(String, bool)> {
    // An IfNode with statements containing the lvar and predicate = cond.
    if let Some(iff) = stmt.as_if_node() {
        let st = iff.statements()?;
        let lst: Vec<_> = st.body().iter().collect();
        if lst.len() != 1 { return None; }
        let cls = classify(&lst[0], pname);
        if iff.subsequent().is_some() { return None; } // not a modifier
        let cond = iff.predicate();
        let cond_src = source.get(cond.location().start_offset()..cond.location().end_offset())?.to_string();
        match cls {
            BranchKind::Param => return Some((cond_src, true)),
            BranchKind::NextItem => return Some((cond_src, true)),
            _ => return None,
        }
    }
    if let Some(uf) = stmt.as_unless_node() {
        let st = uf.statements()?;
        let lst: Vec<_> = st.body().iter().collect();
        if lst.len() != 1 { return None; }
        let cls = classify(&lst[0], pname);
        if uf.else_clause().is_some() { return None; }
        let cond = uf.predicate();
        let cond_src = source.get(cond.location().start_offset()..cond.location().end_offset())?.to_string();
        match cls {
            BranchKind::Param => return Some((cond_src, false)),
            BranchKind::NextItem => return Some((cond_src, false)),
            _ => return None,
        }
    }
    None
}

/// Analyze a guard clause form: `next if cond\n\n item` (with trailing lvar).
/// Body shape: StatementsNode with 2 stmts: guard (if/unless with `next`) then lvar.
/// Whether a `next` node has any arguments (e.g. `next item`, `next nil` → true; bare `next` → false).
fn next_has_args(node: &Node) -> bool {
    let n = match node.as_next_node() { Some(n) => n, None => return false };
    let args = match n.arguments() { Some(a) => a, None => return false };
    args.arguments().iter().count() > 0
}

fn analyze_guard<'a>(stmts: &[Node<'a>], pname: &str, source: &'a str) -> Option<(String, bool)> {
    if stmts.len() != 2 { return None; }
    let last = &stmts[1];
    let last_cls = classify(last, pname);

    // Matching RuboCop patterns:
    //   (begin (if $_ next nil?) $(lvar _))     — last must be lvar-ref (Param)
    //   (begin (if $_ (next $(lvar _)) nil?) (nil))  — last must be nil literal; guard body is `next item`
    // The latter requires last_cls==Nil AND the actual node is a nil literal (not bare next).
    let last_is_nil_literal = is_nil_literal(last);

    let guard = &stmts[0];
    let (is_if, cond, inner_stmt) = if let Some(iff) = guard.as_if_node() {
        if iff.subsequent().is_some() { return None; }
        let st = iff.statements()?;
        let lst: Vec<_> = st.body().iter().collect();
        if lst.len() != 1 { return None; }
        (true, iff.predicate(), lst.into_iter().next().unwrap())
    } else if let Some(uf) = guard.as_unless_node() {
        if uf.else_clause().is_some() { return None; }
        let st = uf.statements()?;
        let lst: Vec<_> = st.body().iter().collect();
        if lst.len() != 1 { return None; }
        (false, uf.predicate(), lst.into_iter().next().unwrap())
    } else {
        return None;
    };

    let inner_cls = classify(&inner_stmt, pname);

    // Validate shape: guard body must be `next`/`next nil` (Nil-type) or `next item` (NextItem)
    if !matches!(inner_cls, BranchKind::Nil | BranchKind::NextItem) { return None; }

    // If guard body is NextItem, last must be a bare nil literal.
    // If guard body is Nil, last must be Param.
    match (&inner_cls, &last_cls, last_is_nil_literal) {
        (BranchKind::Nil, BranchKind::Param, _) => {}
        (BranchKind::NextItem, BranchKind::Nil, true) => {}
        _ => return None,
    }

    let cond_src = source.get(cond.location().start_offset()..cond.location().end_offset())?.to_string();

    // RuboCop truthy_branch_for_guard: look at the guard if-node's if_branch (the `next` stmt).
    //   if modifier `if`:     truthy = (next has args)
    //   if modifier `unless`: truthy = (next has no args)
    let next_has = next_has_args(&inner_stmt);
    let truthy = if is_if { next_has } else { !next_has };
    Some((cond_src, truthy))
}

/// Analyze block body: returns (condition_source, truthy_returns_item).
fn analyze_block<'a>(block: &BlockNode<'a>, pname: &str, source: &'a str) -> Option<(String, bool)> {
    let body = block.body()?;
    if let Node::BeginNode { .. } = &body { return None; }
    let stmts = body.as_statements_node()?;
    let list: Vec<_> = stmts.body().iter().collect();

    if list.len() == 1 {
        // Single if/unless
        if let Some(iff) = list[0].as_if_node() {
            // could be a modifier OR a full if/else
            // Full if: has statements body and maybe an else
            // A modifier has statements len 1 without else; we let analyze_if handle else-present case and analyze_modifier the modifier case.
            // Try full-if first if subsequent present OR body is non-empty stmts — both same entry.
            // analyze_if requires either `item/next` returning from both branches. If it's a modifier (no else, single-stmt), analyze_if would set else=Nil and match if_cls=Param → truthy=true. So that covers modifier-if returning item, too.
            // But modifier: `item if cond` → iff.statements == [item]. analyze_if → if_cls=Param, else=None→Nil → truthy=true. OK.
            return analyze_if(&iff, pname, source);
        }
        if let Some(uf) = list[0].as_unless_node() {
            // Treat as !if: build like `if cond; else_branch; else; if_branch; end`
            let cond = uf.predicate();
            let cond_src = source.get(cond.location().start_offset()..cond.location().end_offset())?.to_string();
            let st = uf.statements()?;
            let lst: Vec<_> = st.body().iter().collect();
            if lst.len() != 1 { return None; }
            let un_cls = classify(&lst[0], pname);
            let else_cls = if let Some(ec) = uf.else_clause() {
                let st2 = ec.statements()?;
                let l2: Vec<_> = st2.body().iter().collect();
                if l2.len() != 1 { return None; }
                classify(&l2[0], pname)
            } else {
                BranchKind::Nil
            };
            let truthy = match (&un_cls, &else_cls) {
                (BranchKind::Param, BranchKind::Nil) => false, // unless cond: item (same as if !cond then item) → reject
                (BranchKind::Nil, BranchKind::Param) => true,
                (BranchKind::NextItem, BranchKind::Nil) => false,
                (BranchKind::Nil, BranchKind::NextItem) => true,
                _ => return None,
            };
            return Some((cond_src, truthy));
        }
        // Ternary: IfNode with ternary (Prism models ternary as IfNode too — already handled)
    }

    // 2+ stmt block: guard form
    if list.len() >= 2 {
        // The guard form has exactly 2 statements: the guard (if/unless) and the return value.
        if list.len() == 2 {
            return analyze_guard(&list, pname, source);
        }
        return None;
    }

    None
}

fn call_method_is(call: &CallNode, name: &str) -> bool {
    node_name!(call) == name
}

/// If `node.receiver` is a block whose send_node.name is `target`, return (block_any_node, block_node, send_call).
fn block_wrapping<'a>(call: &CallNode<'a>, target: &str) -> Option<(Node<'a>, BlockNode<'a>, CallNode<'a>)> {
    // For `foo.map { ... }.compact`, the compact call's receiver is foo.map CallNode
    // (whose block is attached). For `foo.map do |x| ... end`, same.
    let recv = call.receiver()?;
    let rc = recv.as_call_node()?;
    if node_name!(rc) != target { return None; }
    let blk = rc.block()?;
    let bn = blk.as_block_node()?;
    Some((blk, bn, rc))
}

impl Cop for MapCompactWithConditionalBlock {
    fn name(&self) -> &'static str { "Style/MapCompactWithConditionalBlock" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        let m: &str = &method;
        if !matches!(m, "compact" | "filter_map") { return vec![]; }

        if m == "compact" {
            // compact must have no args
            if node.arguments().is_some() { return vec![]; }
            // receiver = call with block; the wrapped call is :map or :filter_map
            let (blk_any, block, map_send) = match block_wrapping(node, "map")
                .or_else(|| block_wrapping(node, "filter_map"))
            {
                Some(v) => v,
                None => return vec![],
            };
            let pname = match block_param_name(&block) { Some(n) => n, None => return vec![] };
            let (_, truthy) = match analyze_block(&block, &pname, ctx.source) {
                Some(v) => v,
                None => return vec![],
            };
            let replacement = if truthy { "select" } else { "reject" };
            let inner_method = node_name!(map_send).into_owned();
            let current = if inner_method == "filter_map" {
                "filter_map { ... }.compact".to_string()
            } else {
                "map { ... }.compact".to_string()
            };
            let message = format!("Replace `{}` with `{}`.", current, replacement);

            // Offense range: from map_send's selector to end of compact call.
            let sel = map_send.message_loc().expect("selector");
            let start = sel.start_offset();
            let end = node.location().end_offset();

            // Correction: "select { |item| cond }"
            let (cond_src, _) = match analyze_block(&block, &pname, ctx.source) {
                Some(v) => v, None => return vec![],
            };
            // Range to replace = from map_send selector start to end of compact call
            // But rubocop's map_with_compact_range begins at map_send.receiver.send_node.selector — same thing we have.
            let corrected_body = format!("{} {{ |{}| {} }}", replacement, pname, cond_src);

            // Determine replace start — must preserve receiver. Rubocop's range starts at map's selector.
            let mut off = ctx.offense_with_range(self.name(), &message, self.severity(), start, end);
            off = off.with_correction(Correction::replace(start, end, corrected_body));
            return vec![off];
        }

        // filter_map form: node is a filter_map call with block.
        if m == "filter_map" {
            let blk_any = match node.block() { Some(b) => b, None => return vec![] };
            let block = match blk_any.as_block_node() { Some(b) => b, None => return vec![] };
            let pname = match block_param_name(&block) { Some(n) => n, None => return vec![] };
            let (_, truthy) = match analyze_block(&block, &pname, ctx.source) {
                Some(v) => v,
                None => return vec![],
            };
            let replacement = if truthy { "select" } else { "reject" };

            // Skip if this filter_map is immediately chained with `.compact` — the compact
            // case above handles it with message "filter_map { ... }.compact".
            let after = node.location().end_offset();
            let tail = ctx.source.get(after..).unwrap_or("");
            if tail.starts_with(".compact") || tail.starts_with("&.compact") {
                return vec![];
            }

            let current = "filter_map { ... }".to_string();
            let message = format!("Replace `{}` with `{}`.", current, replacement);

            let sel = node.message_loc().expect("selector");
            let start = sel.start_offset();
            let end = blk_any.location().end_offset();

            let (cond_src, _) = match analyze_block(&block, &pname, ctx.source) {
                Some(v) => v, None => return vec![],
            };
            let corrected_body = format!("{} {{ |{}| {} }}", replacement, pname, cond_src);

            let mut off = ctx.offense_with_range(self.name(), &message, self.severity(), start, end);
            off = off.with_correction(Correction::replace(start, end, corrected_body));
            return vec![off];
        }

        let _ = call_method_is;
        vec![]
    }
}

crate::register_cop!("Style/MapCompactWithConditionalBlock",
    |_cfg| Some(Box::new(MapCompactWithConditionalBlock::new())));
