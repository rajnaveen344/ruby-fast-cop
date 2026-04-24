//! Style/FileRead cop
//!
//! `File.open(f).read` / `File.open(f, &:read)` / `File.open(f) { |x| x.read }`
//! → `File.read(f)` (or `binread` if mode ends with 'b').

use crate::cops::{CheckContext, Cop};
use crate::helpers::node_match as m;
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

const READ_MODES: &[&str] = &["r", "rt", "rb", "r+", "r+t", "r+b"];

#[derive(Default)]
pub struct FileRead;

impl FileRead {
    pub fn new() -> Self {
        Self
    }
}

fn match_mode(s: &str) -> bool {
    READ_MODES.contains(&s)
}

fn mode_of_call(call: &ruby_prism::CallNode, _src: &str) -> Option<String> {
    // Extracts mode string from File.open regular args. Returns None if invalid,
    // or Some("") if mode absent (then defaults to "r").
    let args = call.arguments()?;
    let list: Vec<_> = args.arguments().iter().collect();
    if list.is_empty() {
        return None;
    }
    if list.len() == 1 {
        return Some("".to_string());
    }
    if list.len() == 2 {
        let s = list[1].as_string_node()?;
        let mode = String::from_utf8_lossy(s.unescaped()).to_string();
        if match_mode(&mode) {
            return Some(mode);
        }
        return None;
    }
    None
}

fn filename_src<'s>(call: &ruby_prism::CallNode, src: &'s str) -> Option<&'s str> {
    let args = call.arguments()?;
    let list: Vec<_> = args.arguments().iter().collect();
    let first = list.into_iter().next()?;
    let loc = first.location();
    Some(&src[loc.start_offset()..loc.end_offset()])
}

fn has_block_pass_read(call: &ruby_prism::CallNode) -> bool {
    if let Some(b) = call.block() {
        if let Some(bp) = b.as_block_argument_node() {
            if let Some(expr) = bp.expression() {
                if let Some(sym) = expr.as_symbol_node() {
                    return String::from_utf8_lossy(sym.unescaped()) == "read";
                }
            }
        }
    }
    false
}

fn read_method_for(mode: &str) -> &'static str {
    if mode.ends_with('b') {
        "binread"
    } else {
        "read"
    }
}

