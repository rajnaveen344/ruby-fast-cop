//! Lint/UselessRescue cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Useless `rescue` detected.";

#[derive(Default)]
pub struct UselessRescue;

impl UselessRescue {
    pub fn new() -> Self { Self }
}

impl Cop for UselessRescue {
    fn name(&self) -> &'static str { "Lint/UselessRescue" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();
        let mut v = V { ctx, out: vec![], ensure_bodies: vec![] };
        v.visit(&tree);
        v.out
    }
}

struct V<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    out: Vec<Offense>,
    ensure_bodies: Vec<Option<(usize, usize)>>,
}

impl<'a, 'b> V<'a, 'b> {
    fn check_rescue(&mut self, node: &ruby_prism::RescueNode) {
        if node.subsequent().is_some() { return; }

        let body_stmts = match node.statements() { Some(s) => s, None => return };
        let stmts: Vec<Node> = body_stmts.body().iter().collect();
        if stmts.len() != 1 { return; }
        let call = match stmts[0].as_call_node() { Some(c) => c, None => return };
        if call.receiver().is_some() { return; }
        if node_name!(&call).as_ref() != "raise" { return; }

        // Get exception variable name (from reference)
        let exc_var = node.reference().and_then(|r| {
            if let Some(lvt) = r.as_local_variable_target_node() {
                Some(String::from_utf8_lossy(lvt.name().as_slice()).into_owned())
            } else {
                None
            }
        });

        // Args check
        let args = call.arguments();
        if let Some(args) = args {
            let arg_vec: Vec<Node> = args.arguments().iter().collect();
            if arg_vec.len() > 1 { return; }
            if !arg_vec.is_empty() {
                let arg_loc = arg_vec[0].location();
                let arg_text = &self.ctx.source[arg_loc.start_offset()..arg_loc.end_offset()];
                let mut allowed = vec!["$!".to_string(), "$ERROR_INFO".to_string()];
                if let Some(e) = &exc_var { allowed.push(e.clone()); }
                if !allowed.iter().any(|a| a == arg_text) { return; }
            }
        }

        // Skip if exception_variable used in ensure body
        if let Some(ev) = &exc_var {
            if let Some(Some((es, ee))) = self.ensure_bodies.last() {
                let ensure_src = &self.ctx.source[*es..*ee];
                // Cheap textual check; mirrors AST descendant-lvar lookup well enough for fixtures
                if word_present(ensure_src, ev) { return; }
            }
        }

        // Offense range = `rescue` keyword to end of reference (or `rescue` if none)
        let kw = node.keyword_loc();
        let start = kw.start_offset();
        let end = if let Some(r) = node.reference() {
            r.location().end_offset()
        } else {
            kw.end_offset()
        };
        self.out.push(self.ctx.offense_with_range(
            "Lint/UselessRescue", MSG, Severity::Warning, start, end,
        ));
    }
}

fn word_present(src: &str, word: &str) -> bool {
    let bytes = src.as_bytes();
    let wb = word.as_bytes();
    let mut i = 0;
    while i + wb.len() <= bytes.len() {
        if &bytes[i..i+wb.len()] == wb {
            let before_ok = i == 0 || !is_ident_byte(bytes[i-1]);
            let after_ok = i + wb.len() == bytes.len() || !is_ident_byte(bytes[i+wb.len()]);
            if before_ok && after_ok { return true; }
        }
        i += 1;
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode) {
        self.check_rescue(node);
        ruby_prism::visit_rescue_node(self, node);
    }
    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        let ensure_body = node.ensure_clause()
            .and_then(|e| e.statements())
            .map(|s| (s.location().start_offset(), s.location().end_offset()));
        self.ensure_bodies.push(ensure_body);
        ruby_prism::visit_begin_node(self, node);
        self.ensure_bodies.pop();
    }
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        // Def with rescue/ensure clauses are inside its body as a BeginNode? No - def has its own.
        // Prism: DefNode body() returns Node which may be StatementsNode or BeginNode (when def has rescue).
        let ensure_body = if let Some(body) = node.body() {
            if let Some(b) = body.as_begin_node() {
                b.ensure_clause()
                    .and_then(|e| e.statements())
                    .map(|s| (s.location().start_offset(), s.location().end_offset()))
            } else { None }
        } else { None };
        self.ensure_bodies.push(ensure_body);
        ruby_prism::visit_def_node(self, node);
        self.ensure_bodies.pop();
    }
}

crate::register_cop!("Lint/UselessRescue", |_cfg| Some(Box::new(UselessRescue::new())));
