//! Lint/ShadowingOuterLocalVariable - block parameters shadow outer local variables.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/v1.85.0/lib/rubocop/cop/lint/shadowing_outer_local_variable.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};

const COP: &str = "Lint/ShadowingOuterLocalVariable";

/// Identifies a position within a branching tree as Vec<(conditional_id, branch_index)>.
type BranchPath = Vec<(usize, usize)>;

#[derive(Default)]
pub struct ShadowingOuterLocalVariable;

impl ShadowingOuterLocalVariable {
    pub fn new() -> Self { Self }
}

impl Cop for ShadowingOuterLocalVariable {
    fn name(&self) -> &'static str { COP }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();
        let mut v = V {
            ctx,
            scopes: vec![Scope::default()],
            current_branch: BranchPath::new(),
            in_ractor_stack: Vec::new(),
            assignment_range_stack: Vec::new(),
            out: Vec::new(),
        };
        use ruby_prism::Visit;
        v.visit(&tree);
        v.out
    }
}

#[derive(Default, Clone)]
struct Scope {
    locals: Vec<LocalDecl>,
    is_block: bool,
    /// Names already reported as shadowing in this block (dedup).
    reported: Vec<String>,
}

#[derive(Clone)]
struct LocalDecl {
    name: String,
    /// Byte offset of declaration start.
    decl_start: usize,
    /// Byte offset of declaration end (full enclosing write/param node range).
    decl_end: usize,
    /// Branch path at the time of declaration.
    branch: BranchPath,
}

struct V<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    scopes: Vec<Scope>,
    current_branch: BranchPath,
    in_ractor_stack: Vec<bool>,
    /// Stack of (start, end) ranges for currently-active local variable assignments,
    /// so block params inside the RHS can detect "same statement" shadowing.
    assignment_range_stack: Vec<(usize, usize, String)>,
    out: Vec<Offense>,
}

impl<'a, 'b> V<'a, 'b> {
    fn add_local(&mut self, name: String, start: usize, end: usize) {
        if let Some(s) = self.scopes.last_mut() {
            if !s.locals.iter().any(|d| d.name == name) {
                s.locals.push(LocalDecl {
                    name,
                    decl_start: start,
                    decl_end: end,
                    branch: self.current_branch.clone(),
                });
            }
        }
    }

    fn in_ractor(&self) -> bool {
        *self.in_ractor_stack.last().unwrap_or(&false)
    }

    /// Returns Some(decl) when an outer scope has a same-named declaration that shadows.
    fn find_shadowing_outer(&self, name: &str, param_start: usize) -> Option<&LocalDecl> {
        for i in (0..self.scopes.len().saturating_sub(1)).rev() {
            let sc = &self.scopes[i];
            if let Some(d) = sc.locals.iter().find(|d| d.name == name) {
                // Outer must be declared lexically before this param.
                if d.decl_start >= param_start { return None; }
                // Outer's branch path must be compatible: must be a prefix of current path
                // up to the point of divergence — i.e., must NOT diverge into a different branch.
                if !branch_compatible(&d.branch, &self.current_branch) { return None; }
                return Some(d);
            }
            if !sc.is_block {
                return None;
            }
        }
        None
    }

    fn report_param(&mut self, name: &str, full_start: usize, full_end: usize) {
        if name.is_empty() || name.starts_with('_') { return; }
        let in_block = self.scopes.last().map(|s| s.is_block).unwrap_or(false);
        if !in_block { return; }
        if self.in_ractor() { return; }

        // Skip if param sits inside an active local-variable assignment with the same name
        // (e.g. `foo = bar { |foo| ... }`).
        if self.assignment_range_stack.iter().any(|(s, e, n)| n == name && *s <= full_start && full_end <= *e) {
            return;
        }

        // Dedup within current block scope: only report a given name once.
        if let Some(sc) = self.scopes.last() {
            if sc.reported.iter().any(|n| n == name) { return; }
        }

        let outer = match self.find_shadowing_outer(name, full_start) { Some(o) => o, None => return };
        let _ = outer;

        if let Some(sc) = self.scopes.last_mut() {
            sc.reported.push(name.to_string());
        }

        let msg = format!("Shadowing outer local variable - `{}`.", name);
        self.out.push(self.ctx.offense_with_range(
            COP, &msg, Severity::Warning, full_start, full_end,
        ));
    }
}

