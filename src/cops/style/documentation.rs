//! Style/Documentation - Checks for missing top-level documentation of classes and modules.
//!
//! Classes with no body are exempt. Namespace modules (containing only classes, modules,
//! constant definitions, or constant visibility declarations) are exempt.
//! Classes/modules with `#:nodoc:` are exempt, and `#:nodoc: all` exempts all children.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/documentation.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

const MSG: &str = "Missing top-level documentation comment for `%TYPE% %ID%`.";

/// Annotation keywords that don't count as documentation comments.
const ANNOTATION_KEYWORDS: &[&str] = &["TODO", "FIXME", "OPTIMIZE", "HACK", "REVIEW", "NOTE"];

pub struct Documentation {
    allowed_constants: HashSet<String>,
}

impl Documentation {
    pub fn new() -> Self {
        Self {
            allowed_constants: HashSet::new(),
        }
    }

    pub fn with_allowed_constants(allowed: Vec<String>) -> Self {
        Self {
            allowed_constants: allowed.into_iter().collect(),
        }
    }
}

impl Default for Documentation {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about a class/module node needed for documentation checking.
struct ClassModuleInfo {
    /// "class" or "module"
    kind: &'static str,
    /// Start byte offset of the keyword (class/module)
    keyword_start: usize,
    /// End byte offset of the constant name
    name_end: usize,
    /// The short name (last segment) of the constant
    short_name: String,
    /// Whether the name uses compact style (contains `::`)
    is_compact: bool,
    /// Whether the body is empty (no statements)
    has_body: bool,
    /// Whether this is a namespace (body contains only classes/modules/constants)
    is_namespace: bool,
    /// Whether the body is only include/extend/prepend statements
    is_include_only: bool,
}

impl Cop for Documentation {
    fn name(&self) -> &'static str {
        "Style/Documentation"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut comment_list: Vec<CommentInfo> = Vec::new();
        for c in result.comments() {
            let loc = c.location();
            let text = &ctx.source[loc.start_offset()..loc.end_offset()];
            let line = line_at_offset(ctx.source, loc.start_offset());
            comment_list.push(CommentInfo {
                text: text.to_string(),
                line,
                start_offset: loc.start_offset(),
            });
        }

        let mut visitor = DocVisitor {
            cop: self,
            source: ctx.source,
            filename: ctx.filename,
            comments: &comment_list,
            offenses: Vec::new(),
            nodoc_all_depth: 0,
            ancestor_names: Vec::new(),
        };
        visitor.visit(&result.node());
        visitor.offenses
    }
}

struct CommentInfo {
    text: String,
    line: usize,
    start_offset: usize,
}

/// Compute 1-indexed line number from a byte offset.
fn line_at_offset(source: &str, offset: usize) -> usize {
    let mut line = 1usize;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
        }
    }
    line
}

struct DocVisitor<'a> {
    cop: &'a Documentation,
    source: &'a str,
    filename: &'a str,
    comments: &'a [CommentInfo],
    offenses: Vec<Offense>,
    /// Depth of `:nodoc: all` nesting. When > 0, all children are exempt.
    nodoc_all_depth: usize,
    /// Stack of ancestor class/module names for building fully qualified identifiers.
    ancestor_names: Vec<String>,
}

