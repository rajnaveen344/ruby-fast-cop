//! Style/FileEmpty cop
//!
//! Flags idioms that check whether a file is empty in favor of `File.empty?`
//! (or `FileTest.empty?`).

use crate::cops::{CheckContext, Cop};
use crate::helpers::node_match as m;
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

#[derive(Default)]
pub struct FileEmpty;

impl FileEmpty {
    pub fn new() -> Self {
        Self
    }
}

// Returns Some((class_name_src, arg_src, bang_prefix)) if `node` matches an
// offensive pattern, else None.
fn match_pattern<'a>(
    node: &ruby_prism::CallNode,
    src: &'a str,
) -> Option<(String, String, bool)> {
    let method = node_name!(node).into_owned();

    // Pattern: `C.zero?(arg)` with C = File|FileTest.
    if method == "zero?" {
        let recv = node.receiver()?;
        if let Some((class_name, _class_loc)) = match_file_class(&recv, src) {
            // arg is first & only arg
            let args_node = node.arguments()?;
            let args: Vec<_> = args_node.arguments().iter().collect();
            if args.len() == 1 {
                let arg_src = src_of(&args[0], src);
                return Some((class_name, arg_src, false));
            }
            // Pattern: `C.size(arg).zero?` (receiver is a call)
        }
        // Pattern: `C.size(arg).zero?` -> receiver is call
        if let Some(inner) = recv.as_call_node() {
            if node_name!(inner) == "size" {
                if let Some(inner_recv) = inner.receiver() {
                    if let Some((class_name, _)) = match_file_class(&inner_recv, src) {
                        let inner_args_node = inner.arguments()?;
                        let inner_args: Vec<_> =
                            inner_args_node.arguments().iter().collect();
                        if inner_args.len() == 1 {
                            let arg_src = src_of(&inner_args[0], src);
                            return Some((class_name, arg_src, false));
                        }
                    }
                }
            }
        }
        return None;
    }

    // Pattern: `C.{read,binread}(arg).empty?`
    if method == "empty?" {
        let recv = node.receiver()?;
        if let Some(inner) = recv.as_call_node() {
            let inner_name = node_name!(inner);
            if inner_name == "read" || inner_name == "binread" {
                if let Some(inner_recv) = inner.receiver() {
                    if let Some((class_name, _)) = match_file_class(&inner_recv, src) {
                        let inner_args_node = inner.arguments()?;
                        let inner_args: Vec<_> =
                            inner_args_node.arguments().iter().collect();
                        if inner_args.len() == 1 {
                            let arg_src = src_of(&inner_args[0], src);
                            return Some((class_name, arg_src, false));
                        }
                    }
                }
            }
        }
        return None;
    }

    // Patterns with binary operators (==, !=, >=)
    if method == "==" || method == "!=" || method == ">=" {
        let lhs = node.receiver()?;
        let rhs_args_node = node.arguments()?;
        let rhs_args: Vec<_> = rhs_args_node.arguments().iter().collect();
        if rhs_args.len() != 1 {
            return None;
        }
        let rhs = &rhs_args[0];

        // Strip leading `!` from lhs (Prism: CallNode with method `!`).
        let (lhs_core, has_bang) = strip_bang(&lhs);

        let lhs_call = lhs_core.as_call_node()?;
        let lhs_method = node_name!(lhs_call).into_owned();
        let lhs_recv = lhs_call.receiver()?;
        let (class_name, _) = match_file_class(&lhs_recv, src)?;

        // size pattern with `== 0` or `>= 0`
        if lhs_method == "size" && (method == "==" || method == ">=") {
            // rhs must be integer 0
            if !is_integer_zero(rhs) {
                return None;
            }
            let lhs_args_node = lhs_call.arguments()?;
            let lhs_args: Vec<_> = lhs_args_node.arguments().iter().collect();
            if lhs_args.len() != 1 {
                return None;
            }
            let arg_src = src_of(&lhs_args[0], src);
            // Bang logic:
            //  (==)  bang iff lhs has bang      (!File.size == 0 => !File.empty?)
            //  (>=)  bang iff NOT has_bang     (File.size >= 0 => !File.empty?;
            //                                    !File.size >= 0 => File.empty?)
            let bang = match method.as_str() {
                "==" => has_bang,
                ">=" => !has_bang,
                _ => unreachable!(),
            };
            return Some((class_name, arg_src, bang));
        }

        // read/binread pattern with `== ''` or `!= ''`
        if lhs_method == "read" || lhs_method == "binread" {
            if method == ">=" {
                return None;
            }
            // rhs must be empty string literal
            if !is_empty_string_lit(rhs, src) {
                return None;
            }
            let lhs_args_node = lhs_call.arguments()?;
            let lhs_args: Vec<_> = lhs_args_node.arguments().iter().collect();
            if lhs_args.len() != 1 {
                return None;
            }
            let arg_src = src_of(&lhs_args[0], src);
            // Bang logic:
            //  (==) bang iff has_bang
            //  (!=) bang iff NOT has_bang
            let bang = match method.as_str() {
                "==" => has_bang,
                "!=" => !has_bang,
                _ => unreachable!(),
            };
            return Some((class_name, arg_src, bang));
        }
    }

    None
}

