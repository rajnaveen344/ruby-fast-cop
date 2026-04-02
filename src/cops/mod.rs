pub mod layout;
pub mod lint;
pub mod metrics;
pub mod naming;
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
            target_ruby_version: 2.7, // Matches RuboCop's TargetRuby::DEFAULT_VERSION
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

    // ── Location helpers (mirrors RuboCop's RangeHelp mixin) ──

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

    /// Create an offense with custom byte range
    pub fn offense_with_range(
        &self,
        cop_name: &str,
        message: &str,
        severity: Severity,
        start_offset: usize,
        end_offset: usize,
    ) -> Offense {
        let location = Location::from_offsets(self.source, start_offset, end_offset);
        Offense::new(cop_name, message, severity, location, self.filename)
    }

    // ── Source position helpers (mirrors RuboCop's Alignment / RangeHelp) ──

    /// Source bytes for efficient byte-level scanning.
    #[inline]
    pub fn bytes(&self) -> &[u8] {
        self.source.as_bytes()
    }

    /// 1-indexed line number at byte offset.
    #[inline]
    pub fn line_of(&self, offset: usize) -> usize {
        1 + self.source.as_bytes()[..offset].iter().filter(|&&b| b == b'\n').count()
    }

    /// 0-indexed column (byte-based) at byte offset.
    #[inline]
    pub fn col_of(&self, offset: usize) -> usize {
        let mut i = offset;
        while i > 0 && self.source.as_bytes()[i - 1] != b'\n' {
            i -= 1;
        }
        offset - i
    }

    /// Byte offset of the start of the line containing `offset`.
    #[inline]
    pub fn line_start(&self, offset: usize) -> usize {
        self.source[..offset].rfind('\n').map_or(0, |p| p + 1)
    }

    /// Whether `offset` is the first non-whitespace on its line.
    pub fn begins_its_line(&self, offset: usize) -> bool {
        let s = self.source.as_bytes();
        let mut i = offset;
        while i > 0 {
            i -= 1;
            if s[i] == b'\n' { return true; }
            if s[i] != b' ' && s[i] != b'\t' { return false; }
        }
        true // start of file
    }

    /// Whether two byte offsets are on the same line.
    #[inline]
    pub fn same_line(&self, a: usize, b: usize) -> bool {
        !self.source.as_bytes()[a.min(b)..a.max(b)].contains(&b'\n')
    }

    /// Column of the first non-whitespace character on the line containing `offset`.
    pub fn indentation_of(&self, offset: usize) -> usize {
        let ls = self.line_start(offset);
        let s = self.source.as_bytes();
        let mut i = ls;
        while i < s.len() && (s[i] == b' ' || s[i] == b'\t') {
            i += 1;
        }
        i - ls
    }

    /// Extract the text of the line containing `offset` (without trailing newline).
    pub fn line_text(&self, offset: usize) -> &str {
        let start = self.line_start(offset);
        let end = self.source[start..].find('\n').map_or(self.source.len(), |p| start + p);
        &self.source[start..end]
    }

    /// Slice source bytes as &str.
    #[inline]
    pub fn src(&self, start: usize, end: usize) -> &str {
        &self.source[start..end]
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

    /// Check a WhileNode
    fn check_while(&self, _node: &ruby_prism::WhileNode, _ctx: &CheckContext) -> Vec<Offense> {
        vec![]
    }

    /// Check an UntilNode
    fn check_until(&self, _node: &ruby_prism::UntilNode, _ctx: &CheckContext) -> Vec<Offense> {
        vec![]
    }

    /// Check an UnlessNode
    fn check_unless(&self, _node: &ruby_prism::UnlessNode, _ctx: &CheckContext) -> Vec<Offense> {
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

    /// Check a KeywordHashNode (implicit hash in method arguments)
    fn check_keyword_hash(
        &self,
        _node: &ruby_prism::KeywordHashNode,
        _ctx: &CheckContext,
    ) -> Vec<Offense> {
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

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        for cop in self.cops {
            self.offenses.extend(cop.check_while(node, &self.ctx));
        }
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        for cop in self.cops {
            self.offenses.extend(cop.check_until(node, &self.ctx));
        }
        ruby_prism::visit_until_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        for cop in self.cops {
            self.offenses.extend(cop.check_unless(node, &self.ctx));
        }
        ruby_prism::visit_unless_node(self, node);
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

    fn visit_keyword_hash_node(&mut self, node: &ruby_prism::KeywordHashNode) {
        for cop in self.cops {
            self.offenses
                .extend(cop.check_keyword_hash(node, &self.ctx));
        }
        ruby_prism::visit_keyword_hash_node(self, node);
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
        Box::new(lint::AssignmentInCondition::new(false)), // User's config: AllowSafeAssignment: false
        Box::new(lint::Debugger::new()),
        Box::new(lint::DuplicateMethods::new()),
        Box::new(lint::LiteralAsCondition::new()),
        Box::new(lint::LiteralInInterpolation::new()),
        Box::new(lint::RedundantTypeConversion::new()),
        Box::new(lint::UnreachableCode::new()),
        Box::new(lint::UselessAccessModifier::new()),
        Box::new(lint::Void::new(false)),
        // Layout
        Box::new(layout::EmptyLinesAroundAccessModifier::new(layout::EmptyLinesAroundAccessModifierStyle::Around)),
        Box::new(layout::EndAlignment::new(layout::EndAlignmentStyle::Keyword)),
        Box::new(layout::IndentationWidth::new(2)),
        Box::new(layout::LeadingCommentSpace::new()),
        Box::new(layout::LineLength::new(160)), // User's config: Max: 160 (allow_uri=true by default)
        Box::new(layout::SpaceAfterComma::new()),
        Box::new(layout::MultilineMethodCallIndentation::new(layout::MultilineMethodCallIndentationStyle::Aligned, None)),
        Box::new(layout::SpaceInsidePercentLiteralDelimiters::new()),
        Box::new(layout::TrailingEmptyLines::new(layout::TrailingEmptyLinesStyle::FinalNewline)),
        Box::new(layout::TrailingWhitespace::new()),
        // Metrics
        Box::new(metrics::BlockLength::new(50)), // User's config: Max: 50
        Box::new(metrics::ClassLength::new(100)),
        Box::new(metrics::MethodLength::new(10)),
        // Naming
        Box::new(naming::MethodName::new()),
        Box::new(naming::PredicateMethod::new(naming::PredicateMethodMode::Conservative)),
        // Style
        Box::new(style::AccessModifierDeclarations::new(style::AccessModifierDeclarationsStyle::Group)),
        Box::new(style::AutoResourceCleanup::new()),
        Box::new(style::BlockDelimiters::new(style::BlockDelimitersStyle::LineCountBased)),
        Box::new(style::ConditionalAssignment::new(style::ConditionalAssignmentStyle::AssignInsideCondition)),
        Box::new(style::FormatStringToken::new(style::FormatStringTokenStyle::Template)), // User's config
        Box::new(style::FrozenStringLiteralComment::new(style::FrozenStringLiteralCommentStyle::Always)),
        Box::new(style::HashSyntax::new(style::HashSyntaxStyle::Ruby19NoMixedKeys)), // User's config
        Box::new(style::MethodCalledOnDoEndBlock::new()),
        Box::new(style::MutableConstant::new(style::MutableConstantStyle::Literals)),
        Box::new(style::NegativeArrayIndex::new()),
        Box::new(style::NumericLiterals::new(5)),
        Box::new(style::RaiseArgs::new(style::RaiseArgsStyle::Compact)), // User's config
        Box::new(style::RedundantParentheses::new()),
        Box::new(style::RedundantRegexpEscape::new()),
        Box::new(style::RedundantStringEscape::new()),
        Box::new(style::RescueStandardError::new(style::RescueStandardErrorStyle::Implicit)), // User's config
        Box::new(style::SafeNavigation::new()),
        Box::new(style::SelectByRegexp::new()),
        Box::new(style::Semicolon::new(false)),
        Box::new(style::StringLiterals::new(style::StringLiteralsStyle::SingleQuotes)),
        Box::new(style::StringMethods::new()),
        Box::new(style::TrailingCommaInArguments::new(style::TrailingCommaInArgumentsStyle::NoComma)),
    ]
}
