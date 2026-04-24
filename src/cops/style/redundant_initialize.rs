//! Style/RedundantInitialize
//!
//! Flags `initialize` methods that are either empty (no args, no body) or
//! only call `super`/`super(...)` with the same arguments.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{DefNode, Node};

const MSG: &str = "Remove unnecessary `initialize` method.";
const MSG_EMPTY: &str = "Remove unnecessary empty `initialize` method.";

pub struct RedundantInitialize {
    allow_comments: bool,
}

impl Default for RedundantInitialize {
    fn default() -> Self { Self { allow_comments: true } }
}

impl RedundantInitialize {
    pub fn new() -> Self { Self::default() }
    pub fn with_config(allow_comments: bool) -> Self { Self { allow_comments } }
}

fn extract_simple_params(def: &DefNode) -> Option<Vec<String>> {
    let Some(params) = def.parameters() else { return Some(Vec::new()); };
    if params.rest().is_some() { return None; }
    if params.keyword_rest().is_some() { return None; }
    if params.optionals().iter().next().is_some() { return None; }
    if params.keywords().iter().next().is_some() { return None; }

    let mut names = Vec::new();
    for p in params.requireds().iter() {
        if let Some(req) = p.as_required_parameter_node() {
            names.push(String::from_utf8_lossy(req.name().as_slice()).into_owned());
        } else {
            return None;
        }
    }
    Some(names)
}

fn has_any_unmatchable(def: &DefNode) -> bool {
    let Some(params) = def.parameters() else { return false };
    if params.rest().is_some() { return true; }
    if params.keyword_rest().is_some() { return true; }
    for p in params.requireds().iter() {
        if matches!(p, Node::ForwardingParameterNode { .. }) { return true; }
        if let Some(req) = p.as_required_parameter_node() {
            let name = String::from_utf8_lossy(req.name().as_slice());
            if name == "_" { return true; }
        }
    }
    false
}

fn check_super(body: &Node, def: &DefNode) -> bool {
    match body {
        Node::SuperNode { .. } => {
            let s = body.as_super_node().unwrap();
            let arg_list: Vec<_> = s
                .arguments()
                .map(|a| a.arguments().iter().collect())
                .unwrap_or_default();
            let Some(params) = extract_simple_params(def) else { return false };
            if arg_list.len() != params.len() { return false; }
            for (arg, pname) in arg_list.iter().zip(params.iter()) {
                match arg {
                    Node::LocalVariableReadNode { .. } => {
                        let n = arg.as_local_variable_read_node().unwrap();
                        let ident = String::from_utf8_lossy(n.name().as_slice());
                        if ident.as_ref() != pname.as_str() { return false; }
                    }
                    _ => return false,
                }
            }
            true
        }
        Node::ForwardingSuperNode { .. } => extract_simple_params(def).is_some(),
        _ => false,
    }
}

fn has_internal_comments(def: &DefNode, source: &str) -> bool {
    let start = def.location().start_offset();
    let end = def.location().end_offset();
    let slice = &source[start..end];
    slice.lines().skip(1).any(|l| l.trim_start().starts_with('#'))
}

impl Cop for RedundantInitialize {
    fn name(&self) -> &'static str { "Style/RedundantInitialize" }

    fn check_def(&self, node: &DefNode, ctx: &CheckContext) -> Vec<Offense> {
        let mname = node_name!(node);
        if mname != "initialize" { return vec![]; }
        if has_any_unmatchable(node) { return vec![]; }
        if self.allow_comments && has_internal_comments(node, ctx.source) {
            return vec![];
        }

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let body = node.body();

        let msg = match body {
            None => {
                let has_args = node
                    .parameters()
                    .map(|p| p.requireds().iter().next().is_some())
                    .unwrap_or(false);
                if has_args { return vec![]; }
                MSG_EMPTY
            }
            Some(b) => {
                let stmts = if let Some(s) = b.as_statements_node() {
                    s.body().iter().collect::<Vec<_>>()
                } else {
                    vec![b]
                };
                if stmts.len() != 1 { return vec![]; }
                if !check_super(&stmts[0], node) { return vec![]; }
                MSG
            }
        };

        let correction = Correction::delete(start, end);

        vec![ctx
            .offense_with_range(self.name(), msg, Severity::Convention, start, end)
            .with_correction(correction)]
    }
}

crate::register_cop!("Style/RedundantInitialize", |cfg| {
    let allow_comments = cfg.get_cop_config("Style/RedundantInitialize")
        .and_then(|c| c.raw.get("AllowComments"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    Some(Box::new(RedundantInitialize::with_config(allow_comments)))
});
