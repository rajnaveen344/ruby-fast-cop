//! Naming/BlockForwarding
//!
//! Two styles:
//! - `anonymous` (default, Ruby 3.1+): prefer `def f(&)` + `bar(&)`.
//! - `explicit`: prefer named `&block`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{BlockParameterNode, DefNode, Node, Visit};

const MSG_ANON: &str = "Use anonymous block forwarding.";
const MSG_EXPL: &str = "Use explicit block forwarding.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockForwardingStyle { Anonymous, Explicit }

pub struct BlockForwarding {
    style: BlockForwardingStyle,
    forwarding_name: String,
}

impl BlockForwarding {
    pub fn new() -> Self {
        Self { style: BlockForwardingStyle::Anonymous, forwarding_name: "block".into() }
    }
    pub fn with_config(style: BlockForwardingStyle, name: String) -> Self {
        Self { style, forwarding_name: name }
    }
}

impl Default for BlockForwarding {
    fn default() -> Self { Self::new() }
}

fn def_block_param<'a>(node: &'a DefNode<'a>) -> Option<BlockParameterNode<'a>> {
    let params = node.parameters()?;
    params.block()
}

fn name_of_block_param(bp: &BlockParameterNode) -> Option<String> {
    bp.name_loc().map(|l| String::from_utf8_lossy(l.as_slice()).into_owned())
}

/// Walk body: check whether `var` is used as a value (not just `&var` pass).
/// Any lvar read, any assignment to it, any op-asgn, counts as non-forwarding use.
struct UsageScan<'a> {
    var: &'a str,
    // forwarded passes: positions of `&var` that should be flagged
    forwarding_sites: Vec<(usize, usize)>,
    // any usage outside a `&var` block arg → disqualify
    used_as_value: bool,
    // positions of `&var` nested inside an outer block (may need Ruby >= 3.4 gate)
    forwarding_sites_in_nested: Vec<(usize, usize)>,
    source: &'a str,
    block_depth: usize,
}

impl<'a> UsageScan<'a> {
    fn is_name(&self, slice: &[u8]) -> bool {
        slice == self.var.as_bytes()
    }
}

impl<'a> Visit<'_> for UsageScan<'a> {
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        self.block_depth += 1;
        ruby_prism::visit_block_node(self, node);
        self.block_depth -= 1;
    }

    fn visit_block_argument_node(&mut self, node: &ruby_prism::BlockArgumentNode) {
        // `&expr` inside call args. If expr is lvar read matching var → forwarding site.
        if let Some(expr) = node.expression() {
            if let Some(lvr) = expr.as_local_variable_read_node() {
                if self.is_name(lvr.name().as_slice()) {
                    let loc = node.location();
                    if self.block_depth == 0 {
                        self.forwarding_sites.push((loc.start_offset(), loc.end_offset()));
                    } else {
                        self.forwarding_sites_in_nested
                            .push((loc.start_offset(), loc.end_offset()));
                    }
                    return;
                }
            }
        }
        ruby_prism::visit_block_argument_node(self, node);
    }

    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        if self.is_name(node.name().as_slice()) {
            self.used_as_value = true;
        }
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        if self.is_name(node.name().as_slice()) {
            self.used_as_value = true;
        }
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_local_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableOperatorWriteNode,
    ) {
        if self.is_name(node.name().as_slice()) { self.used_as_value = true; }
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }

    fn visit_local_variable_and_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableAndWriteNode,
    ) {
        if self.is_name(node.name().as_slice()) { self.used_as_value = true; }
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }

    fn visit_local_variable_or_write_node(
        &mut self,
        node: &ruby_prism::LocalVariableOrWriteNode,
    ) {
        if self.is_name(node.name().as_slice()) { self.used_as_value = true; }
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
}

struct V<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a BlockForwarding,
    offenses: Vec<Offense>,
}

impl<'a> V<'a> {
    fn push(&mut self, start: usize, end: usize, msg: &'static str) {
        self.offenses.push(self.ctx.offense_with_range(
            "Naming/BlockForwarding", msg, Severity::Convention, start, end,
        ));
    }
}

