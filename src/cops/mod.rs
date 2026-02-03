pub mod layout;
pub mod lint;
pub mod metrics;
pub mod style;

use crate::offense::{Location, Offense, Severity};
use ruby_prism::{ParseResult, Visit};

/// Context passed to cops during checking
pub struct CheckContext<'a> {
    pub source: &'a str,
    pub filename: &'a str,
    /// Target Ruby version (e.g., 2.5, 3.0, 3.2)
    pub target_ruby_version: f64,
}

impl<'a> CheckContext<'a> {
    pub fn new(source: &'a str, filename: &'a str) -> Self {
        Self {
            source,
            filename,
            target_ruby_version: 2.5, // Default to oldest supported
        }
    }

    pub fn with_ruby_version(source: &'a str, filename: &'a str, target_ruby_version: f64) -> Self {
        Self {
            source,
            filename,
            target_ruby_version,
        }
    }

    /// Check if target Ruby version is at least the given version
    pub fn ruby_version_at_least(&self, major: u32, minor: u32) -> bool {
        let required = major as f64 + (minor as f64 / 10.0);
        self.target_ruby_version >= required
    }

    /// Create a Location from a Prism node location
    pub fn location(&self, loc: &ruby_prism::Location) -> Location {
        Location::from_offsets(self.source, loc.start_offset(), loc.end_offset())
    }

    /// Create an offense
    pub fn offense(
        &self,
        cop_name: &str,
        message: &str,
        severity: Severity,
        loc: &ruby_prism::Location,
    ) -> Offense {
        Offense::new(cop_name, message, severity, self.location(loc), self.filename)
    }
}

/// Trait that all cops must implement
pub trait Cop: Send + Sync {
    /// The name of the cop (e.g., "Lint/Debugger")
    fn name(&self) -> &'static str;

    /// Default severity for this cop
    fn severity(&self) -> Severity {
        Severity::Convention
    }

    /// Check a CallNode (method call)
    fn check_call(&self, _node: &ruby_prism::CallNode, _ctx: &CheckContext) -> Vec<Offense> {
        vec![]
    }

    /// Check a DefNode (method definition)
    fn check_def(&self, _node: &ruby_prism::DefNode, _ctx: &CheckContext) -> Vec<Offense> {
        vec![]
    }

    /// Check a ClassNode
    fn check_class(&self, _node: &ruby_prism::ClassNode, _ctx: &CheckContext) -> Vec<Offense> {
        vec![]
    }

    /// Check a ModuleNode
    fn check_module(&self, _node: &ruby_prism::ModuleNode, _ctx: &CheckContext) -> Vec<Offense> {
        vec![]
    }

    /// Check a StringNode
    fn check_string(&self, _node: &ruby_prism::StringNode, _ctx: &CheckContext) -> Vec<Offense> {
        vec![]
    }

    /// Check a SymbolNode
    fn check_symbol(&self, _node: &ruby_prism::SymbolNode, _ctx: &CheckContext) -> Vec<Offense> {
        vec![]
    }

    /// Check an IfNode
    fn check_if(&self, _node: &ruby_prism::IfNode, _ctx: &CheckContext) -> Vec<Offense> {
        vec![]
    }

    /// Check a LocalVariableWriteNode
    fn check_local_variable_write(
        &self,
        _node: &ruby_prism::LocalVariableWriteNode,
        _ctx: &CheckContext,
    ) -> Vec<Offense> {
        vec![]
    }

    /// Check an ArrayNode
    fn check_array(&self, _node: &ruby_prism::ArrayNode, _ctx: &CheckContext) -> Vec<Offense> {
        vec![]
    }

    /// Check a HashNode
    fn check_hash(&self, _node: &ruby_prism::HashNode, _ctx: &CheckContext) -> Vec<Offense> {
        vec![]
    }

    /// Check a BlockNode
    fn check_block(&self, _node: &ruby_prism::BlockNode, _ctx: &CheckContext) -> Vec<Offense> {
        vec![]
    }

    /// Check the entire program (for file-level checks like frozen string literal)
    fn check_program(
        &self,
        _node: &ruby_prism::ProgramNode,
        _ctx: &CheckContext,
    ) -> Vec<Offense> {
        vec![]
    }
}