impl Cop for FileRead {
    fn name(&self) -> &'static str {
        "Style/FileRead"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node).into_owned();
        let src = ctx.source;

        // Case 1: outer .read on File.open(...). Pattern: (send (send File :open $args) :read)
        if method == "read" {
            if node.arguments().is_some() {
                // `.read` should be zero-arg
                return vec![];
            }
            // No block on the outer `.read`.
            if node.block().is_some() {
                return vec![];
            }
            let recv = match node.receiver() {
                Some(r) => r,
                None => return vec![],
            };
            let inner = match recv.as_call_node() {
                Some(c) => c,
                None => return vec![],
            };
            if node_name!(inner) != "open" {
                return vec![];
            }
            let inner_recv = match inner.receiver() {
                Some(r) => r,
                None => return vec![],
            };
            if !m::is_toplevel_constant_named(&inner_recv, "File") {
                return vec![];
            }
            // No block on inner (just args).
            if inner.block().is_some() {
                return vec![];
            }
            let mode = match mode_of_call(&inner, src) {
                Some(v) => v,
                None => return vec![],
            };
            let mode_ref = if mode.is_empty() { "r" } else { mode.as_str() };
            let filename = match filename_src(&inner, src) {
                Some(v) => v,
                None => return vec![],
            };

            // Offense range = full outer `.read` node. Correction replaces
            // from `open` selector of inner to end of outer.
            let outer_loc = node.location();
            let offense_start = outer_loc.start_offset();
            let offense_end = outer_loc.end_offset();
            let selector_start = inner.message_loc().map(|l| l.start_offset()).unwrap_or(offense_start);
            let replacement = format!("{}({})", read_method_for(mode_ref), filename);
            let msg = format!("Use `File.{}`.", read_method_for(mode_ref));
            return vec![ctx
                .offense_with_range(self.name(), &msg, self.severity(), offense_start, offense_end)
                .with_correction(Correction::replace(selector_start, offense_end, replacement))];
        }

        // Cases 2 & 3: outer is File.open with either block_pass(&:read) or
        // explicit block { |f| f.read }.
        if method == "open" {
            let recv = match node.receiver() {
                Some(r) => r,
                None => return vec![],
            };
            if !m::is_toplevel_constant_named(&recv, "File") {
                return vec![];
            }

            // block_pass with :read?
            let has_bp_read = has_block_pass_read(node);

            // explicit block with body `f.read`?
            let mut block_read_ok = false;
            if let Some(b) = node.block() {
                if let Some(blk) = b.as_block_node() {
                    // Parameters must be (|f|) one arg.
                    let param_name: Option<String> = match blk.parameters() {
                        Some(p) => {
                            // p is Node::BlockParametersNode
                            if let Some(bp) = p.as_block_parameters_node() {
                                if let Some(params_node) = bp.parameters() {
                                    let reqs: Vec<_> = params_node.requireds().iter().collect();
                                    if reqs.len() == 1 {
                                        reqs[0]
                                            .as_required_parameter_node()
                                            .map(|r| node_name!(r).into_owned())
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        None => None,
                    };
                    if let Some(pname) = param_name {
                        // Body = single statement: `pname.read`
                        let body = blk.body();
                        let stmt: Option<Node> = match body {
                            Some(b) => {
                                if let Some(stmts) = b.as_statements_node() {
                                    let v: Vec<_> = stmts.body().iter().collect();
                                    if v.len() == 1 { Some(v.into_iter().next().unwrap()) } else { None }
                                } else {
                                    Some(b)
                                }
                            }
                            None => None,
                        };
                        if let Some(s) = stmt {
                            if let Some(c) = s.as_call_node() {
                                if node_name!(c) == "read" && c.arguments().is_none() {
                                    if let Some(r) = c.receiver() {
                                        if let Some(lv) = r.as_local_variable_read_node() {
                                            if node_name!(lv) == pname {
                                                block_read_ok = true;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if !has_bp_read && !block_read_ok {
                return vec![];
            }

            // Determine mode: for block case, mode is read from regular args.
            // For block_pass case, mode_of_call handles stripping block pass.
            let mode = if has_bp_read {
                match mode_of_call(node, src) {
                    Some(v) => v,
                    None => return vec![],
                }
            } else {
                // explicit block: mode from args (no block pass).
                let list: Vec<_> = match node.arguments() {
                    Some(a) => a.arguments().iter().collect(),
                    None => vec![],
                };
                if list.is_empty() {
                    return vec![];
                }
                if list.len() == 1 {
                    "".to_string()
                } else if list.len() == 2 {
                    let Some(s) = list[1].as_string_node() else { return vec![] };
                    let mode = String::from_utf8_lossy(s.unescaped()).to_string();
                    if !match_mode(&mode) {
                        return vec![];
                    }
                    mode
                } else {
                    return vec![];
                }
            };
            let mode_ref = if mode.is_empty() { "r" } else { mode.as_str() };
            let filename = match filename_src(node, src) {
                Some(v) => v,
                None => return vec![],
            };
            // Offense range:
            //   block_pass case: node itself (File.open(...)) — outer call loc.
            //   block case:      node with block included — call + block end.
            let outer_loc = node.location();
            let offense_start = outer_loc.start_offset();
            let offense_end = match node.block() {
                Some(b) => {
                    if b.as_block_node().is_some() {
                        b.location().end_offset()
                    } else {
                        outer_loc.end_offset()
                    }
                }
                None => outer_loc.end_offset(),
            };

            let selector_start = node.message_loc().map(|l| l.start_offset()).unwrap_or(offense_start);
            let replacement = format!("{}({})", read_method_for(mode_ref), filename);
            let msg = format!("Use `File.{}`.", read_method_for(mode_ref));
            return vec![ctx
                .offense_with_range(self.name(), &msg, self.severity(), offense_start, offense_end)
                .with_correction(Correction::replace(selector_start, offense_end, replacement))];
        }

        vec![]
    }
}

crate::register_cop!("Style/FileRead", |_cfg| Some(Box::new(FileRead::new())));
