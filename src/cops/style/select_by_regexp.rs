use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};

#[derive(Default)]
pub struct SelectByRegexp;

#[derive(Debug, Clone, Copy, PartialEq)]
enum MethodKind { Select, Reject }

#[derive(Debug, Clone, Copy, PartialEq)]
enum MatchKind { Positive, Negative }

impl MatchKind {
    fn negate(self) -> Self {
        match self {
            MatchKind::Positive => MatchKind::Negative,
            MatchKind::Negative => MatchKind::Positive,
        }
    }
}

impl SelectByRegexp {
    pub fn new() -> Self { Self }

    fn method_kind(name: &str) -> Option<MethodKind> {
        match name {
            "select" | "find_all" | "filter" => Some(MethodKind::Select),
            "reject" => Some(MethodKind::Reject),
            _ => None,
        }
    }

    fn replacement_method(method_kind: MethodKind, match_kind: MatchKind) -> &'static str {
        match (method_kind, match_kind) {
            (MethodKind::Select, MatchKind::Positive) | (MethodKind::Reject, MatchKind::Negative) => "grep",
            (MethodKind::Select, MatchKind::Negative) | (MethodKind::Reject, MatchKind::Positive) => "grep_v",
        }
    }

    fn is_hash_receiver(receiver: &ruby_prism::Node, source: &str) -> bool {
        match receiver {
            ruby_prism::Node::HashNode { .. } => true,
            ruby_prism::Node::CallNode { .. } => {
                let call = receiver.as_call_node().unwrap();
                let method_name = node_name!(call);
                if let Some(recv) = call.receiver() {
                    if let Some(name) = Self::constant_name(&recv, source) {
                        if name == "Hash" && matches!(method_name.as_ref(), "new" | "[]") {
                            return true;
                        }
                    }
                }
                matches!(method_name.as_ref(), "to_h" | "to_hash")
            }
            ruby_prism::Node::ConstantReadNode { .. } => {
                node_name!(receiver.as_constant_read_node().unwrap()) == "ENV"
            }
            ruby_prism::Node::ConstantPathNode { .. } => {
                let path_node = receiver.as_constant_path_node().unwrap();
                path_node.parent().is_none()
                    && path_node.name().map_or(false, |n| String::from_utf8_lossy(n.as_slice()) == "ENV")
            }
            _ => false,
        }
    }

    fn constant_name(node: &ruby_prism::Node, _source: &str) -> Option<String> {
        match node {
            ruby_prism::Node::ConstantReadNode { .. } => {
                Some(node_name!(node.as_constant_read_node().unwrap()).into_owned())
            }
            ruby_prism::Node::ConstantPathNode { .. } => {
                let path_node = node.as_constant_path_node().unwrap();
                if path_node.parent().is_none() {
                    path_node.name().map(|n| String::from_utf8_lossy(n.as_slice()).into_owned())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn block_param_name(block: &ruby_prism::BlockNode) -> Option<String> {
        let params = block.parameters()?;
        match &params {
            ruby_prism::Node::BlockParametersNode { .. } => {
                let block_params = params.as_block_parameters_node().unwrap();
                let inner_params = block_params.parameters()?;
                let req_count = inner_params.requireds().iter().count();
                if req_count != 1 || inner_params.optionals().iter().next().is_some()
                    || inner_params.rest().is_some() || inner_params.keywords().iter().next().is_some()
                {
                    return None;
                }
                let requireds: Vec<_> = inner_params.requireds().iter().collect();
                if let ruby_prism::Node::RequiredParameterNode { .. } = &requireds[0] {
                    Some(node_name!(requireds[0].as_required_parameter_node().unwrap()).into_owned())
                } else {
                    None
                }
            }
            ruby_prism::Node::NumberedParametersNode { .. } => {
                if params.as_numbered_parameters_node().unwrap().maximum() == 1 {
                    Some("_1".to_string())
                } else {
                    None
                }
            }
            ruby_prism::Node::ItParametersNode { .. } => Some("it".to_string()),
            _ => None,
        }
    }

    fn analyze_block_body(
        block: &ruby_prism::BlockNode, param_name: &str, source: &str,
    ) -> Option<(MatchKind, String)> {
        let body = block.body()?;
        let stmts_node = body.as_statements_node()?;
        let stmts: Vec<_> = stmts_node.body().iter().collect();
        if stmts.len() != 1 { return None; }
        Self::analyze_stmt(&stmts[0], param_name, source)
    }

    fn analyze_stmt(
        stmt: &ruby_prism::Node, param_name: &str, source: &str,
    ) -> Option<(MatchKind, String)> {
        if let ruby_prism::Node::CallNode { .. } = stmt {
            let call = stmt.as_call_node().unwrap();
            if node_name!(call) == "!" {
                if let Some(inner) = call.receiver() {
                    if let Some((k, r)) = Self::analyze_match_expr(&inner, param_name, source) {
                        return Some((k.negate(), r));
                    }
                    if let ruby_prism::Node::ParenthesesNode { .. } = &inner {
                        let paren = inner.as_parentheses_node().unwrap();
                        if let Some(paren_body) = paren.body() {
                            if let Some(inner_stmts) = paren_body.as_statements_node() {
                                let inner_body: Vec<_> = inner_stmts.body().iter().collect();
                                if inner_body.len() == 1 {
                                    if let Some((k, r)) = Self::analyze_match_expr(&inner_body[0], param_name, source) {
                                        return Some((k.negate(), r));
                                    }
                                }
                            }
                        }
                    }
                }
                return None;
            }
        }
        Self::analyze_match_expr(stmt, param_name, source)
    }

    fn analyze_match_expr(
        node: &ruby_prism::Node, param_name: &str, source: &str,
    ) -> Option<(MatchKind, String)> {
        match node {
            ruby_prism::Node::CallNode { .. } => {
                Self::analyze_call(&node.as_call_node().unwrap(), param_name, source)
            }
            ruby_prism::Node::MatchWriteNode { .. } => {
                Self::analyze_call(&node.as_match_write_node().unwrap().call(), param_name, source)
            }
            _ => None,
        }
    }

    fn analyze_call(
        call: &ruby_prism::CallNode, param_name: &str, source: &str,
    ) -> Option<(MatchKind, String)> {
        let method = node_name!(call);
        let receiver = call.receiver()?;
        let args: Vec<_> = call.arguments().map_or(vec![], |a| a.arguments().iter().collect());
        if args.len() != 1 { return None; }

        match method.as_ref() {
            "match?" => {
                if Self::is_param_ref(&receiver, param_name) {
                    Some((MatchKind::Positive, Self::node_source(&args[0], source)?))
                } else if Self::is_param_ref(&args[0], param_name) {
                    Some((MatchKind::Positive, Self::node_source(&receiver, source)?))
                } else {
                    None
                }
            }
            "=~" | "!~" => {
                let kind = if method == "=~" { MatchKind::Positive } else { MatchKind::Negative };
                if Self::is_param_ref(&receiver, param_name) {
                    Some((kind, Self::node_source(&args[0], source)?))
                } else if Self::is_param_ref(&args[0], param_name) {
                    Some((kind, Self::node_source(&receiver, source)?))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn is_param_ref(node: &ruby_prism::Node, param_name: &str) -> bool {
        match node {
            ruby_prism::Node::LocalVariableReadNode { .. } => {
                node_name!(node.as_local_variable_read_node().unwrap()) == param_name
            }
            ruby_prism::Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                node_name!(call) == param_name
                    && call.receiver().is_none()
                    && call.arguments().is_none()
            }
            ruby_prism::Node::ItLocalVariableReadNode { .. } => param_name == "it",
            _ => false,
        }
    }

    fn node_source(node: &ruby_prism::Node, source: &str) -> Option<String> {
        let loc = node.location();
        source.get(loc.start_offset()..loc.end_offset()).map(|s| s.to_string())
    }

    fn block_offense_end(block: &ruby_prism::BlockNode, source: &str) -> usize {
        let open_loc = block.opening_loc();
        let is_do = source.get(open_loc.start_offset()..open_loc.end_offset()) == Some("do");
        if is_do {
            block.parameters().map_or(open_loc.end_offset(), |p| p.location().end_offset())
        } else {
            block.location().end_offset()
        }
    }

    fn uses_safe_navigation(call_node: &ruby_prism::CallNode, source: &str) -> bool {
        call_node.call_operator_loc().map_or(false, |op_loc| {
            source.get(op_loc.start_offset()..op_loc.end_offset()) == Some("&.")
        })
    }
}

impl Cop for SelectByRegexp {
    fn name(&self) -> &'static str { "Style/SelectByRegexp" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method_name = node_name!(node);
        let method_kind = match Self::method_kind(&method_name) {
            Some(kind) => kind,
            None => return vec![],
        };

        if method_name == "filter" && !ctx.ruby_version_at_least(2, 6) {
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

        if node.receiver().map_or(false, |r| Self::is_hash_receiver(&r, ctx.source)) {
            return vec![];
        }

        let param_name = match Self::block_param_name(&block_node) {
            Some(name) => name,
            None => return vec![],
        };

        let (match_kind, regexp_src) = match Self::analyze_block_body(&block_node, &param_name, ctx.source) {
            Some(result) => result,
            None => return vec![],
        };

        let replacement = Self::replacement_method(method_kind, match_kind);
        if replacement == "grep_v" && !ctx.ruby_version_at_least(2, 3) {
            return vec![];
        }

        let message = format!("Prefer `{}` to `{}` with a regexp match.", replacement, method_name);

        let start_offset = node.receiver().map_or(node.location().start_offset(), |r| r.location().start_offset());
        let offense_end_offset = Self::block_offense_end(&block_node, ctx.source);
        let correction_end_offset = block_node.location().end_offset();

        let mut offense = ctx.offense_with_range(self.name(), &message, self.severity(), start_offset, offense_end_offset);

        let safe_nav = Self::uses_safe_navigation(node, ctx.source);
        let receiver_src = node.receiver().map(|r| {
            ctx.source.get(r.location().start_offset()..r.location().end_offset()).unwrap_or("").to_string()
        });
        let nav_op = if safe_nav { "&." } else { "." };
        let corrected = match receiver_src {
            Some(recv_src) => format!("{}{}{}({})", recv_src, nav_op, replacement, regexp_src),
            None => format!("{}({})", replacement, regexp_src),
        };

        offense = offense.with_correction(Correction::replace(start_offset, correction_end_offset, corrected));
        vec![offense]
    }
}