/// Outer is "compatible" with current if outer's branch path agrees with current's path
/// at every position they share. That is, walking both paths element-by-element, at any
/// shared depth (same conditional id), the chosen branch index must match. Outer's path
/// must also be no-deeper-than current at the point of disagreement (outer must be in an
/// ancestor or sibling-of-ancestor — actually: it must be in a prefix-equal branch).
fn branch_compatible(outer: &BranchPath, current: &BranchPath) -> bool {
    // Compare element by element. At first conditional that both share, branch indices must match.
    // If outer extends to a conditional not present in current's path, they're in different
    // sibling subtrees → not compatible.
    let mut oi = 0;
    let mut ci = 0;
    while oi < outer.len() {
        // Find a matching conditional id in current at or after position ci.
        let (cid, oidx) = outer[oi];
        // Look up cid in current[ci..]. If found, compare branch indices.
        let mut found = None;
        for k in ci..current.len() {
            if current[k].0 == cid { found = Some(k); break; }
        }
        match found {
            Some(k) => {
                if current[k].1 != oidx { return false; }
                ci = k + 1;
                oi += 1;
            }
            None => {
                // Outer is in a conditional that current is NOT in — outer is inside a branch
                // that current never entered. Not compatible.
                return false;
            }
        }
    }
    true
}

fn is_ractor_new_call(call: &ruby_prism::CallNode) -> bool {
    let m = String::from_utf8_lossy(call.name().as_slice()).to_string();
    if m != "new" { return false; }
    let recv = match call.receiver() { Some(r) => r, None => return false };
    if let Some(c) = recv.as_constant_read_node() {
        return String::from_utf8_lossy(c.name().as_slice()) == "Ractor";
    }
    false
}

