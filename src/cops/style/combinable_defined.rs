//! Style/CombinableDefined — combine nested `defined?` calls joined by `&&`/`and`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/combinable_defined.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/CombinableDefined";
const MSG: &str = "Combine nested `defined?` calls.";

#[derive(Default)]
pub struct CombinableDefined;

impl CombinableDefined {
    pub fn new() -> Self { Self }
}

impl Cop for CombinableDefined {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = AndVisitor { ctx, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct AndVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visit<'_> for AndVisitor<'a> {
    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        self.check_and(node);
        ruby_prism::visit_and_node(self, node);
    }
}

#[derive(Clone)]
struct Leaf {
    is_defined: bool,
    leaf_start: usize,
    leaf_end: usize,
    is_last_in_parent: bool,
    subject_range: Option<(usize, usize)>,
    namespace_range: Option<(usize, usize)>,
}

impl<'a> AndVisitor<'a> {
    fn check_and(&mut self, node: &ruby_prism::AndNode) {
        let mut leaves: Vec<Leaf> = Vec::new();
        flatten_and(node, &mut leaves);
        if let Some(last) = leaves.last_mut() {
            last.is_last_in_parent = true;
        }

        if leaves.is_empty() || !leaves.iter().all(|l| l.is_defined) {
            return;
        }

        // Compute namespace texts (a leaf contributes its namespace, if any).
        let namespace_texts: Vec<&str> = leaves.iter()
            .filter_map(|l| l.namespace_range.map(|(s, e)| self.ctx.src(s, e)))
            .collect();

        let mut edits: Vec<Edit> = Vec::new();
        let mut redundant_count = 0;
        for l in &leaves {
            if let Some((ss, se)) = l.subject_range {
                let sub = self.ctx.src(ss, se);
                if namespace_texts.iter().any(|n| *n == sub) {
                    redundant_count += 1;
                    if let Some(edit) = remove_term_edit(self.ctx, l) {
                        edits.push(edit);
                    }
                }
            }
        }

        if redundant_count == 0 {
            return;
        }

        let n_start = node.location().start_offset();
        let n_end = node.location().end_offset();
        let offense = self.ctx
            .offense_with_range(COP_NAME, MSG, Severity::Convention, n_start, n_end)
            .with_correction(Correction { edits });

        self.offenses.push(offense);
    }
}

/// Walk an and-tree, push each leaf (non-and child) into `out`.
/// All leaves get is_last_in_parent=false; caller flips the LAST one.
fn flatten_and<'a>(and_node: &ruby_prism::AndNode<'a>, out: &mut Vec<Leaf>) {
    let left = and_node.left();
    let right = and_node.right();
    visit_side(&left, false, out);
    visit_side(&right, false, out);
}

fn visit_side<'a>(n: &Node<'a>, is_last_in_parent: bool, out: &mut Vec<Leaf>) {
    if let Node::AndNode { .. } = n {
        let an = n.as_and_node().unwrap();
        flatten_and(&an, out);
        return;
    }
    let mut info = Leaf {
        is_defined: false,
        leaf_start: n.location().start_offset(),
        leaf_end: n.location().end_offset(),
        is_last_in_parent,
        subject_range: None,
        namespace_range: None,
    };
    if let Node::DefinedNode { .. } = n {
        let dn = n.as_defined_node().unwrap();
        let val = dn.value();
        info.is_defined = true;
        info.subject_range = Some((val.location().start_offset(), val.location().end_offset()));
        if let Some(r) = namespace_offsets(&val) {
            info.namespace_range = Some(r);
        }
    }
    out.push(info);
}

fn namespace_offsets(node: &Node) -> Option<(usize, usize)> {
    match node {
        Node::ConstantPathNode { .. } => {
            let cp = node.as_constant_path_node().unwrap();
            let parent = cp.parent()?;
            Some((parent.location().start_offset(), parent.location().end_offset()))
        }
        Node::CallNode { .. } => {
            let c = node.as_call_node().unwrap();
            let r = c.receiver()?;
            Some((r.location().start_offset(), r.location().end_offset()))
        }
        _ => None,
    }
}

fn remove_term_edit(ctx: &CheckContext, leaf: &Leaf) -> Option<Edit> {
    let bytes = ctx.source.as_bytes();
    if leaf.is_last_in_parent {
        // RHS: scan backwards from leaf.start to find preceding `&&`/`and`.
        let mut pos = leaf.leaf_start;
        loop {
            if pos == 0 { return None; }
            pos -= 1;
            let after = &ctx.source[pos..];
            if after.starts_with("&&") || after.starts_with("and ") || after.starts_with("and\t") {
                // Found op start at pos.
                break;
            }
        }
        let begin_pos = pos.saturating_sub(1);
        let end_pos = leaf.leaf_end;
        let (_lo, hi) = trim_right_space(bytes, end_pos);
        Some(Edit { start_offset: begin_pos, end_offset: hi, replacement: String::new() })
    } else {
        // LHS: scan forwards from leaf.end to find following `&&`/`and`.
        let mut pos = leaf.leaf_end;
        loop {
            if pos >= ctx.source.len() { return None; }
            let after = &ctx.source[pos..];
            if after.starts_with("&&") {
                pos += 2;
                break;
            }
            if after.starts_with("and") {
                pos += 3;
                break;
            }
            pos += 1;
        }
        let begin_pos = leaf.leaf_start;
        let (_lo, hi) = trim_right_space(bytes, pos);
        Some(Edit { start_offset: begin_pos, end_offset: hi, replacement: String::new() })
    }
}

fn trim_right_space(bytes: &[u8], hi: usize) -> (usize, usize) {
    let mut h = hi;
    while h < bytes.len() && (bytes[h] == b' ' || bytes[h] == b'\t') {
        h += 1;
    }
    (0, h)
}

crate::register_cop!("Style/CombinableDefined", |_cfg| Some(Box::new(CombinableDefined::new())));
