//! Style/GlobalVars - Looks for uses of global variables.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/global_vars.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;
use std::collections::HashSet;

const MSG: &str = "Do not introduce global variables.";

/// Built-in global variables and their English aliases.
/// https://www.zenspider.com/ruby/quickref.html
const BUILT_IN_VARS: &[&str] = &[
    "$:", "$LOAD_PATH",
    "$\"", "$LOADED_FEATURES",
    "$0", "$PROGRAM_NAME",
    "$!", "$ERROR_INFO",
    "$@", "$ERROR_POSITION",
    "$;", "$FS", "$FIELD_SEPARATOR",
    "$,", "$OFS", "$OUTPUT_FIELD_SEPARATOR",
    "$/", "$RS", "$INPUT_RECORD_SEPARATOR",
    "$\\", "$ORS", "$OUTPUT_RECORD_SEPARATOR",
    "$.", "$NR", "$INPUT_LINE_NUMBER",
    "$_", "$LAST_READ_LINE",
    "$>", "$DEFAULT_OUTPUT",
    "$<", "$DEFAULT_INPUT",
    "$$", "$PID", "$PROCESS_ID",
    "$?", "$CHILD_STATUS",
    "$~", "$LAST_MATCH_INFO",
    "$=", "$IGNORECASE",
    "$*", "$ARGV",
    "$&", "$MATCH",
    "$`", "$PREMATCH",
    "$'", "$POSTMATCH",
    "$+", "$LAST_PAREN_MATCH",
    "$stdin", "$stdout", "$stderr",
    "$DEBUG", "$FILENAME", "$VERBOSE", "$SAFE",
    "$-0", "$-a", "$-d", "$-F", "$-i", "$-I", "$-l", "$-p", "$-v", "$-w",
    "$CLASSPATH", "$JRUBY_VERSION", "$JRUBY_REVISION", "$ENV_JAVA",
];

#[derive(Default)]
pub struct GlobalVars {
    allowed_variables: HashSet<String>,
}

impl GlobalVars {
    pub fn new() -> Self {
        Self {
            allowed_variables: HashSet::new(),
        }
    }

    pub fn with_allowed_variables(allowed: Vec<String>) -> Self {
        Self {
            allowed_variables: allowed.into_iter().collect(),
        }
    }

    fn is_allowed(&self, name: &str) -> bool {
        // Backreferences like $1, $2, etc are not global variables
        if name.len() >= 2 && name.as_bytes()[1].is_ascii_digit() {
            return true;
        }
        BUILT_IN_VARS.contains(&name) || self.allowed_variables.contains(name)
    }
}

struct GlobalVarsVisitor<'a> {
    cop: &'a GlobalVars,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> GlobalVarsVisitor<'a> {
    fn check_global_var(&mut self, name: &[u8], loc: &ruby_prism::Location) {
        let name = String::from_utf8_lossy(name);
        if !self.cop.is_allowed(&name) {
            self.offenses
                .push(self.ctx.offense(self.cop.name(), MSG, self.cop.severity(), loc));
        }
    }
}

impl Visit<'_> for GlobalVarsVisitor<'_> {
    fn visit_global_variable_read_node(&mut self, node: &ruby_prism::GlobalVariableReadNode) {
        self.check_global_var(node.name().as_slice(), &node.location());
    }

    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode) {
        self.check_global_var(node.name().as_slice(), &node.name_loc());
        ruby_prism::visit_global_variable_write_node(self, node);
    }

    fn visit_global_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::GlobalVariableOperatorWriteNode,
    ) {
        self.check_global_var(node.name().as_slice(), &node.name_loc());
        ruby_prism::visit_global_variable_operator_write_node(self, node);
    }

    fn visit_global_variable_and_write_node(
        &mut self,
        node: &ruby_prism::GlobalVariableAndWriteNode,
    ) {
        self.check_global_var(node.name().as_slice(), &node.name_loc());
        ruby_prism::visit_global_variable_and_write_node(self, node);
    }

    fn visit_global_variable_or_write_node(
        &mut self,
        node: &ruby_prism::GlobalVariableOrWriteNode,
    ) {
        self.check_global_var(node.name().as_slice(), &node.name_loc());
        ruby_prism::visit_global_variable_or_write_node(self, node);
    }
}

impl Cop for GlobalVars {
    fn name(&self) -> &'static str {
        "Style/GlobalVars"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = GlobalVarsVisitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}
