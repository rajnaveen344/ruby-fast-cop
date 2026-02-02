//! Extract RuboCop RSpec tests into YAML fixtures using Prism parser.
//!
//! This tool parses RSpec spec files and extracts test cases with their
//! expect_offense/expect_no_offenses/expect_correction blocks into YAML format.
//!
//! Usage:
//!   cargo run --bin extract-rubocop-tests -- --source /tmp/rubocop-specs/spec/rubocop/cop --output tests/fixtures

use glob::glob;
use ruby_prism::{CallNode, Node, Visit, visit_call_node};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Departments to process
const DEPARTMENTS: &[&str] = &[
    "lint",
    "style",
    "layout",
    "metrics",
    "naming",
    "bundler",
    "gemspec",
    "security",
    "internal_affairs",
    "migration",
];

/// Cops that are implemented in ruby-fast-cop
const IMPLEMENTED_COPS: &[&str] = &[
    "Lint/Debugger",
    "Lint/AssignmentInCondition",
    "Layout/LineLength",
    "Metrics/BlockLength",
    "Style/AutoResourceCleanup",
    "Style/FormatStringToken",
    "Style/HashSyntax",
    "Style/MethodCalledOnDoEndBlock",
    "Style/RaiseArgs",
    "Style/RescueStandardError",
    "Style/StringMethods",
];

/// A single test case extracted from RSpec
#[derive(Debug, Clone, Serialize)]
struct TestCase {
    name: String,
    source: String,
    offenses: Vec<Offense>,
    #[serde(skip_serializing_if = "Option::is_none")]
    corrected: Option<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    config: HashMap<String, serde_yaml::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ruby_version: Option<String>,
    /// True if source contains Ruby interpolation (#{...}) - requires manual sync
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    interpolated: bool,
}

/// An expected offense
#[derive(Debug, Clone, Serialize)]
struct Offense {
    line: u32,
    column_start: u32,
    column_end: u32,
    message: String,
}

/// Complete test file for a cop
#[derive(Debug, Serialize)]
struct CopTestFile {
    cop: String,
    department: String,
    severity: String,
    implemented: bool,
    tests: Vec<TestCase>,
}

/// Helper to get method name as string
fn get_method_name(node: &CallNode) -> String {
    String::from_utf8_lossy(node.name().as_slice()).to_string()
}

/// Helper to convert &[u8] to String
fn bytes_to_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

/// Visitor to extract test cases from RSpec
struct TestExtractor<'a> {
    source: &'a str,
    tests: Vec<TestCase>,
    context_stack: Vec<ContextInfo>,
    current_cop_config: HashMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone)]
struct ContextInfo {
    name: String,
    ruby_version: Option<String>,
}

