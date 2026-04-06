//! Naming/VariableNumber cop
//!
//! Checks that all numbered variables use the configured style:
//! `normalcase` (default), `snake_case`, or `non_integer`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use regex::Regex;
use ruby_prism::Visit;

const COP_NAME: &str = "Naming/VariableNumber";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VariableNumberStyle {
    NormalCase,
    SnakeCase,
    NonInteger,
}

pub struct VariableNumber {
    enforced_style: VariableNumberStyle,
    check_method_names: bool,
    check_symbols: bool,
    allowed_identifiers: Vec<String>,
    allowed_patterns: Vec<String>,
}

impl VariableNumber {
    pub fn new() -> Self {
        Self {
            enforced_style: VariableNumberStyle::NormalCase,
            check_method_names: true,
            check_symbols: true,
            allowed_identifiers: vec![],
            allowed_patterns: vec![],
        }
    }

    pub fn with_config(
        enforced_style: VariableNumberStyle,
        check_method_names: bool,
        check_symbols: bool,
        allowed_identifiers: Vec<String>,
        allowed_patterns: Vec<String>,
    ) -> Self {
        Self {
            enforced_style,
            check_method_names,
            check_symbols,
            allowed_identifiers,
            allowed_patterns,
        }
    }

    /// Check if the name matches the enforced numbering style.
    /// Mirrors ConfigurableNumbering::FORMATS
    fn valid_name(&self, name: &str) -> bool {
        let implicit_param = name.len() >= 2
            && name.starts_with('_')
            && name[1..].chars().all(|c| c.is_ascii_digit());

        match self.enforced_style {
            // snake_case: /(?:\D|_\d+|\A\d+)\z/
            VariableNumberStyle::SnakeCase => {
                !ends_with_bare_digits(name) || is_all_digits(name)
            }
            // normalcase: /(?:\D|[^_\d]\d+|\A\d+)\z|#{implicit_param}/
            VariableNumberStyle::NormalCase => {
                !ends_with_underscore_digits(name) || is_all_digits(name) || implicit_param
            }
            // non_integer: /(\D|\A\d+)\z|#{implicit_param}/
            VariableNumberStyle::NonInteger => {
                !name.chars().last().unwrap_or('x').is_ascii_digit()
                    || is_all_digits(name)
                    || implicit_param
            }
        }
    }

    fn allowed_identifier(&self, name: &str) -> bool {
        if self.allowed_identifiers.is_empty() {
            return false;
        }
        let stripped = name.trim_start_matches(|c: char| c == '@' || c == '$');
        self.allowed_identifiers.iter().any(|id| id == stripped)
    }

    fn matches_allowed_pattern(&self, name: &str) -> bool {
        self.allowed_patterns
            .iter()
            .any(|pat| Regex::new(pat).map_or(false, |re| re.is_match(name)))
    }

    fn style_name(&self) -> &'static str {
        match self.enforced_style {
            VariableNumberStyle::NormalCase => "normalcase",
            VariableNumberStyle::SnakeCase => "snake_case",
            VariableNumberStyle::NonInteger => "non_integer",
        }
    }
}

/// Does name end with digits preceded by underscore? (e.g., foo_1)
fn ends_with_underscore_digits(name: &str) -> bool {
    let bytes = name.as_bytes();
    let len = bytes.len();
    let mut i = len;
    while i > 0 && bytes[i - 1].is_ascii_digit() {
        i -= 1;
    }
    i < len && i > 0 && bytes[i - 1] == b'_'
}

/// Does name end with digits NOT preceded by underscore? (e.g., foo1 but not foo_1)
fn ends_with_bare_digits(name: &str) -> bool {
    let bytes = name.as_bytes();
    let len = bytes.len();
    let mut i = len;
    while i > 0 && bytes[i - 1].is_ascii_digit() {
        i -= 1;
    }
    // Has trailing digits, and char before them is not underscore
    i < len && (i == 0 || bytes[i - 1] != b'_')
}

fn is_all_digits(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_digit())
}

impl Default for VariableNumber {
    fn default() -> Self {
        Self::new()
    }
}

// ── AST Visitor ──

