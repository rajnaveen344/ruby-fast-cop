//! Style/FileWrite cop
//!
//! `File.open(f, 'w').write(c)` / `File.open(f, 'w') { |x| x.write(c) }`
//! → `File.write(f, c)` (or `binwrite` if mode ends with 'b').

use crate::cops::{CheckContext, Cop};
use crate::helpers::node_match as m;
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

const WRITE_MODES: &[&str] = &["w", "wt", "wb", "w+", "w+t", "w+b"];

#[derive(Default)]
pub struct FileWrite;

impl FileWrite {
    pub fn new() -> Self {
        Self
    }
}

fn match_mode(s: &str) -> bool {
    WRITE_MODES.contains(&s)
}

fn write_method_for(mode: &str) -> &'static str {
    if mode.ends_with('b') { "binwrite" } else { "write" }
}

fn arg_src<'s>(node: &Node, src: &'s str) -> &'s str {
    let loc = node.location();
    &src[loc.start_offset()..loc.end_offset()]
}

// Check File.open receiver/args. Returns (filename_src, mode) if matches.
fn parse_file_open<'s>(call: &ruby_prism::CallNode, src: &'s str) -> Option<(&'s str, String)> {
    // Method must be `open`
    if node_name!(call) != "open" {
        return None;
    }
    // Receiver must be File (or ::File)
    let recv = call.receiver()?;
    if !m::is_toplevel_constant_named(&recv, "File") {
        return None;
    }
    let args = call.arguments()?;
    let list: Vec<_> = args.arguments().iter().collect();
    if list.len() != 2 {
        return None;
    }
    let filename = arg_src(&list[0], src);
    let s = list[1].as_string_node()?;
    let mode = String::from_utf8_lossy(s.unescaped()).to_string();
    if !match_mode(&mode) {
        return None;
    }
    Some((filename, mode))
}

// Is a node a splat (SplatNode)?
fn is_splat(node: &Node) -> bool {
    node.as_splat_node().is_some()
}

// If `call` = `recv.write(content)` with exactly 1 non-splat arg, return content node.
fn send_write_content<'a>(call: &ruby_prism::CallNode<'a>) -> Option<Node<'a>> {
    if node_name!(call) != "write" {
        return None;
    }
    let args = call.arguments()?;
    let list: Vec<_> = args.arguments().iter().collect();
    if list.len() != 1 {
        return None;
    }
    if is_splat(&list[0]) {
        return None;
    }
    // Must not have block on write itself
    if call.block().is_some() {
        return None;
    }
    Some(list.into_iter().next().unwrap())
}

// Check heredoc: StringNode with opening starting with `<<`.
fn heredoc_trail<'s>(node: &Node, src: &'s str) -> Option<&'s str> {
    let s = node.as_string_node()?;
    let open = s.opening_loc()?;
    let open_src = &src[open.start_offset()..open.end_offset()];
    if !open_src.starts_with("<<") {
        return None;
    }
    let content = s.content_loc();
    let close = s.closing_loc()?;
    let mut end = close.end_offset();
    // closing_loc for heredocs often includes the trailing newline; strip it.
    let bytes = src.as_bytes();
    while end > 0 && (bytes[end - 1] == b'\n' || bytes[end - 1] == b'\r') {
        end -= 1;
    }
    Some(&src[content.start_offset()..end])
}

