//! Lint/ConstantReassignment cop
//!
//! Translates RuboCop's ConstantReassignment. Tracks constants defined in the
//! current file and namespace; flags subsequent re-assignments via
//! `NAME = value` (`casgn`).

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashMap;

#[derive(Default)]
pub struct ConstantReassignment;

impl ConstantReassignment {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for ConstantReassignment {
    fn name(&self) -> &'static str {
        "Lint/ConstantReassignment"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor {
            ctx,
            offenses: Vec::new(),
            namespaces: Vec::new(),
            ancestor_kinds: Vec::new(),
            constants: HashMap::new(),
        };
        let stmts = node.statements();
        for s in stmts.body().iter() {
            visitor.walk(&s);
        }
        visitor.offenses
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AncestorKind {
    Module,
    Class,
    Begin,    // BeginNode (kwbegin) — transparent
    Constant, // CASGN ancestor — transparent for "simple" check
    LiteralCollection, // ArrayNode/HashNode that's RHS of a casgn — transparent
    Freeze,   // .freeze call wrapping casgn RHS — transparent
    Other,    // anything else — disqualifies simple_assignment / unconditional
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    namespaces: Vec<String>,
    ancestor_kinds: Vec<AncestorKind>,
    constants: HashMap<String, ()>,
}

fn fq(namespaces: &[String], short: &str) -> String {
    let mut s = String::from("::");
    if !namespaces.is_empty() {
        s.push_str(&namespaces.join("::"));
        s.push_str("::");
    }
    s.push_str(short);
    s
}

/// Walk a ConstantPathNode collecting non-rightmost segments into `ns` (left-to-right).
/// Returns Some(absolute) on success — `absolute` is true if path has cbase root.
/// Returns None if any segment is a non-constant (e.g., variable).
fn collect_constant_path_namespace(
    cp: &ruby_prism::ConstantPathNode,
    ns: &mut Vec<String>,
) -> Option<bool> {
    let absolute;
    let mut left_chain: Vec<String> = Vec::new();
    if let Some(parent) = cp.parent() {
        match &parent {
            Node::ConstantReadNode { .. } => {
                let cr = parent.as_constant_read_node().unwrap();
                left_chain.push(String::from_utf8_lossy(cr.name().as_slice()).to_string());
                absolute = false;
            }
            Node::ConstantPathNode { .. } => {
                let inner = parent.as_constant_path_node().unwrap();
                let mut inner_ns = Vec::new();
                let inner_abs = collect_constant_path_namespace(&inner, &mut inner_ns)?;
                absolute = inner_abs;
                left_chain.extend(inner_ns);
                let inner_short =
                    inner.name().map(|n| String::from_utf8_lossy(n.as_slice()).to_string())?;
                left_chain.push(inner_short);
            }
            Node::SelfNode { .. } => {
                absolute = false;
            }
            _ => return None, // variable receiver
        }
    } else {
        absolute = true;
    }
    ns.extend(left_chain);
    Some(absolute)
}

fn class_or_module_name(identifier: &Node) -> Option<(Vec<String>, String, bool)> {
    match identifier {
        Node::ConstantReadNode { .. } => {
            let cr = identifier.as_constant_read_node().unwrap();
            let name = String::from_utf8_lossy(cr.name().as_slice()).to_string();
            Some((vec![], name, false))
        }
        Node::ConstantPathNode { .. } => {
            let cp = identifier.as_constant_path_node().unwrap();
            let short = cp.name().map(|n| String::from_utf8_lossy(n.as_slice()).to_string())?;
            let mut ns = Vec::new();
            let absolute = collect_constant_path_namespace(&cp, &mut ns)?;
            Some((ns, short, absolute))
        }
        _ => None,
    }
}

impl<'a> Visitor<'a> {
    fn simple_assignment(&self) -> bool {
        // RuboCop: walk ancestors innermost-out. Early-return true on module/class.
        // begin/casgn/literal/freeze are transparent. Anything else disqualifies.
        for k in self.ancestor_kinds.iter().rev() {
            match k {
                AncestorKind::Module | AncestorKind::Class => return true,
                AncestorKind::Begin
                | AncestorKind::Constant
                | AncestorKind::LiteralCollection
                | AncestorKind::Freeze => continue,
                AncestorKind::Other => return false,
            }
        }
        true
    }