impl<'a> DocVisitor<'a> {
    fn check_node(&mut self, info: &ClassModuleInfo, keyword_start: usize, name_end: usize) {
        // Classes without bodies are exempt
        if info.kind == "class" && !info.has_body {
            return;
        }

        // Namespace modules/classes are exempt
        if info.is_namespace {
            return;
        }

        // Include-only modules are exempt
        if info.is_include_only {
            return;
        }

        // Check if the constant is in the allowed list
        if self.cop.allowed_constants.contains(&info.short_name) {
            return;
        }

        // Check for `:nodoc: all` from ancestors
        if self.nodoc_all_depth > 0 {
            return;
        }

        // Check for `:nodoc:` on this node
        if self.has_nodoc(keyword_start) {
            return;
        }

        // Check for `:nodoc:` on compact namespace parent
        if info.is_compact {
            // For compact style like `A::B`, check if there's a `:nodoc:` comment
            // on any outer module that contains `::` in a parent scope
            if self.has_nodoc_on_compact_parent(keyword_start) {
                return;
            }
        }

        // Check for documentation comment
        if self.has_documentation_comment(keyword_start) {
            return;
        }

        // Build fully qualified identifier
        let identifier = self.build_identifier(&info.short_name, info.is_compact, keyword_start, name_end);

        let message = MSG
            .replace("%TYPE%", info.kind)
            .replace("%ID%", &identifier);

        let location =
            crate::offense::Location::from_offsets(self.source, keyword_start, name_end);
        self.offenses.push(Offense::new(
            "Style/Documentation",
            message,
            Severity::Convention,
            location,
            self.filename,
        ));
    }

    fn build_identifier(&self, short_name: &str, is_compact: bool, keyword_start: usize, name_end: usize) -> String {
        let mut parts: Vec<String> = self.ancestor_names.clone();
        if is_compact {
            // For compact style, the name source already includes `::`
            let name_src = &self.source[keyword_start..name_end];
            // Extract everything after "class " or "module "
            let const_part = if let Some(pos) = name_src.find(' ') {
                name_src[pos + 1..].trim()
            } else {
                short_name
            };
            parts.push(const_part.to_string());
        } else {
            parts.push(short_name.to_string());
        }
        parts.join("::")
    }

    /// Check if there's a `:nodoc:` comment on the same line as the keyword.
    fn has_nodoc(&self, keyword_start: usize) -> bool {
        let node_line = line_at_offset(self.source, keyword_start);
        self.comments.iter().any(|c| {
            c.line == node_line && is_nodoc(&c.text, false)
        })
    }

    /// Check if there's a `:nodoc:` on a compact parent module.
    fn has_nodoc_on_compact_parent(&self, _keyword_start: usize) -> bool {
        // For compact style `A::B`, we don't check parent modules
        // RuboCop checks outer_module which looks for (const (const nil? _) _)
        // In practice, compact-style with :nodoc: on same line is handled by has_nodoc
        false
    }

    /// Check if there's a `:nodoc: all` comment on the same line.
    fn has_nodoc_all(&self, keyword_start: usize) -> bool {
        let node_line = line_at_offset(self.source, keyword_start);
        self.comments.iter().any(|c| {
            c.line == node_line && is_nodoc(&c.text, true)
        })
    }

    /// Check if a comment is on its own line (not an inline comment after code).
    fn is_comment_line(&self, comment: &CommentInfo) -> bool {
        // Find the start of the line containing this comment
        let line_start = if comment.start_offset == 0 {
            0
        } else {
            self.source[..comment.start_offset]
                .rfind('\n')
                .map_or(0, |p| p + 1)
        };
        // Check that everything before the comment on this line is whitespace
        self.source[line_start..comment.start_offset]
            .chars()
            .all(|c| c.is_ascii_whitespace())
    }

    /// Check if there's a documentation comment preceding this node.
    fn has_documentation_comment(&self, keyword_start: usize) -> bool {
        let node_line = line_at_offset(self.source, keyword_start);

        // Collect comments that are directly preceding the node (contiguous block)
        // Only consider comment-only lines (not inline comments after code)
        let mut preceding: Vec<&CommentInfo> = Vec::new();
        for comment in self.comments.iter().rev() {
            if comment.line >= node_line {
                continue;
            }
            // Skip inline comments (comments on the same line as code)
            if !self.is_comment_line(comment) {
                continue;
            }
            if preceding.is_empty() {
                // First comment before the node must be on the immediately preceding line
                if comment.line + 1 == node_line {
                    preceding.push(comment);
                } else {
                    break;
                }
            } else {
                // Subsequent comments must be contiguous
                let last_line = preceding.last().unwrap().line;
                if comment.line + 1 == last_line {
                    preceding.push(comment);
                } else {
                    break;
                }
            }
        }

        if preceding.is_empty() {
            return false;
        }

        // At least one comment in the block must not be an annotation, directive, or magic comment
        preceding.iter().any(|c| {
            !is_annotation_comment(&c.text)
                && !is_interpreter_directive(&c.text)
                && !is_rubocop_directive(&c.text)
        })
    }