impl<'a, 'b> ruby_prism::Visit<'_> for V<'a, 'b> {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let loc = node.location();
        let s = loc.start_offset();
        let e = loc.end_offset();
        self.add_local(name.clone(), s, e);
        self.assignment_range_stack.push((s, e, name));
        ruby_prism::visit_local_variable_write_node(self, node);
        self.assignment_range_stack.pop();
    }

    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let loc = node.location();
        self.add_local(name, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }

    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let loc = node.location();
        self.add_local(name, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }

    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let loc = node.location();
        self.add_local(name, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }

    fn visit_required_parameter_node(&mut self, node: &ruby_prism::RequiredParameterNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let loc = node.location();
        self.report_param(&name, loc.start_offset(), loc.end_offset());
        self.add_local(name, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_required_parameter_node(self, node);
    }

    fn visit_optional_parameter_node(&mut self, node: &ruby_prism::OptionalParameterNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let nl = node.name_loc();
        self.report_param(&name, nl.start_offset(), nl.end_offset());
        self.add_local(name, nl.start_offset(), nl.end_offset());
        ruby_prism::visit_optional_parameter_node(self, node);
    }

    fn visit_rest_parameter_node(&mut self, node: &ruby_prism::RestParameterNode) {
        if let Some(name_id) = node.name() {
            let name = String::from_utf8_lossy(name_id.as_slice()).to_string();
            let loc = node.location();
            self.report_param(&name, loc.start_offset(), loc.end_offset());
            self.add_local(name, loc.start_offset(), loc.end_offset());
        }
        ruby_prism::visit_rest_parameter_node(self, node);
    }

    fn visit_keyword_rest_parameter_node(&mut self, node: &ruby_prism::KeywordRestParameterNode) {
        if let Some(name_id) = node.name() {
            let name = String::from_utf8_lossy(name_id.as_slice()).to_string();
            let loc = node.location();
            self.report_param(&name, loc.start_offset(), loc.end_offset());
            self.add_local(name, loc.start_offset(), loc.end_offset());
        }
        ruby_prism::visit_keyword_rest_parameter_node(self, node);
    }

    fn visit_required_keyword_parameter_node(&mut self, node: &ruby_prism::RequiredKeywordParameterNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let nl = node.name_loc();
        self.report_param(&name, nl.start_offset(), nl.end_offset());
        self.add_local(name, nl.start_offset(), nl.end_offset());
        ruby_prism::visit_required_keyword_parameter_node(self, node);
    }

    fn visit_optional_keyword_parameter_node(&mut self, node: &ruby_prism::OptionalKeywordParameterNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let nl = node.name_loc();
        self.report_param(&name, nl.start_offset(), nl.end_offset());
        self.add_local(name, nl.start_offset(), nl.end_offset());
        ruby_prism::visit_optional_keyword_parameter_node(self, node);
    }

    fn visit_block_parameter_node(&mut self, node: &ruby_prism::BlockParameterNode) {
        if let Some(name_id) = node.name() {
            let name = String::from_utf8_lossy(name_id.as_slice()).to_string();
            let loc = node.location();
            self.report_param(&name, loc.start_offset(), loc.end_offset());
            self.add_local(name, loc.start_offset(), loc.end_offset());
        }
        ruby_prism::visit_block_parameter_node(self, node);
    }

    fn visit_block_local_variable_node(&mut self, node: &ruby_prism::BlockLocalVariableNode) {
        let name = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let loc = node.location();
        self.report_param(&name, loc.start_offset(), loc.end_offset());
        self.add_local(name, loc.start_offset(), loc.end_offset());
        ruby_prism::visit_block_local_variable_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        self.scopes.push(Scope { locals: Vec::new(), is_block: false, reported: Vec::new() });
        ruby_prism::visit_def_node(self, node);
        self.scopes.pop();
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        self.scopes.push(Scope { locals: Vec::new(), is_block: false, reported: Vec::new() });
        ruby_prism::visit_class_node(self, node);
        self.scopes.pop();
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        self.scopes.push(Scope { locals: Vec::new(), is_block: false, reported: Vec::new() });
        ruby_prism::visit_module_node(self, node);
        self.scopes.pop();
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        self.scopes.push(Scope { locals: Vec::new(), is_block: false, reported: Vec::new() });
        ruby_prism::visit_singleton_class_node(self, node);
        self.scopes.pop();
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let is_ractor = is_ractor_new_call(node);
        if is_ractor { self.in_ractor_stack.push(true); }
        ruby_prism::visit_call_node(self, node);
        if is_ractor { self.in_ractor_stack.pop(); }
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        let inherit = self.in_ractor();
        self.in_ractor_stack.push(inherit);
        self.scopes.push(Scope { locals: Vec::new(), is_block: true, reported: Vec::new() });
        ruby_prism::visit_block_node(self, node);
        self.scopes.pop();
        self.in_ractor_stack.pop();
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        let inherit = self.in_ractor();
        self.in_ractor_stack.push(inherit);
        self.scopes.push(Scope { locals: Vec::new(), is_block: true, reported: Vec::new() });
        ruby_prism::visit_lambda_node(self, node);
        self.scopes.pop();
        self.in_ractor_stack.pop();
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        // For if/else, branch index 0 = then, 1 = else (and elsif is encoded as nested IfNode in subsequent).
        let id = node.location().start_offset();
        // Then-branch
        if let Some(stmts) = node.statements() {
            self.current_branch.push((id, 0));
            ruby_prism::visit_statements_node(self, &stmts);
            self.current_branch.pop();
        }
        // Subsequent (else / elsif)
        if let Some(sub) = node.subsequent() {
            self.current_branch.push((id, 1));
            self.visit(&sub);
            self.current_branch.pop();
        }
        // Predicate not in branches
        let pred = node.predicate();
        self.visit(&pred);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        let id = node.location().start_offset();
        if let Some(stmts) = node.statements() {
            self.current_branch.push((id, 0));
            ruby_prism::visit_statements_node(self, &stmts);
            self.current_branch.pop();
        }
        if let Some(els) = node.else_clause() {
            self.current_branch.push((id, 1));
            ruby_prism::visit_else_node(self, &els);
            self.current_branch.pop();
        }
        let pred = node.predicate();
        self.visit(&pred);
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        let id = node.location().start_offset();
        for (i, when) in node.conditions().iter().enumerate() {
            self.current_branch.push((id, i));
            self.visit(&when);
            self.current_branch.pop();
        }
        if let Some(els) = node.else_clause() {
            self.current_branch.push((id, 999));
            ruby_prism::visit_else_node(self, &els);
            self.current_branch.pop();
        }
        if let Some(pred) = node.predicate() {
            self.visit(&pred);
        }
    }
}

crate::register_cop!("Lint/ShadowingOuterLocalVariable", |_cfg| Some(Box::new(
    ShadowingOuterLocalVariable::new()
)));
