//! Style/DocumentationMethod - Missing documentation comment for public methods.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/documentation_method.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Location, Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

const MSG: &str = "Missing method documentation comment.";
const ANNOTATION_KEYWORDS: &[&str] = &["TODO", "FIXME", "OPTIMIZE", "HACK", "REVIEW", "NOTE"];

#[derive(Default)]
pub struct DocumentationMethod {
    require_for_non_public_methods: bool,
    allowed_methods: HashSet<String>,
    annotation_keywords: Vec<String>,
}

impl DocumentationMethod {
    pub fn new(require_for_non_public_methods: bool, allowed_methods: Vec<String>, annotation_keywords: Vec<String>) -> Self {
        Self {
            require_for_non_public_methods,
            allowed_methods: allowed_methods.into_iter().collect(),
            annotation_keywords,
        }
    }
}

impl Cop for DocumentationMethod {
    fn name(&self) -> &'static str { "Style/DocumentationMethod" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut comments: Vec<CommentInfo> = Vec::new();
        for c in result.comments() {
            let loc = c.location();
            let text = ctx.source[loc.start_offset()..loc.end_offset()].to_string();
            let line = line_at_offset(ctx.source, loc.start_offset());
            comments.push(CommentInfo { text, line, start_offset: loc.start_offset() });
        }
        let mut visitor = Visitor {
            cop: self,
            source: ctx.source,
            filename: ctx.filename,
            comments: &comments,
            offenses: Vec::new(),
            scope_stack: vec![Scope::default()],
        };
        visitor.visit(&result.node());
        visitor.offenses
    }
}

#[derive(Default, Clone)]
struct Scope {
    /// Current visibility ("public" / "private" / "protected") for siblings.
    visibility: VisibilityCell,
}

/// Mutable visibility tracker keyed by sibling-set.
type VisibilityCell = String;

struct CommentInfo {
    text: String,
    line: usize,
    start_offset: usize,
}

fn line_at_offset(source: &str, offset: usize) -> usize {
    let mut line = 1usize;
    for (i, b) in source.as_bytes().iter().enumerate() {
        if i >= offset { break; }
        if *b == b'\n' { line += 1; }
    }
    line
}

struct Visitor<'a> {
    cop: &'a DocumentationMethod,
    source: &'a str,
    filename: &'a str,
    comments: &'a [CommentInfo],
    offenses: Vec<Offense>,
    scope_stack: Vec<Scope>,
}

