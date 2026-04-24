//! Lint/RedundantDirGlobSort
//!
//! Since Ruby 3.0, `Dir.glob` and `Dir[]` return sorted results by default,
//! so an explicit trailing `.sort` is redundant.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{CallNode, Node};

const MSG: &str = "Remove redundant `sort`.";

#[derive(Default)]
pub struct RedundantDirGlobSort;

impl RedundantDirGlobSort {
    pub fn new() -> Self {
        Self
    }
}

fn is_dir_glob_call(recv: &CallNode) -> bool {
    let method: std::borrow::Cow<'_, str> = node_name!(recv);
    if method != "glob" && method != "[]" {
        return false;
    }
    let Some(inner_recv) = recv.receiver() else { return false };
    // Inner receiver must be the constant `Dir` (or `::Dir`).
    match &inner_recv {
        Node::ConstantReadNode { .. } => {
            let cr = inner_recv.as_constant_read_node().unwrap();
            String::from_utf8_lossy(cr.name().as_slice()) == "Dir"
        }
        Node::ConstantPathNode { .. } => {
            let cp = inner_recv.as_constant_path_node().unwrap();
            // `::Dir` has no parent; `name()` is `Dir`.
            if cp.parent().is_some() {
                return false;
            }
            if let Some(n) = cp.name() {
                String::from_utf8_lossy(n.as_slice()) == "Dir"
            } else {
                false
            }
        }
        _ => false,
    }
}

fn multiple_argument(recv: &CallNode) -> bool {
    let Some(args) = recv.arguments() else { return false };
    let list: Vec<_> = args.arguments().iter().collect();
    if list.len() >= 2 {
        return true;
    }
    if let Some(first) = list.first() {
        if matches!(first, Node::SplatNode { .. }) {
            return true;
        }
    }
    false
}

impl Cop for RedundantDirGlobSort {
    fn name(&self) -> &'static str {
        "Lint/RedundantDirGlobSort"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(3, 0) {
            return vec![];
        }
        let method = node_name!(node);
        if method != "sort" {
            return vec![];
        }
        if node.arguments().is_some() || node.block().is_some() {
            return vec![];
        }
        let Some(recv_node) = node.receiver() else { return vec![] };
        let Some(recv_call) = recv_node.as_call_node() else { return vec![] };
        if !is_dir_glob_call(&recv_call) {
            return vec![];
        }
        if multiple_argument(&recv_call) {
            return vec![];
        }

        // `selector` = `sort` identifier range. For a normal call `foo.sort`,
        // selector starts after `.` and spans the method name.
        let selector_loc = node.message_loc().expect("sort call must have selector");
        let sel_start = selector_loc.start_offset();
        let sel_end = selector_loc.end_offset();

        // Autocorrect: remove `.sort` including the leading dot.
        let dot_loc = node.call_operator_loc();
        let edit_start = dot_loc.map(|l| l.start_offset()).unwrap_or(sel_start);
        let correction = Correction::delete(edit_start, sel_end);

        vec![ctx
            .offense_with_range(self.name(), MSG, self.severity(), sel_start, sel_end)
            .with_correction(correction)]
    }
}

crate::register_cop!("Lint/RedundantDirGlobSort", |_cfg| Some(Box::new(RedundantDirGlobSort::new())));
