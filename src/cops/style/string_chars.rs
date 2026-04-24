//! Style/StringChars cop
//!
//! `str.split(//)` / `str.split('')` / `str.split("")` → `str.chars`.

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};

#[derive(Default)]
pub struct StringChars;

impl StringChars {
    pub fn new() -> Self { Self }
}

impl Cop for StringChars {
    fn name(&self) -> &'static str { "Style/StringChars" }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node) != "split" { return vec![]; }
        let args = match node.arguments() { Some(a) => a, None => return vec![] };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 { return vec![]; }
        let arg = &arg_list[0];
        let arg_start = arg.location().start_offset();
        let arg_end = arg.location().end_offset();
        let arg_src = &ctx.source[arg_start..arg_end];
        if !matches!(arg_src, "//" | "''" | "\"\"") { return vec![]; }

        let msg_loc = match node.message_loc() { Some(l) => l, None => return vec![] };
        let start = msg_loc.start_offset();
        let end = node.location().end_offset();
        let current = &ctx.source[start..end];
        let msg = format!("Use `chars` instead of `{}`.", current);
        vec![ctx
            .offense_with_range(self.name(), &msg, Severity::Convention, start, end)
            .with_correction(Correction::replace(start, end, "chars".to_string()))]
    }
}

crate::register_cop!("Style/StringChars", |_cfg| Some(Box::new(StringChars::new())));
