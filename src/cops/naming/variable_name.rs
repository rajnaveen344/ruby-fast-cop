//! Naming/VariableName cop
//!
//! Checks that variable names match the configured style (snake_case or camelCase).
//! Checks local variables, instance variables, class variables, and method arguments.
//! Also supports ForbiddenIdentifiers/ForbiddenPatterns for global variables.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use regex::Regex;
use ruby_prism::Visit;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VariableNameStyle {
    SnakeCase,
    CamelCase,
}

pub struct VariableName {
    enforced_style: VariableNameStyle,
    allowed_identifiers: Vec<String>,
    allowed_patterns: Vec<String>,
    forbidden_identifiers: Vec<String>,
    forbidden_patterns: Vec<String>,
}

impl VariableName {
    pub fn new() -> Self {
        Self {
            enforced_style: VariableNameStyle::SnakeCase,
            allowed_identifiers: vec![],
            allowed_patterns: vec![],
            forbidden_identifiers: vec![],
            forbidden_patterns: vec![],
        }
    }

    pub fn with_config(
        enforced_style: VariableNameStyle,
        allowed_identifiers: Vec<String>,
        allowed_patterns: Vec<String>,
        forbidden_identifiers: Vec<String>,
        forbidden_patterns: Vec<String>,
    ) -> Self {
        Self {
            enforced_style,
            allowed_identifiers,
            allowed_patterns,
            forbidden_identifiers,
            forbidden_patterns,
        }
    }
}

impl Default for VariableName {
    fn default() -> Self {
        Self::new()
    }
}

/// Strip sigils (@, @@, $) from a variable name
fn strip_sigils(name: &str) -> &str {
    name.strip_prefix("@@")
        .or_else(|| name.strip_prefix('@'))
        .or_else(|| name.strip_prefix('$'))
        .unwrap_or(name)
}