fn strip_bang<'a>(node: &'a Node<'a>) -> (&'a Node<'a>, bool) {
    if let Some(call) = node.as_call_node() {
        if node_name!(call) == "!" && call.arguments().is_none() {
            // Prism: `!expr` => CallNode (recv=expr, name="!")
            // We need the inner expression: it's the receiver.
            if let Some(_inner) = call.receiver() {
                // Can't return a reference to a temporary; but receiver() returns owned Node.
                // Trick: we don't actually need to return the Node reference; we use it via
                // the caller by converting. Since Rust lifetimes make this tricky, do it
                // without a recursive strip (only strip once).
                // Strategy change: return None signal via tuple.
                return (node, true); // placeholder; real stripping done in caller below
            }
        }
    }
    (node, false)
}

fn match_file_class(node: &Node, src: &str) -> Option<(String, (usize, usize))> {
    // Matches `File` or `FileTest` (with optional `::` prefix).
    let name = m::constant_simple_name(node)?;
    if name != "File" && name != "FileTest" {
        return None;
    }
    // Accept top-level only (matches `{nil? cbase}` in matcher).
    if !m::is_toplevel_constant_named(node, &name) {
        return None;
    }
    let loc = node.location();
    Some((src[loc.start_offset()..loc.end_offset()].to_string(), (loc.start_offset(), loc.end_offset())))
}

fn src_of(node: &Node, src: &str) -> String {
    let loc = node.location();
    src[loc.start_offset()..loc.end_offset()].to_string()
}

fn is_integer_zero(node: &Node) -> bool {
    if let Some(i) = node.as_integer_node() {
        let s = String::from_utf8_lossy(i.location().as_slice());
        return s.trim() == "0";
    }
    false
}

fn is_empty_string_lit(node: &Node, _src: &str) -> bool {
    if let Some(s) = node.as_string_node() {
        return s.unescaped().is_empty();
    }
    false
}

impl Cop for FileEmpty {
    fn name(&self) -> &'static str {
        "Style/FileEmpty"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if !ctx.ruby_version_at_least(2, 4) {
            return vec![];
        }

        // Use a dedicated matcher that handles `!expr` unwrapping internally.
        let matched = match_with_unwrap(node, ctx.source);
        let (class_name, arg_src, bang) = match matched {
            Some(v) => v,
            None => return vec![],
        };

        let replacement = format!(
            "{}{}.empty?({})",
            if bang { "!" } else { "" },
            class_name,
            arg_src
        );
        let msg = format!("Use `{}.empty?({})` instead.", class_name, arg_src);

        // Offense range = full node (with `!` prefix if present, matching `node` passed).
        let loc = node.location();
        // If this call is `a == b` where a has `!`, the offense spans the whole binop.
        // That's already `node.location()`.
        let start = loc.start_offset();
        let end = loc.end_offset();
        vec![ctx
            .offense_with_range(self.name(), &msg, self.severity(), start, end)
            .with_correction(Correction::replace(start, end, replacement))]
    }
}

/// Matcher that unwraps `!expr` on the binop lhs when needed.
fn match_with_unwrap(
    node: &ruby_prism::CallNode,
    src: &str,
) -> Option<(String, String, bool)> {
    let method = node_name!(node).into_owned();

    // zero? / empty?: no bang unwrapping needed (receiver check is direct).
    if method == "zero?" || method == "empty?" {
        return match_pattern(node, src);
    }

    // Binary ops: ==, !=, >=
    if !matches!(method.as_str(), "==" | "!=" | ">=") {
        return None;
    }
    let lhs = node.receiver()?;
    let rhs_args_node = node.arguments()?;
    let rhs_args: Vec<_> = rhs_args_node.arguments().iter().collect();
    if rhs_args.len() != 1 {
        return None;
    }
    let rhs = &rhs_args[0];

    // Determine lhs_core and has_bang.
    let (lhs_call, has_bang) = {
        let call = lhs.as_call_node()?;
        if node_name!(call) == "!" && call.arguments().is_none() {
            let inner = call.receiver()?;
            let inner_call = inner.as_call_node()?;
            (inner_call, true)
        } else {
            (call, false)
        }
    };

    let lhs_method = node_name!(lhs_call).into_owned();
    let lhs_recv = lhs_call.receiver()?;
    let (class_name, _) = match_file_class(&lhs_recv, src)?;

    if lhs_method == "size" && (method == "==" || method == ">=") {
        if !is_integer_zero(rhs) {
            return None;
        }
        let lhs_args_node = lhs_call.arguments()?;
        let lhs_args: Vec<_> = lhs_args_node.arguments().iter().collect();
        if lhs_args.len() != 1 {
            return None;
        }
        let arg_src = src_of(&lhs_args[0], src);
        let bang = match method.as_str() {
            "==" => has_bang,
            ">=" => !has_bang,
            _ => unreachable!(),
        };
        return Some((class_name, arg_src, bang));
    }

    if (lhs_method == "read" || lhs_method == "binread")
        && (method == "==" || method == "!=")
    {
        if !is_empty_string_lit(rhs, src) {
            return None;
        }
        let lhs_args_node = lhs_call.arguments()?;
        let lhs_args: Vec<_> = lhs_args_node.arguments().iter().collect();
        if lhs_args.len() != 1 {
            return None;
        }
        let arg_src = src_of(&lhs_args[0], src);
        let bang = match method.as_str() {
            "==" => has_bang,
            "!=" => !has_bang,
            _ => unreachable!(),
        };
        return Some((class_name, arg_src, bang));
    }

    None
}

crate::register_cop!("Style/FileEmpty", |_cfg| Some(Box::new(FileEmpty::new())));
