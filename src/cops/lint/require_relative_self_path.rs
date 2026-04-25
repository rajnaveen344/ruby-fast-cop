//! Lint/RequireRelativeSelfPath cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use std::path::Path;

const MSG: &str = "Remove the `require_relative` that requires itself.";

#[derive(Default)]
pub struct RequireRelativeSelfPath;

impl RequireRelativeSelfPath {
    pub fn new() -> Self { Self }
}

impl Cop for RequireRelativeSelfPath {
    fn name(&self) -> &'static str { "Lint/RequireRelativeSelfPath" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node).as_ref() != "require_relative" { return vec![]; }
        if node.receiver().is_some() { return vec![]; }
        let args = match node.arguments() { Some(a) => a, None => return vec![] };
        let first = match args.arguments().iter().next() { Some(f) => f, None => return vec![] };
        let s = match first.as_string_node() { Some(s) => s, None => return vec![] };
        let arg = String::from_utf8_lossy(s.unescaped()).to_string();

        let stem = Path::new(ctx.filename).file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let base = Path::new(ctx.filename).file_name().and_then(|s| s.to_str()).unwrap_or("");
        if arg != stem && arg != base { return vec![]; }

        let loc = node.location();
        let src = ctx.source.as_bytes();
        let mut line_start = loc.start_offset();
        while line_start > 0 && src[line_start - 1] != b'\n' { line_start -= 1; }
        let mut line_end = loc.end_offset();
        while line_end < src.len() && src[line_end] != b'\n' { line_end += 1; }
        if line_end < src.len() { line_end += 1; }

        let off = ctx.offense_with_range(self.name(), MSG, self.severity(), loc.start_offset(), loc.end_offset())
            .with_correction(Correction::delete(line_start, line_end));
        vec![off]
    }
}

crate::register_cop!("Lint/RequireRelativeSelfPath", |_cfg| Some(Box::new(RequireRelativeSelfPath::new())));