    fn unconditional(&self) -> bool {
        // Class/module definition — only allowed ancestors are begin/module/class.
        self.ancestor_kinds.iter().all(|k| {
            matches!(k, AncestorKind::Begin | AncestorKind::Module | AncestorKind::Class)
        })
    }

    fn handle_class_or_module(&mut self, identifier: &Node, body: Option<Node>, kind: AncestorKind) {
        let parsed = class_or_module_name(identifier);
        let mut pushed_namespaces: Vec<String> = vec![];

        if let Some((extra_ns, short, absolute)) = parsed {
            // Track the class/module constant only if unconditional.
            if self.unconditional() {
                let base_ns = if absolute { vec![] } else { self.namespaces.clone() };
                let mut full_ns = base_ns;
                full_ns.extend(extra_ns.iter().cloned());
                let fq_name = fq(&full_ns, &short);
                self.constants.entry(fq_name).or_insert(());
            }
            pushed_namespaces.push(short);
        }

        let pushed_count = pushed_namespaces.len();
        for n in pushed_namespaces {
            self.namespaces.push(n);
        }
        self.ancestor_kinds.push(kind);

        if let Some(b) = body {
            self.walk(&b);
        }

        self.ancestor_kinds.pop();
        for _ in 0..pushed_count {
            self.namespaces.pop();
        }
    }

    fn handle_constant_write_lhs(
        &mut self,
        absolute: bool,
        extra_ns: Vec<String>,
        short: String,
        loc_start: usize,
        loc_end: usize,
    ) {
        if !self.simple_assignment() {
            return;
        }
        let base_ns = if absolute { vec![] } else { self.namespaces.clone() };
        let mut full_ns = base_ns;
        full_ns.extend(extra_ns.iter().cloned());
        let fq_name = fq(&full_ns, &short);

        if self.constants.contains_key(&fq_name) {
            let mut display = extra_ns.clone();
            display.push(short.clone());
            let display_name = display.join("::");
            let msg = format!("Constant `{}` is already assigned in this namespace.", display_name);
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/ConstantReassignment",
                &msg,
                Severity::Warning,
                loc_start,
                loc_end,
            ));
        } else {
            self.constants.insert(fq_name, ());
        }
    }