/// Visitor that runs all cops against the AST
struct CopRunner<'a> {
    cops: &'a [Box<dyn Cop>],
    ctx: CheckContext<'a>,
    offenses: Vec<Offense>,
}

impl<'a> CopRunner<'a> {
    fn new(cops: &'a [Box<dyn Cop>], ctx: CheckContext<'a>) -> Self {
        Self {
            cops,
            ctx,
            offenses: Vec::new(),
        }
    }
}

impl Visit<'_> for CopRunner<'_> {
    fn visit_program_node(&mut self, node: &ruby_prism::ProgramNode) {
        for cop in self.cops {
            self.offenses.extend(cop.check_program(node, &self.ctx));
        }
        ruby_prism::visit_program_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        for cop in self.cops {
            self.offenses.extend(cop.check_call(node, &self.ctx));
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        for cop in self.cops {
            self.offenses.extend(cop.check_def(node, &self.ctx));
        }
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        for cop in self.cops {
            self.offenses.extend(cop.check_class(node, &self.ctx));
        }
        ruby_prism::visit_class_node(self, node);
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        for cop in self.cops {
            self.offenses.extend(cop.check_module(node, &self.ctx));
        }
        ruby_prism::visit_module_node(self, node);
    }

    fn visit_string_node(&mut self, node: &ruby_prism::StringNode) {
        for cop in self.cops {
            self.offenses.extend(cop.check_string(node, &self.ctx));
        }
        ruby_prism::visit_string_node(self, node);
    }

    fn visit_symbol_node(&mut self, node: &ruby_prism::SymbolNode) {
        for cop in self.cops {
            self.offenses.extend(cop.check_symbol(node, &self.ctx));
        }
        ruby_prism::visit_symbol_node(self, node);
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        for cop in self.cops {
            self.offenses.extend(cop.check_if(node, &self.ctx));
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        for cop in self.cops {
            self.offenses
                .extend(cop.check_local_variable_write(node, &self.ctx));
        }
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        for cop in self.cops {
            self.offenses.extend(cop.check_array(node, &self.ctx));
        }
        ruby_prism::visit_array_node(self, node);
    }

    fn visit_hash_node(&mut self, node: &ruby_prism::HashNode) {
        for cop in self.cops {
            self.offenses.extend(cop.check_hash(node, &self.ctx));
        }
        ruby_prism::visit_hash_node(self, node);
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        for cop in self.cops {
            self.offenses.extend(cop.check_block(node, &self.ctx));
        }
        ruby_prism::visit_block_node(self, node);
    }
}

/// Run all cops against a parse result
pub fn run_cops(
    cops: &[Box<dyn Cop>],
    result: &ParseResult<'_>,
    source: &str,
    filename: &str,
) -> Vec<Offense> {
    run_cops_with_version(cops, result, source, filename, 2.5)
}

/// Run all cops against a parse result with a specific Ruby version
pub fn run_cops_with_version(
    cops: &[Box<dyn Cop>],
    result: &ParseResult<'_>,
    source: &str,
    filename: &str,
    target_ruby_version: f64,
) -> Vec<Offense> {
    let ctx = CheckContext::with_ruby_version(source, filename, target_ruby_version);
    let mut runner = CopRunner::new(cops, ctx);
    runner.visit(&result.node());
    runner.offenses
}

/// Returns all available cops with default configuration
pub fn all() -> Vec<Box<dyn Cop>> {
    vec![
        // Lint
        Box::new(lint::Debugger::new()),
        Box::new(lint::AssignmentInCondition::new(false)), // User's config: AllowSafeAssignment: false
        // Layout
        Box::new(layout::LineLength::new(160)), // User's config: Max: 160
        // Metrics
        Box::new(metrics::BlockLength::new(50)), // User's config: Max: 50
        // Style
        Box::new(style::AutoResourceCleanup::new()),
        Box::new(style::FormatStringToken::new(style::FormatStringTokenStyle::Template)), // User's config
        Box::new(style::HashSyntax::new(style::HashSyntaxStyle::Ruby19NoMixedKeys)), // User's config
        Box::new(style::MethodCalledOnDoEndBlock::new()),
        Box::new(style::RaiseArgs::new(style::RaiseArgsStyle::Compact)), // User's config
        Box::new(style::RescueStandardError::new(style::RescueStandardErrorStyle::Implicit)), // User's config
        Box::new(style::StringMethods::new()),
    ]
}
