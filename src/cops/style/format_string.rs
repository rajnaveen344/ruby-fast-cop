//! Style/FormatString - enforce a single string formatting style (format/sprintf/%).
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/format_string.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Node;

#[derive(Clone, Copy, PartialEq)]
enum Style { Format, Sprintf, Percent }

pub struct FormatString { style: Style }

impl FormatString {
    pub fn new(style: Style) -> Self { Self { style } }
}

impl Default for FormatString { fn default() -> Self { Self::new(Style::Format) } }

const AUTOCORRECTABLE_METHODS: &[&str] = &["to_d", "to_f", "to_h", "to_i", "to_r", "to_s", "to_sym"];

fn method_name_str(style: Style) -> &'static str {
    match style {
        Style::Format => "format",
        Style::Sprintf => "sprintf",
        Style::Percent => "String#%",
    }
}

fn source_of(node: &Node, source: &str) -> String {
    let loc = node.location();
    source[loc.start_offset()..loc.end_offset()].to_string()
}

/// Detects which style a CallNode uses, if any. Returns the detected style.
fn detect_style(call: &ruby_prism::CallNode) -> Option<Style> {
    let name = node_name!(call);
    let name_str: &str = &name;
    match name_str {
        "format" | "sprintf" => {
            if call.receiver().is_some() { return None; }
            let args = call.arguments()?;
            let count = args.arguments().iter().count();
            if count < 2 { return None; }
            Some(if name_str == "format" { Style::Format } else { Style::Sprintf })
        }
        "%" => {
            let recv = call.receiver()?;
            let args = call.arguments()?;
            let arg_list: Vec<Node> = args.arguments().iter().collect();
            if arg_list.len() != 1 { return None; }
            let arg = &arg_list[0];
            // Case A: string/dstr receiver, any arg
            let recv_is_str = matches!(
                recv,
                Node::StringNode { .. } | Node::InterpolatedStringNode { .. }
            );
            if recv_is_str {
                return Some(Style::Percent);
            }
            // Case B: non-nil, non-string receiver with array/hash arg
            if matches!(arg, Node::ArrayNode { .. } | Node::HashNode { .. } | Node::KeywordHashNode { .. }) {
                return Some(Style::Percent);
            }
            None
        }
        _ => None,
    }
}

/// Check if a `str % arg` conversion can be autocorrected to format/sprintf
/// (needs to replicate RuboCop's variable_argument? / autocorrectable? guard).
fn autocorrectable_percent_arg(arg: &Node) -> bool {
    // Not a local variable ref (lvar): in RuboCop lvars are NOT autocorrectable
    // because `"%s" % arr` works but `format("%s", arr)` behaves differently.
    match arg {
        Node::LocalVariableReadNode { .. } => false,
        Node::CallNode { .. } => {
            let c = arg.as_call_node().unwrap();
            // No arguments (attribute-like call): autocorrectable unless the method
            // is in AUTOCORRECTABLE_METHODS... wait, RuboCop's autocorrect check is:
            //   variable_argument?(node) := (str % autocorrectable?(arg))
            //   autocorrectable?(arg) := lvar_type? OR (send_type? AND !AUTOCORRECTABLE_METHODS.include?(method))
            // If autocorrectable? returns TRUE, then variable_argument? matches and
            // autocorrection is SKIPPED. Inverting: we CAN autocorrect when
            // autocorrectable? is FALSE — i.e. not lvar and (not send OR send with a
            // known-safe conversion method).
            let method = node_name!(c);
            let m: &str = &method;
            AUTOCORRECTABLE_METHODS.contains(&m)
        }
        _ => true, // literal array/hash/string/integer/etc: autocorrectable
    }
}

