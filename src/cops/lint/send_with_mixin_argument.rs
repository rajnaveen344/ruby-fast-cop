//! Lint/SendWithMixinArgument - Use include/prepend/extend directly instead of send.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Use `%method %modules` instead of `%bad_method`.";
const MIXIN_METHODS: &[&str] = &["include", "prepend", "extend"];
const SEND_METHODS: &[&str] = &["send", "public_send", "__send__"];

#[derive(Default)]
pub struct SendWithMixinArgument;

impl SendWithMixinArgument {
    pub fn new() -> Self { Self }
}

impl Cop for SendWithMixinArgument {
    fn name(&self) -> &'static str { "Lint/SendWithMixinArgument" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl Visit<'_> for Visitor<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let method = String::from_utf8_lossy(node.name().as_slice());
        let method_str = method.as_ref();

        if SEND_METHODS.contains(&method_str) {
            // Must have a receiver (Foo.send(...))
            if node.receiver().is_some() {
                self.check_send(node);
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
}

impl<'a> Visitor<'a> {
    fn check_send(&mut self, node: &ruby_prism::CallNode) {
        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() < 2 { return; }

        // First arg must be :include/:prepend/:extend or "include"/"prepend"/"extend"
        let mixin_method = match extract_symbol_or_string(&arg_list[0]) {
            Some(s) => s,
            None => return,
        };
        if !MIXIN_METHODS.contains(&mixin_method.as_str()) { return; }

        // Remaining args must be constants
        let module_sources: Vec<String> = arg_list[1..]
            .iter()
            .map(|a| {
                let loc = a.location();
                self.ctx.source[loc.start_offset()..loc.end_offset()].to_string()
            })
            .collect();
        if module_sources.is_empty() { return; }
        let modules_str = module_sources.join(", ");

        // Bad location: from selector (send/__send__/public_send) to end of call
        let node_loc = node.location();
        // selector = method name location
        // We compute: from the dot+method-name through end of call args
        // Per RuboCop: range_between(loc.selector.begin_pos, loc.expression.end_pos)
        // The selector is the method name itself. We find it via name_loc if available.
        // For CallNode, use location of node minus receiver
        let recv_loc = node.receiver().map(|r| r.location());
        let bad_start = if let Some(rl) = recv_loc {
            // skip receiver + dot: find the method start
            // dot is 1 or 2 chars (. or &.), then method name
            let after_recv = rl.end_offset();
            // selector start: skip dot(s)
            let src_bytes = self.ctx.source.as_bytes();
            let mut sel_start = after_recv;
            while sel_start < node_loc.end_offset()
                && (src_bytes[sel_start] == b'.' || src_bytes[sel_start] == b'&')
            {
                sel_start += 1;
            }
            sel_start
        } else {
            node_loc.start_offset()
        };
        let bad_end = node_loc.end_offset();

        let bad_src = &self.ctx.source[bad_start..bad_end];
        let msg = MSG
            .replace("%method", &mixin_method)
            .replace("%modules", &modules_str)
            .replace("%bad_method", bad_src);

        let replacement = format!("{} {}", mixin_method, modules_str);
        let mut offense = self.ctx.offense_with_range(
            "Lint/SendWithMixinArgument",
            &msg,
            Severity::Warning,
            bad_start,
            bad_end,
        );
        offense = offense.with_correction(Correction::replace(bad_start, bad_end, replacement));
        self.offenses.push(offense);
    }
}

fn extract_symbol_or_string(node: &ruby_prism::Node) -> Option<String> {
    if let Some(sym) = node.as_symbol_node() {
        let s = String::from_utf8_lossy(sym.unescaped()).to_string();
        return Some(s);
    }
    if let Some(str_node) = node.as_string_node() {
        let s = String::from_utf8_lossy(str_node.unescaped()).to_string();
        return Some(s);
    }
    None
}

crate::register_cop!("Lint/SendWithMixinArgument", |_cfg| Some(Box::new(SendWithMixinArgument::new())));
