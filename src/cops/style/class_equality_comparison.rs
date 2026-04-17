//! Style/ClassEqualityComparison — prefer `Object#instance_of?` over class comparison.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/class_equality_comparison.rb

use crate::cops::{CheckContext, Cop};
use crate::helpers::allowed_methods::is_method_allowed;
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const COP_NAME: &str = "Style/ClassEqualityComparison";
const RESTRICT_ON_SEND: &[&str] = &["==", "equal?", "eql?"];
const CLASS_NAME_METHODS: &[&str] = &["name", "to_s", "inspect"];

#[derive(Default)]
pub struct ClassEqualityComparison {
    allowed_methods: Vec<String>,
    allowed_patterns: Vec<String>,
}

impl ClassEqualityComparison {
    pub fn new() -> Self {
        // RuboCop default: AllowedMethods = ['==', 'equal?', 'eql?']
        Self {
            allowed_methods: vec!["==".into(), "equal?".into(), "eql?".into()],
            allowed_patterns: Vec::new(),
        }
    }

    pub fn with_config(allowed_methods: Vec<String>, allowed_patterns: Vec<String>) -> Self {
        Self {
            allowed_methods,
            allowed_patterns,
        }
    }
}

impl Cop for ClassEqualityComparison {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = Visitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
            def_stack: Vec::new(),
            in_class_or_module: 0,
        };
        v.visit_program_node(node);
        v.offenses
    }
}

struct Visitor<'a> {
    cop: &'a ClassEqualityComparison,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    def_stack: Vec<String>,
    in_class_or_module: usize,
}

impl<'a> Visitor<'a> {
    fn check_comparison(&mut self, node: &ruby_prism::CallNode) {
        let method = node_name!(node);
        if !RESTRICT_ON_SEND.contains(&method.as_ref()) {
            return;
        }

        // Skip when inside an allowed method definition.
        if let Some(cur) = self.def_stack.last() {
            if is_method_allowed(
                &self.cop.allowed_methods,
                &self.cop.allowed_patterns,
                cur,
                None,
            ) {
                return;
            }
        }

        // Must have a receiver (chained comparison) and exactly one argument.
        let Some(receiver) = node.receiver() else {
            return;
        };
        let Some(args) = node.arguments() else {
            return;
        };
        let arg_list: Vec<Node> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return;
        }
        let class_node = &arg_list[0];

        // Pattern A: receiver is `(send _ :class)`
        // Pattern B: receiver is `(send (send _ :class) #class_name_method?)`
        let Some((inner_class_call, is_class_name_branch)) = classify_receiver(&receiver) else {
            return;
        };

        // Skip interpolated string comparisons — type undetermined.
        if matches!(class_node, Node::InterpolatedStringNode { .. }) {
            return;
        }

        let class_name_text =
            self.class_name(class_node, is_class_name_branch, &inner_class_call);

        // Offense range: from `class` selector in `inner_class_call` to outer call end.
        let Some(sel) = inner_class_call.message_loc() else {
            return;
        };
        let range_start = sel.start_offset();
        let range_end = node.location().end_offset();

        let class_argument = class_name_text
            .as_ref()
            .map(|n| format!("({})", n))
            .unwrap_or_default();
        let msg = format!(
            "Use `instance_of?{}` instead of comparing classes.",
            class_argument
        );

        let mut offense =
            self.ctx
                .offense_with_range(COP_NAME, &msg, Severity::Convention, range_start, range_end);

        if class_name_text.is_some() {
            offense = offense.with_correction(Correction::replace(
                range_start,
                range_end,
                format!("instance_of?{}", class_argument),
            ));
        }