impl FormatString {
    /// For %-style conversions: build `style(receiver, args...)` replacement.
    fn autocorrect_from_percent(&self, call: &ruby_prism::CallNode, source: &str) -> Option<Correction> {
        let recv = call.receiver()?;
        let args = call.arguments()?;
        let first = args.arguments().iter().next()?;
        // Skip if argument isn't autocorrectable.
        if !autocorrectable_percent_arg(&first) { return None; }

        let style = match self.style {
            Style::Format => "format",
            Style::Sprintf => "sprintf",
            _ => return None,
        };
        let recv_src = source_of(&recv, source);
        let args_src = match &first {
            Node::ArrayNode { .. } => {
                let arr = first.as_array_node().unwrap();
                let parts: Vec<String> = arr.elements().iter().map(|e| source_of(&e, source)).collect();
                parts.join(", ")
            }
            Node::HashNode { .. } => {
                let h = first.as_hash_node().unwrap();
                let parts: Vec<String> = h.elements().iter().map(|e| source_of(&e, source)).collect();
                parts.join(", ")
            }
            Node::KeywordHashNode { .. } => {
                let h = first.as_keyword_hash_node().unwrap();
                let parts: Vec<String> = h.elements().iter().map(|e| source_of(&e, source)).collect();
                parts.join(", ")
            }
            _ => source_of(&first, source),
        };
        let replacement = format!("{style}({recv_src}, {args_src})");
        let loc = call.location();
        Some(Correction::replace(loc.start_offset(), loc.end_offset(), replacement))
    }

    /// For format/sprintf calls, swap the selector between format/sprintf.
    fn autocorrect_swap_selector(&self, call: &ruby_prism::CallNode) -> Option<Correction> {
        let sel = call.message_loc()?;
        let new_name = match self.style {
            Style::Format => "format",
            Style::Sprintf => "sprintf",
            _ => return None,
        };
        Some(Correction::replace(sel.start_offset(), sel.end_offset(), new_name))
    }

    /// For format/sprintf → percent: rebuild `receiver % args` replacement.
    fn autocorrect_to_percent(&self, call: &ruby_prism::CallNode, source: &str) -> Option<Correction> {
        let args = call.arguments()?;
        let arg_list: Vec<Node> = args.arguments().iter().collect();
        if arg_list.is_empty() { return None; }
        let format_arg = &arg_list[0];
        let format_src = source_of(format_arg, source);
        let param_args = &arg_list[1..];
        if param_args.is_empty() { return None; }

        let args_src = if param_args.len() == 1 {
            format_single_parameter(&param_args[0], source)
        } else {
            let joined: Vec<String> = param_args.iter().map(|a| source_of(a, source)).collect();
            format!("[{}]", joined.join(", "))
        };

        let replacement = format!("{format_src} % {args_src}");
        let loc = call.location();
        Some(Correction::replace(loc.start_offset(), loc.end_offset(), replacement))
    }
}

fn format_single_parameter(arg: &Node, source: &str) -> String {
    let s = source_of(arg, source);
    match arg {
        Node::HashNode { .. } | Node::KeywordHashNode { .. } => format!("{{ {} }}", s),
        Node::CallNode { .. } => {
            let c = arg.as_call_node().unwrap();
            let name = node_name!(c);
            let is_operator = !name.chars().all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '!' || ch == '?' || ch == '=');
            let has_parens = c.opening_loc().is_some();
            if is_operator && !has_parens { format!("({})", s) } else { s }
        }
        _ => s,
    }
}

impl Cop for FormatString {
    fn name(&self) -> &'static str { "Style/FormatString" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let detected = match detect_style(node) { Some(s) => s, None => return vec![] };
        if detected == self.style { return vec![]; }

        let sel = match node.message_loc() { Some(s) => s, None => return vec![] };
        let msg = format!(
            "Favor `{}` over `{}`.",
            method_name_str(self.style),
            method_name_str(detected)
        );
        let mut off = ctx.offense_with_range(
            self.name(),
            &msg,
            self.severity(),
            sel.start_offset(),
            sel.end_offset(),
        );

        // Autocorrect
        let correction = match detected {
            Style::Percent => self.autocorrect_from_percent(node, ctx.source),
            Style::Format | Style::Sprintf => match self.style {
                Style::Percent => self.autocorrect_to_percent(node, ctx.source),
                _ => self.autocorrect_swap_selector(node),
            },
        };
        if let Some(c) = correction { off = off.with_correction(c); }
        vec![off]
    }
}

crate::register_cop!("Style/FormatString", |cfg| {
    let style = cfg
        .get_cop_config("Style/FormatString")
        .and_then(|c| c.enforced_style.as_deref())
        .map(|s| match s {
            "sprintf" => Style::Sprintf,
            "percent" => Style::Percent,
            _ => Style::Format,
        })
        .unwrap_or(Style::Format);
    Some(Box::new(FormatString::new(style)))
});