impl Cop for FileWrite {
    fn name(&self) -> &'static str {
        "Style/FileWrite"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node).into_owned();
        let src = ctx.source;

        // Pattern 1: `File.open(f, 'w').write(content)`
        // node = outer `.write`. Recv = CallNode File.open(f, 'w').
        if method == "write" {
            let recv = match node.receiver() {
                Some(r) => r,
                None => return vec![],
            };
            let inner = match recv.as_call_node() {
                Some(c) => c,
                None => return vec![],
            };
            if inner.block().is_some() {
                return vec![];
            }
            let (filename, mode) = match parse_file_open(&inner, src) {
                Some(v) => v,
                None => return vec![],
            };
            let content = match send_write_content(node) {
                Some(c) => c,
                None => return vec![],
            };
            let content_src = arg_src(&content, src);
            let wm = write_method_for(&mode);
            let msg = format!("Use `File.{}`.", wm);
            let outer_loc = node.location();
            let offense_start = outer_loc.start_offset();
            let offense_end = outer_loc.end_offset();
            let selector_start = inner
                .message_loc()
                .map(|l| l.start_offset())
                .unwrap_or(offense_start);
            // No heredoc in pattern 1 (single-line only by nature of `.write(x)` after `.open(...)`).
            let mut replacement = format!("{}({}, {})", wm, filename, content_src);
            if let Some(trail) = heredoc_trail(&content, src) {
                replacement.push('\n');
                replacement.push_str(trail);
            }
            return vec![ctx
                .offense_with_range(self.name(), &msg, self.severity(), offense_start, offense_end)
                .with_correction(Correction::replace(selector_start, offense_end, replacement))];
        }

        // Pattern 2: `File.open(f, 'w') { |x| x.write(content) }`
        // node = File.open with block.
        if method == "open" {
            let (filename, mode) = match parse_file_open(node, src) {
                Some(v) => v,
                None => return vec![],
            };
            let block_node = match node.block() {
                Some(b) => b,
                None => return vec![],
            };
            let blk = match block_node.as_block_node() {
                Some(b) => b,
                None => return vec![],
            };
            // Parameters: exactly 1 required.
            let params = match blk.parameters() { Some(p) => p, None => return vec![] };
            let bp = match params.as_block_parameters_node() { Some(b) => b, None => return vec![] };
            let pn = match bp.parameters() { Some(p) => p, None => return vec![] };
            let reqs: Vec<_> = pn.requireds().iter().collect();
            if reqs.len() != 1 {
                return vec![];
            }
            let r = match reqs[0].as_required_parameter_node() { Some(r) => r, None => return vec![] };
            let pname: String = node_name!(r).into_owned();
            // Body: single statement `pname.write(content)`.
            let body = match blk.body() {
                Some(b) => b,
                None => return vec![],
            };
            let stmt: Node = if let Some(stmts) = body.as_statements_node() {
                let v: Vec<_> = stmts.body().iter().collect();
                if v.len() != 1 {
                    return vec![];
                }
                v.into_iter().next().unwrap()
            } else {
                body
            };
            let call = match stmt.as_call_node() {
                Some(c) => c,
                None => return vec![],
            };
            // Receiver must be local var named pname.
            let recv = match call.receiver() {
                Some(r) => r,
                None => return vec![],
            };
            let lv = match recv.as_local_variable_read_node() {
                Some(l) => l,
                None => return vec![],
            };
            if node_name!(lv) != pname {
                return vec![];
            }
            let content = match send_write_content(&call) {
                Some(c) => c,
                None => return vec![],
            };
            let content_src = arg_src(&content, src);
            let wm = write_method_for(&mode);
            let msg = format!("Use `File.{}`.", wm);

            // Offense range = node loc (File.open(...) — NOT including block); Rubocop
            // uses write_node = parent (block) source range. The fixture column_end matches
            // the first-line end which is widened from newline rule. We emit range =
            // (File.open start, block end) so widening yields col_at_newline+1 = end of line 1.
            let offense_start = node.location().start_offset();
            let offense_end = block_node.location().end_offset();

            // Correction: from inner selector (open) to end of block, replace with
            // `write(filename, content)` plus heredoc trail if needed.
            let selector_start = node
                .message_loc()
                .map(|l| l.start_offset())
                .unwrap_or(offense_start);
            let mut replacement = format!("{}({}, {})", wm, filename, content_src);
            if let Some(trail) = heredoc_trail(&content, src) {
                replacement.push('\n');
                replacement.push_str(trail);
            }
            return vec![ctx
                .offense_with_range(self.name(), &msg, self.severity(), offense_start, offense_end)
                .with_correction(Correction::replace(selector_start, offense_end, replacement))];
        }

        vec![]
    }
}

crate::register_cop!("Style/FileWrite", |_cfg| Some(Box::new(FileWrite::new())));
