use crate::cops::{CheckContext, Cop};
use crate::helpers::hash_subset::{build_key_source, extract_hash_subset, is_except_like};
use crate::offense::{Correction, Offense, Severity};

#[derive(Default)]
pub struct HashSlice {
    active_support: bool,
}

impl HashSlice {
    pub fn new() -> Self { Self { active_support: false } }
    pub fn with_config(active_support: bool) -> Self { Self { active_support } }
}

fn block_two_args_kv<'a>(
    block: &ruby_prism::BlockNode<'a>,
) -> Option<(String, String)> {
    let params = block.parameters()?;
    let bp = match &params {
        ruby_prism::Node::BlockParametersNode { .. } => params.as_block_parameters_node().unwrap(),
        _ => return None,
    };
    let inner = bp.parameters()?;
    let req: Vec<_> = inner.requireds().iter().collect();
    if req.len() != 2 { return None; }
    if inner.optionals().iter().next().is_some() || inner.rest().is_some()
        || inner.keywords().iter().next().is_some()
    {
        return None;
    }
    let key = match &req[0] {
        ruby_prism::Node::RequiredParameterNode { .. } => {
            std::str::from_utf8(req[0].as_required_parameter_node().unwrap().name().as_slice())
                .ok()?.to_string()
        }
        _ => return None,
    };
    let value = match &req[1] {
        ruby_prism::Node::RequiredParameterNode { .. } => {
            std::str::from_utf8(req[1].as_required_parameter_node().unwrap().name().as_slice())
                .ok()?.to_string()
        }
        _ => return None,
    };
    Some((key, value))
}

pub(crate) fn try_check_hash_subset(
    cop_name: &'static str,
    severity: Severity,
    node: &ruby_prism::CallNode,
    ctx: &CheckContext,
    active_support: bool,
    want_except: bool,
    preferred: &str,
    min_ruby: (u32, u32),
) -> Vec<Offense> {
    if !ctx.ruby_version_at_least(min_ruby.0, min_ruby.1) { return vec![]; }

    let method = String::from_utf8_lossy(node.name().as_slice()).into_owned();
    if !matches!(method.as_str(), "select" | "filter" | "reject") {
        return vec![];
    }

    let block = match node.block() {
        Some(b) => b,
        None => return vec![],
    };
    let block_node = match &block {
        ruby_prism::Node::BlockNode { .. } => block.as_block_node().unwrap(),
        _ => return vec![],
    };

    let (key_arg, value_arg) = match block_two_args_kv(&block_node) {
        Some(p) => p,
        None => return vec![],
    };

    let body = match block_node.body() {
        Some(b) => b,
        None => return vec![],
    };
    // body is StatementsNode; need single stmt
    let stmts = match body.as_statements_node() { Some(s) => s, None => return vec![] };
    let stmts_v: Vec<_> = stmts.body().iter().collect();
    if stmts_v.len() != 1 { return vec![]; }

    let es = match extract_hash_subset(&stmts_v[0], &key_arg, &value_arg, ctx.source, active_support) {
        Some(e) => e,
        None => return vec![],
    };

    let except_like = is_except_like(&method, &es);
    if want_except != except_like { return vec![]; }

    let key_src = build_key_source(&es, ctx.source);
    let preferred_call = format!("{}({})", preferred, key_src);
    let message = format!("Use `{}` instead.", preferred_call);

    // Offense range: selector start -> block end
    let selector_loc = node.message_loc().expect("call has selector");
    let start = selector_loc.start_offset();
    let end = block_node.location().end_offset();

    let mut offense = ctx.offense_with_range(cop_name, &message, severity, start, end);
    offense = offense.with_correction(Correction::replace(start, end, preferred_call));
    vec![offense]
}

impl Cop for HashSlice {
    fn name(&self) -> &'static str { "Style/HashSlice" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        try_check_hash_subset(
            "Style/HashSlice", self.severity(), node, ctx,
            self.active_support, false, "slice", (2, 5),
        )
    }
}

crate::register_cop!("Style/HashSlice", |cfg| {
    let cop_config = cfg.get_cop_config("Style/HashSlice");
    let active_support = cop_config
        .and_then(|c| c.raw.get("ActiveSupportExtensionsEnabled"))
        .and_then(|v| v.as_bool())
        .or_else(|| cop_config
            .and_then(|c| c.raw.get("AllCopsActiveSupportExtensionsEnabled"))
            .and_then(|v| v.as_bool()))
        .unwrap_or(false);
    Some(Box::new(HashSlice::with_config(active_support)))
});
