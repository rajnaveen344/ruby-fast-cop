//! Style/FetchEnvVar - flag `ENV['X']` → `ENV.fetch('X', nil)` (or `.fetch('X')`).
//!
//! Ported from `lib/rubocop/cop/style/fetch_env_var.rb`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

#[derive(Default)]
pub struct FetchEnvVar {
    allowed_vars: HashSet<String>,
    default_to_nil: bool,
}

impl FetchEnvVar {
    pub fn new(allowed: HashSet<String>, default_to_nil: bool) -> Self {
        Self { allowed_vars: allowed, default_to_nil }
    }
}

impl Cop for FetchEnvVar {
    fn name(&self) -> &'static str { "Style/FetchEnvVar" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor {
            ctx,
            allowed_vars: &self.allowed_vars,
            default_to_nil: self.default_to_nil,
            stack: Vec::new(),
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

#[derive(Clone)]
enum Frame {
    /// Parent is a CallNode. (call_start, call_end, message_start, message_end,
    /// is_receiver_position, is_arg_position, is_dot_or_safe_nav)
    Call {
        is_receiver: bool,
    },
    /// Parent is an If/Unless. (condition_start, condition_end)
    If {
        cond_start: usize,
        cond_end: usize,
    },
    /// We are visiting INSIDE an if/unless/while/until BODY. Condition source
    /// captured + set of ENV-key argument source strings used in condition.
    IfBody {
        cond_src: String,
        allowed_keys: Vec<String>,
    },
    /// Parent is an Or (`||`). is_lhs = whether current node is LHS of this Or
    Or {
        is_lhs: bool,
    },
    /// Parent is OperatorWriteNode (||= / &&= / +=) — current is the LHS receiver
    AssignLhs,
    /// Parent is something else
    Other,
}

/// Walk a node, collecting all key-arg source strings from `ENV[KEY]`,
/// `ENV.<method>(KEY)`, or `<anything>(ENV[KEY])` calls.
fn collect_env_keys(node: &Node, source: &str, out: &mut Vec<String>) {
    match node {
        Node::CallNode { .. } => {
            let c = node.as_call_node().unwrap();
            // Receiver = `ENV` constant?
            let recv_is_env = c.receiver().as_ref().map_or(false, |r| {
                matches!(r, Node::ConstantReadNode { .. }) && {
                    let cr = r.as_constant_read_node().unwrap();
                    String::from_utf8_lossy(cr.name().as_slice()) == "ENV"
                }
            });
            if recv_is_env {
                if let Some(args) = c.arguments() {
                    if let Some(first) = args.arguments().iter().next() {
                        let l = first.location();
                        out.push(source[l.start_offset()..l.end_offset()].to_string());
                    }
                }
            }
            // Recurse into receiver/args/block
            if let Some(r) = c.receiver() { collect_env_keys(&r, source, out); }
            if let Some(args) = c.arguments() {
                for a in args.arguments().iter() {
                    collect_env_keys(&a, source, out);
                }
            }
        }
        Node::OrNode { .. } => {
            let o = node.as_or_node().unwrap();
            collect_env_keys(&o.left(), source, out);
            collect_env_keys(&o.right(), source, out);
        }
        Node::AndNode { .. } => {
            let a = node.as_and_node().unwrap();
            collect_env_keys(&a.left(), source, out);
            collect_env_keys(&a.right(), source, out);
        }
        _ => {}
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    allowed_vars: &'a HashSet<String>,
    default_to_nil: bool,
    stack: Vec<Frame>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn check_env_call(&mut self, node: &ruby_prism::CallNode) {
        // Must be `ENV[arg]`: name=`[]`, recv=ConstantRead(ENV), 1 arg, not safe-nav.
        let method = node_name!(node);
        if method.as_ref() != "[]" { return; }
        if node.is_safe_navigation() { return; }

        let recv = match node.receiver() {
            Some(r) => r,
            None => return,
        };
        let is_env = match recv {
            Node::ConstantReadNode { .. } => {
                let c = recv.as_constant_read_node().unwrap();
                String::from_utf8_lossy(c.name().as_slice()) == "ENV"
            }
            _ => false,
        };
        if !is_env { return; }

        let args_node = match node.arguments() {
            Some(a) => a,
            None => return,
        };
        let args: Vec<_> = args_node.arguments().iter().collect();
        if args.len() != 1 { return; }

        let key_node = &args[0];
        let key_loc = key_node.location();
        let key_src = &self.ctx.source[key_loc.start_offset()..key_loc.end_offset()];

        // Allowed vars (only string literal keys)
        if let Node::StringNode { .. } = key_node {
            let s = key_node.as_string_node().unwrap();
            if let Ok(val) = std::str::from_utf8(s.unescaped()) {
                if self.allowed_vars.contains(val) { return; }
            }
        }

        // Apply guards by inspecting parent stack
        if self.is_allowable_use(node) { return; }

        let node_loc = node.location();
        let start = node_loc.start_offset();
        let end = node_loc.end_offset();

        let new_src = if self.default_to_nil {
            format!("ENV.fetch({}, nil)", key_src)
        } else {
            format!("ENV.fetch({})", key_src)
        };
        let msg = if self.default_to_nil {
            format!("Use `ENV.fetch({}, nil)` instead of `ENV[{}]`.", key_src, key_src)
        } else {
            format!("Use `ENV.fetch({})` instead of `ENV[{}]`.", key_src, key_src)
        };

        let offense = self.ctx
            .offense_with_range("Style/FetchEnvVar", &msg, Severity::Convention, start, end)
            .with_correction(Correction::replace(start, end, new_src));
        self.offenses.push(offense);
    }

    fn is_allowable_use(&self, node: &ruby_prism::CallNode) -> bool {
        let node_loc = node.location();
        let node_src = &self.ctx.source[node_loc.start_offset()..node_loc.end_offset()];
        // Compute current ENV[X]'s key argument source
        let key_src = node
            .arguments()
            .and_then(|a| a.arguments().iter().next().map(|n| {
                let l = n.location();
                self.ctx.source[l.start_offset()..l.end_offset()].to_string()
            }))
            .unwrap_or_default();

        // Direct parent first (Or LHS, Call receiver, AssignLhs)
        if let Some(parent) = self.stack.last() {
            match parent {
                Frame::Call { is_receiver: true } => return true,
                Frame::AssignLhs => return true,
                Frame::Or { is_lhs: true } => return true,
                _ => {}
            }
        }
        // Anywhere in stack: inside If condition → allowable as flag.
        // Inside If body with matching condition source → allowable.
        for f in self.stack.iter().rev() {
            match f {
                Frame::If { .. } => return true,
                Frame::IfBody { cond_src, allowed_keys } => {
                    if cond_src.contains(node_src) {
                        return true;
                    }
                    if !key_src.is_empty() && allowed_keys.iter().any(|k| k == &key_src) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn push_visit(&mut self, frame: Frame, body: impl FnOnce(&mut Self)) {
        self.stack.push(frame);
        body(self);
        self.stack.pop();
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Check if this is `ENV[]`
        self.check_env_call(node);

        // Determine whether THIS call is `!ENV['X']` (prefix bang) or comparison
        // method on ENV['X']. Mark via Or/Other? We need a custom CallParent frame.
        // Simpler: detect at parent visit by checking children locations.

        // Visit children with appropriate frame.
        let method = node_name!(node);
        let is_unary_bang = method.as_ref() == "!" && node.receiver().is_some()
            && node.arguments().is_none();
        let is_comparison = matches!(method.as_ref(),
            "==" | "!=" | "<" | "<=" | ">" | ">=" | "<=>" | "===" | "=~" | "!~");

        // Visit receiver first
        if let Some(recv) = node.receiver() {
            let frame = if is_unary_bang || is_comparison {
                Frame::If { cond_start: 0, cond_end: 0 } // treat as flag-like
            } else {
                Frame::Call { is_receiver: true }
            };
            self.stack.push(frame);
            self.visit(&recv);
            self.stack.pop();
        }
        // Visit arguments
        if let Some(args) = node.arguments() {
            for a in args.arguments().iter() {
                self.stack.push(Frame::Other);
                self.visit(&a);
                self.stack.pop();
            }
        }
        if let Some(block) = node.block() {
            self.stack.push(Frame::Other);
            self.visit(&block);
            self.stack.pop();
        }
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        let cond = node.predicate();
        let cond_loc = cond.location();
        let cs = cond_loc.start_offset();
        let ce = cond_loc.end_offset();
        let cond_src = self.ctx.source[cs..ce].to_string();
        let mut keys: Vec<String> = Vec::new();
        collect_env_keys(&cond, self.ctx.source, &mut keys);

        self.stack.push(Frame::If { cond_start: cs, cond_end: ce });
        self.visit(&cond);
        self.stack.pop();

        if let Some(stmts) = node.statements() {
            self.stack.push(Frame::IfBody { cond_src: cond_src.clone(), allowed_keys: keys.clone() });
            self.visit_statements_node(&stmts);
            self.stack.pop();
        }
        if let Some(sub) = node.subsequent() {
            self.stack.push(Frame::IfBody { cond_src, allowed_keys: keys });
            self.visit(&sub);
            self.stack.pop();
        }
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        let cond = node.predicate();
        let cs = cond.location().start_offset();
        let ce = cond.location().end_offset();
        let cond_src = self.ctx.source[cs..ce].to_string();
        let mut keys: Vec<String> = Vec::new();
        collect_env_keys(&cond, self.ctx.source, &mut keys);
        self.stack.push(Frame::If { cond_start: cs, cond_end: ce });
        self.visit(&cond);
        self.stack.pop();
        if let Some(stmts) = node.statements() {
            self.stack.push(Frame::IfBody { cond_src: cond_src.clone(), allowed_keys: keys.clone() });
            self.visit_statements_node(&stmts);
            self.stack.pop();
        }
        if let Some(sub) = node.else_clause() {
            self.stack.push(Frame::IfBody { cond_src, allowed_keys: keys });
            self.visit_else_node(&sub);
            self.stack.pop();
        }
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        let cond = node.predicate();
        let cs = cond.location().start_offset();
        let ce = cond.location().end_offset();
        let cond_src = self.ctx.source[cs..ce].to_string();
        let mut keys: Vec<String> = Vec::new();
        collect_env_keys(&cond, self.ctx.source, &mut keys);
        self.stack.push(Frame::If { cond_start: cs, cond_end: ce });
        self.visit(&cond);
        self.stack.pop();
        if let Some(stmts) = node.statements() {
            self.stack.push(Frame::IfBody { cond_src, allowed_keys: keys });
            self.visit_statements_node(&stmts);
            self.stack.pop();
        }
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        let cond = node.predicate();
        let cs = cond.location().start_offset();
        let ce = cond.location().end_offset();
        let cond_src = self.ctx.source[cs..ce].to_string();
        let mut keys: Vec<String> = Vec::new();
        collect_env_keys(&cond, self.ctx.source, &mut keys);
        self.stack.push(Frame::If { cond_start: cs, cond_end: ce });
        self.visit(&cond);
        self.stack.pop();
        if let Some(stmts) = node.statements() {
            self.stack.push(Frame::IfBody { cond_src, allowed_keys: keys });
            self.visit_statements_node(&stmts);
            self.stack.pop();
        }
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        // LHS of Or — flag-context (allowable if ENV is here)
        let lhs = node.left();
        let rhs = node.right();
        self.stack.push(Frame::Or { is_lhs: true });
        self.visit(&lhs);
        self.stack.pop();
        // Check if THIS or-node is itself the LHS of an outer Or chain — RuboCop's
        // or_lhs? returns true if parent is Or. We need to detect this.
        let outer_is_or = matches!(self.stack.last(), Some(Frame::Or { .. }));
        let rhs_is_lhs = outer_is_or; // Inside chain like `a || b || c`, the RHS may itself be inside Or
        self.stack.push(Frame::Or { is_lhs: rhs_is_lhs });
        self.visit(&rhs);
        self.stack.pop();
    }

}

crate::register_cop!("Style/FetchEnvVar", |cfg| {
    let cop_cfg = cfg.get_cop_config("Style/FetchEnvVar");
    let allowed: HashSet<String> = cop_cfg
        .and_then(|c| c.raw.get("AllowedVars"))
        .and_then(|v| v.as_sequence())
        .map(|s| s.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let default_to_nil = cop_cfg
        .and_then(|c| c.raw.get("DefaultToNil").and_then(|v| v.as_bool()))
        .unwrap_or(true);
    Some(Box::new(FetchEnvVar::new(allowed, default_to_nil)))
});
