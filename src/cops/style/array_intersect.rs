use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};

#[derive(Default)]
pub struct ArrayIntersect {
    active_support_extensions: bool,
}

/// Methods that indicate a "straight" (positive) intersect? replacement
const STRAIGHT_METHODS: &[&str] = &["present?", "any?", ">", "positive?", "!="];
/// Predicate methods checked on the intersection result (without ActiveSupport)
const PREDICATES: &[&str] = &["any?", "empty?", "none?"];
/// With ActiveSupport
const ACTIVE_SUPPORT_PREDICATES: &[&str] = &["any?", "empty?", "none?", "present?", "blank?"];
/// Size methods (count/length/size)
const SIZE_METHODS: &[&str] = &["count", "length", "size"];

impl ArrayIntersect {
    pub fn new() -> Self {
        Self { active_support_extensions: false }
    }

    pub fn with_config(active_support_extensions: bool) -> Self {
        Self { active_support_extensions }
    }

    fn predicates(&self) -> &[&str] {
        if self.active_support_extensions {
            ACTIVE_SUPPORT_PREDICATES
        } else {
            PREDICATES
        }
    }

    fn is_straight(method: &str) -> bool {
        STRAIGHT_METHODS.contains(&method)
    }

    /// Extract the dot operator source ("." or "&.") for a call node
    fn dot_source<'a>(call: &ruby_prism::CallNode<'a>, source: &'a str) -> &'a str {
        call.call_operator_loc()
            .and_then(|loc| source.get(loc.start_offset()..loc.end_offset()))
            .unwrap_or(".")
    }

