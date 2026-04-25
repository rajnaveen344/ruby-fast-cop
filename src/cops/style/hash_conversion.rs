//! Style/HashConversion
//!
//! Replace `Hash[ary]` with `ary.to_h`; multi-arg `Hash[k1, v1, ...]` with `{k1 => v1, ...}`.
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/hash_conversion.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG_TO_H: &str = "Prefer `ary.to_h` to `Hash[ary]`.";
const MSG_LITERAL_MULTI_ARG: &str = "Prefer literal hash to `Hash[arg1, arg2, ...]`.";
const MSG_LITERAL_HASH_ARG: &str = "Prefer literal hash to `Hash[key: value, ...]`.";
const MSG_SPLAT: &str = "Prefer `array_of_pairs.to_h` to `Hash[*array]`.";

#[derive(Default)]
pub struct HashConversion {
    allow_splat: bool,
}

impl HashConversion {
    pub fn new() -> Self { Self { allow_splat: true } }
    pub fn with_config(allow_splat: bool) -> Self { Self { allow_splat } }
}

impl Cop for HashConversion {
    fn name(&self) -> &'static str { "Style/HashConversion" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = HashVisitor {
            cop: self,
            ctx,
            ignored_offsets: Vec::new(),
            offenses: Vec::new(),
            // Stack of CallNode (start, end) byte offsets for ancestor parent (most recent first).
            parent_stack: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct HashVisitor<'a, 'b> {
    cop: &'b HashConversion,
    ctx: &'a CheckContext<'a>,
    ignored_offsets: Vec<(usize, usize)>,
    offenses: Vec<Offense>,
    parent_stack: Vec<Node<'a>>,
}

impl<'a, 'b> Visit<'a> for HashVisitor<'a, 'b> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'a>) {
        // If this Hash[] call is inside an already-ignored Hash[] call, skip.
        let loc = node.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());
        let ignored = self.ignored_offsets.iter().any(|&(s, e)| s <= start && end <= e && (s, e) != (start, end));

        if !ignored && Self::is_hash_subscript(node) {
            self.check(node);
            self.ignored_offsets.push((start, end));
        }

        // Track parent stack
        let n = node.as_node();
        self.parent_stack.push(n);
        ruby_prism::visit_call_node(self, node);
        self.parent_stack.pop();
    }
}

impl<'a, 'b> HashVisitor<'a, 'b> {
    fn is_hash_subscript(node: &ruby_prism::CallNode<'a>) -> bool {
        let mname = String::from_utf8_lossy(node.name().as_slice());
        if mname != "[]" { return false; }
        let recv = match node.receiver() { Some(r) => r, None => return false };
        if let Some(c) = recv.as_constant_read_node() {
            return String::from_utf8_lossy(c.name().as_slice()) == "Hash";
        }
        false
    }

    fn args(node: &ruby_prism::CallNode<'a>) -> Vec<Node<'a>> {
        let mut v = Vec::new();
        if let Some(args) = node.arguments() {
            for a in args.arguments().iter() { v.push(a); }
        }
        v
    }

    fn check(&mut self, node: &ruby_prism::CallNode<'a>) {
        let args = Self::args(node);
        if args.len() == 1 {
            self.single_argument(node, &args[0]);
        } else {
            self.multi_argument(node, &args);
        }
    }

    fn loc_range(node: &ruby_prism::CallNode) -> (usize, usize) {
        let loc = node.location();
        (loc.start_offset(), loc.end_offset())
    }

    fn single_argument(&mut self, node: &ruby_prism::CallNode<'a>, arg: &Node<'a>) {
        let (start, end) = Self::loc_range(node);
        let src = self.ctx.source;
        match arg {
            Node::HashNode { .. } | Node::KeywordHashNode { .. } => {
                let arg_src = node_src(arg, src);
                let mut offense = self.ctx.offense_with_range(
                    "Style/HashConversion", MSG_LITERAL_HASH_ARG, Severity::Convention, start, end);
                let mut edits = vec![Edit { start_offset: start, end_offset: end, replacement: format!("{{{}}}", arg_src) }];
                // Add parens around parent send if needed
                if let Some(parent) = self.parent_send_for_paren_addition(start, end) {
                    edits.extend(parent);
                }
                offense.correction = Some(Correction { edits });
                self.offenses.push(offense);
            }
            Node::SplatNode { .. } => {
                if !self.cop.allow_splat {
                    let offense = self.ctx.offense_with_range(
                        "Style/HashConversion", MSG_SPLAT, Severity::Convention, start, end);
                    self.offenses.push(offense);
                }
            }
            _ => {
                // Check zip-no-arg pattern
                if let Some(replacement) = self.zip_no_arg_replacement(arg) {
                    let mut offense = self.ctx.offense_with_range(
                        "Style/HashConversion", MSG_TO_H, Severity::Convention, start, end);
                    offense.correction = Some(Correction::replace(start, end, replacement));
                    self.offenses.push(offense);
                    return;
                }
                let arg_src = node_src(arg, src);
                let replacement = if requires_parens(arg) {
                    format!("({}).to_h", arg_src)
                } else {
                    format!("{}.to_h", arg_src)
                };
                let mut offense = self.ctx.offense_with_range(
                    "Style/HashConversion", MSG_TO_H, Severity::Convention, start, end);
                offense.correction = Some(Correction::replace(start, end, replacement));
                self.offenses.push(offense);
            }
        }
    }

    fn zip_no_arg_replacement(&self, arg: &Node<'a>) -> Option<String> {
        let call = arg.as_call_node()?;
        let m = String::from_utf8_lossy(call.name().as_slice());
        if m != "zip" { return None; }
        let arg_count = call.arguments().map_or(0, |a| a.arguments().iter().count());
        if arg_count != 0 { return None; }
        // Replace zip → zip([])
        let src = self.ctx.source;
        let arg_src = node_src(arg, src);
        // Determine if zip has parens. opening_loc on CallNode is for `(`.
        if call.opening_loc().is_some() {
            // `array.zip()` → `array.zip([]).to_h` (insert before `)`)
            // arg_src ends in `)`. Insert `[]` before final `)`.
            let trimmed = &arg_src[..arg_src.len() - 1];
            Some(format!("{}[]).to_h", trimmed))
        } else {
            // `array.zip` → `array.zip([]).to_h`
            Some(format!("{}([]).to_h", arg_src))
        }
    }

    fn multi_argument(&mut self, node: &ruby_prism::CallNode<'a>, args: &[Node<'a>]) {
        let (start, end) = Self::loc_range(node);
        let src = self.ctx.source;
        if args.len() % 2 != 0 {
            // odd → no autocorrect
            let offense = self.ctx.offense_with_range(
                "Style/HashConversion", MSG_LITERAL_MULTI_ARG, Severity::Convention, start, end);
            self.offenses.push(offense);
            return;
        }
        let content: Vec<String> = args.chunks(2).map(|pair| {
            format!("{} => {}", node_src(&pair[0], src), node_src(&pair[1], src))
        }).collect();
        let replacement = format!("{{{}}}", content.join(", "));
        let mut offense = self.ctx.offense_with_range(
            "Style/HashConversion", MSG_LITERAL_MULTI_ARG, Severity::Convention, start, end);
        let mut edits = vec![Edit { start_offset: start, end_offset: end, replacement }];
        if let Some(extra) = self.parent_send_for_paren_addition_multi(start, end) {
            edits.extend(extra);
        }
        offense.correction = Some(Correction { edits });
        self.offenses.push(offense);
    }

