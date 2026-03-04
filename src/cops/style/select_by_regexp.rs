//! Style/SelectByRegexp - Prefer `grep`/`grep_v` to `select`/`reject` with regexp match.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/select_by_regexp.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};

/// Checks for `select`/`filter`/`find_all`/`reject` calls with blocks that
/// perform regexp matching, and suggests using `grep` or `grep_v` instead.
///
/// # Examples
///
/// ```ruby
/// # bad
/// array.select { |x| x.match? /regexp/ }
/// array.reject { |x| x =~ /regexp/ }
///
/// # good
/// array.grep(/regexp/)
/// array.grep_v(/regexp/)
/// ```
pub struct SelectByRegexp;

/// Which method was called: select-like or reject
#[derive(Debug, Clone, Copy, PartialEq)]
enum MethodKind {
    /// select, filter, find_all
    Select,
    /// reject
    Reject,
}

/// What type of regexp match was found in the block body
#[derive(Debug, Clone, Copy, PartialEq)]
enum MatchKind {
    /// Positive match: match?, =~
    Positive,
    /// Negative match: !~, or negated positive match (!)
    Negative,
}

impl SelectByRegexp {
    pub fn new() -> Self {
        Self
    }

    /// Check if a method name is one of the select-like methods or reject
    fn method_kind(name: &str) -> Option<MethodKind> {
        match name {
            "select" | "find_all" => Some(MethodKind::Select),
            "filter" => Some(MethodKind::Select),
            "reject" => Some(MethodKind::Reject),
            _ => None,
        }
    }