    fn walk(&mut self, n: &Node<'_>) {
        match n {
            Node::StatementsNode { .. } => {
                let s = n.as_statements_node().unwrap();
                for stmt in s.body().iter() {
                    self.walk(&stmt);
                }
            }
            Node::ClassNode { .. } => {
                let c = n.as_class_node().unwrap();
                let id = c.constant_path();
                self.handle_class_or_module(&id, c.body(), AncestorKind::Class);
            }
            Node::ModuleNode { .. } => {
                let m = n.as_module_node().unwrap();
                let id = m.constant_path();
                self.handle_class_or_module(&id, m.body(), AncestorKind::Module);
            }
            Node::BeginNode { .. } => {
                let b = n.as_begin_node().unwrap();
                self.ancestor_kinds.push(AncestorKind::Begin);
                if let Some(s) = b.statements() {
                    for stmt in s.body().iter() {
                        self.walk(&stmt);
                    }
                }
                self.ancestor_kinds.pop();
                // Don't descend into rescue/else/ensure — RuboCop's begin_type? is transparent
                // but rescue branches make casgn non-simple via Other-ness.
            }
            Node::ConstantWriteNode { .. } => {
                let w = n.as_constant_write_node().unwrap();
                let name = String::from_utf8_lossy(w.name().as_slice()).to_string();
                let l = w.location();
                self.handle_constant_write_lhs(false, vec![], name, l.start_offset(), l.end_offset());

                self.ancestor_kinds.push(AncestorKind::Constant);
                self.walk(&w.value());
                self.ancestor_kinds.pop();
            }
            Node::ConstantPathWriteNode { .. } => {
                let w = n.as_constant_path_write_node().unwrap();
                let target = w.target();
                let mut ns = Vec::new();
                let abs_opt = collect_constant_path_namespace(&target, &mut ns);
                let short_opt =
                    target.name().map(|nm| String::from_utf8_lossy(nm.as_slice()).to_string());

                if let (Some(short), Some(absolute)) = (short_opt, abs_opt) {
                    let l = w.location();
                    self.handle_constant_write_lhs(
                        absolute,
                        ns,
                        short,
                        l.start_offset(),
                        l.end_offset(),
                    );
                }

                self.ancestor_kinds.push(AncestorKind::Constant);
                self.walk(&w.value());
                self.ancestor_kinds.pop();
            }
            Node::ArrayNode { .. } => {
                self.ancestor_kinds.push(AncestorKind::LiteralCollection);
                let a = n.as_array_node().unwrap();
                for el in a.elements().iter() {
                    self.walk(&el);
                }
                self.ancestor_kinds.pop();
            }
            Node::HashNode { .. } => {
                self.ancestor_kinds.push(AncestorKind::LiteralCollection);
                let h = n.as_hash_node().unwrap();
                for el in h.elements().iter() {
                    self.walk(&el);
                }
                self.ancestor_kinds.pop();
            }
            Node::AssocNode { .. } => {
                let a = n.as_assoc_node().unwrap();
                self.walk(&a.key());
                self.walk(&a.value());
            }
            Node::CallNode { .. } => {
                let c = n.as_call_node().unwrap();
                let method = node_name!(c);
                if method == "remove_const" {
                    let recv_ok = match c.receiver() {
                        None => true,
                        Some(Node::SelfNode { .. }) => true,
                        _ => false,
                    };
                    if recv_ok {
                        if let Some(args) = c.arguments() {
                            let arg_vec: Vec<_> = args.arguments().iter().collect();
                            if arg_vec.len() == 1 {
                                let constant_name = match &arg_vec[0] {
                                    Node::SymbolNode { .. } => {
                                        let s = arg_vec[0].as_symbol_node().unwrap();
                                        s.value_loc().map(|v| {
                                            String::from_utf8_lossy(
                                                &self.ctx.source.as_bytes()
                                                    [v.start_offset()..v.end_offset()],
                                            )
                                            .to_string()
                                        })
                                    }
                                    Node::StringNode { .. } => {
                                        let s = arg_vec[0].as_string_node().unwrap();
                                        let cl = s.content_loc();
                                        Some(
                                            String::from_utf8_lossy(
                                                &self.ctx.source.as_bytes()
                                                    [cl.start_offset()..cl.end_offset()],
                                            )
                                            .to_string(),
                                        )
                                    }
                                    _ => None,
                                };
                                if let Some(cname) = constant_name {
                                    if !self.namespaces.is_empty() {
                                        let fq_name = fq(&self.namespaces.clone(), &cname);
                                        self.constants.remove(&fq_name);
                                    }
                                }
                            }
                        }
                    }
                    return;
                }

                // .freeze on something — treat as transparent so `[FOO=:a].freeze` still finds casgns.
                let is_freeze = method == "freeze" && c.arguments().is_none();
                if is_freeze {
                    if let Some(recv) = c.receiver() {
                        self.ancestor_kinds.push(AncestorKind::Freeze);
                        self.walk(&recv);
                        self.ancestor_kinds.pop();
                    }
                    return;
                }

                // Generic call: descend into receiver/args/block under Other.
                self.ancestor_kinds.push(AncestorKind::Other);
                if let Some(recv) = c.receiver() {
                    self.walk(&recv);
                }
                if let Some(args) = c.arguments() {
                    for a in args.arguments().iter() {
                        self.walk(&a);
                    }
                }
                if let Some(blk) = c.block() {
                    self.walk(&blk);
                }
                self.ancestor_kinds.pop();
            }
            Node::IfNode { .. }
            | Node::UnlessNode { .. }
            | Node::CaseNode { .. }
            | Node::CaseMatchNode { .. }
            | Node::WhileNode { .. }
            | Node::UntilNode { .. } => {
                // Walk children under Other so inner casgns fail simple_assignment,
                // but inner class/module bodies still produce offenses for their casgns
                // (early-return on Class/Module ancestor).
                self.ancestor_kinds.push(AncestorKind::Other);
                self.walk_children_generic(n);
                self.ancestor_kinds.pop();
            }
            Node::BlockNode { .. } | Node::LambdaNode { .. } => {
                // Inside blocks, casgns are not tracked (RuboCop behavior).
                self.ancestor_kinds.push(AncestorKind::Other);
                self.walk_children_generic(n);
                self.ancestor_kinds.pop();
            }
            _ => {
                self.walk_children_generic(n);
            }
        }
    }

