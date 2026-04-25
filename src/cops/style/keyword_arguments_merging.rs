//! Style/KeywordArgumentsMerging cop

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

const MSG: &str = "Provide additional arguments directly rather than using `merge`.";

#[derive(Default)]
pub struct KeywordArgumentsMerging;

impl KeywordArgumentsMerging {
    pub fn new() -> Self { Self }
}

impl Cop for KeywordArgumentsMerging {
    fn name(&self) -> &'static str { "Style/KeywordArgumentsMerging" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let tree = result.node();
        let mut v = V { ctx, out: vec![] };
        v.visit(&tree);
        v.out
    }
}

struct V<'a, 'b> {
    ctx: &'a CheckContext<'b>,
    out: Vec<Offense>,
}

fn format_arg(n: &ruby_prism::Node, src: &str) -> String {
    let loc = n.location();
    let s = loc.start_offset();
    let e = loc.end_offset();
    if n.as_hash_node().is_some() {
        // Strip outer `{` and `}`, preserve inner whitespace.
        return src[s + 1..e - 1].to_string();
    }
    if n.as_keyword_hash_node().is_some() {
        return src[s..e].to_string();
    }
    format!("**{}", &src[s..e])
}

impl<'a, 'b> Visit<'_> for V<'a, 'b> {
    fn visit_assoc_splat_node(&mut self, node: &ruby_prism::AssocSplatNode) {
        let Some(value) = node.value() else {
            ruby_prism::visit_assoc_splat_node(self, node);
            return;
        };

        let Some(call) = value.as_call_node() else {
            ruby_prism::visit_assoc_splat_node(self, node);
            return;
        };
        if node_name!(&call) != "merge" || call.arguments().is_none() {
            ruby_prism::visit_assoc_splat_node(self, node);
            return;
        }

        // Walk the merge chain outermost-first, collecting each merge's args text
        // (innermost args land at the end of the vec).
        let src = self.ctx.source;
        let mut chain_args: Vec<Vec<String>> = Vec::new();
        chain_args.push(
            call.arguments().unwrap().arguments().iter()
                .map(|a| format_arg(&a, src)).collect()
        );
        let mut current_recv = call.receiver();
        let base_loc;
        loop {
            let Some(recv) = current_recv else { return };
            if let Some(rc) = recv.as_call_node() {
                if node_name!(&rc) == "merge" {
                    if let Some(rargs) = rc.arguments() {
                        chain_args.push(
                            rargs.arguments().iter().map(|a| format_arg(&a, src)).collect()
                        );
                        current_recv = rc.receiver();
                        continue;
                    }
                }
            }
            base_loc = recv.location();
            break;
        }

        let mut out = String::new();
        out.push_str(&src[base_loc.start_offset()..base_loc.end_offset()]);
        // chain_args order: outermost first; emit innermost first.
        for args in chain_args.iter().rev() {
            for a in args {
                out.push_str(", ");
                out.push_str(a);
            }
        }

        let value_loc = value.location();
        let s = value_loc.start_offset();
        let e = value_loc.end_offset();
        let off = self
            .ctx
            .offense_with_range("Style/KeywordArgumentsMerging", MSG, Severity::Convention, s, e)
            .with_correction(Correction::replace(s, e, &out));
        self.out.push(off);

        ruby_prism::visit_assoc_splat_node(self, node);
    }
}

crate::register_cop!("Style/KeywordArgumentsMerging", |_cfg| Some(Box::new(KeywordArgumentsMerging::new())));
