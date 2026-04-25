//! Lint/UnexpectedBlockArity — flag blocks with fewer positional args than the method needs.
//! Ports `RuboCop::Cop::Lint::UnexpectedBlockArity`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use std::collections::HashMap;

pub struct UnexpectedBlockArity {
    methods: HashMap<String, i64>,
}

impl UnexpectedBlockArity {
    pub fn new(methods: HashMap<String, i64>) -> Self { Self { methods } }
}

const MSG_TEMPLATE: &str = "`{m}` expects at least {e} positional arguments, got {a}.";

impl Cop for UnexpectedBlockArity {
    fn name(&self) -> &'static str { "Lint/UnexpectedBlockArity" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Block must be present and a real BlockNode (not a BlockArgumentNode `&proc`).
        let blk_node = match node.block() {
            Some(b) => b,
            None => return vec![],
        };
        let method = String::from_utf8_lossy(node.name().as_slice()).to_string();
        let expected = match self.methods.get(&method) {
            Some(e) => *e,
            None => return vec![],
        };
        // Only with receiver (acceptable? = !(included && receiver)).
        if node.receiver().is_none() { return vec![]; }

        // Compute arg count.
        let arg_cnt: ArgCount = if let Some(bn) = blk_node.as_block_node() {
            count_block_args(&bn)
        } else {
            return vec![];
        };

        let actual = match arg_cnt {
            ArgCount::Infinite => return vec![],
            ArgCount::N(n) => n,
        };
        if actual >= expected { return vec![]; }

        // Offense range: the entire call expression.
        let l = node.location();
        let msg = MSG_TEMPLATE
            .replace("{m}", &method)
            .replace("{e}", &expected.to_string())
            .replace("{a}", &actual.to_string());
        vec![ctx.offense_with_range(
            "Lint/UnexpectedBlockArity",
            &msg,
            Severity::Warning,
            l.start_offset(),
            l.end_offset(),
        )]
    }
}

enum ArgCount { N(i64), Infinite }

fn count_block_args(block: &ruby_prism::BlockNode) -> ArgCount {
    let params = match block.parameters() {
        Some(p) => p,
        None => return ArgCount::N(0),
    };

    // BlockParametersNode → inner ParametersNode
    if let Some(bp) = params.as_block_parameters_node() {
        let inner = match bp.parameters() {
            Some(i) => i,
            None => return ArgCount::N(0),
        };
        // rest → infinite
        if inner.rest().is_some() { return ArgCount::Infinite; }
        let req: i64 = inner.requireds().iter().count() as i64;
        let opt: i64 = inner.optionals().iter().count() as i64;
        let posts: i64 = inner.posts().iter().count() as i64;
        // mlhs (destructuring) appears in `requireds()` as MultiTargetNode in Prism — already counted.
        return ArgCount::N(req + opt + posts);
    }
    // NumberedParametersNode → maximum
    if let Some(np) = params.as_numbered_parameters_node() {
        return ArgCount::N(np.maximum() as i64);
    }
    // ItParametersNode → 1
    if params.as_it_parameters_node().is_some() {
        return ArgCount::N(1);
    }
    ArgCount::N(0)
}

crate::register_cop!("Lint/UnexpectedBlockArity", |cfg| {
    let mut methods: HashMap<String, i64> = HashMap::new();
    if let Some(c) = cfg.get_cop_config("Lint/UnexpectedBlockArity") {
        if let Some(serde_yaml::Value::Mapping(m)) = c.raw.get("Methods") {
            for (k, v) in m {
                if let (Some(name), Some(n)) = (k.as_str(), v.as_i64()) {
                    methods.insert(name.to_string(), n);
                }
            }
        }
    }
    Some(Box::new(UnexpectedBlockArity::new(methods)))
});