impl<'a> Visitor<'a> {
    fn process_body(&mut self, body: Option<Node<'a>>) {
        // Track visibility through siblings.
        let elements: Vec<Node<'a>> = match body {
            None => return,
            Some(b) => {
                if let Some(stmts) = b.as_statements_node() {
                    stmts.body().iter().collect()
                } else {
                    vec![b]
                }
            }
        };
        self.scope_stack.push(Scope { visibility: "public".to_string() });
        for el in &elements {
            self.process_sibling(el);
        }
        self.scope_stack.pop();
    }

    fn process_sibling(&mut self, node: &Node<'a>) {
        // Detect bare access modifier → update visibility for following siblings.
        if let Some(call) = node.as_call_node() {
            if call.receiver().is_none()
                && call.arguments().is_none()
                && call.block().is_none()
            {
                let m = node_name!(call);
                match m.as_ref() {
                    "private" => { self.set_visibility("private"); return; }
                    "protected" => { self.set_visibility("protected"); return; }
                    "public" => { self.set_visibility("public"); return; }
                    _ => {}
                }
            }

            // Inline modifier wraps a def: `private def foo`, `module_function def foo`, etc.
            if call.receiver().is_none() {
                if let Some(args) = call.arguments() {
                    let arg_list: Vec<Node> = args.arguments().iter().collect();
                    if arg_list.len() == 1 {
                        if let Some(def) = arg_list[0].as_def_node() {
                            let m = node_name!(call).to_string();
                            self.handle_inline_modifier_def(node, &m, &def);
                            // Recurse into def body too (for nested defs/classes).
                            self.visit_def_body(&def);
                            return;
                        }
                    }
                }
            }
        }

        // Direct def at this scope level
        if let Some(def) = node.as_def_node() {
            self.check_def(&def, None);
            self.visit_def_body(&def);
            return;
        }

        // Recurse into class/module/sclass.
        if let Some(class) = node.as_class_node() {
            self.process_body(class.body());
            return;
        }
        if let Some(m) = node.as_module_node() {
            self.process_body(m.body());
            return;
        }
        if let Some(sc) = node.as_singleton_class_node() {
            self.process_body(sc.body());
            return;
        }

        // Recurse generically into other nodes (e.g. `if`, `begin`).
        self.visit(node);
    }

    fn visit_def_body(&mut self, def: &ruby_prism::DefNode<'a>) {
        // Walk into method body to find nested classes/modules/methods.
        if let Some(body) = def.body() {
            self.scope_stack.push(Scope { visibility: "public".to_string() });
            // body may be a StatementsNode
            let elements: Vec<Node> = if let Some(stmts) = body.as_statements_node() {
                stmts.body().iter().collect()
            } else {
                vec![body]
            };
            for el in &elements {
                self.process_sibling(el);
            }
            self.scope_stack.pop();
        }
    }

    fn current_visibility(&self) -> &str {
        self.scope_stack.last().map(|s| s.visibility.as_str()).unwrap_or("public")
    }

    fn set_visibility(&mut self, v: &str) {
        if let Some(top) = self.scope_stack.last_mut() {
            top.visibility = v.to_string();
        }
    }

    fn handle_inline_modifier_def(
        &mut self,
        send_node: &Node<'a>,
        modifier: &str,
        def: &ruby_prism::DefNode<'a>,
    ) {
        // Determine if this modifier counts as "non-public" or not.
        let is_non_public = matches!(modifier, "private" | "protected" | "private_class_method");
        let is_modifier_kind = matches!(modifier, "module_function" | "ruby2_keywords");

        let visibility_override = if is_non_public { Some("private") } else { None };

        // For modifier_node? (module_function/ruby2_keywords): offense range = parent send.
        // Otherwise (private/protected/private_class_method): offense range = def.
        let range_node: &Node<'a> = if is_modifier_kind {
            send_node
        } else {
            // Need to refer to the def via `node`. Caller passes in the call node.
            // Simpler: pass def's location.
            send_node // placeholder; overridden below
        };

        let _ = range_node;
        let (off_start, off_end) = if is_modifier_kind {
            (send_node.location().start_offset(), send_node.location().end_offset())
        } else {
            (def.location().start_offset(), def.location().end_offset())
        };

        self.check_def(def, Some((off_start, off_end, visibility_override)));
    }

    fn check_def(
        &mut self,
        def: &ruby_prism::DefNode,
        modifier_info: Option<(usize, usize, Option<&str>)>,
    ) {
        let method_name = node_name!(def).to_string();
        if method_name == "initialize" {
            return;
        }

        // Determine visibility
        let visibility = match modifier_info.and_then(|(_, _, v)| v) {
            Some(v) => v.to_string(),
            None => self.current_visibility().to_string(),
        };
        let non_public = visibility != "public";

        if non_public && !self.cop.require_for_non_public_methods {
            return;
        }

        if self.cop.allowed_methods.contains(&method_name) {
            return;
        }

        // Determine offense range
        let (start, end) = match modifier_info {
            Some((s, e, _)) => (s, e),
            None => (def.location().start_offset(), def.location().end_offset()),
        };

        if self.has_documentation_comment(start) {
            return;
        }

        let location = Location::from_offsets(self.source, start, end);
        self.offenses.push(Offense::new(
            "Style/DocumentationMethod",
            MSG,
            Severity::Convention,
            location,
            self.filename,
        ));
    }

    /// Check if a comment is on its own line (not inline after code).
    fn is_comment_line(&self, comment: &CommentInfo) -> bool {
        let line_start = if comment.start_offset == 0 {
            0
        } else {
            self.source[..comment.start_offset]
                .rfind('\n')
                .map_or(0, |p| p + 1)
        };
        self.source[line_start..comment.start_offset]
            .chars()
            .all(|c| c.is_ascii_whitespace())
    }

    fn has_documentation_comment(&self, keyword_start: usize) -> bool {
        let node_line = line_at_offset(self.source, keyword_start);
        let mut preceding: Vec<&CommentInfo> = Vec::new();
        for comment in self.comments.iter().rev() {
            if comment.line >= node_line { continue; }
            if !self.is_comment_line(comment) { continue; }
            if preceding.is_empty() {
                if comment.line + 1 == node_line {
                    preceding.push(comment);
                } else { break; }
            } else {
                let last_line = preceding.last().unwrap().line;
                if comment.line + 1 == last_line {
                    preceding.push(comment);
                } else { break; }
            }
        }
        if preceding.is_empty() { return false; }
        preceding.iter().any(|c| {
            !is_annotation_comment(&c.text, &self.cop.annotation_keywords)
                && !is_interpreter_directive(&c.text)
                && !is_rubocop_directive(&c.text)
        })
    }
}

fn is_annotation_comment(text: &str, keywords: &[String]) -> bool {
    let content = text.trim_start_matches('#').trim_start();
    let kw_iter = if keywords.is_empty() {
        ANNOTATION_KEYWORDS.iter().map(|s| s.to_string()).collect::<Vec<_>>()
    } else {
        keywords.iter().cloned().collect::<Vec<_>>()
    };
    kw_iter.iter().any(|kw| {
        content.starts_with(kw.as_str()) && {
            let after = &content[kw.len()..];
            after.is_empty() || after.starts_with(':') || after.starts_with(' ')
        }
    })
}

fn is_interpreter_directive(text: &str) -> bool {
    let content = text.trim_start_matches('#').trim_start();
    content.starts_with("frozen_string_literal:") || content.starts_with("encoding:")
}

fn is_rubocop_directive(text: &str) -> bool {
    let content = text.trim_start_matches('#').trim_start();
    content.starts_with("rubocop:")
}

impl<'a> Visit<'a> for Visitor<'a> {
    fn visit_program_node(&mut self, node: &ruby_prism::ProgramNode<'a>) {
        let stmts = node.statements();
        let elements: Vec<Node<'a>> = stmts.body().iter().collect();
        self.scope_stack.push(Scope { visibility: "public".to_string() });
        for el in &elements {
            self.process_sibling(el);
        }
        self.scope_stack.pop();
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    require_for_non_public_methods: bool,
    allowed_methods: Vec<String>,
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct AnnotationCfg {
    keywords: Vec<String>,
}

crate::register_cop!("Style/DocumentationMethod", |cfg| {
    let c: Cfg = cfg.typed("Style/DocumentationMethod");
    let ann: AnnotationCfg = cfg.typed("Style/CommentAnnotation");
    Some(Box::new(DocumentationMethod::new(
        c.require_for_non_public_methods,
        c.allowed_methods,
        ann.keywords,
    )))
});