    /// Determine the replacement method based on the original method and match type
    fn replacement_method(method_kind: MethodKind, match_kind: MatchKind) -> &'static str {
        match (method_kind, match_kind) {
            (MethodKind::Select, MatchKind::Positive) => "grep",
            (MethodKind::Select, MatchKind::Negative) => "grep_v",
            (MethodKind::Reject, MatchKind::Positive) => "grep_v",
            (MethodKind::Reject, MatchKind::Negative) => "grep",
        }
    }

    /// Check if the receiver of the select/reject call is hash-like.
    fn is_hash_receiver(receiver: &ruby_prism::Node, source: &str) -> bool {
        match receiver {
            // Hash literal: {}.select, { foo: :bar }.select
            ruby_prism::Node::HashNode { .. } => true,

            ruby_prism::Node::CallNode { .. } => {
                let call = receiver.as_call_node().unwrap();
                let method_name = String::from_utf8_lossy(call.name().as_slice());

                // Check for Hash.new, Hash.new(:default), Hash.new { ... }
                // Also Hash[...] (which is also a CallNode with name "[]")
                if let Some(recv) = call.receiver() {
                    if let Some(name) = Self::constant_name(&recv, source) {
                        if name == "Hash"
                            && (method_name == "new" || method_name == "[]")
                        {
                            return true;
                        }
                    }
                }

                // Check for to_h, to_hash as the immediate receiver
                if method_name == "to_h" || method_name == "to_hash" {
                    return true;
                }

                false
            }

            // ENV or ::ENV
            ruby_prism::Node::ConstantReadNode { .. } => {
                let const_node = receiver.as_constant_read_node().unwrap();
                let name = String::from_utf8_lossy(const_node.name().as_slice());
                name == "ENV"
            }

            ruby_prism::Node::ConstantPathNode { .. } => {
                let path_node = receiver.as_constant_path_node().unwrap();
                if path_node.parent().is_none() {
                    if let Some(name_token) = path_node.name() {
                        let name = String::from_utf8_lossy(name_token.as_slice());
                        return name == "ENV";
                    }
                }
                false
            }

            _ => false,
        }
    }

    /// Get the constant name from a node
    fn constant_name(node: &ruby_prism::Node, _source: &str) -> Option<String> {
        match node {
            ruby_prism::Node::ConstantReadNode { .. } => {
                let const_node = node.as_constant_read_node().unwrap();
                Some(String::from_utf8_lossy(const_node.name().as_slice()).into_owned())
            }
            ruby_prism::Node::ConstantPathNode { .. } => {
                let path_node = node.as_constant_path_node().unwrap();
                if path_node.parent().is_none() {
                    path_node
                        .name()
                        .map(|n| String::from_utf8_lossy(n.as_slice()).into_owned())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Get the block parameter name.
    /// Returns None if the block has no parameters, multiple params, or is otherwise invalid.
    fn block_param_name(block: &ruby_prism::BlockNode) -> Option<String> {
        if let Some(params) = block.parameters() {
            match &params {
                ruby_prism::Node::BlockParametersNode { .. } => {
                    let block_params = params.as_block_parameters_node().unwrap();
                    if let Some(inner_params) = block_params.parameters() {
                        let req_count = inner_params.requireds().iter().count();
                        let opt_count = inner_params.optionals().iter().count();
                        let rest = inner_params.rest();
                        let kw_count = inner_params.keywords().iter().count();
                        if req_count == 1 && opt_count == 0 && rest.is_none() && kw_count == 0 {
                            let requireds: Vec<_> = inner_params.requireds().iter().collect();
                            if let ruby_prism::Node::RequiredParameterNode { .. } = &requireds[0] {
                                let param = requireds[0].as_required_parameter_node().unwrap();
                                return Some(
                                    String::from_utf8_lossy(param.name().as_slice()).into_owned(),
                                );
                            }
                        }
                    }
                    None
                }
                ruby_prism::Node::NumberedParametersNode { .. } => {
                    let num_params = params.as_numbered_parameters_node().unwrap();
                    if num_params.maximum() == 1 {
                        Some("_1".to_string())
                    } else {
                        None
                    }
                }
                ruby_prism::Node::ItParametersNode { .. } => Some("it".to_string()),
                _ => None,
            }
        } else {
            None
        }
    }

    /// Analyze the block body. Returns (MatchKind, regexp_source_text) if it
    /// contains a single regexp-matching expression involving the block parameter.
    fn analyze_block_body(
        block: &ruby_prism::BlockNode,
        param_name: &str,
        source: &str,
    ) -> Option<(MatchKind, String)> {
        let body = block.body()?;
        let stmts_node = match &body {
            ruby_prism::Node::StatementsNode { .. } => body.as_statements_node().unwrap(),
            _ => return None,
        };
        let stmts: Vec<_> = stmts_node.body().iter().collect();
        if stmts.len() != 1 {
            return None;
        }
        Self::analyze_stmt(&stmts[0], param_name, source)
    }

    /// Analyze a single statement in the block body.
    fn analyze_stmt(
        stmt: &ruby_prism::Node,
        param_name: &str,
        source: &str,
    ) -> Option<(MatchKind, String)> {
        // Check for negation: !expr
        if let ruby_prism::Node::CallNode { .. } = stmt {
            let call = stmt.as_call_node().unwrap();
            let method = String::from_utf8_lossy(call.name().as_slice());
            if method == "!" {
                if let Some(inner) = call.receiver() {
                    // !x.match?(...), !regexp.match?(x)
                    if let Some((inner_kind, regexp_src)) =
                        Self::analyze_match_expr(&inner, param_name, source)
                    {
                        return Some((Self::negate(inner_kind), regexp_src));
                    }
                    // !(x =~ /regexp/), !(/regexp/ =~ x)
                    if let ruby_prism::Node::ParenthesesNode { .. } = &inner {
                        let paren = inner.as_parentheses_node().unwrap();
                        if let Some(paren_body) = paren.body() {
                            if let ruby_prism::Node::StatementsNode { .. } = &paren_body {
                                let inner_stmts = paren_body.as_statements_node().unwrap();
                                let inner_body: Vec<_> = inner_stmts.body().iter().collect();
                                if inner_body.len() == 1 {
                                    if let Some((inner_kind, regexp_src)) =
                                        Self::analyze_match_expr(&inner_body[0], param_name, source)
                                    {
                                        return Some((Self::negate(inner_kind), regexp_src));
                                    }
                                }
                            }
                        }
                    }
                }
                return None;
            }
        }

        // Non-negated match expression
        Self::analyze_match_expr(stmt, param_name, source)
    }

    /// Analyze a match expression (not negated at this level).
    fn analyze_match_expr(
        node: &ruby_prism::Node,
        param_name: &str,
        source: &str,
    ) -> Option<(MatchKind, String)> {
        match node {
            ruby_prism::Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                Self::analyze_call(&call, param_name, source)
            }
            ruby_prism::Node::MatchWriteNode { .. } => {
                // /regexp/ =~ x can produce MatchWriteNode when LHS is a
                // regexp literal with named captures. The inner call is a CallNode.
                let mw = node.as_match_write_node().unwrap();
                let inner_call = mw.call();
                Self::analyze_call(&inner_call, param_name, source)
            }
            _ => None,
        }
    }

    /// Analyze a CallNode for match?, =~, !~ patterns.
    fn analyze_call(
        call: &ruby_prism::CallNode,
        param_name: &str,
        source: &str,
    ) -> Option<(MatchKind, String)> {
        let method = String::from_utf8_lossy(call.name().as_slice());

        match method.as_ref() {
            "match?" => {
                // x.match?(/regexp/) or /regexp/.match?(x)
                // Also: match?(x) without receiver - should NOT match (bare match?)
                let receiver = call.receiver()?;
                let args: Vec<_> =
                    call.arguments().map_or(vec![], |a| a.arguments().iter().collect());
                if args.len() != 1 {
                    return None;
                }

                if Self::is_param_ref(&receiver, param_name) {
                    // x.match?(/regexp/)
                    let regexp_src = Self::node_source(&args[0], source)?;
                    Some((MatchKind::Positive, regexp_src))
                } else if Self::is_simple_param_ref(&args[0], param_name) {
                    // /regexp/.match?(x) - param must be direct, not foo(x)
                    let regexp_src = Self::node_source(&receiver, source)?;
                    Some((MatchKind::Positive, regexp_src))
                } else {
                    None
                }
            }
            "=~" => {
                // x =~ /regexp/ or /regexp/ =~ x
                let receiver = call.receiver()?;
                let args: Vec<_> =
                    call.arguments().map_or(vec![], |a| a.arguments().iter().collect());
                if args.len() != 1 {
                    return None;
                }

                if Self::is_param_ref(&receiver, param_name) {
                    let regexp_src = Self::node_source(&args[0], source)?;
                    Some((MatchKind::Positive, regexp_src))
                } else if Self::is_param_ref(&args[0], param_name) {
                    let regexp_src = Self::node_source(&receiver, source)?;
                    Some((MatchKind::Positive, regexp_src))
                } else {
                    None
                }
            }
            "!~" => {
                // x !~ /regexp/ or /regexp/ !~ x
                let receiver = call.receiver()?;
                let args: Vec<_> =
                    call.arguments().map_or(vec![], |a| a.arguments().iter().collect());
                if args.len() != 1 {
                    return None;
                }

                if Self::is_param_ref(&receiver, param_name) {
                    let regexp_src = Self::node_source(&args[0], source)?;
                    Some((MatchKind::Negative, regexp_src))
                } else if Self::is_param_ref(&args[0], param_name) {
                    let regexp_src = Self::node_source(&receiver, source)?;
                    Some((MatchKind::Negative, regexp_src))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn negate(kind: MatchKind) -> MatchKind {
        match kind {
            MatchKind::Positive => MatchKind::Negative,
            MatchKind::Negative => MatchKind::Positive,
        }
    }

    /// Check if a node is a reference to the block parameter.
    fn is_param_ref(node: &ruby_prism::Node, param_name: &str) -> bool {
        match node {
            ruby_prism::Node::LocalVariableReadNode { .. } => {
                let lvar = node.as_local_variable_read_node().unwrap();
                let name = String::from_utf8_lossy(lvar.name().as_slice());
                name == param_name
            }
            ruby_prism::Node::CallNode { .. } => {
                // `it` in Ruby 3.4 may be parsed as a CallNode with no receiver and no arguments
                let call = node.as_call_node().unwrap();
                let name = String::from_utf8_lossy(call.name().as_slice());
                name == param_name
                    && call.receiver().is_none()
                    && call.arguments().is_none()
            }
            ruby_prism::Node::ItLocalVariableReadNode { .. } => param_name == "it",
            _ => false,
        }
    }

    /// Check if a node is a simple (direct) reference to the block parameter.
    /// This is used for the argument position in /regexp/.match?(x) to ensure
    /// it's not wrapped in a method call like /regexp/.match?(foo(x)).
    fn is_simple_param_ref(node: &ruby_prism::Node, param_name: &str) -> bool {
        // A simple ref is a direct local variable read or `it`/`_1` reference
        // If it were wrapped in a call like foo(x), the argument node would be
        // a CallNode with the param as its argument, not a direct param ref.
        Self::is_param_ref(node, param_name)
    }

    /// Get the source text of a node.
    fn node_source(node: &ruby_prism::Node, source: &str) -> Option<String> {
        let loc = node.location();
        source
            .get(loc.start_offset()..loc.end_offset())
            .map(|s| s.to_string())
    }

    /// Check if the receiver of the call is hash-like.
    fn is_hash_like_receiver(call_node: &ruby_prism::CallNode, source: &str) -> bool {
        if let Some(receiver) = call_node.receiver() {
            Self::is_hash_receiver(&receiver, source)
        } else {
            false
        }
    }

    /// Determine the end offset of the offense for the block.
    /// For brace blocks: end of the closing `}`
    /// For do...end blocks: end of the opening line (parameters or `do`)
    fn block_offense_end(block: &ruby_prism::BlockNode, source: &str) -> usize {
        let open_loc = block.opening_loc();
        let open_text = source
            .get(open_loc.start_offset()..open_loc.end_offset())
            .unwrap_or("");

        if open_text == "do" {
            // Multiline do...end block.
            // Offense ends at the end of parameters if present, otherwise at end of "do".
            if let Some(params) = block.parameters() {
                // End at the closing `|` of block parameters
                params.location().end_offset()
            } else {
                // End at the end of `do`
                open_loc.end_offset()
            }
        } else {
            // Brace block: offense covers the entire block including `}`
            block.location().end_offset()
        }
    }

    /// Check if the call uses safe navigation (&.).
    fn uses_safe_navigation(call_node: &ruby_prism::CallNode, source: &str) -> bool {
        if let Some(op_loc) = call_node.call_operator_loc() {
            if let Some(op) = source.get(op_loc.start_offset()..op_loc.end_offset()) {
                return op == "&.";
            }
        }
        false
    }
}

impl Default for SelectByRegexp {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for SelectByRegexp {
    fn name(&self) -> &'static str {
        "Style/SelectByRegexp"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let method_name = String::from_utf8_lossy(node.name().as_slice());
        let method_kind = match Self::method_kind(&method_name) {
            Some(kind) => kind,
            None => return vec![],
        };

        // `filter` requires Ruby >= 2.6
        if method_name == "filter" && !ctx.ruby_version_at_least(2, 6) {
            return vec![];
        }

        // Check if the call has a block
        let block = match node.block() {
            Some(b) => b,
            None => return vec![],
        };

        // Must be a BlockNode (not a block pass like &:even?)
        let block_node = match &block {
            ruby_prism::Node::BlockNode { .. } => block.as_block_node().unwrap(),
            _ => return vec![],
        };

        // Check the receiver is not hash-like
        if Self::is_hash_like_receiver(node, ctx.source) {
            return vec![];
        }

        // Get the param name (returns None if invalid arity, multiple params, etc.)
        let param_name = match Self::block_param_name(&block_node) {
            Some(name) => name,
            None => return vec![],
        };

        // Analyze the block body for regexp matching patterns
        let (match_kind, regexp_src) =
            match Self::analyze_block_body(&block_node, &param_name, ctx.source) {
                Some(result) => result,
                None => return vec![],
            };

        let replacement = Self::replacement_method(method_kind, match_kind);

        // grep_v requires Ruby >= 2.3
        if replacement == "grep_v" && !ctx.ruby_version_at_least(2, 3) {
            return vec![];
        }

        let message = format!(
            "Prefer `{}` to `{}` with a regexp match.",
            replacement, method_name
        );

        // Offense spans from receiver start (or call start if no receiver) to block end
        let start_offset = if let Some(receiver) = node.receiver() {
            receiver.location().start_offset()
        } else {
            node.location().start_offset()
        };

        // For multiline (do...end) blocks, the offense location end is at the end of the
        // opening line (end of "do |x|" or "do"), not at "end".
        // For brace blocks, the offense covers the entire expression including "}".
        let offense_end_offset = Self::block_offense_end(&block_node, ctx.source);
        // The correction always covers the full expression (including "end" for do...end)
        let correction_end_offset = block_node.location().end_offset();

        let mut offense =
            ctx.offense_with_range(self.name(), &message, self.severity(), start_offset, offense_end_offset);

        // Build correction
        let safe_nav = Self::uses_safe_navigation(node, ctx.source);
        let receiver_src = node.receiver().map(|r| {
            ctx.source
                .get(r.location().start_offset()..r.location().end_offset())
                .unwrap_or("")
                .to_string()
        });

        let nav_op = if safe_nav { "&." } else { "." };

        let corrected = if let Some(recv_src) = receiver_src {
            format!("{}{}{}({})", recv_src, nav_op, replacement, regexp_src)
        } else {
            format!("{}({})", replacement, regexp_src)
        };

        offense = offense.with_correction(Correction::replace(start_offset, correction_end_offset, corrected));

        vec![offense]
    }
}
