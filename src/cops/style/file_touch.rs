//! Style/FileTouch cop
//!
//! `File.open(f, 'a') {}` (empty block in append mode) → `FileUtils.touch(f)`.

use crate::cops::{CheckContext, Cop};
use crate::helpers::node_match as m;
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
const APPEND_MODES: &[&str] = &["a", "a+", "ab", "a+b", "at", "a+t"];

#[derive(Default)]
pub struct FileTouch;

impl FileTouch {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for FileTouch {
    fn name(&self) -> &'static str {
        "Style/FileTouch"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node) != "open" {
            return vec![];
        }
        let recv = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        if !m::is_toplevel_constant_named(&recv, "File") {
            return vec![];
        }
        // Must have a block that is empty.
        let block = match node.block() {
            Some(b) => b,
            None => return vec![],
        };
        let block = match block.as_block_node() {
            Some(b) => b,
            None => return vec![],
        };
        if block.body().is_some() {
            return vec![];
        }

        // Args: exactly 2, first = filename (any expr), second = string literal in APPEND_MODES.
        let args_node = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let args: Vec<_> = args_node.arguments().iter().collect();
        if args.len() != 2 {
            return vec![];
        }
        let mode_s = match args[1].as_string_node() {
            Some(s) => s,
            None => return vec![],
        };
        let mode = String::from_utf8_lossy(mode_s.unescaped()).to_string();
        if !APPEND_MODES.contains(&mode.as_str()) {
            return vec![];
        }
        // filename source
        let arg0 = &args[0];
        // Reject if arg0 is itself a string with append mode? No - first arg is path, not mode. Proceed.
        let _ = arg0; // discard warning about matching Node

        let name_loc = match args.first() {
            Some(n) => n.location(),
            None => return vec![],
        };
        let name_src = &ctx.source[name_loc.start_offset()..name_loc.end_offset()];

        // Range = entire call including block.
        let call_loc = node.location();
        let start = call_loc.start_offset();
        let end = match node.block() {
            Some(b) => b.location().end_offset(),
            None => call_loc.end_offset(),
        };
        let msg = format!(
            "Use `FileUtils.touch({})` instead of `File.open` in append mode with empty block.",
            name_src
        );
        let replacement = format!("FileUtils.touch({})", name_src);
        vec![ctx
            .offense_with_range(self.name(), &msg, self.severity(), start, end)
            .with_correction(Correction::replace(start, end, replacement))]
    }
}

crate::register_cop!("Style/FileTouch", |_cfg| Some(Box::new(FileTouch::new())));