        self.offenses.push(offense);
    }

    /// Determine the concrete class name to substitute, or None if undeterminable.
    fn class_name(
        &self,
        class_node: &Node,
        is_class_name_branch: bool,
        inner_class_call: &ruby_prism::CallNode,
    ) -> Option<String> {
        if is_class_name_branch {
            // When outer pattern is `var.class.name == X`: if X is itself a `something.{name,to_s,inspect}`
            // call, use its receiver.source (e.g. `Date.name` → `Date`).
            if let Some(cn) = class_node.as_call_node() {
                let method = node_name!(cn);
                if CLASS_NAME_METHODS.contains(&method.as_ref()) {
                    if let Some(recv) = cn.receiver() {
                        let loc = recv.location();
                        return Some(
                            self.ctx.source[loc.start_offset()..loc.end_offset()].to_string(),
                        );
                    }
                }
            }
            match class_node {
                Node::StringNode { .. } => {
                    let sn = class_node.as_string_node().unwrap();
                    let loc = sn.location();
                    let raw = &self.ctx.source[loc.start_offset()..loc.end_offset()];
                    let trimmed: String = raw.chars().filter(|c| *c != '"' && *c != '\'').collect();
                    let mut value = trimmed;
                    if self.in_class_or_module > 0 {
                        value.insert_str(0, "::");
                    }
                    Some(value)
                }
                _ if is_variable_or_call(class_node) => None,
                _ => {
                    let loc = class_node.location();
                    Some(self.ctx.source[loc.start_offset()..loc.end_offset()].to_string())
                }
            }
        } else {
            // `var.class == X` form — use X's source verbatim.
            let _ = inner_class_call;
            let loc = class_node.location();
            Some(self.ctx.source[loc.start_offset()..loc.end_offset()].to_string())
        }
    }
}

/// Match the receiver against either:
/// - A: `(send _ :class)` — returns (that call, false)
/// - B: `(send (send _ :class) :name|:to_s|:inspect)` — returns (inner class call, true)
fn classify_receiver<'a>(receiver: &Node<'a>) -> Option<(ruby_prism::CallNode<'a>, bool)> {
    let call = receiver.as_call_node()?;
    let name = node_name!(call);
    if name == "class" {
        // Must have no arguments and a receiver (e.g. `var.class`).
        if call.arguments().is_some() {
            return None;
        }
        if call.receiver().is_none() {
            return None;
        }
        return Some((call, false));
    }
    if CLASS_NAME_METHODS.contains(&name.as_ref()) {
        if call.arguments().is_some() {
            return None;
        }
        let inner = call.receiver()?;
        let inner_call = inner.as_call_node()?;
        if node_name!(inner_call) != "class" {
            return None;
        }
        if inner_call.arguments().is_some() {
            return None;
        }
        if inner_call.receiver().is_none() {
            return None;
        }
        return Some((inner_call, true));
    }
    None
}

fn is_variable_or_call(node: &Node) -> bool {
    matches!(
        node,
        Node::LocalVariableReadNode { .. }
            | Node::InstanceVariableReadNode { .. }
            | Node::ClassVariableReadNode { .. }
            | Node::GlobalVariableReadNode { .. }
            | Node::CallNode { .. }
    )
}

impl<'a> Visit<'a> for Visitor<'a> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'a>) {
        self.check_comparison(node);
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'a>) {
        self.def_stack.push(node_name!(node).into_owned());
        ruby_prism::visit_def_node(self, node);
        self.def_stack.pop();
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode<'a>) {
        self.in_class_or_module += 1;
        ruby_prism::visit_class_node(self, node);
        self.in_class_or_module -= 1;
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode<'a>) {
        self.in_class_or_module += 1;
        ruby_prism::visit_module_node(self, node);
        self.in_class_or_module -= 1;
    }
}

crate::register_cop!("Style/ClassEqualityComparison", |cfg| {
    let cop_config = cfg.get_cop_config("Style/ClassEqualityComparison");
    let mut allowed_methods: Vec<String> = Vec::new();
    for key in &["AllowedMethods", "IgnoredMethods", "ExcludedMethods"] {
        if let Some(seq) = cop_config.and_then(|c| c.raw.get(*key)).and_then(|v| v.as_sequence()) {
            for v in seq {
                if let Some(s) = v.as_str() {
                    allowed_methods.push(s.to_string());
                }
            }
        }
    }
    let mut allowed_patterns: Vec<String> = Vec::new();
    for key in &["AllowedPatterns", "IgnoredPatterns"] {
        if let Some(seq) = cop_config.and_then(|c| c.raw.get(*key)).and_then(|v| v.as_sequence()) {
            for v in seq {
                if let Some(s) = v.as_str() {
                    allowed_patterns.push(s.to_string());
                }
            }
        }
    }
    Some(Box::new(ClassEqualityComparison::with_config(allowed_methods, allowed_patterns)))
});
