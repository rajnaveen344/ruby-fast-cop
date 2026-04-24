//! Style/CollectionQuerying - Prefer `any?`/`none?`/`one?`/`many?` over `count`-based predicates.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/collection_querying.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::CallNode;

pub struct CollectionQuerying {
    active_support_extensions_enabled: bool,
}

impl Default for CollectionQuerying {
    fn default() -> Self { Self { active_support_extensions_enabled: false } }
}

impl CollectionQuerying {
    pub fn new() -> Self { Self::default() }
    pub fn with_config(active_support_extensions_enabled: bool) -> Self {
        Self { active_support_extensions_enabled }
    }
}

fn replacement_for(method: &str, arg_int: Option<i64>) -> Option<&'static str> {
    match (method, arg_int) {
        ("positive?", None) => Some("any?"),
        (">", Some(0)) => Some("any?"),
        ("!=", Some(0)) => Some("any?"),
        ("zero?", None) => Some("none?"),
        ("==", Some(0)) => Some("none?"),
        ("==", Some(1)) => Some("one?"),
        (">", Some(1)) => Some("many?"),
        _ => None,
    }
}

fn find_count_call<'a>(predicate: &CallNode<'a>) -> Option<CallNode<'a>> {
    let recv = predicate.receiver()?;
    let c = recv.as_call_node()?;
    if node_name!(c) == "count" && c.receiver().is_some() { Some(c) } else { None }
}

fn count_has_valid_args(count_call: &CallNode) -> bool {
    // No positional args allowed. Block-pass is stored as CallNode.block() (BlockArgumentNode);
    // block literal is also in .block() as BlockNode. Both are fine.
    if let Some(args) = count_call.arguments() {
        let list: Vec<_> = args.arguments().iter().collect();
        return list.is_empty();
    }
    true
}

impl Cop for CollectionQuerying {
    fn name(&self) -> &'static str { "Style/CollectionQuerying" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        let method_s: &str = &method;
        if !matches!(method_s, "positive?" | ">" | "!=" | "zero?" | "==") { return vec![]; }

        let arg_int: Option<i64> = if matches!(method_s, ">" | "!=" | "==") {
            let args = match node.arguments() { Some(a) => a, None => return vec![] };
            let list: Vec<_> = args.arguments().iter().collect();
            if list.len() != 1 { return vec![]; }
            let int = match list[0].as_integer_node() { Some(i) => i, None => return vec![] };
            let src = ctx.source.get(int.location().start_offset()..int.location().end_offset()).unwrap_or("");
            match src.parse::<i64>().ok() { Some(v) => Some(v), None => return vec![] }
        } else {
            None
        };
        let replacement = match replacement_for(method_s, arg_int) {
            Some(r) => r,
            None => return vec![],
        };

        if replacement == "many?" && !self.active_support_extensions_enabled {
            return vec![];
        }

        let count_call = match find_count_call(node) {
            Some(c) => c,
            None => return vec![],
        };
        if !count_has_valid_args(&count_call) { return vec![]; }

        // predicate must be a bare send (not safe nav) per RuboCop pattern `(send ...)`.
        if let Some(op) = node.call_operator_loc() {
            if ctx.source.get(op.start_offset()..op.end_offset()) == Some("&.") {
                return vec![];
            }
        }

        let count_sel = count_call.message_loc().expect("count selector");
        let off_start = count_sel.start_offset();
        let off_end = node.location().end_offset();
        let message = format!("Use `{}` instead.", replacement);
        let mut off = ctx.offense_with_range(self.name(), &message, self.severity(), off_start, off_end);

        let sel_start = count_sel.start_offset();
        let sel_end = count_sel.end_offset();
        let pred_sel = node.message_loc().expect("predicate selector");
        let mut remove_start = node.call_operator_loc().map_or(pred_sel.start_offset(), |d| d.start_offset());
        // Extend left over surrounding whitespace (spaces/tabs/newline) — matches RuboCop's
        // `range_with_surrounding_space(..., side: :left)`.
        let bytes = ctx.source.as_bytes();
        while remove_start > sel_end {
            let b = bytes[remove_start - 1];
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                remove_start -= 1;
            } else { break; }
        }
        let remove_end = node.location().end_offset();

        let edit1 = crate::offense::Edit {
            start_offset: sel_start, end_offset: sel_end, replacement: replacement.to_string(),
        };
        let edit2 = crate::offense::Edit {
            start_offset: remove_start, end_offset: remove_end, replacement: String::new(),
        };
        off = off.with_correction(Correction { edits: vec![edit1, edit2] });
        vec![off]
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    #[serde(rename = "AllCopsActiveSupportExtensionsEnabled")]
    all_cops_active_support_extensions_enabled: bool,
}

crate::register_cop!("Style/CollectionQuerying", |cfg| {
    let c: Cfg = cfg.typed("Style/CollectionQuerying");
    Some(Box::new(CollectionQuerying::with_config(c.all_cops_active_support_extensions_enabled)))
});
