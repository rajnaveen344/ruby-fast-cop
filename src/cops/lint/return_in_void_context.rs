//! Lint/ReturnInVoidContext - Checks for return with a value in void-context methods.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use crate::node_name;
use ruby_prism::{Node, Visit};

const SCOPE_CHANGING_METHODS: &[&str] = &["lambda", "define_method", "define_singleton_method"];

#[derive(Default)]
pub struct ReturnInVoidContext;

impl ReturnInVoidContext {
    pub fn new() -> Self { Self }
}

impl Cop for ReturnInVoidContext {
    fn name(&self) -> &'static str { "Lint/ReturnInVoidContext" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new(), scope_stack: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

#[derive(Clone)]
enum ScopeKind {
    /// A void-context method (initialize or setter)
    VoidMethod { name: String },
    /// A regular def
    RegularDef,
    /// A scope-changing block (lambda, define_method, define_singleton_method)
    ScopeChangingBlock { method_name: String },
    /// A regular block
    RegularBlock { method_name: String },
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// Stack of enclosing scopes
    scope_stack: Vec<ScopeKind>,
}

impl<'a> Visitor<'a> {
    fn is_void_context_method_name(name: &str) -> bool {
        name == "initialize" || name.ends_with('=')
    }

    fn is_singleton_initialize(node: &ruby_prism::DefNode) -> bool {
        // `def self.initialize` — has receiver
        node.receiver().is_some() && {
            let method_name = String::from_utf8_lossy(node.name().as_slice());
            method_name.as_ref() == "initialize"
        }
    }

    fn check_return(&mut self, node: &ruby_prism::ReturnNode) {
        // Must have a value
        if node.arguments().is_none() {
            return;
        }
        let args = node.arguments().unwrap();
        if args.arguments().len() == 0 {
            return;
        }

        // Find the nearest void-context method in scope_stack
        // But if we cross a scope-changing block or a def, that breaks it
        let mut in_scope_changing_block = false;
        let mut void_method_name: Option<String> = None;

        for scope in self.scope_stack.iter().rev() {
            match scope {
                ScopeKind::VoidMethod { name } => {
                    if !in_scope_changing_block {
                        void_method_name = Some(name.clone());
                    }
                    break;
                }
                ScopeKind::RegularDef => {
                    break; // Stops at any def
                }
                ScopeKind::ScopeChangingBlock { .. } => {
                    in_scope_changing_block = true;
                    // Don't break — keep looking up (we might be in a scope-changing block
                    // inside a void method, but that's OK — no offense)
                    // Actually, if we hit a scope-changing block, no offense regardless
                    break;
                }
                ScopeKind::RegularBlock { .. } => {
                    // Regular blocks don't break the search — return exits the method
                    continue;
                }
            }
        }

        if let Some(method_name) = void_method_name {
            let msg = format!("Do not return a value in `{}`.", method_name);
            // Offense at the `return` keyword location
            let kw_loc = node.keyword_loc();
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/ReturnInVoidContext",
                &msg,
                Severity::Warning,
                kw_loc.start_offset(),
                kw_loc.end_offset(),
            ));
        }
    }
}

impl Visit<'_> for Visitor<'_> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let method_name = node_name!(node).into_owned();
        let is_void = Self::is_void_context_method_name(&method_name)
            && !Self::is_singleton_initialize(node);

        let kind = if is_void {
            ScopeKind::VoidMethod { name: method_name }
        } else {
            ScopeKind::RegularDef
        };

        self.scope_stack.push(kind);
        ruby_prism::visit_def_node(self, node);
        self.scope_stack.pop();
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        // Determine the method name of the call this block is attached to
        // We can't easily get the parent call from a block in Prism (no parent pointer)
        // Instead, we look at the block's source context
        // Actually we need to look at the call that owns this block.
        // Since we can't get parent, we'll use a heuristic: examine the source text before
        // the block's opening delimiter.
        let method_name = get_block_method_name(node, self.ctx.source);

        let kind = if SCOPE_CHANGING_METHODS.contains(&method_name.as_str()) {
            ScopeKind::ScopeChangingBlock { method_name }
        } else {
            ScopeKind::RegularBlock { method_name }
        };

        self.scope_stack.push(kind);
        ruby_prism::visit_block_node(self, node);
        self.scope_stack.pop();
    }

    fn visit_return_node(&mut self, node: &ruby_prism::ReturnNode) {
        self.check_return(node);
        ruby_prism::visit_return_node(self, node);
    }
}

fn get_block_method_name(block: &ruby_prism::BlockNode, source: &str) -> String {
    // The block's opening `do` or `{` is at block.opening_loc().start_offset()
    let block_start = block.opening_loc().start_offset();

    // Look backwards in the source for the method name
    // The source before the block might look like `define_method(:foo) do`
    // We need to extract the method name called right before the block
    let prefix = &source[..block_start];
    // Find the last method name in the prefix
    // Pattern: the method name is the word just before the `(` or ` do` or ` {`
    extract_last_method_name(prefix)
}

fn extract_last_method_name(s: &str) -> String {
    // We need to find the method name of the call that owns the block
    // The source before the block might look like:
    //   "define_method(:foo) "
    //   "self.define_method(:foo) "
    //   "lambda "
    //   "proc "
    // Strategy: scan backwards for the method name before the args/whitespace

    let bytes = s.as_bytes();
    let mut pos = bytes.len();

    // Skip trailing whitespace
    while pos > 0 && (bytes[pos - 1] == b' ' || bytes[pos - 1] == b'\t') {
        pos -= 1;
    }

    // Skip closing paren group if present: find matching open paren
    if pos > 0 && bytes[pos - 1] == b')' {
        pos -= 1; // past ')'
        let mut depth = 1i32;
        while pos > 0 && depth > 0 {
            pos -= 1;
            match bytes[pos] {
                b')' => depth += 1,
                b'(' => depth -= 1,
                _ => {}
            }
        }
        // pos now at '(' — skip it
        if pos > 0 && bytes[pos] == b'(' {
            // don't skip further — pos is AT '('
        }
    }

    // Now extract the identifier before pos
    let end = pos;
    let mut start = end;
    while start > 0 {
        let c = bytes[start - 1] as char;
        if c.is_alphanumeric() || c == '_' || c == '?' || c == '!' {
            start -= 1;
        } else {
            break;
        }
    }

    if start >= end {
        return String::new();
    }

    s[start..end].to_string()
}

crate::register_cop!("Lint/ReturnInVoidContext", |_cfg| Some(Box::new(ReturnInVoidContext::new())));
