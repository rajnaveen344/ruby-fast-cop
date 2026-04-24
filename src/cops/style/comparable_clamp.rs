//! Style/ComparableClamp cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Default)]
pub struct ComparableClamp;
impl ComparableClamp { pub fn new() -> Self { Self } }

fn src<'a>(n: &Node<'a>, source: &'a str) -> &'a str {
    let l = n.location();
    &source[l.start_offset()..l.end_offset()]
}

fn unwrap_parens<'a>(n: Node<'a>) -> Node<'a> {
    if let Some(p) = n.as_parentheses_node() {
        if let Some(b) = p.body() {
            if let Some(s) = b.as_statements_node() {
                let items: Vec<_> = s.body().iter().collect();
                if items.len() == 1 { return items.into_iter().next().unwrap(); }
            }
            return b;
        }
    }
    n
}

/// Extract (lhs_src, op, rhs_src) from a simple comparison `<` or `>`.
fn as_lt_gt<'a>(n: Node<'a>, source: &'a str) -> Option<(String, String, String)> {
    let n = unwrap_parens(n);
    let c = n.as_call_node()?;
    let m = node_name!(c);
    let op = m.to_string();
    if op != "<" && op != ">" { return None; }
    let recv = c.receiver()?;
    let args = c.arguments()?;
    let list: Vec<_> = args.arguments().iter().collect();
    if list.len() != 1 { return None; }
    let rhs = list.into_iter().next().unwrap();
    Some((src(&recv, source).to_string(), op, src(&rhs, source).to_string()))
}

/// Normalize to `(a < b)` pattern: returns (smaller_src, larger_src).
/// For `a < b` -> (a, b); for `a > b` -> (b, a).
fn normalize_lt<'a>(lhs: String, op: &str, rhs: String) -> (String, String) {
    if op == "<" { (lhs, rhs) } else { (rhs, lhs) }
}

/// From a branch with 1 statement, extract that statement's source.
fn branch_single_src<'a>(stmts: &ruby_prism::StatementsNode<'a>, source: &'a str) -> Option<String> {
    let items: Vec<_> = stmts.body().iter().collect();
    if items.len() != 1 { return None; }
    Some(src(&items[0], source).to_string())
}