impl<'a> TestExtractor<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            tests: Vec::new(),
            context_stack: Vec::new(),
            current_cop_config: HashMap::new(),
        }
    }

    /// Extract content from source using byte offsets
    fn slice(&self, start: usize, end: usize) -> &str {
        &self.source[start..end.min(self.source.len())]
    }

    /// Build test name from context stack
    fn build_test_name(&self, test_name: &str) -> String {
        let mut parts: Vec<String> = self
            .context_stack
            .iter()
            .map(|c| {
                c.name
                    .chars()
                    .map(|ch| {
                        if ch.is_alphanumeric() {
                            ch.to_ascii_lowercase()
                        } else {
                            '_'
                        }
                    })
                    .collect::<String>()
            })
            .collect();

        let test_part: String = test_name
            .chars()
            .map(|ch| {
                if ch.is_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect();

        parts.push(test_part);

        // Clean up multiple underscores
        let joined = parts.join("__");
        let mut result = String::new();
        let mut prev_underscore = false;
        for ch in joined.chars() {
            if ch == '_' {
                if !prev_underscore {
                    result.push(ch);
                }
                prev_underscore = true;
            } else {
                result.push(ch);
                prev_underscore = false;
            }
        }
        result.trim_matches('_').to_string()
    }

    /// Get current Ruby version from context stack
    fn current_ruby_version(&self) -> Option<String> {
        for ctx in self.context_stack.iter().rev() {
            if ctx.ruby_version.is_some() {
                return ctx.ruby_version.clone();
            }
        }
        None
    }

    /// Extract cop_config from a let block body
    fn extract_cop_config(&mut self, body: &Node) {
        // The body should contain a HashNode
        match body {
            Node::HashNode { .. } => {
                let hash = body.as_hash_node().unwrap();
                self.parse_hash_to_config(&hash);
            }
            Node::StatementsNode { .. } => {
                // Statements node - look for hash inside
                let stmts = body.as_statements_node().unwrap();
                for stmt in stmts.body().iter() {
                    if let Node::HashNode { .. } = stmt {
                        let hash = stmt.as_hash_node().unwrap();
                        self.parse_hash_to_config(&hash);
                        break;
                    }
                }
            }
            _ => {}
        }
    }

    /// Parse a Ruby HashNode into cop_config HashMap
    fn parse_hash_to_config(&mut self, hash: &ruby_prism::HashNode) {
        for element in hash.elements().iter() {
            if let Node::AssocNode { .. } = element {
                let assoc = element.as_assoc_node().unwrap();

                // Get key (usually a string like 'EnforcedStyle')
                let key = match assoc.key() {
                    Node::StringNode { .. } => {
                        Some(bytes_to_string(assoc.key().as_string_node().unwrap().unescaped()))
                    }
                    Node::SymbolNode { .. } => {
                        Some(bytes_to_string(assoc.key().as_symbol_node().unwrap().unescaped()))
                    }
                    _ => None,
                };

                // Get value
                let value = self.node_to_yaml_value(&assoc.value());

                if let (Some(k), Some(v)) = (key, value) {
                    self.current_cop_config.insert(k, v);
                }
            }
        }
    }

    /// Convert a Ruby AST node to a serde_yaml::Value
    fn node_to_yaml_value(&self, node: &Node) -> Option<serde_yaml::Value> {
        match node {
            Node::StringNode { .. } => {
                let s = node.as_string_node().unwrap();
                Some(serde_yaml::Value::String(bytes_to_string(s.unescaped())))
            }
            Node::SymbolNode { .. } => {
                let s = node.as_symbol_node().unwrap();
                Some(serde_yaml::Value::String(bytes_to_string(s.unescaped())))
            }
            Node::IntegerNode { .. } => {
                // Extract integer value from source
                let loc = node.location();
                let src = self.slice(loc.start_offset(), loc.end_offset());
                src.parse::<i64>().ok().map(|n| serde_yaml::Value::Number(n.into()))
            }
            Node::TrueNode { .. } => Some(serde_yaml::Value::Bool(true)),
            Node::FalseNode { .. } => Some(serde_yaml::Value::Bool(false)),
            Node::NilNode { .. } => Some(serde_yaml::Value::Null),
            Node::ArrayNode { .. } => {
                let arr = node.as_array_node().unwrap();
                let values: Vec<serde_yaml::Value> = arr.elements()
                    .iter()
                    .filter_map(|el| self.node_to_yaml_value(&el))
                    .collect();
                Some(serde_yaml::Value::Sequence(values))
            }
            _ => {
                // For complex expressions, use source text
                let loc = node.location();
                Some(serde_yaml::Value::String(self.slice(loc.start_offset(), loc.end_offset()).to_string()))
            }
        }
    }

    /// Extract string content from a node
    fn extract_string_from_node(&self, node: &Node) -> Option<String> {
        match node {
            Node::InterpolatedStringNode { .. } => {
                let n = node.as_interpolated_string_node().unwrap();
                let opening = n.opening_loc()?;
                let closing = n.closing_loc()?;
                let start = opening.end_offset();
                let end = closing.start_offset();
                let content = self.slice(start, end);

                let opening_str = self.slice(opening.start_offset(), opening.end_offset());
                if opening_str.contains('~') {
                    Some(process_squiggly_heredoc(content))
                } else {
                    Some(content.to_string())
                }
            }
            Node::StringNode { .. } => {
                let n = node.as_string_node().unwrap();
                Some(bytes_to_string(n.unescaped()))
            }
            _ => None,
        }
    }

    /// Parse offense markers from expect_offense heredoc (no regex)
    ///
    /// RuboCop test format supports:
    /// - `^^^` - caret markers for highlighting
    /// - `___` - underscore markers for skipping/alignment
    /// - `_{variable}` - underscores equal to variable length
    /// - `^{variable}` - carets equal to variable length
    /// - `#{'_' * N}` - Ruby interpolation for N underscores
    /// - `#{'^' * N}` - Ruby interpolation for N carets
    fn parse_offense_content(content: &str) -> (String, Vec<Offense>) {
        let lines: Vec<&str> = content.lines().collect();
        let mut source_lines = Vec::new();
        let mut offenses = Vec::new();
        let mut i = 0;

        /// Check if a line is a marker line, returns (prefix_len, caret_len, message)
        /// prefix_len and caret_len are None if they contain variable placeholders
        fn parse_marker_line(line: &str) -> Option<(Option<usize>, Option<usize>, String)> {
            let chars: Vec<char> = line.chars().collect();
            let mut pos = 0;
            let mut prefix_len: Option<usize> = Some(0);
            let mut caret_len: Option<usize> = Some(0);

            // Skip prefix: spaces, underscores, _{var}, #{'_' * N}
            while pos < chars.len() {
                match chars[pos] {
                    ' ' | '\t' => {
                        if let Some(ref mut len) = prefix_len {
                            *len += 1;
                        }
                        pos += 1;
                    }
                    '_' => {
                        // Check for _{variable}
                        if pos + 1 < chars.len() && chars[pos + 1] == '{' {
                            // Find closing }
                            let start = pos;
                            pos += 2;
                            while pos < chars.len() && chars[pos] != '}' {
                                pos += 1;
                            }
                            if pos < chars.len() {
                                pos += 1; // skip }
                            }
                            prefix_len = None; // variable length prefix
                            let _ = start; // silence unused warning
                        } else {
                            if let Some(ref mut len) = prefix_len {
                                *len += 1;
                            }
                            pos += 1;
                        }
                    }
                    '#' => {
                        // Check for #{'_' * N} - underscore interpolation in prefix
                        if pos + 2 < chars.len() && chars[pos + 1] == '{' && chars[pos + 2] == '\'' {
                            let start = pos;
                            // Find closing }
                            let mut temp_pos = pos + 3;
                            while temp_pos < chars.len() && chars[temp_pos] != '}' {
                                temp_pos += 1;
                            }
                            if temp_pos < chars.len() {
                                temp_pos += 1;
                            }
                            // Check if it was underscore interpolation
                            let segment: String = chars[start..temp_pos].iter().collect();
                            if segment.contains("'_'") {
                                prefix_len = None;
                                pos = temp_pos;
                            } else {
                                // Not an underscore pattern - let caret section handle it
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    _ => break,
                }
            }

            // Now parse carets: ^, ^{var}, #{'^' * N}
            let caret_start = pos;
            let mut found_caret = false;
            while pos < chars.len() {
                match chars[pos] {
                    '^' => {
                        found_caret = true;
                        // Check for ^{variable}
                        if pos + 1 < chars.len() && chars[pos + 1] == '{' {
                            pos += 2;
                            while pos < chars.len() && chars[pos] != '}' {
                                pos += 1;
                            }
                            if pos < chars.len() {
                                pos += 1;
                            }
                            caret_len = None;
                        } else {
                            if let Some(ref mut len) = caret_len {
                                *len += 1;
                            }
                            pos += 1;
                        }
                    }
                    '#' => {
                        // Check for #{'^' * N}
                        if pos + 2 < chars.len() && chars[pos + 1] == '{' && chars[pos + 2] == '\'' {
                            let start = pos;
                            while pos < chars.len() && chars[pos] != '}' {
                                pos += 1;
                            }
                            if pos < chars.len() {
                                pos += 1;
                            }
                            let segment: String = chars[start..pos].iter().collect();
                            if segment.contains("'^'") {
                                found_caret = true;
                                caret_len = None;
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    _ => break,
                }
            }

            if !found_caret {
                return None;
            }

            // Must have space then message
            if pos >= chars.len() || chars[pos] != ' ' {
                return None;
            }

            // Skip space(s)
            while pos < chars.len() && chars[pos] == ' ' {
                pos += 1;
            }

            if pos >= chars.len() {
                return None;
            }

            let message: String = chars[pos..].iter().collect();
            let _ = caret_start; // silence unused warning

            Some((prefix_len, caret_len, message))
        }

        while i < lines.len() {
            let line = lines[i];

            // Check if next line is a marker
            if i + 1 < lines.len() {
                if let Some((prefix_len, caret_len, message)) = parse_marker_line(lines[i + 1]) {
                    source_lines.push(line.to_string());

                    let col_start = prefix_len.unwrap_or(0) as u32;
                    let col_end = match (prefix_len, caret_len) {
                        (Some(p), Some(c)) => (p + c) as u32,
                        _ => line.len() as u32, // variable length - use source line length
                    };

                    offenses.push(Offense {
                        line: source_lines.len() as u32,
                        column_start: col_start,
                        column_end: col_end,
                        message,
                    });
                    i += 2;
                    continue;
                }
            }

            // Check if current line is a standalone marker (for multiline offenses)
            if let Some((prefix_len, caret_len, message)) = parse_marker_line(line) {
                let last_source_line = source_lines.last().map(|s| s.as_str()).unwrap_or("");

                let col_start = prefix_len.unwrap_or(0) as u32;
                let col_end = match (prefix_len, caret_len) {
                    (Some(p), Some(c)) => (p + c) as u32,
                    _ => last_source_line.len() as u32,
                };

                offenses.push(Offense {
                    line: source_lines.len() as u32,
                    column_start: col_start,
                    column_end: col_end,
                    message,
                });
                i += 1;
                continue;
            }

            source_lines.push(line.to_string());
            i += 1;
        }

        (source_lines.join("\n"), offenses)
    }
}

impl Visit<'_> for TestExtractor<'_> {
    fn visit_call_node(&mut self, node: &CallNode) {
        let method_name = get_method_name(node);

        match method_name.as_str() {
            "describe" | "context" | "RSpec" => {
                // Extract context name
                let context_name = if let Some(args) = node.arguments() {
                    args.arguments()
                        .iter()
                        .next()
                        .and_then(|arg| match arg {
                            Node::StringNode { .. } => {
                                Some(bytes_to_string(arg.as_string_node().unwrap().unescaped()))
                            }
                            Node::ConstantReadNode { .. } => Some(bytes_to_string(
                                arg.as_constant_read_node().unwrap().name().as_slice(),
                            )),
                            Node::ConstantPathNode { .. } => {
                                let loc = arg.location();
                                Some(self.slice(loc.start_offset(), loc.end_offset()).to_string())
                            }
                            _ => None,
                        })
                        .unwrap_or_default()
                } else {
                    String::new()
                };

                // Check for ruby version tag
                let ruby_version = if let Some(args) = node.arguments() {
                    args.arguments().iter().find_map(|arg| {
                        if let Node::SymbolNode { .. } = arg {
                            let s = arg.as_symbol_node().unwrap();
                            let sym = bytes_to_string(s.unescaped());
                            if sym.starts_with("ruby") {
                                let digits: String =
                                    sym.chars().filter(|c| c.is_ascii_digit()).collect();
                                if digits.len() >= 2 {
                                    return Some(format!(">= {}.{}", &digits[0..1], &digits[1..2]));
                                }
                            }
                        }
                        None
                    })
                } else {
                    None
                };

                self.context_stack.push(ContextInfo {
                    name: context_name,
                    ruby_version,
                });

                // Visit block
                if let Some(block) = node.block() {
                    self.visit(&Node::from(block));
                }

                self.context_stack.pop();
                return; // Don't call default visitor
            }

            "let" | "let!" => {
                // Check for cop_config
                if let Some(args) = node.arguments() {
                    if let Some(first_arg) = args.arguments().iter().next() {
                        if let Node::SymbolNode { .. } = first_arg {
                            let sym = first_arg.as_symbol_node().unwrap();
                            if bytes_to_string(sym.unescaped()) == "cop_config" {
                                // Extract the hash from the block body
                                if let Some(block_node) = node.block() {
                                    if let Node::BlockNode { .. } = &block_node {
                                        let block = block_node.as_block_node().unwrap();
                                        if let Some(body) = block.body() {
                                            self.extract_cop_config(&body);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            "it" | "specify" => {
                // Extract test name
                let test_name = if let Some(args) = node.arguments() {
                    args.arguments()
                        .iter()
                        .next()
                        .and_then(|arg| {
                            if let Node::StringNode { .. } = arg {
                                Some(bytes_to_string(arg.as_string_node().unwrap().unescaped()))
                            } else {
                                None
                            }
                        })
                        .unwrap_or_default()
                } else {
                    String::new()
                };

                // Check for ruby version tag
                let ruby_version_tag = if let Some(args) = node.arguments() {
                    args.arguments().iter().find_map(|arg| {
                        if let Node::SymbolNode { .. } = arg {
                            let s = arg.as_symbol_node().unwrap();
                            let sym = bytes_to_string(s.unescaped());
                            if sym.starts_with("ruby") {
                                let digits: String =
                                    sym.chars().filter(|c| c.is_ascii_digit()).collect();
                                if digits.len() >= 2 {
                                    return Some(format!(">= {}.{}", &digits[0..1], &digits[1..2]));
                                }
                            }
                        }
                        None
                    })
                } else {
                    None
                };

                // Extract test block content
                if let Some(block) = node.block() {
                    let mut finder = ExpectationFinder::new(self.source);
                    finder.visit(&Node::from(block));

                    if let Some(test_data) = finder.build_test() {
                        let test = TestCase {
                            name: self.build_test_name(&test_name),
                            source: test_data.source,
                            offenses: test_data.offenses,
                            corrected: test_data.corrected,
                            config: self.current_cop_config.clone(),
                            ruby_version: ruby_version_tag.or_else(|| self.current_ruby_version()),
                            interpolated: test_data.interpolated,
                        };

                        self.tests.push(test);
                    }
                }
                return; // Don't call default visitor
            }

            "it_behaves_like" | "include_examples" => {
                // Handle shared examples like:
                //   it_behaves_like 'misaligned', <<~RUBY, false
                //     begin
                //       end
                //       ^^^ message
                //   RUBY
                if let Some(args) = node.arguments() {
                    let args_list: Vec<_> = args.arguments().iter().collect();

                    // First arg is the shared example name
                    let shared_name = args_list.first().and_then(|arg| {
                        if let Node::StringNode { .. } = arg {
                            Some(bytes_to_string(arg.as_string_node().unwrap().unescaped()))
                        } else {
                            None
                        }
                    }).unwrap_or_default();

                    // Look for heredoc arguments (they contain the test content)
                    for arg in args_list.iter().skip(1) {
                        let (content, interpolated) = match arg {
                            Node::InterpolatedStringNode { .. } => {
                                let n = arg.as_interpolated_string_node().unwrap();
                                let mut content = String::new();
                                let mut has_interp = false;
                                for part in n.parts().iter() {
                                    match part {
                                        Node::StringNode { .. } => {
                                            let s = part.as_string_node().unwrap();
                                            content.push_str(&bytes_to_string(s.unescaped()));
                                        }
                                        Node::EmbeddedStatementsNode { .. } | Node::EmbeddedVariableNode { .. } => {
                                            has_interp = true;
                                            let loc = part.location();
                                            content.push_str(self.slice(loc.start_offset(), loc.end_offset()));
                                        }
                                        _ => {
                                            let loc = part.location();
                                            content.push_str(self.slice(loc.start_offset(), loc.end_offset()));
                                        }
                                    }
                                }
                                if content.ends_with('\n') {
                                    content.pop();
                                }
                                (Some(content), has_interp)
                            }
                            Node::StringNode { .. } => {
                                let s = arg.as_string_node().unwrap();
                                (Some(bytes_to_string(s.unescaped())), false)
                            }
                            _ => (None, false),
                        };

                        if let Some(content) = content {
                            // Parse the heredoc content for offenses
                            let (source, offenses) = Self::parse_offense_content(&content);
                            if !source.is_empty() {
                                let test = TestCase {
                                    name: self.build_test_name(&format!("{}_{}", shared_name, source.lines().next().unwrap_or("").chars().take(20).collect::<String>())),
                                    source,
                                    offenses,
                                    corrected: None,
                                    config: self.current_cop_config.clone(),
                                    ruby_version: self.current_ruby_version(),
                                    interpolated,
                                };
                                self.tests.push(test);
                            }
                        }
                    }
                }
                return; // Don't call default visitor
            }

            _ => {}
        }

        // Default: continue visiting
        visit_call_node(self, node);
    }
}

/// Finder for expect_offense/expect_no_offenses/expect_correction
struct ExpectationFinder<'a> {
    source: &'a str,
    offense_content: Option<(String, bool)>, // (content, has_interpolation)
    no_offense_content: Option<(String, bool)>,
    correction_content: Option<(String, bool)>,
}

struct TestData {
    source: String,
    offenses: Vec<Offense>,
    corrected: Option<String>,
    interpolated: bool,
}

impl<'a> ExpectationFinder<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            offense_content: None,
            no_offense_content: None,
            correction_content: None,
        }
    }

    fn has_any_interpolation(&self) -> bool {
        self.offense_content.as_ref().map_or(false, |(_, i)| *i)
            || self.no_offense_content.as_ref().map_or(false, |(_, i)| *i)
            || self.correction_content.as_ref().map_or(false, |(_, i)| *i)
    }

    fn slice(&self, start: usize, end: usize) -> &str {
        &self.source[start..end.min(self.source.len())]
    }

    /// Extract heredoc content and whether it contains interpolation
    fn extract_heredoc(&self, node: &CallNode) -> Option<(String, bool)> {
        let args = node.arguments()?;
        for arg in args.arguments().iter() {
            match arg {
                Node::InterpolatedStringNode { .. } => {
                    let n = arg.as_interpolated_string_node().unwrap();

                    // For heredocs, extract content from parts
                    let mut content = String::new();
                    let mut has_interpolation = false;

                    for part in n.parts().iter() {
                        match part {
                            Node::StringNode { .. } => {
                                let s = part.as_string_node().unwrap();
                                content.push_str(&bytes_to_string(s.unescaped()));
                            }
                            Node::EmbeddedStatementsNode { .. } => {
                                // For #{...} interpolations, extract the raw source including #{}
                                has_interpolation = true;
                                let loc = part.location();
                                content.push_str(self.slice(loc.start_offset(), loc.end_offset()));
                            }
                            Node::EmbeddedVariableNode { .. } => {
                                // For #@var or #$var interpolations
                                has_interpolation = true;
                                let loc = part.location();
                                content.push_str(self.slice(loc.start_offset(), loc.end_offset()));
                            }
                            _ => {
                                // Other node types - use source text
                                let loc = part.location();
                                content.push_str(self.slice(loc.start_offset(), loc.end_offset()));
                            }
                        }
                    }

                    // Trim trailing newline if present
                    if content.ends_with('\n') {
                        content.pop();
                    }

                    return Some((content, has_interpolation));
                }
                Node::StringNode { .. } => {
                    let s = arg.as_string_node().unwrap();
                    return Some((bytes_to_string(s.unescaped()), false));
                }
                _ => {}
            }
        }
        None
    }

    fn build_test(self) -> Option<TestData> {
        let interpolated = self.has_any_interpolation();

        if let Some((content, _)) = self.offense_content {
            let (source, offenses) = TestExtractor::parse_offense_content(&content);
            if source.is_empty() {
                return None;
            }
            return Some(TestData {
                source,
                offenses,
                corrected: self.correction_content.map(|(c, _)| c),
                interpolated,
            });
        }

        if let Some((content, _)) = self.no_offense_content {
            if content.is_empty() {
                return None;
            }
            return Some(TestData {
                source: content,
                offenses: vec![],
                corrected: None,
                interpolated,
            });
        }

        None
    }
}

impl Visit<'_> for ExpectationFinder<'_> {
    fn visit_call_node(&mut self, node: &CallNode) {
        let method_name = get_method_name(node);

        match method_name.as_str() {
            "expect_offense" => {
                if let Some(content) = self.extract_heredoc(node) {
                    self.offense_content = Some(content);
                }
            }
            "expect_no_offenses" => {
                if let Some(content) = self.extract_heredoc(node) {
                    self.no_offense_content = Some(content);
                }
            }
            "expect_correction" => {
                if let Some(content) = self.extract_heredoc(node) {
                    self.correction_content = Some(content);
                }
            }
            _ => {}
        }

        visit_call_node(self, node);
    }
}

/// Process squiggly heredoc content (remove common indentation)
fn process_squiggly_heredoc(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    // Find minimum indentation
    let min_indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);

    // Remove common indentation
    lines
        .iter()
        .map(|l| {
            if l.len() >= min_indent {
                &l[min_indent..]
            } else {
                l.trim_start()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Convert CamelCase to snake_case
fn snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

/// Map department directory name to cop namespace
fn department_namespace(dept: &str) -> &str {
    match dept {
        "lint" => "Lint",
        "style" => "Style",
        "layout" => "Layout",
        "metrics" => "Metrics",
        "naming" => "Naming",
        "bundler" => "Bundler",
        "gemspec" => "Gemspec",
        "security" => "Security",
        "internal_affairs" => "InternalAffairs",
        "migration" => "Migration",
        _ => dept,
    }
}

/// Get default severity for a department
fn default_severity(dept: &str) -> &str {
    match dept {
        "lint" | "security" => "warning",
        _ => "convention",
    }
}

/// Convert a serde_yaml::Value to a YAML string representation
fn yaml_value_to_string(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::String(s) => yaml_escape(s),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Null => "null".to_string(),
        serde_yaml::Value::Sequence(arr) => {
            let items: Vec<String> = arr.iter().map(yaml_value_to_string).collect();
            format!("[{}]", items.join(", "))
        }
        serde_yaml::Value::Mapping(map) => {
            let items: Vec<String> = map.iter()
                .map(|(k, v)| format!("{}: {}", yaml_value_to_string(k), yaml_value_to_string(v)))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
        _ => format!("{:?}", value),
    }
}

/// Escape a string for YAML output
fn yaml_escape(value: &str) -> String {
    let needs_quoting = value.is_empty()
        || value.starts_with(|c: char| " :@#[]{}|>&*!?,`'\"%".contains(c))
        || value.ends_with(|c: char| " :".contains(c))
        || value.contains('\n')
        || value.contains('\\')
        || value.contains('"')
        || value.contains('\'')
        || value.contains(':')
        || value.contains('#')
        || value.parse::<f64>().is_ok()
        || matches!(
            value.to_lowercase().as_str(),
            "true" | "false" | "null" | "yes" | "no" | "on" | "off"
        );

    if needs_quoting {
        if value.contains('\\') && !value.contains('\'') {
            format!("'{}'", value.replace('\'', "''"))
        } else {
            let escaped = value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\t', "\\t");
            format!("\"{}\"", escaped)
        }
    } else {
        value.to_string()
    }
}

/// Generate YAML output for a cop test file
fn generate_yaml(test_file: &CopTestFile) -> String {
    let mut lines = vec![
        format!("cop: {}", test_file.cop),
        format!("department: {}", test_file.department),
        format!("severity: {}", test_file.severity),
        format!("implemented: {}", test_file.implemented),
        String::new(),
        "tests:".to_string(),
    ];

    for test in &test_file.tests {
        lines.push(format!("  - name: {}", test.name));
        lines.push("    source: |".to_string());
        for line in test.source.lines() {
            lines.push(format!("      {}", line));
        }

        lines.push("    offenses:".to_string());
        if test.offenses.is_empty() {
            lines.push("      []".to_string());
        } else {
            for offense in &test.offenses {
                lines.push(format!("      - line: {}", offense.line));
                lines.push(format!("        column_start: {}", offense.column_start));
                lines.push(format!("        column_end: {}", offense.column_end));
                lines.push(format!(
                    "        message: {}",
                    yaml_escape(&offense.message)
                ));
            }
        }

        if let Some(corrected) = &test.corrected {
            lines.push("    corrected: |".to_string());
            for line in corrected.lines() {
                lines.push(format!("      {}", line));
            }
        }

        if !test.config.is_empty() {
            lines.push("    config:".to_string());
            for (key, value) in &test.config {
                let val_str = yaml_value_to_string(value);
                lines.push(format!("      {}: {}", key, val_str));
            }
        }

        if let Some(rv) = &test.ruby_version {
            lines.push(format!("    ruby_version: {}", yaml_escape(rv)));
        }

        if test.interpolated {
            lines.push("    interpolated: true".to_string());
        }

        lines.push(String::new());
    }

    lines.join("\n")
}

/// Process a single spec file
fn process_spec_file(spec_file: &Path, dept: &str, output_dir: &Path) -> Result<bool, String> {
    let cop_name = spec_file
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.trim_end_matches("_spec"))
        .ok_or("Invalid file name")?;

    // Convert snake_case to CamelCase
    let camel_cop_name: String = cop_name
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().chain(chars).collect(),
            }
        })
        .collect();

    let full_cop_name = format!("{}/{}", department_namespace(dept), camel_cop_name);

    let source = fs::read_to_string(spec_file)
        .map_err(|e| format!("Failed to read {}: {}", spec_file.display(), e))?;

    let result = ruby_prism::parse(source.as_bytes());
    let mut extractor = TestExtractor::new(&source);
    extractor.visit(&Node::from(result.node()));

    if extractor.tests.is_empty() {
        return Ok(false); // No tests found
    }

    let test_file = CopTestFile {
        cop: full_cop_name.clone(),
        department: dept.to_string(),
        severity: default_severity(dept).to_string(),
        implemented: IMPLEMENTED_COPS.contains(&full_cop_name.as_str()),
        tests: extractor.tests,
    };

    let yaml_content = generate_yaml(&test_file);

    let output_subdir = output_dir.join(dept);
    fs::create_dir_all(&output_subdir).map_err(|e| format!("Failed to create dir: {}", e))?;

    let yaml_file = output_subdir.join(format!("{}.yaml", cop_name));
    fs::write(&yaml_file, yaml_content)
        .map_err(|e| format!("Failed to write {}: {}", yaml_file.display(), e))?;

    println!(
        "  Created: {} ({} tests)",
        yaml_file.display(),
        test_file.tests.len()
    );

    Ok(true)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let mut source_dir = PathBuf::from("/tmp/rubocop-specs/spec/rubocop/cop");
    let mut output_dir = PathBuf::from("tests/fixtures");

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--source" => {
                i += 1;
                if i < args.len() {
                    source_dir = PathBuf::from(&args[i]);
                }
            }
            "--output" => {
                i += 1;
                if i < args.len() {
                    output_dir = PathBuf::from(&args[i]);
                }
            }
            "-h" | "--help" => {
                println!("Usage: extract-rubocop-tests [--source DIR] [--output DIR]");
                println!();
                println!("Options:");
                println!(
                    "  --source DIR   RuboCop specs directory (default: /tmp/rubocop-specs/spec/rubocop/cop)"
                );
                println!("  --output DIR   Output directory (default: tests/fixtures)");
                return;
            }
            _ => {}
        }
        i += 1;
    }

    if !source_dir.exists() {
        eprintln!(
            "Error: Source directory not found: {}",
            source_dir.display()
        );
        eprintln!("Run: .claude/skills/rubocop-test-importer/scripts/download_rubocop_specs.sh");
        std::process::exit(1);
    }

    println!("Extracting RuboCop tests using Prism parser...");
    println!("  Source: {}", source_dir.display());
    println!("  Output: {}", output_dir.display());
    println!();

    let mut stats = (0, 0, 0); // (created, skipped, errors)

    for dept in DEPARTMENTS {
        let spec_dir = source_dir.join(dept);
        if !spec_dir.is_dir() {
            continue;
        }

        let pattern = format!("{}/*_spec.rb", spec_dir.display());
        let spec_files: Vec<PathBuf> = glob(&pattern)
            .expect("Invalid glob pattern")
            .filter_map(|e| e.ok())
            .collect();

        if spec_files.is_empty() {
            continue;
        }

        println!("Processing {}...", department_namespace(dept));

        for spec_file in spec_files {
            match process_spec_file(&spec_file, dept, &output_dir) {
                Ok(true) => stats.0 += 1,
                Ok(false) => stats.1 += 1,
                Err(e) => {
                    eprintln!("  ERROR: {}: {}", spec_file.display(), e);
                    stats.2 += 1;
                }
            }
        }
    }

    println!();
    println!("Summary:");
    println!("  Created: {}", stats.0);
    println!("  Skipped: {}", stats.1);
    println!("  Errors:  {}", stats.2);
}