struct Visitor<'a> {
    cop: &'a VariableNumber,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> Visitor<'a> {
    fn check(&mut self, name: &str, id_type: &str, start: usize, end: usize) {
        if self.cop.allowed_identifier(name)
            || self.cop.valid_name(name)
            || self.cop.matches_allowed_pattern(name)
        {
            return;
        }
        self.offenses.push(self.ctx.offense_with_range(
            COP_NAME,
            &format!("Use {} for {} numbers.", self.cop.style_name(), id_type),
            Severity::Convention,
            start,
            end,
        ));
    }
}

/// Helper to extract name string from a Prism name bytes
fn name_str(name: &[u8]) -> String {
    String::from_utf8_lossy(name).to_string()
}

/// Macro to reduce visitor boilerplate for variable/parameter nodes
macro_rules! visit_named_node {
    // Nodes with name() and name_loc(), need recursion
    (write $method:ident, $node_ty:ty, $recurse:path) => {
        fn $method(&mut self, node: &$node_ty) {
            let name = name_str(node.name().as_slice());
            let loc = node.name_loc();
            self.check(&name, "variable", loc.start_offset(), loc.end_offset());
            $recurse(self, node);
        }
    };
    // Nodes with name() and location(), no recursion (leaf params)
    (param $method:ident, $node_ty:ty) => {
        fn $method(&mut self, node: &$node_ty) {
            let name = name_str(node.name().as_slice());
            let loc = node.location();
            self.check(&name, "variable", loc.start_offset(), loc.end_offset());
        }
    };
    // Keyword params: name includes trailing colon, offset by name.len()
    (kwparam $method:ident, $node_ty:ty $(, $recurse:path)?) => {
        fn $method(&mut self, node: &$node_ty) {
            let name = name_str(node.name().as_slice());
            let loc = node.name_loc();
            let start = loc.start_offset();
            self.check(&name, "variable", start, start + name.len());
            $($recurse(self, node);)?
        }
    };
    // Optional name_loc nodes (rest, keyword_rest, block params)
    (opt $method:ident, $node_ty:ty) => {
        fn $method(&mut self, node: &$node_ty) {
            if let Some(name_loc) = node.name_loc() {
                let name = name_str(name_loc.as_slice());
                self.check(&name, "variable", name_loc.start_offset(), name_loc.end_offset());
            }
        }
    };
}

impl Visit<'_> for Visitor<'_> {
    visit_named_node!(write visit_local_variable_write_node, ruby_prism::LocalVariableWriteNode, ruby_prism::visit_local_variable_write_node);
    visit_named_node!(write visit_instance_variable_write_node, ruby_prism::InstanceVariableWriteNode, ruby_prism::visit_instance_variable_write_node);
    visit_named_node!(write visit_class_variable_write_node, ruby_prism::ClassVariableWriteNode, ruby_prism::visit_class_variable_write_node);
    visit_named_node!(write visit_global_variable_write_node, ruby_prism::GlobalVariableWriteNode, ruby_prism::visit_global_variable_write_node);
    visit_named_node!(write visit_optional_parameter_node, ruby_prism::OptionalParameterNode, ruby_prism::visit_optional_parameter_node);
    visit_named_node!(param visit_required_parameter_node, ruby_prism::RequiredParameterNode);
    visit_named_node!(kwparam visit_required_keyword_parameter_node, ruby_prism::RequiredKeywordParameterNode);
    visit_named_node!(kwparam visit_optional_keyword_parameter_node, ruby_prism::OptionalKeywordParameterNode);
    visit_named_node!(opt visit_rest_parameter_node, ruby_prism::RestParameterNode);
    visit_named_node!(opt visit_keyword_rest_parameter_node, ruby_prism::KeywordRestParameterNode);
    visit_named_node!(opt visit_block_parameter_node, ruby_prism::BlockParameterNode);

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        if self.cop.check_method_names {
            let name = name_str(node.name().as_slice());
            let loc = node.name_loc();
            self.check(&name, "method name", loc.start_offset(), loc.end_offset());
        }
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_symbol_node(&mut self, node: &ruby_prism::SymbolNode) {
        if self.cop.check_symbols {
            let name = String::from_utf8_lossy(node.unescaped()).to_string();
            if !name.is_empty() && !is_all_digits(&name) {
                let loc = node.location();
                self.check(&name, "symbol", loc.start_offset(), loc.end_offset());
            }
        }
        ruby_prism::visit_symbol_node(self, node);
    }
}

impl Cop for VariableNumber {
    fn name(&self) -> &'static str {
        COP_NAME
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
            cop: self,
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}
