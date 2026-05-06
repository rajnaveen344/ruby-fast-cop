//! Naming/BlockForwarding
//!
//! Two styles:
//! - `anonymous` (default, Ruby 3.1+): prefer `def f(&)` + `bar(&)`.
//! - `explicit`: prefer named `&block`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
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

/// Info about a forwarding site (a `&var` in a call).
#[derive(Debug)]
struct ForwardingSite {
    /// Range of the `&var` BlockArgumentNode
    start: usize,
    end: usize,
    /// Whether the parent call has parentheses already
    call_has_parens: bool,
    /// Position of call's message_loc end (for inserting `(`)
    call_message_end: Option<usize>,
    /// End of the ArgumentsNode (for inserting `)`)
    args_end: Option<usize>,
    /// Whether this is inside a nested block (depth > 0)
    in_nested: bool,
}

/// Walk body: check whether `var` is used as a value (not just `&var` pass).
struct UsageScan<'a> {
    var: &'a str,
    /// Forwarding sites with context
    sites: Vec<ForwardingSite>,
    /// any usage outside a `&var` block arg → disqualify
    used_as_value: bool,
    source: &'a str,
    block_depth: usize,
    /// byte offsets of `&var` block args already captured — skip LocalVariableReadNode inside them
    skip_ranges: Vec<(usize, usize)>,
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

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // In Prism, `&var` is node.block() as a BlockArgumentNode, NOT in node.arguments()
        if let Some(block_node) = node.block() {
            if let Some(ba) = block_node.as_block_argument_node() {
                if let Some(expr) = ba.expression() {
                    if let Some(lvr) = expr.as_local_variable_read_node() {
                        if self.is_name(lvr.name().as_slice()) {
                            let loc = ba.location();
                            let call_has_parens = node.opening_loc().is_some();
                            let call_message_end = node.message_loc().map(|l| l.end_offset());
                            let args_end = node.arguments().map(|a| a.location().end_offset());
                            // Record the range to skip when visiting local_variable_read
                            self.skip_ranges.push((loc.start_offset(), loc.end_offset()));
                            self.sites.push(ForwardingSite {
                                start: loc.start_offset(),
                                end: loc.end_offset(),
                                call_has_parens,
                                call_message_end,
                                args_end,
                                in_nested: self.block_depth > 0,
                            });
                        }
                    }
                }
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        if self.is_name(node.name().as_slice()) {
            // Don't flag as value-use if this read is inside a `&var` site we already captured
            let offset = node.location().start_offset();
            let inside_block_arg = self.skip_ranges.iter().any(|(s, e)| offset >= *s && offset < *e);
            if !inside_block_arg {
                self.used_as_value = true;
            }
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

/// Build correction edits for replacing `&name` → `&` at a site,
/// adding parens if needed.
fn anon_correction_edits(
    _source: &str,
    site_start: usize,
    site_end: usize,
    call_has_parens: bool,
    call_message_end: Option<usize>,
    args_end: Option<usize>,
) -> Vec<Edit> {
    let mut edits = Vec::new();
    // Replace `&name` with `&`
    edits.push(Edit { start_offset: site_start, end_offset: site_end, replacement: "&".into() });
    // Add parens if needed
    if !call_has_parens {
        if let Some(msg_end) = call_message_end {
            // Replace from msg_end to site_start with `(` (removes the space between method and args)
            // e.g. `bar &block` → `bar(&)`: replace bytes [3..4] (space) with `(`
            edits.push(Edit { start_offset: msg_end, end_offset: site_start, replacement: "(".into() });
            // Insert `)` after the last arg (or end of block arg if no positional args)
            let close_at = args_end.unwrap_or(site_end);
            edits.push(Edit { start_offset: close_at, end_offset: close_at, replacement: ")".into() });
        }
    }
    edits
}

/// Build correction edits for replacing `&` → `&name` at a site (explicit style).
fn explicit_correction_edits(
    site_start: usize,
    site_end: usize,
    name: &str,
    call_has_parens: bool,
    call_message_end: Option<usize>,
    args_end: Option<usize>,
) -> Vec<Edit> {
    let mut edits = Vec::new();
    edits.push(Edit { start_offset: site_start, end_offset: site_end, replacement: format!("&{}", name) });
    if !call_has_parens {
        if let Some(msg_end) = call_message_end {
            edits.push(Edit { start_offset: msg_end, end_offset: site_start, replacement: "(".into() });
            let close_at = args_end.unwrap_or(site_end);
            edits.push(Edit { start_offset: close_at, end_offset: close_at, replacement: ")".into() });
        }
    }
    edits
}

struct V<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a BlockForwarding,
    offenses: Vec<Offense>,
}

impl<'a> V<'a> {
    fn push_with_correction(&mut self, start: usize, end: usize, msg: &'static str, correction: Correction) {
        let o = self.ctx.offense_with_range(
            "Naming/BlockForwarding", msg, Severity::Convention, start, end,
        ).with_correction(correction);
        self.offenses.push(o);
    }

    fn push_no_correction(&mut self, start: usize, end: usize, msg: &'static str) {
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
                    sites: Vec::new(),
                    used_as_value: false,
                    source: self.ctx.source,
                    block_depth: 0,
                    skip_ranges: Vec::new(),
                };
                if let Some(body) = node.body() {
                    scan.visit(&body);
                }
                if scan.used_as_value { return }

                let has_nested = scan.sites.iter().any(|s| s.in_nested);
                // Pre-3.4: nested forwarding would be a syntax error → skip entirely
                if !self.ctx.ruby_version_at_least(3, 4) && has_nested {
                    return;
                }

                // Determine if def has parens
                let def_has_parens = node.lparen_loc().is_some();

                // Def param correction: replace `&block` with `&`, add parens to def if needed
                {
                    let mut edits = Vec::new();
                    edits.push(Edit { start_offset: bp_loc.start_offset(), end_offset: bp_loc.end_offset(), replacement: "&".into() });
                    if !def_has_parens {
                        // Insert `(` after method name, `)` after the block param
                        // method name end = node.name_loc().end_offset()
                        let name_end = node.name_loc().end_offset();
                        // The params span from name_end+space to bp_loc.end
                        // We insert `(` at name_end (before any space/params), replacing space with `(`
                        // Find start of params (first param or block param)
                        let params_start = if let Some(params) = node.parameters() {
                            params.location().start_offset()
                        } else {
                            bp_loc.start_offset()
                        };
                        // Replace from name_end to params_start with `(`
                        edits.push(Edit { start_offset: name_end, end_offset: params_start, replacement: "(".into() });
                        // Insert `)` after block param (which becomes `&`)
                        edits.push(Edit { start_offset: bp_loc.end_offset(), end_offset: bp_loc.end_offset(), replacement: ")".into() });
                    }
                    let correction = Correction { edits };
                    self.push_with_correction(bp_loc.start_offset(), bp_loc.end_offset(), MSG_ANON, correction);
                }

                // Flag each forwarding site
                for site in &scan.sites {
                    let edits = anon_correction_edits(
                        self.ctx.source,
                        site.start, site.end,
                        site.call_has_parens,
                        site.call_message_end,
                        site.args_end,
                    );
                    let correction = Correction { edits };
                    self.push_with_correction(site.start, site.end, MSG_ANON, correction);
                }
            }
            BlockForwardingStyle::Explicit => {
                // Anonymous `&` in def → if name_loc is None, this is anonymous.
                if name_opt.is_some() { return }
                // Flag def's `&`
                // Correction: replace `&` with `&name`
                let name = &self.cop.forwarding_name;

                // Check if `name` is already in use as a local variable in the method
                // (we can't autocorrect if it would create a conflict)
                let already_in_use = self.is_name_in_use(node, name);

                // Def param: replace `&` with `&name`
                if already_in_use {
                    // No correction (RuboCop also registers offense but doesn't autocorrect)
                    self.push_no_correction(bp_loc.start_offset(), bp_loc.end_offset(), MSG_EXPL);
                } else {
                    let correction = Correction::replace(bp_loc.start_offset(), bp_loc.end_offset(), format!("&{}", name));
                    self.push_with_correction(bp_loc.start_offset(), bp_loc.end_offset(), MSG_EXPL, correction);
                }

                // Walk body for `&` block argument w/ no expression (anonymous forwards).
                struct BAV {
                    hits: Vec<(usize, usize, bool, Option<usize>, Option<usize>)>, // (start, end, has_parens, msg_end, args_end)
                }
                impl Visit<'_> for BAV {
                    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
                        // `&` (anonymous block arg) is node.block() as BlockArgumentNode in Prism
                        if let Some(block_node) = node.block() {
                            if let Some(ba) = block_node.as_block_argument_node() {
                                if ba.expression().is_none() {
                                    let loc = ba.location();
                                    let has_parens = node.opening_loc().is_some();
                                    let msg_end = node.message_loc().map(|l| l.end_offset());
                                    let args_end = node.arguments().map(|a| a.location().end_offset());
                                    self.hits.push((loc.start_offset(), loc.end_offset(), has_parens, msg_end, args_end));
                                }
                            }
                        }
                        ruby_prism::visit_call_node(self, node);
                    }
                }
                let mut bav = BAV { hits: Vec::new() };
                if let Some(body) = node.body() {
                    bav.visit(&body);
                }
                for (s, e, has_parens, msg_end, args_end) in bav.hits {
                    if already_in_use {
                        self.push_no_correction(s, e, MSG_EXPL);
                    } else {
                        let edits = explicit_correction_edits(s, e, name, has_parens, msg_end, args_end);
                        let correction = Correction { edits };
                        self.push_with_correction(s, e, MSG_EXPL, correction);
                    }
                }
            }
        }
    }

    fn is_name_in_use(&self, node: &DefNode, name: &str) -> bool {
        // Check if `name` appears as a local variable (not as block forward) in the method
        struct LVScan<'c> { name: &'c str, found: bool }
        impl<'c> Visit<'_> for LVScan<'c> {
            fn visit_local_variable_read_node(&mut self, n: &ruby_prism::LocalVariableReadNode) {
                if n.name().as_slice() == self.name.as_bytes() { self.found = true; }
            }
            fn visit_required_parameter_node(&mut self, n: &ruby_prism::RequiredParameterNode) {
                if n.name().as_slice() == self.name.as_bytes() { self.found = true; }
            }
            fn visit_optional_parameter_node(&mut self, n: &ruby_prism::OptionalParameterNode) {
                if n.name().as_slice() == self.name.as_bytes() { self.found = true; }
            }
        }
        let mut scan = LVScan { name, found: false };
        if let Some(params) = node.parameters() {
            scan.visit_parameters_node(&params);
        }
        if let Some(body) = node.body() {
            scan.visit(&body);
        }
        scan.found
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