    fn walk_children_generic(&mut self, n: &Node<'_>) {
        // Use the Visit trait dispatcher for generic recursion.
        // Simpler: iterate via match on common nodes that contain children.
        match n {
            Node::IfNode { .. } => {
                let i = n.as_if_node().unwrap();
                self.walk(&i.predicate());
                if let Some(s) = i.statements() {
                    for stmt in s.body().iter() {
                        self.walk(&stmt);
                    }
                }
                if let Some(sub) = i.subsequent() {
                    self.walk(&sub);
                }
            }
            Node::UnlessNode { .. } => {
                let u = n.as_unless_node().unwrap();
                self.walk(&u.predicate());
                if let Some(s) = u.statements() {
                    for stmt in s.body().iter() {
                        self.walk(&stmt);
                    }
                }
                if let Some(sub) = u.else_clause() {
                    if let Some(s) = sub.statements() {
                        for stmt in s.body().iter() {
                            self.walk(&stmt);
                        }
                    }
                }
            }
            Node::CaseNode { .. } => {
                let c = n.as_case_node().unwrap();
                if let Some(p) = c.predicate() {
                    self.walk(&p);
                }
                for cond in c.conditions().iter() {
                    self.walk(&cond);
                }
                if let Some(con) = c.else_clause() {
                    if let Some(s) = con.statements() {
                        for stmt in s.body().iter() {
                            self.walk(&stmt);
                        }
                    }
                }
            }
            Node::CaseMatchNode { .. } => {
                let c = n.as_case_match_node().unwrap();
                if let Some(p) = c.predicate() {
                    self.walk(&p);
                }
                for cond in c.conditions().iter() {
                    self.walk(&cond);
                }
                if let Some(con) = c.else_clause() {
                    if let Some(s) = con.statements() {
                        for stmt in s.body().iter() {
                            self.walk(&stmt);
                        }
                    }
                }
            }
            Node::WhileNode { .. } => {
                let w = n.as_while_node().unwrap();
                self.walk(&w.predicate());
                if let Some(s) = w.statements() {
                    for stmt in s.body().iter() {
                        self.walk(&stmt);
                    }
                }
            }
            Node::UntilNode { .. } => {
                let u = n.as_until_node().unwrap();
                self.walk(&u.predicate());
                if let Some(s) = u.statements() {
                    for stmt in s.body().iter() {
                        self.walk(&stmt);
                    }
                }
            }
            Node::BlockNode { .. } => {
                let b = n.as_block_node().unwrap();
                if let Some(body) = b.body() {
                    self.walk(&body);
                }
            }
            Node::LambdaNode { .. } => {
                let lam = n.as_lambda_node().unwrap();
                if let Some(body) = lam.body() {
                    self.walk(&body);
                }
            }
            Node::ParenthesesNode { .. } => {
                let p = n.as_parentheses_node().unwrap();
                if let Some(b) = p.body() {
                    self.walk(&b);
                }
            }
            Node::AndNode { .. } => {
                let a = n.as_and_node().unwrap();
                self.walk(&a.left());
                self.walk(&a.right());
            }
            Node::OrNode { .. } => {
                let o = n.as_or_node().unwrap();
                self.walk(&o.left());
                self.walk(&o.right());
            }
            _ => {}
        }
    }
}

impl<'a> Visit<'_> for Visitor<'a> {}

crate::register_cop!("Lint/ConstantReassignment", |_cfg| {
    Some(Box::new(ConstantReassignment::new()))
});