    /// If the immediate parent is a CallNode without parens AND its argument is this Hash[...],
    /// produce edits to wrap the arg list in parens.
    fn parent_send_for_paren_addition(&self, start: usize, end: usize) -> Option<Vec<Edit>> {
        self.compute_parent_paren_edits(start, end, /*hash_arg=*/ true)
    }

    fn parent_send_for_paren_addition_multi(&self, start: usize, end: usize) -> Option<Vec<Edit>> {
        self.compute_parent_paren_edits(start, end, /*hash_arg=*/ false)
    }

    fn compute_parent_paren_edits(&self, _start: usize, _end: usize, hash_arg: bool) -> Option<Vec<Edit>> {
        // Walk back through parent_stack to find nearest CallNode that is a method call (has message)
        // and whose arguments contain our Hash[] call.
        for parent in self.parent_stack.iter().rev() {
            let p = parent.as_call_node()?;
            // Skip the Hash[] call itself (we pushed before recursing)
            // It's identifiable by message="[]"
            let mname = String::from_utf8_lossy(p.name().as_slice());
            if mname == "[]" {
                // continue searching
                continue;
            }
            // Is parent already parenthesized?
            if p.opening_loc().is_some() { return None; }
            // For multi-arg case, RuboCop also skips when parent.method?(:to_h)
            if !hash_arg && mname == "to_h" { return None; }
            // Add parens: insert `(` after message, `)` at end
            let msg_loc = p.message_loc()?;
            let after_msg = msg_loc.end_offset();
            // arguments span: from first arg start to last arg end
            let p_args = p.arguments()?;
            let first = p_args.arguments().iter().next()?;
            let mut last_end = first.location().end_offset();
            for a in p_args.arguments().iter() {
                last_end = a.location().end_offset();
            }
            let first_start = first.location().start_offset();
            // We want to wrap `do_something X` → `do_something(X)`.
            let edits = vec![
                Edit { start_offset: after_msg, end_offset: first_start, replacement: "(".to_string() },
                Edit { start_offset: last_end, end_offset: last_end, replacement: ")".to_string() },
            ];
            return Some(edits);
        }
        None
    }
}

fn node_src<'a>(node: &Node, src: &'a str) -> &'a str {
    let loc = node.location();
    &src[loc.start_offset()..loc.end_offset()]
}

fn requires_parens(node: &Node) -> bool {
    if let Some(call) = node.as_call_node() {
        let mname = String::from_utf8_lossy(call.name().as_slice());
        if mname == "[]" { return false; }
        let has_args = call.arguments().map_or(false, |a| a.arguments().iter().count() > 0);
        if has_args && call.opening_loc().is_none() {
            return true;
        }
    }
    matches!(node, Node::AndNode { .. } | Node::OrNode { .. })
}

crate::register_cop!("Style/HashConversion", |cfg| {
    let allow_splat = cfg.get_cop_config("Style/HashConversion")
        .and_then(|c| c.raw.get("AllowSplatArgument"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    Some(Box::new(HashConversion::with_config(allow_splat)))
});