    /// Extract the constant name from a class/module node's constant path.
    fn const_name_from_node(source: &str, constant_path: &Node) -> String {
        let loc = constant_path.location();
        source[loc.start_offset()..loc.end_offset()].to_string()
    }

    /// Get the short name (last segment after `::`) from a constant path.
    fn short_name(full_name: &str) -> String {
        if let Some(pos) = full_name.rfind("::") {
            full_name[pos + 2..].to_string()
        } else {
            full_name.to_string()
        }
    }

    /// Check if a constant path contains `::` (compact namespace style).
    fn is_compact_name(name: &str) -> bool {
        name.contains("::")
    }
}

/// Check if a body node represents a namespace (only contains classes, modules, constant defs).
fn is_namespace(body: Option<&Node>) -> bool {
    let body = match body {
        Some(b) => b,
        None => return false,
    };

    match body {
        Node::StatementsNode { .. } => {
            let stmts = body.as_statements_node().unwrap();
            stmts.body().iter().all(|child| is_constant_declaration(&child))
        }
        _ => is_constant_definition(body),
    }
}

/// Check if a node is a class, module, or constant assignment.
fn is_constant_declaration(node: &Node) -> bool {
    is_constant_definition(node) || is_constant_visibility_declaration(node)
}

/// Check if a node is a class, module, or casgn.
fn is_constant_definition(node: &Node) -> bool {
    matches!(
        node,
        Node::ClassNode { .. } | Node::ModuleNode { .. } | Node::ConstantWriteNode { .. } | Node::ConstantPathWriteNode { .. }
    )
}

/// Check if a node is a constant visibility declaration like `private_constant :Foo`.
fn is_constant_visibility_declaration(node: &Node) -> bool {
    if let Node::CallNode { .. } = node {
        let call = node.as_call_node().unwrap();
        let name = String::from_utf8_lossy(call.name().as_slice());
        if name == "public_constant" || name == "private_constant" {
            if call.receiver().is_none() {
                if let Some(args) = call.arguments() {
                    let arg_list: Vec<_> = args.arguments().iter().collect();
                    if arg_list.len() == 1 {
                        return matches!(
                            &arg_list[0],
                            Node::SymbolNode { .. } | Node::StringNode { .. }
                        );
                    }
                }
            }
        }
    }
    false
}

/// Check if a body consists only of include/extend/prepend statements.
fn is_include_only(body: Option<&Node>) -> bool {
    let body = match body {
        Some(b) => b,
        None => return false,
    };

    match body {
        Node::StatementsNode { .. } => {
            let stmts = body.as_statements_node().unwrap();
            stmts.body().iter().all(|child| is_include_statement(&child))
        }
        _ => is_include_statement(body),
    }
}

/// Check if a node is an include/extend/prepend call.
fn is_include_statement(node: &Node) -> bool {
    if let Node::CallNode { .. } = node {
        let call = node.as_call_node().unwrap();
        let name = String::from_utf8_lossy(call.name().as_slice());
        if matches!(name.as_ref(), "include" | "extend" | "prepend") {
            if call.receiver().is_none() {
                if let Some(args) = call.arguments() {
                    let arg_list: Vec<_> = args.arguments().iter().collect();
                    if arg_list.len() == 1 {
                        return matches!(
                            &arg_list[0],
                            Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. }
                        );
                    }
                }
            }
        }
    }
    false
}

