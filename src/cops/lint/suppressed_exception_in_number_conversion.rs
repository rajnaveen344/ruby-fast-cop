//! Lint/SuppressedExceptionInNumberConversion cop
//!
//! Translates RuboCop's SuppressedExceptionInNumberConversion. Flags
//! `Integer(x) rescue nil` (and BigDecimal/Float/Complex/Rational) and
//! the equivalent begin..rescue..end form.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use crate::node_name;
use ruby_prism::{Node, Visit};

const NUMERIC_METHODS: &[&str] = &["Integer", "BigDecimal", "Complex", "Rational", "Float"];
const EXPECTED_EXCEPTIONS: &[&str] =
    &["ArgumentError", "TypeError", "::ArgumentError", "::TypeError"];

#[derive(Default)]
pub struct SuppressedExceptionInNumberConversion;

impl SuppressedExceptionInNumberConversion {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for SuppressedExceptionInNumberConversion {
    fn name(&self) -> &'static str {
        "Lint/SuppressedExceptionInNumberConversion"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(2, 6) {
            return vec![];
        }
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn emit(&mut self, off_start: usize, off_end: usize, prefer: &str) {
        let msg = format!("Use `{}` instead.", prefer);
        let mut offense = self.ctx.offense_with_range(
            "Lint/SuppressedExceptionInNumberConversion",
            &msg,
            Severity::Warning,
            off_start,
            off_end,
        );
        offense = offense.with_correction(Correction {
            edits: vec![Edit {
                start_offset: off_start,
                end_offset: off_end,
                replacement: prefer.to_string(),
            }],
        });
        self.offenses.push(offense);
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_rescue_modifier_node(&mut self, node: &ruby_prism::RescueModifierNode) {
        // `expr rescue nil`
        if matches!(node.rescue_expression(), Node::NilNode { .. }) {
            let expr = node.expression();
            if let Some(prefer) = numeric_call_prefer(&expr, self.ctx) {
                let loc = node.location();
                self.emit(loc.start_offset(), loc.end_offset(), &prefer);
            }
        }
        ruby_prism::visit_rescue_modifier_node(self, node);
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        if let Some((method_call, prefer)) = begin_numeric_rescue(node, self.ctx) {
            let _ = method_call;
            let loc = node.location();
            self.emit(loc.start_offset(), loc.end_offset(), &prefer);
        }
        ruby_prism::visit_begin_node(self, node);
    }
}

/// Returns Some(replacement_text) if `node` is a Kernel#Integer-style call.
fn numeric_call_prefer(node: &Node, ctx: &CheckContext) -> Option<String> {
    let call = match node {
        Node::CallNode { .. } => node.as_call_node().unwrap(),
        _ => return None,
    };
    let method = node_name!(call);
    if !NUMERIC_METHODS.contains(&method.as_ref()) {
        return None;
    }
    if !receiver_ok(&call, method.as_ref()) {
        return None;
    }
    // Float allows exactly 1 arg; others allow 1 or 2.
    let arg_count = call.arguments().map(|args| args.arguments().iter().count()).unwrap_or(0);
    if method == "Float" {
        if arg_count != 1 {
            return None;
        }
    } else if !(arg_count == 1 || arg_count == 2) {
        return None;
    }

    let mut arg_sources: Vec<String> = Vec::new();
    if let Some(args) = call.arguments() {
        for a in args.arguments().iter() {
            let l = a.location();
            arg_sources.push(ctx.source[l.start_offset()..l.end_offset()].to_string());
        }
    }
    arg_sources.push("exception: false".to_string());

    let prefer = format!("{}({})", method, arg_sources.join(", "));
    let prefer = if let Some(recv) = call.receiver() {
        let recv_loc = recv.location();
        let recv_src = &ctx.source[recv_loc.start_offset()..recv_loc.end_offset()];
        let dot = call
            .call_operator_loc()
            .map(|l| ctx.source[l.start_offset()..l.end_offset()].to_string())
            .unwrap_or_else(|| "::".to_string());
        format!("{}{}{}", recv_src, dot, prefer)
    } else {
        prefer
    };
    Some(prefer)
}

fn receiver_ok(call: &ruby_prism::CallNode, _method: &str) -> bool {
    match call.receiver() {
        None => true,
        Some(Node::ConstantReadNode { .. }) => {
            let n = call.receiver().unwrap();
            let cr = n.as_constant_read_node().unwrap();
            String::from_utf8_lossy(cr.name().as_slice()) == "Kernel"
        }
        Some(Node::ConstantPathNode { .. }) => {
            // ::Kernel — parent must be cbase, name must be Kernel
            let n = call.receiver().unwrap();
            let cp = n.as_constant_path_node().unwrap();
            // Name from rightmost segment
            let name_text = match cp.name() {
                Some(name_id) => String::from_utf8_lossy(name_id.as_slice()).to_string(),
                None => return false,
            };
            if name_text != "Kernel" {
                return false;
            }
            // parent: None (cbase) acceptable
            cp.parent().is_none()
        }
        _ => false,
    }
}

/// If `begin` matches `begin; <numeric_call>; rescue [classes]; (nil) end`,
/// return (call, prefer_text).
fn begin_numeric_rescue<'a>(
    begin: &ruby_prism::BeginNode<'a>,
    ctx: &CheckContext,
) -> Option<(Node<'a>, String)> {
    // Body must be a single statement = the numeric call.
    let stmts = begin.statements()?;
    let body: Vec<_> = stmts.body().iter().collect();
    if body.len() != 1 {
        return None;
    }
    let call = body.into_iter().next().unwrap();
    let prefer = numeric_call_prefer(&call, ctx)?;

    let rescue = begin.rescue_clause()?;
    if rescue.subsequent().is_some() {
        return None; // single rescue clause only
    }
    if begin.else_clause().is_some() {
        return None; // any else_clause => not a match (mirrors RuboCop pattern: nil? for else)
    }
    if begin.ensure_clause().is_some() {
        return None;
    }

    // Rescue body must be empty or a single `nil` literal.
    if let Some(rs) = rescue.statements() {
        let rb: Vec<_> = rs.body().iter().collect();
        if rb.len() > 1 {
            return None;
        }
        if rb.len() == 1 && !matches!(rb[0], Node::NilNode { .. }) {
            return None;
        }
    }

    // Exception classes — none, or all must be ArgumentError/TypeError (with optional ::).
    let exc_iter = rescue.exceptions();
    let exceptions: Vec<_> = exc_iter.iter().collect();
    if !exceptions.is_empty() {
        for e in &exceptions {
            let l = e.location();
            let src = &ctx.source[l.start_offset()..l.end_offset()];
            if !EXPECTED_EXCEPTIONS.contains(&src) {
                return None;
            }
        }
    }

    Some((call, prefer))
}

crate::register_cop!("Lint/SuppressedExceptionInNumberConversion", |_cfg| {
    Some(Box::new(SuppressedExceptionInNumberConversion::new()))
});