    fn node_source<'a>(node: &ruby_prism::Node, source: &'a str) -> &'a str {
        let loc = node.location();
        &source[loc.start_offset()..loc.end_offset()]
    }

    /// Try to match `(a & b).predicate?` or `a.intersection(b).predicate?` patterns.
    /// Returns (receiver_src, argument_src, dot, method_name, whole_start, whole_end).
    fn match_predicate_call<'a>(
        &self,
        call: &ruby_prism::CallNode<'a>,
        source: &'a str,
    ) -> Option<(&'a str, &'a str, &'a str, &'a str, usize, usize)> {
        let method = std::str::from_utf8(call.name().as_slice()).ok()?;
        if !self.predicates().contains(&method) {
            return None;
        }
        // Must not have a block
        if call.block().is_some() {
            return None;
        }
        // Must not have arguments (e.g. `.any?(&:block)`)
        if call.arguments().map_or(false, |a| a.arguments().iter().count() > 0) {
            return None;
        }

        let receiver = call.receiver()?;
        self.extract_intersection(&receiver, source)
            .map(|(recv_src, arg_src, dot)| {
                let whole_start = call.location().start_offset();
                let whole_end = call.location().end_offset();
                (recv_src, arg_src, dot, method, whole_start, whole_end)
            })
    }

    /// Try to match `(a & b).count > 0` / `.count.zero?` / `.count.positive?` etc.
    /// Returns (receiver_src, argument_src, dot, comparison_method, whole_start, whole_end).
    fn match_size_check<'a>(
        &self,
        call: &ruby_prism::CallNode<'a>,
        source: &'a str,
    ) -> Option<(&'a str, &'a str, &'a str, &'a str, usize, usize)> {
        let method = std::str::from_utf8(call.name().as_slice()).ok()?;

        match method {
            // (a & b).count > 0, (a & b).count == 0, (a & b).count != 0
            ">" | "==" | "!=" => {
                let args: Vec<_> = call.arguments()?.arguments().iter().collect();
                if args.len() != 1 { return None; }
                // Check the argument is `0`
                if let ruby_prism::Node::IntegerNode { .. } = &args[0] {
                    let int_src = Self::node_source(&args[0], source);
                    if int_src != "0" { return None; }
                } else {
                    return None;
                }
                // Receiver should be a call to count/length/size on an intersection
                let size_call_node = call.receiver()?;
                let size_call = size_call_node.as_call_node()?;
                let size_method = std::str::from_utf8(size_call.name().as_slice()).ok()?;
                if !SIZE_METHODS.contains(&size_method) { return None; }
                let intersection_node = size_call.receiver()?;
                let (recv_src, arg_src, dot) = self.extract_intersection(&intersection_node, source)?;
                let whole_start = call.location().start_offset();
                let whole_end = call.location().end_offset();
                Some((recv_src, arg_src, dot, method, whole_start, whole_end))
            }
            // (a & b).count.zero?, (a & b).count.positive?
            "zero?" | "positive?" => {
                let size_call_node = call.receiver()?;
                let size_call = size_call_node.as_call_node()?;
                let size_method = std::str::from_utf8(size_call.name().as_slice()).ok()?;
                if !SIZE_METHODS.contains(&size_method) { return None; }
                let intersection_node = size_call.receiver()?;
                let (recv_src, arg_src, dot) = self.extract_intersection(&intersection_node, source)?;
                let whole_start = call.location().start_offset();
                let whole_end = call.location().end_offset();
                Some((recv_src, arg_src, dot, method, whole_start, whole_end))
            }
            _ => None,
        }
    }

    /// Extract receiver and argument from an intersection pattern:
    /// - `(a & b)` -> Some(("a", "b", "."))
    /// - `a.intersection(b)` -> Some(("a", "b", "."))
    /// - `a&.intersection(b)` -> Some(("a", "b", "&."))
    fn extract_intersection<'a>(
        &self,
        node: &ruby_prism::Node<'a>,
        source: &'a str,
    ) -> Option<(&'a str, &'a str, &'a str)> {
        match node {
            // (a & b) - ParenthesesNode containing a & call
            ruby_prism::Node::ParenthesesNode { .. } => {
                let paren = node.as_parentheses_node().unwrap();
                let body = paren.body()?;
                let stmts = body.as_statements_node()?;
                let stmts_vec: Vec<_> = stmts.body().iter().collect();
                if stmts_vec.len() != 1 { return None; }
                let amp_call = stmts_vec[0].as_call_node()?;
                let amp_method = std::str::from_utf8(amp_call.name().as_slice()).ok()?;
                if amp_method != "&" { return None; }
                let receiver = amp_call.receiver()?;
                let args: Vec<_> = amp_call.arguments()?.arguments().iter().collect();
                if args.len() != 1 { return None; }
                let recv_src = Self::node_source(&receiver, source);
                let arg_src = Self::node_source(&args[0], source);
                Some((recv_src, arg_src, "."))
            }
            // a.intersection(b)
            ruby_prism::Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let method = std::str::from_utf8(call.name().as_slice()).ok()?;
                if method != "intersection" { return None; }
                let receiver = call.receiver()?;
                let args: Vec<_> = call.arguments()?.arguments().iter().collect();
                if args.len() != 1 { return None; }
                let recv_src = Self::node_source(&receiver, source);
                let arg_src = Self::node_source(&args[0], source);
                let dot = Self::dot_source(&call, source);
                Some((recv_src, arg_src, dot))
            }
            _ => None,
        }
    }

    /// Check if a call node has a block with the pattern `{ |e| array2.member?(e) }`
    fn check_block_member_pattern<'a>(
        &self,
        call: &ruby_prism::CallNode<'a>,
        source: &'a str,
        ctx: &CheckContext,
    ) -> Option<(&'a str, &'a str, &'a str, &'a str, usize, usize)> {
        let method = std::str::from_utf8(call.name().as_slice()).ok()?;
        if method != "any?" && method != "none?" {
            return None;
        }
        // Must have a block
        let block_node = call.block()?;
        let block = block_node.as_block_node()?;

        // Get parameter name
        let param_name = self.block_param_name(&block, ctx)?;

        // Get block body - should be a single `array2.member?(param)` call
        let body = block.body()?;
        let stmts = body.as_statements_node()?;
        let stmts_vec: Vec<_> = stmts.body().iter().collect();
        if stmts_vec.len() != 1 { return None; }

        let member_call = stmts_vec[0].as_call_node()?;
        let member_method = std::str::from_utf8(member_call.name().as_slice()).ok()?;
        if member_method != "member?" { return None; }

        // Check argument is the block parameter
        let args: Vec<_> = member_call.arguments()?.arguments().iter().collect();
        if args.len() != 1 { return None; }
        if !self.is_param_ref(&args[0], &param_name) { return None; }

        // The receiver of member? is the argument (array2)
        let argument_node = member_call.receiver()?;
        let arg_src = Self::node_source(&argument_node, source);

        // The receiver of any?/none? is the receiver (array1)
        let receiver_node = call.receiver()?;
        let recv_src = Self::node_source(&receiver_node, source);

        let dot = Self::dot_source(call, source);

        // The whole offense spans from the receiver start to the block end
        let whole_start = receiver_node.location().start_offset();
        let whole_end = block.location().end_offset();

        Some((recv_src, arg_src, dot, method, whole_start, whole_end))
    }

    fn block_param_name(&self, block: &ruby_prism::BlockNode, ctx: &CheckContext) -> Option<String> {
        let params = block.parameters()?;
        match &params {
            ruby_prism::Node::BlockParametersNode { .. } => {
                let block_params = params.as_block_parameters_node().unwrap();
                let inner_params = block_params.parameters()?;
                let requireds: Vec<_> = inner_params.requireds().iter().collect();
                if requireds.len() != 1 { return None; }
                if let ruby_prism::Node::RequiredParameterNode { .. } = &requireds[0] {
                    Some(String::from_utf8_lossy(
                        requireds[0].as_required_parameter_node().unwrap().name().as_slice()
                    ).into_owned())
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
            ruby_prism::Node::ItParametersNode { .. } => {
                // `it` parameters only available in Ruby 3.4+
                if ctx.ruby_version_at_least(3, 4) {
                    Some("it".to_string())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn is_param_ref(&self, node: &ruby_prism::Node, param_name: &str) -> bool {
        match node {
            ruby_prism::Node::LocalVariableReadNode { .. } => {
                let name = String::from_utf8_lossy(
                    node.as_local_variable_read_node().unwrap().name().as_slice()
                );
                name == param_name
            }
            ruby_prism::Node::ItLocalVariableReadNode { .. } => param_name == "it",
            _ => false,
        }
    }
}

impl Cop for ArrayIntersect {
    fn name(&self) -> &'static str { "Style/ArrayIntersect" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        // Minimum Ruby 3.1
        if !ctx.ruby_version_at_least(3, 1) {
            return vec![];
        }

        // Try predicate pattern: (a & b).any? / a.intersection(b).empty?
        if let Some((recv, arg, dot, method, start, end)) = self.match_predicate_call(node, ctx.source) {
            let bang = if Self::is_straight(method) { "" } else { "!" };
            let replacement = format!("{}{}{}intersect?({})", bang, recv, dot, arg);
            let existing = &ctx.source[start..end];
            let message = format!("Use `{}` instead of `{}`.", replacement, existing);
            let mut offense = ctx.offense_with_range(self.name(), &message, self.severity(), start, end);
            offense = offense.with_correction(Correction::replace(start, end, replacement));
            return vec![offense];
        }

        // Try size check pattern: (a & b).count > 0
        if let Some((recv, arg, dot, method, start, end)) = self.match_size_check(node, ctx.source) {
            let bang = if Self::is_straight(method) { "" } else { "!" };
            let replacement = format!("{}{}{}intersect?({})", bang, recv, dot, arg);
            let existing = &ctx.source[start..end];
            let message = format!("Use `{}` instead of `{}`.", replacement, existing);
            let mut offense = ctx.offense_with_range(self.name(), &message, self.severity(), start, end);
            offense = offense.with_correction(Correction::replace(start, end, replacement));
            return vec![offense];
        }

        // Try block pattern: array1.any? { |e| array2.member?(e) }
        if let Some((recv, arg, dot, method, start, end)) = self.check_block_member_pattern(node, ctx.source, ctx) {
            let bang = if Self::is_straight(method) { "" } else { "!" };
            let replacement = format!("{}{}{}intersect?({})", bang, recv, dot, arg);
            let existing = &ctx.source[start..end];
            let message = format!("Use `{}` instead of `{}`.", replacement, existing);
            let mut offense = ctx.offense_with_range(self.name(), &message, self.severity(), start, end);
            offense = offense.with_correction(Correction::replace(start, end, replacement));
            return vec![offense];
        }

        vec![]
    }
}

crate::register_cop!("Style/ArrayIntersect", |cfg| {
    let cop_config = cfg.get_cop_config("Style/ArrayIntersect");
    let active_support = cop_config
        .and_then(|c| c.raw.get("ActiveSupportExtensionsEnabled"))
        .and_then(|v| v.as_bool())
        .or_else(|| cop_config
            .and_then(|c| c.raw.get("AllCopsActiveSupportExtensionsEnabled"))
            .and_then(|v| v.as_bool()))
        .unwrap_or(false);
    Some(Box::new(ArrayIntersect::with_config(active_support)))
});