/// Check if name matches snake_case: all lowercase, digits, or underscores
fn is_snake_case(name: &str) -> bool {
    let base = strip_sigils(name);
    base.is_empty() || base.chars().all(|c| c.is_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Check if name matches camelCase: optional _ prefix, starts lowercase, no underscores after
fn is_camel_case(name: &str) -> bool {
    let base = strip_sigils(name);
    if base.is_empty() || base == "_" {
        return true;
    }
    let rest = base.strip_prefix('_').unwrap_or(base);
    rest.starts_with(|c: char| c.is_lowercase()) && !rest.contains('_')
}

fn matches_style(name: &str, style: VariableNameStyle) -> bool {
    match style {
        VariableNameStyle::SnakeCase => is_snake_case(name),
        VariableNameStyle::CamelCase => is_camel_case(name),
    }
}

fn matches_any_pattern(name: &str, patterns: &[String]) -> bool {
    patterns
        .iter()
        .any(|pat| Regex::new(pat).map_or(false, |re| re.is_match(name)))
}

fn style_message(style: VariableNameStyle) -> &'static str {
    match style {
        VariableNameStyle::SnakeCase => "Use snake_case for variable names.",
        VariableNameStyle::CamelCase => "Use camelCase for variable names.",
    }
}

fn name_str(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a VariableName,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn check(&mut self, full_name: &str, start: usize, end: usize, global_only: bool) {
        let bare = strip_sigils(full_name);

        // Global variables: only check forbidden names
        if global_only {
            if self.is_forbidden(bare) {
                self.add_forbidden_offense(full_name, start, end);
            }
            return;
        }

        // AllowedIdentifiers check
        if self.cop.allowed_identifiers.contains(&bare.to_string()) {
            return;
        }

        // Forbidden check takes priority
        if self.is_forbidden(bare) {
            self.add_forbidden_offense(full_name, start, end);
            return;
        }

        // Style check
        if !matches_style(bare, self.cop.enforced_style)
            && !matches_any_pattern(bare, &self.cop.allowed_patterns)
        {
            self.offenses.push(self.ctx.offense_with_range(
                "Naming/VariableName",
                style_message(self.cop.enforced_style),
                Severity::Convention,
                start,
                end,
            ));
        }
    }

    fn is_forbidden(&self, bare_name: &str) -> bool {
        self.cop.forbidden_identifiers.contains(&bare_name.to_string())
            || matches_any_pattern(bare_name, &self.cop.forbidden_patterns)
    }

    fn add_forbidden_offense(&mut self, full_name: &str, start: usize, end: usize) {
        self.offenses.push(self.ctx.offense_with_range(
            "Naming/VariableName",
            &format!("`{}` is forbidden, use another name instead.", full_name),
            Severity::Convention,
            start,
            end,
        ));
    }
}

/// Macro to reduce visitor boilerplate for variable/parameter nodes
macro_rules! visit_var {
    // Nodes with name() + name_loc(), recurse into children
    (write $method:ident, $node_ty:ty, $recurse:path, global = $global:expr) => {
        fn $method(&mut self, node: &$node_ty) {
            let name = name_str(node.name().as_slice());
            let loc = node.name_loc();
            self.check(&name, loc.start_offset(), loc.end_offset(), $global);
            $recurse(self, node);
        }
    };
    // Nodes with name() + location(), no recursion (leaf params)
    (param $method:ident, $node_ty:ty) => {
        fn $method(&mut self, node: &$node_ty) {
            let name = name_str(node.name().as_slice());
            let loc = node.location();
            self.check(&name, loc.start_offset(), loc.end_offset(), false);
        }
    };
    // Keyword params: name includes trailing colon, exclude it from range
    (kwparam $method:ident, $node_ty:ty $(, $recurse:path)?) => {
        fn $method(&mut self, node: &$node_ty) {
            let name = name_str(node.name().as_slice());
            let trimmed = name.trim_end_matches(':');
            let loc = node.name_loc();
            self.check(trimmed, loc.start_offset(), loc.end_offset() - 1, false);
            $($recurse(self, node);)?
        }
    };
    // Optional name_loc nodes (rest, keyword_rest, block params)
    (opt $method:ident, $node_ty:ty) => {
        fn $method(&mut self, node: &$node_ty) {
            if let Some(name_loc) = node.name_loc() {
                let name = name_str(name_loc.as_slice());
                self.check(&name, name_loc.start_offset(), name_loc.end_offset(), false);
            }
        }
    };
}

impl Visit<'_> for Visitor<'_> {
    visit_var!(write visit_local_variable_write_node, ruby_prism::LocalVariableWriteNode, ruby_prism::visit_local_variable_write_node, global = false);
    visit_var!(write visit_instance_variable_write_node, ruby_prism::InstanceVariableWriteNode, ruby_prism::visit_instance_variable_write_node, global = false);
    visit_var!(write visit_class_variable_write_node, ruby_prism::ClassVariableWriteNode, ruby_prism::visit_class_variable_write_node, global = false);
    visit_var!(write visit_global_variable_write_node, ruby_prism::GlobalVariableWriteNode, ruby_prism::visit_global_variable_write_node, global = true);
    visit_var!(write visit_optional_parameter_node, ruby_prism::OptionalParameterNode, ruby_prism::visit_optional_parameter_node, global = false);
    visit_var!(param visit_required_parameter_node, ruby_prism::RequiredParameterNode);
    visit_var!(kwparam visit_required_keyword_parameter_node, ruby_prism::RequiredKeywordParameterNode);
    visit_var!(kwparam visit_optional_keyword_parameter_node, ruby_prism::OptionalKeywordParameterNode, ruby_prism::visit_optional_keyword_parameter_node);
    visit_var!(opt visit_rest_parameter_node, ruby_prism::RestParameterNode);
    visit_var!(opt visit_keyword_rest_parameter_node, ruby_prism::KeywordRestParameterNode);
    visit_var!(opt visit_block_parameter_node, ruby_prism::BlockParameterNode);

    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let name = name_str(node.name().as_slice());
        let loc = node.location();
        self.check(&name, loc.start_offset(), loc.end_offset(), false);
    }
}

impl Cop for VariableName {
    fn name(&self) -> &'static str {
        "Naming/VariableName"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = Visitor {
            ctx,
            cop: self,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

crate::register_cop!("Naming/VariableName", |cfg| {
    let cop_config = cfg.get_cop_config("Naming/VariableName");
    let style = cop_config
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "camelCase" => VariableNameStyle::CamelCase,
            _ => VariableNameStyle::SnakeCase,
        })
        .unwrap_or(VariableNameStyle::SnakeCase);
    let allowed_identifiers = cop_config
        .and_then(|c| c.raw.get("AllowedIdentifiers"))
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let allowed_patterns = cop_config
        .and_then(|c| c.raw.get("AllowedPatterns"))
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let forbidden_identifiers = cop_config
        .and_then(|c| c.raw.get("ForbiddenIdentifiers"))
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let forbidden_patterns = cop_config
        .and_then(|c| c.raw.get("ForbiddenPatterns"))
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    Some(Box::new(VariableName::with_config(
        style, allowed_identifiers, allowed_patterns, forbidden_identifiers, forbidden_patterns,
    )))
});