impl<'a> Visit<'_> for V<'a> {
    fn visit_def_node(&mut self, node: &DefNode) {
        self.process(node);
        ruby_prism::visit_def_node(self, node);
    }
}

impl<'a> V<'a> {
    fn process(&mut self, node: &DefNode) {
        let Some(bp) = def_block_param(node) else { return };
        let bp_loc = bp.location();
        let name_opt = name_of_block_param(&bp);

        match self.cop.style {
            BlockForwardingStyle::Anonymous => {
                // Named `&block` → convert to `&`
                let Some(name) = name_opt else { return };
                // Pre-Ruby 3.2: anonymous `&` cannot coexist with keyword params
                if !self.ctx.ruby_version_at_least(3, 2) {
                    if let Some(params) = node.parameters() {
                        let has_kw = params.keywords().iter().count() > 0
                            || params.keyword_rest().is_some();
                        if has_kw { return }
                    }
                }
                // Scan body: if `name` used as value or written, skip.
                let mut scan = UsageScan {
                    var: &name,
                    forwarding_sites: Vec::new(),
                    used_as_value: false,
                    forwarding_sites_in_nested: Vec::new(),
                    source: self.ctx.source,
                    block_depth: 0,
                };
                if let Some(body) = node.body() {
                    scan.visit(&body);
                }
                if scan.used_as_value { return }

                // Pre-3.4: nested forwarding would be a syntax error → skip entirely
                // if any forwarding sites are inside nested blocks.
                if !self.ctx.ruby_version_at_least(3, 4) && !scan.forwarding_sites_in_nested.is_empty() {
                    return;
                }

                // Flag def's `&block`
                self.push(bp_loc.start_offset(), bp_loc.end_offset(), MSG_ANON);
                // Flag each forwarding site
                for (s, e) in scan.forwarding_sites.iter().chain(scan.forwarding_sites_in_nested.iter()) {
                    self.push(*s, *e, MSG_ANON);
                }
            }
            BlockForwardingStyle::Explicit => {
                // Anonymous `&` in def → if name_loc is None, this is anonymous.
                if name_opt.is_some() { return }
                // Flag def's `&`
                self.push(bp_loc.start_offset(), bp_loc.end_offset(), MSG_EXPL);

                // Walk body for `&` block argument w/ no expression.
                struct BAV<'b> {
                    hits: Vec<(usize, usize)>,
                    _src: &'b str,
                }
                impl<'b> Visit<'_> for BAV<'b> {
                    fn visit_block_argument_node(&mut self, node: &ruby_prism::BlockArgumentNode) {
                        // expression is None iff anonymous.
                        if node.expression().is_none() {
                            let loc = node.location();
                            self.hits.push((loc.start_offset(), loc.end_offset()));
                        }
                        ruby_prism::visit_block_argument_node(self, node);
                    }
                }
                let mut bav = BAV { hits: Vec::new(), _src: self.ctx.source };
                if let Some(body) = node.body() {
                    bav.visit(&body);
                }
                for (s, e) in bav.hits { self.push(s, e, MSG_EXPL); }
            }
        }
    }
}

impl Cop for BlockForwarding {
    fn name(&self) -> &'static str { "Naming/BlockForwarding" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(3, 1) { return vec![] }
        let mut v = V { ctx, cop: self, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

crate::register_cop!("Naming/BlockForwarding", |cfg| {
    let cfg_entry = cfg.get_cop_config("Naming/BlockForwarding");
    let style = cfg_entry
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "explicit" => BlockForwardingStyle::Explicit,
            _ => BlockForwardingStyle::Anonymous,
        })
        .unwrap_or(BlockForwardingStyle::Anonymous);
    let name = cfg_entry
        .and_then(|c| c.raw.get("BlockForwardingName"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| "block".into());
    Some(Box::new(BlockForwarding::with_config(style, name)))
});
