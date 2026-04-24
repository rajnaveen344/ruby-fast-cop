//! Style/ExactRegexpMatch cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};

#[derive(Default)]
pub struct ExactRegexpMatch;
impl ExactRegexpMatch { pub fn new() -> Self { Self } }

fn regex_literal_text(src: &str) -> Option<&str> {
    // src = content inside `/.../` e.g. `\Astring\z`
    let s = src;
    if !s.starts_with("\\A") { return None; }
    if !s.ends_with("\\z") { return None; }
    let inner = &s[2..s.len()-2];
    // Check inner has no regex metacharacters or escapes.
    // Accept ASCII letters, digits, spaces. (Conservative: no `.*+?()[]{}|\\^$`)
    for c in inner.chars() {
        if matches!(c, '.'|'*'|'+'|'?'|'('|')'|'['|']'|'{'|'}'|'|'|'\\'|'^'|'$') {
            return None;
        }
    }
    Some(inner)
}

impl Cop for ExactRegexpMatch {
    fn name(&self) -> &'static str { "Style/ExactRegexpMatch" }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        if !matches!(method.as_ref(), "=~" | "===" | "!~" | "match" | "match?") {
            return vec![];
        }
        let receiver = match node.receiver() { Some(r) => r, None => return vec![] };

        // Regexp may be RHS (for =~/!~/===/match) as first argument.
        let args_node = match node.arguments() { Some(a) => a, None => return vec![] };
        let args: Vec<_> = args_node.arguments().iter().collect();
        if args.len() != 1 { return vec![]; }
        let re = match args[0].as_regular_expression_node() { Some(r) => r, None => return vec![] };

        // No flags
        if re.is_ignore_case() || re.is_extended() || re.is_multi_line() || re.is_once() {
            return vec![];
        }

        let cl = re.content_loc();
        let content = &ctx.source[cl.start_offset()..cl.end_offset()];
        let inner = match regex_literal_text(content) { Some(i) => i, None => return vec![] };

        let recv_loc = receiver.location();
        let recv_src = &ctx.source[recv_loc.start_offset()..recv_loc.end_offset()];
        let new_op = if method == "!~" { "!=" } else { "==" };
        let prefer = format!("{} {} '{}'", recv_src, new_op, inner);
        let msg = format!("Use `{}`.", prefer);

        let start = node.location().start_offset();
        let end = node.location().end_offset();
        vec![ctx.offense_with_range(self.name(), &msg, Severity::Convention, start, end)
            .with_correction(Correction::replace(start, end, prefer))]
    }
}

crate::register_cop!("Style/ExactRegexpMatch", |_cfg| Some(Box::new(ExactRegexpMatch::new())));