impl Cop for ComparableClamp {
    fn name(&self) -> &'static str { "Style/ComparableClamp" }
    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(2, 4) { return vec![]; }
        let mut v = V { ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct V<'a> { ctx: &'a CheckContext<'a>, offenses: Vec<Offense> }

impl<'a> V<'a> {
    /// Check if/elsif/else pattern.
    fn check_if(&mut self, node: &ruby_prism::IfNode<'a>) {
        let source = self.ctx.source;
        // First branch
        let cond1 = node.predicate();
        let stmts1 = match node.statements() { Some(s) => s, None => return };
        let b1 = match branch_single_src(&stmts1, source) { Some(v) => v, None => return };
        let (c1_lhs, c1_op, c1_rhs) = match as_lt_gt(cond1, source) { Some(v) => v, None => return };
        // Second branch: subsequent should be IfNode (elsif)
        let sub = match node.subsequent() { Some(s) => s, None => return };
        let elsif = match sub.as_if_node() { Some(i) => i, None => return };
        let cond2 = elsif.predicate();
        let stmts2 = match elsif.statements() { Some(s) => s, None => return };
        let b2 = match branch_single_src(&stmts2, source) { Some(v) => v, None => return };
        let (c2_lhs, c2_op, c2_rhs) = match as_lt_gt(cond2, source) { Some(v) => v, None => return };
        // Else branch
        let else_sub = match elsif.subsequent() { Some(s) => s, None => return };
        let else_n = match else_sub.as_else_node() { Some(e) => e, None => return };
        let else_stmts = match else_n.statements() { Some(s) => s, None => return };
        let b3 = match branch_single_src(&else_stmts, source) { Some(v) => v, None => return };

        // Normalize: (smaller, larger) for each comparison
        let (c1_s, c1_l) = normalize_lt(c1_lhs, &c1_op, c1_rhs);
        let (c2_s, c2_l) = normalize_lt(c2_lhs, &c2_op, c2_rhs);

        // Pattern 1 (lower branch first): smaller < x → b=low, larger < x → would be different branches
        // Interpreting: branch1 asserts `x < low` (so smaller=x, larger=low), returns low.
        // branch2 asserts `high < x` (so smaller=high, larger=x), returns high.
        // branch3 returns x.
        // Generalized: identify (x, low, high).
        // Attempt: branch1 result == one of c1_s/c1_l, branch2 result == one of c2_s/c2_l, branch3 = x shared.
        //
        // Try all 4 combos mapping:
        // variant A: b1 = c1_l (x<low pattern: c1_s=x, c1_l=low, b1=low), b2 = c2_s (high<x: c2_s=high, c2_l=x, b2=high)
        //            shared x = c1_s == c2_l.
        // variant B: b1 = c1_l, b2 = c2_l (x<low; x>high: c2_s=high, c2_l=x OR c2_s=x, c2_l=high).
        // Simpler: for each comparison, the branch body equals one operand; the OTHER operand is x.
        // Then x must match across both branches AND else branch.
        // And branches: branch1 body ∈ {low, high}, branch2 body ∈ {low, high}, distinct.
        let (x1, bound1) = if c1_s == b1 { (c1_l.clone(), c1_s.clone()) }
                          else if c1_l == b1 { (c1_s.clone(), c1_l.clone()) }
                          else { return };
        let (x2, bound2) = if c2_s == b2 { (c2_l.clone(), c2_s.clone()) }
                          else if c2_l == b2 { (c2_s.clone(), c2_l.clone()) }
                          else { return };
        if x1 != x2 { return; }
        if x1 != b3 { return; }
        if bound1 == bound2 { return; }
        // Determine low vs high: low branch = one where branch body was the larger (i.e. x < low → body=c1_l=low, so c1_s=x, which is the smaller) ... Actually:
        //  - "x < low" → branch returns low (the upper-of-two). smaller=x, larger=low, body=larger.
        //  - "low > x" → branch returns low. smaller=x, larger=low, body=larger.
        //  - "high < x" → branch returns high. smaller=high, larger=x, body=smaller.
        //  - "x > high" → branch returns high. smaller=high, larger=x, body=smaller.
        // So: if body == larger → this branch is the LOW (body = low). if body == smaller → this branch is the HIGH (body = high).
        let b1_is_low = b1 == c1_l;
        let b2_is_low = b2 == c2_l;
        if b1_is_low == b2_is_low { return; }
        let (low, high) = if b1_is_low { (bound1, bound2) } else { (bound2, bound1) };
        let replacement = format!("{}.clamp({}, {})", x1, low, high);
        // Offense range: cond1 only (from node.if_keyword_loc start to end of cond1? Actually fixture:
        //   "if x < low" → column 0..10 (the whole `if <cond1>` line)
        //   "if high < x" → column 0..11
        // So: from if_keyword start to end of cond1.
        let start = node.if_keyword_loc().map(|l| l.start_offset()).unwrap_or_else(|| node.location().start_offset());
        let end = {
            let c = node.predicate();
            c.location().end_offset()
        };
        let msg = format!("Use `{}` instead of `if/elsif/else`.", replacement);
        // Correction: determine if this is top-level if or elsif
        let is_elsif = node.if_keyword_loc()
            .map(|l| &source[l.start_offset()..l.end_offset()] == "elsif")
            .unwrap_or(false);
        let correction = if is_elsif {
            // Replace from elsif keyword through end of else_branch with "else\n<indent><replacement>"
            let b3_node = {
                let items: Vec<_> = else_stmts.body().iter().collect();
                items.into_iter().next().unwrap().location()
            };
            let elsif_start = start;
            let line_start = source[..b3_node.start_offset()].rfind('\n').map_or(0, |p| p + 1);
            let indent = &source[line_start..b3_node.start_offset()];
            let new_text = format!("else\n{}{}", indent, replacement);
            Correction::replace(elsif_start, b3_node.end_offset(), new_text)
        } else {
            // Replace whole IfNode location (which covers through `end`)
            let loc = node.location();
            Correction::replace(loc.start_offset(), loc.end_offset(), replacement)
        };
        self.offenses.push(
            self.ctx.offense_with_range("Style/ComparableClamp", &msg, Severity::Convention, start, end)
                .with_correction(correction),
        );
    }

    fn check_array_chain(&mut self, node: &ruby_prism::CallNode<'a>) {
        // Outer call: [..., ...].min   or   [..., ...].max
        let source = self.ctx.source;
        let m = node_name!(node);
        let outer = match &*m {
            "min" | "max" => m.to_string(),
            _ => return,
        };
        if node.arguments().is_some() { return; }
        let recv = match node.receiver() { Some(r) => r, None => return };
        let arr = match recv.as_array_node() { Some(a) => a, None => return };
        // Must be `[...]` not `%w[...]`
        if let Some(o) = arr.opening_loc() {
            let o_txt = &source[o.start_offset()..o.end_offset()];
            if !o_txt.starts_with('[') { return; }
        }
        let elems: Vec<_> = arr.elements().iter().collect();
        if elems.len() != 2 { return; }
        // One of the two elements should be a `[a, b].max` or `[a, b].min` call; the other a plain operand.
        let inner_opposite = if outer == "min" { "max" } else { "min" };
        // Try each as inner
        let mut correctable = false;
        let mut found = false;
        for (i, other_i) in [(0usize, 1usize), (1, 0)] {
            let inner = &elems[i];
            let other = &elems[other_i];
            let c = match inner.as_call_node() { Some(v) => v, None => continue };
            let c_m = node_name!(c);
            if &*c_m != inner_opposite { continue; }
            if c.arguments().is_some() { continue; }
            let ir = match c.receiver() { Some(r) => r, None => continue };
            let iarr = match ir.as_array_node() { Some(a) => a, None => continue };
            if let Some(o) = iarr.opening_loc() {
                let o_txt = &source[o.start_offset()..o.end_offset()];
                if !o_txt.starts_with('[') { continue; }
            }
            let ie: Vec<_> = iarr.elements().iter().collect();
            if ie.len() != 2 { continue; }
            // Pattern match: based on outer.min/inner.max vs outer.max/inner.min
            // outer.min + inner.max = [[x,low].max, high].min (correctable)
            // outer.max + inner.min = [low, [x,high].min].max (not correctable per fixtures)
            found = true;
            correctable = outer == "min";
            // ensure `other` is not itself a similar pattern; just check it's an operand
            let _ = other;
            break;
        }
        if !found { return; }
        let loc = node.location();
        let start = loc.start_offset();
        let end = loc.end_offset();
        let msg = "Use `Comparable#clamp` instead.";
        let off = self.ctx.offense_with_range("Style/ComparableClamp", msg, Severity::Convention, start, end);
        // No correction emitted for array-chain pattern (fixture has no `corrected =`)
        let _ = correctable;
        self.offenses.push(off);
    }
}

impl<'a> Visit<'a> for V<'a> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'a>) {
        self.check_if(node);
        ruby_prism::visit_if_node(self, node);
    }
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'a>) {
        self.check_array_chain(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/ComparableClamp", |_cfg| Some(Box::new(ComparableClamp::new())));
