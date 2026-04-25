//! Lint/UselessDefaultValueArgument cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/useless_default_value_argument.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{BlockNode, CallNode, Node, Visit};

pub struct UselessDefaultValueArgument {
    allowed_receivers: Vec<String>,
}

impl Default for UselessDefaultValueArgument {
    fn default() -> Self { Self { allowed_receivers: Vec::new() } }
}

impl UselessDefaultValueArgument {
    pub fn new() -> Self { Self::default() }
    pub fn with_config(allowed_receivers: Vec<String>) -> Self { Self { allowed_receivers } }
}

impl Cop for UselessDefaultValueArgument {
    fn name(&self) -> &'static str { "Lint/UselessDefaultValueArgument" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = DefaultArgVisitor { cop: self, ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct DefaultArgVisitor<'a> {
    cop: &'a UselessDefaultValueArgument,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

fn receiver_src<'a>(call: &CallNode, src: &'a str) -> Option<&'a str> {
    let r = call.receiver()?;
    let loc = r.location();
    Some(&src[loc.start_offset()..loc.end_offset()])
}

fn is_array_receiver(call: &CallNode) -> bool {
    let recv = match call.receiver() {
        Some(r) => r,
        None => return false,
    };
    if let Some(cr) = recv.as_constant_read_node() {
        let n = String::from_utf8_lossy(cr.name().as_slice());
        return n == "Array";
    }
    if let Some(cp) = recv.as_constant_path_node() {
        if let Some(name) = cp.name() {
            let n = String::from_utf8_lossy(name.as_slice());
            return n == "Array";
        }
    }
    false
}

/// Returns Some((prev_arg_loc, default_value_node)) if call matches the pattern.
/// We return locations (Copy) for prev_arg and the actual Node for default_value.
fn match_pattern<'a>(call: &CallNode<'a>) -> Option<(ruby_prism::Location<'a>, Node<'a>)> {
    let method = String::from_utf8_lossy(call.name().as_slice()).to_string();
    let args = call.arguments()?;
    let mut iter = args.arguments().iter();
    let prev = iter.next()?;
    let default = iter.next()?;
    if iter.next().is_some() {
        return None;
    }

    if method == "fetch" {
        if call.receiver().is_none() {
            return None;
        }
    } else if method == "new" {
        if !is_array_receiver(call) {
            return None;
        }
    } else {
        return None;
    }

    let prev_loc = prev.location();
    Some((prev_loc, default))
}

/// Test: hash without braces (kwargs literal) — skip these.
fn is_hash_without_braces(node: &Node, source: &str) -> bool {
    if let Some(h) = node.as_hash_node() {
        let loc = h.location();
        let bytes = source.as_bytes();
        if loc.start_offset() < bytes.len() {
            return bytes[loc.start_offset()] != b'{';
        }
    }
    if node.as_keyword_hash_node().is_some() {
        return true;
    }
    false
}

impl<'a> DefaultArgVisitor<'a> {
    fn allowed_receiver(&self, call: &CallNode) -> bool {
        let recv_src = match receiver_src(call, self.ctx.source) {
            Some(s) => s,
            None => return false,
        };
        self.cop.allowed_receivers.iter().any(|a| a == recv_src)
    }

    fn check_block_call(&mut self, _block: &BlockNode, call_node: &CallNode) {
        let (pa_loc, default_value) = match match_pattern(call_node) {
            Some(p) => p,
            None => return,
        };
        if self.allowed_receiver(call_node) {
            return;
        }
        if is_hash_without_braces(&default_value, self.ctx.source) {
            return;
        }

        let dv_loc = default_value.location();
        let msg = "Block supersedes default value argument.";

        let edit_start = pa_loc.end_offset();
        let edit_end = dv_loc.end_offset();

        let off = self.ctx.offense_with_range(
            "Lint/UselessDefaultValueArgument",
            msg,
            Severity::Warning,
            dv_loc.start_offset(),
            dv_loc.end_offset(),
        ).with_correction(Correction::delete(edit_start, edit_end));
        self.offenses.push(off);
    }
}

impl<'a> Visit<'_> for DefaultArgVisitor<'a> {
    fn visit_block_node(&mut self, node: &BlockNode) {
        // Block's parent CallNode wraps it via `.block()`. Walk: the visitor visits
        // CallNode first, then BlockNode as child. We need the CallNode that owns this block.
        // Approach: walk in visit_call_node instead.
        ruby_prism::visit_block_node(self, node);
    }

    fn visit_call_node(&mut self, node: &CallNode) {
        if let Some(blk) = node.block() {
            if let Some(b) = blk.as_block_node() {
                self.check_block_call(&b, node);
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allowed_receivers: Vec<String>,
}

crate::register_cop!("Lint/UselessDefaultValueArgument", |cfg| {
    let c: Cfg = cfg.typed("Lint/UselessDefaultValueArgument");
    Some(Box::new(UselessDefaultValueArgument::with_config(c.allowed_receivers)))
});