/// Check if comment text matches `:nodoc:` pattern.
fn is_nodoc(text: &str, require_all: bool) -> bool {
    let trimmed = text.trim_start_matches('#').trim_start();
    if require_all {
        trimmed.starts_with(":nodoc:") && trimmed.contains("all")
    } else {
        trimmed.starts_with(":nodoc:")
    }
}

/// Check if a comment is an annotation comment (TODO, FIXME, etc.).
fn is_annotation_comment(text: &str) -> bool {
    let content = text.trim_start_matches('#').trim_start();
    ANNOTATION_KEYWORDS.iter().any(|kw| {
        content.starts_with(kw) && {
            let after = &content[kw.len()..];
            after.is_empty() || after.starts_with(':') || after.starts_with(' ')
        }
    })
}

/// Check if a comment is an interpreter directive (frozen_string_literal, encoding).
fn is_interpreter_directive(text: &str) -> bool {
    let content = text.trim_start_matches('#').trim_start();
    content.starts_with("frozen_string_literal:") || content.starts_with("encoding:")
}

/// Check if a comment is a RuboCop directive (rubocop:disable, etc.).
fn is_rubocop_directive(text: &str) -> bool {
    let content = text.trim_start_matches('#').trim_start();
    content.starts_with("rubocop:")
}

impl Visit<'_> for DocVisitor<'_> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        let constant_path = node.constant_path();
        let full_name = DocVisitor::const_name_from_node(self.source, &constant_path);
        let short_name = DocVisitor::short_name(&full_name);
        let is_compact = DocVisitor::is_compact_name(&full_name);

        let keyword_start = node.location().start_offset();
        let name_end = constant_path.location().end_offset();

        let body_node = node.body();
        let has_body = body_node.is_some();
        let is_ns = is_namespace(body_node.as_ref());
        let is_inc_only = is_include_only(body_node.as_ref());

        // Check for :nodoc: all
        let had_nodoc_all = self.has_nodoc_all(keyword_start);
        if had_nodoc_all {
            self.nodoc_all_depth += 1;
        }

        let info = ClassModuleInfo {
            kind: "class",
            keyword_start,
            name_end,
            short_name,
            is_compact,
            has_body,
            is_namespace: is_ns,
            is_include_only: is_inc_only,
        };

        self.check_node(&info, keyword_start, name_end);

        // Push ancestor name for nested classes/modules
        self.ancestor_names.push(full_name);
        ruby_prism::visit_class_node(self, node);
        self.ancestor_names.pop();

        if had_nodoc_all {
            self.nodoc_all_depth -= 1;
        }
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        let constant_path = node.constant_path();
        let full_name = DocVisitor::const_name_from_node(self.source, &constant_path);
        let short_name = DocVisitor::short_name(&full_name);
        let is_compact = DocVisitor::is_compact_name(&full_name);

        let keyword_start = node.location().start_offset();
        let name_end = constant_path.location().end_offset();

        let body_node = node.body();
        // Modules with empty body still need documentation (unlike classes)
        let has_body = true;
        let is_ns = is_namespace(body_node.as_ref());
        let is_inc_only = is_include_only(body_node.as_ref());

        // Check for :nodoc: all
        let had_nodoc_all = self.has_nodoc_all(keyword_start);
        if had_nodoc_all {
            self.nodoc_all_depth += 1;
        }

        let info = ClassModuleInfo {
            kind: "module",
            keyword_start,
            name_end,
            short_name,
            is_compact,
            has_body,
            is_namespace: is_ns,
            is_include_only: is_inc_only,
        };

        self.check_node(&info, keyword_start, name_end);

        // Push ancestor name for nested classes/modules
        self.ancestor_names.push(full_name);
        ruby_prism::visit_module_node(self, node);
        self.ancestor_names.pop();

        if had_nodoc_all {
            self.nodoc_all_depth -= 1;
        }
    }
}
