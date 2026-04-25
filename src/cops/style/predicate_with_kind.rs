//! Style/PredicateWithKind — `array.any? { |x| x.is_a?(K) }` → `array.any?(K)`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/predicate_with_kind.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use crate::node_name;
use ruby_prism::Node;

const COP_NAME: &str = "Style/PredicateWithKind";

const PREDICATES: &[&str] = &["any?", "all?", "none?", "one?"];
const KIND_METHODS: &[&str] = &["is_a?", "kind_of?", "instance_of?"];

#[derive(Default)]
pub struct PredicateWithKind;

impl PredicateWithKind {
    pub fn new() -> Self { Self }
}

impl Cop for PredicateWithKind {
    fn name(&self) -> &'static str { COP_NAME }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method = node_name!(node);
        let m = method.as_ref();
        if !PREDICATES.contains(&m) {
            return vec![];
        }
        // Don't flag when there's already a positional argument.
        if let Some(args) = node.arguments() {
            if args.arguments().iter().count() > 0 {
                return vec![];
            }
        }
        let Some(block) = node.block() else { return vec![] };
        let Node::BlockNode { .. } = block else { return vec![] };
        let block_node = block.as_block_node().unwrap();

        // Block body must be a single expression: a CallNode `(send (lvar X) KIND_METHOD _)`.
        let Some(body) = block_node.body() else { return vec![] };
        let kind_call = match &body {
            Node::StatementsNode { .. } => {
                let s = body.as_statements_node().unwrap();
                let stmts: Vec<_> = s.body().iter().collect();
                if stmts.len() != 1 { return vec![]; }
                match &stmts[0] {
                    Node::CallNode { .. } => stmts[0].as_call_node().unwrap(),
                    _ => return vec![],
                }
            }
            Node::CallNode { .. } => body.as_call_node().unwrap(),
            _ => return vec![],
        };

        let kind_method = node_name!(kind_call);
        if !KIND_METHODS.contains(&kind_method.as_ref()) {
            return vec![];
        }
        // Receiver must be a local variable matching the block param.
        let Some(recv) = kind_call.receiver() else { return vec![] };
        match &recv {
            Node::LocalVariableReadNode { .. } | Node::ItLocalVariableReadNode { .. } => {}
            _ => return vec![],
        }
        // Argument: exactly one.
        let kind_args = kind_call.arguments();
        let kind_arg_list: Vec<_> = kind_args.as_ref().map(|a| a.arguments().iter().collect()).unwrap_or_default();
        if kind_arg_list.len() != 1 {
            return vec![];
        }
        let kind_arg = &kind_arg_list[0];

        // Determine the expected receiver name.
        let expected_name = match block_node.parameters() {
            Some(Node::BlockParametersNode { .. }) => {
                let bp = block_node.parameters().unwrap();
                let bp = bp.as_block_parameters_node().unwrap();
                let Some(pn) = bp.parameters() else { return vec![] };
                let req: Vec<_> = pn.requireds().iter().collect();
                if req.len() != 1 { return vec![]; }
                if pn.optionals().iter().count() > 0
                    || pn.rest().is_some()
                    || pn.keywords().iter().count() > 0
                    || pn.block().is_some()
                { return vec![]; }
                let Node::RequiredParameterNode { .. } = &req[0] else { return vec![] };
                let rpn = req[0].as_required_parameter_node().unwrap();
                String::from_utf8_lossy(rpn.name().as_slice()).to_string()
            }
            Some(Node::NumberedParametersNode { .. }) => "_1".to_string(),
            Some(Node::ItParametersNode { .. }) => "it".to_string(),
            _ => return vec![],
        };

        // Match receiver identifier.
        let recv_name = match &recv {
            Node::LocalVariableReadNode { .. } => {
                let lv = recv.as_local_variable_read_node().unwrap();
                String::from_utf8_lossy(lv.name().as_slice()).to_string()
            }
            Node::ItLocalVariableReadNode { .. } => "it".to_string(),
            _ => return vec![],
        };
        if recv_name != expected_name {
            return vec![];
        }

        // Build offense — RuboCop registers on the BLOCK node (which in their AST wraps the call;
        // its source range covers the entire `array.any? { ... }` expression).
        let arg_src = ctx.src(kind_arg.location().start_offset(), kind_arg.location().end_offset()).to_string();
        let replacement = format!("{}({})", m, arg_src);
        let original = format!("{} {{ ... }}", m);
        let message = format!("Prefer `{}` to `{}` with a kind check.", replacement, original);

        // Range = whole call+block (RuboCop attaches to block_node).
        let off_start = node.location().start_offset();
        let off_end = node.location().end_offset();

        // Correction range: from selector (method name) to end of block.
        let sel_loc = node.message_loc();
        let block_end = block_node.location().end_offset();
        let edit_start = match sel_loc {
            Some(loc) => loc.start_offset(),
            None => off_start,
        };

        let offense = ctx
            .offense_with_range(COP_NAME, &message, Severity::Convention, off_start, off_end)
            .with_correction(Correction { edits: vec![Edit {
                start_offset: edit_start,
                end_offset: block_end,
                replacement,
            }]});

        vec![offense]
    }
}

crate::register_cop!("Style/PredicateWithKind", |_cfg| Some(Box::new(PredicateWithKind::new())));
