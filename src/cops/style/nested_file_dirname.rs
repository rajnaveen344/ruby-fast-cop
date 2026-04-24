//! Style/NestedFileDirname cop
//!
//! `File.dirname(File.dirname(path))` → `File.dirname(path, 2)` (Ruby 3.1+).

use crate::cops::{CheckContext, Cop};
use crate::helpers::node_match as m;
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{CallNode, Visit};

#[derive(Default)]
pub struct NestedFileDirname;

impl NestedFileDirname {
    pub fn new() -> Self {
        Self
    }
}

fn is_file_dirname(node: &ruby_prism::Node) -> bool {
    let call = match node.as_call_node() {
        Some(c) => c,
        None => return false,
    };
    if node_name!(call) != "dirname" {
        return false;
    }
    let recv = match call.receiver() {
        Some(r) => r,
        None => return false,
    };
    m::is_toplevel_constant_named(&recv, "File")
}

impl Cop for NestedFileDirname {
    fn name(&self) -> &'static str {
        "Style/NestedFileDirname"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(3, 1) {
            return vec![];
        }
        let mut v = Visitor { ctx, inside_file_dirname: false, offenses: Vec::new() };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    inside_file_dirname: bool,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn try_flag(&mut self, call: &CallNode) {
        // Not `File.dirname`? nothing to do.
        let name = node_name!(call);
        if name != "dirname" {
            return;
        }
        let recv = match call.receiver() {
            Some(r) => r,
            None => return,
        };
        if !m::is_toplevel_constant_named(&recv, "File") {
            return;
        }
        // Parent is File.dirname → skip (outer will emit).
        if self.inside_file_dirname {
            return;
        }
        // First arg must be File.dirname.
        let args = match call.arguments() {
            Some(a) => a,
            None => return,
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.is_empty() {
            return;
        }
        if !is_file_dirname(&arg_list[0]) {
            return;
        }

        // Walk inner File.dirname chain by location-lookup via the outer node.
        // Use recursive helper that carries level and returns (path byte range, level).
        let (path_start, path_end, level) =
            match unwrap_dirname_chain(&arg_list[0], 2) {
                Some(v) => v,
                None => return,
            };
        if level < 2 {
            return;
        }
        let path_src = &self.ctx.source[path_start..path_end];
        let msg = format!("Use `dirname({}, {})` instead.", path_src, level);

        let outer_loc = call.location();
        let msg_loc = call.message_loc().unwrap();
        let start = msg_loc.start_offset();
        let end = outer_loc.end_offset();
        let replacement = format!("dirname({}, {})", path_src, level);
        self.offenses.push(
            self.ctx
                .offense_with_range(
                    "Style/NestedFileDirname",
                    &msg,
                    Severity::Convention,
                    start,
                    end,
                )
                .with_correction(Correction::replace(start, end, replacement)),
        );
    }
}

fn unwrap_dirname_chain(node: &ruby_prism::Node, level: u32) -> Option<(usize, usize, u32)> {
    // node is known to be File.dirname(...). Walk its first arg.
    let call = node.as_call_node()?;
    let args = call.arguments()?;
    let arg_list: Vec<_> = args.arguments().iter().collect();
    let first = arg_list.into_iter().next()?;
    if is_file_dirname(&first) {
        unwrap_dirname_chain(&first, level + 1)
    } else {
        let loc = first.location();
        Some((loc.start_offset(), loc.end_offset(), level))
    }
}

impl<'src, 'a> Visit<'src> for Visitor<'a> {
    fn visit_call_node(&mut self, node: &CallNode<'src>) {
        self.try_flag(node);

        // Track whether we're descending into a File.dirname call so inner
        // File.dirname calls can detect parent.
        let is_fd = {
            let n = node_name!(node);
            if n == "dirname" {
                if let Some(r) = node.receiver() {
                    m::is_toplevel_constant_named(&r, "File")
                } else {
                    false
                }
            } else {
                false
            }
        };
        let prev = self.inside_file_dirname;
        if is_fd {
            self.inside_file_dirname = true;
        }
        ruby_prism::visit_call_node(self, node);
        self.inside_file_dirname = prev;
    }
}

crate::register_cop!("Style/NestedFileDirname", |_cfg| Some(Box::new(NestedFileDirname::new())));
