//! Lint/UselessRuby2Keywords cop.
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/useless_ruby2_keywords.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::{CallNode, Node, Visit};

#[derive(Default)]
pub struct UselessRuby2Keywords;

impl UselessRuby2Keywords {
    pub fn new() -> Self { Self }
}

impl Cop for UselessRuby2Keywords {
    fn name(&self) -> &'static str { "Lint/UselessRuby2Keywords" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = StackedVisitor {
            ctx,
            offenses: Vec::new(),
            frames: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

fn parameters_allowed(p: &ruby_prism::ParametersNode) -> bool {
    let has_restarg = p.rest().is_some();
    let any_required = p.requireds().iter().next().is_some();
    let any_optional = p.optionals().iter().next().is_some();
    let any_post = p.posts().iter().next().is_some();
    let any_keywords = p.keywords().iter().next().is_some();
    let has_kwrest = p.keyword_rest().is_some();

    // Empty arguments → not allowed (offense)
    if !has_restarg && !any_required && !any_optional && !any_post
        && !any_keywords && !has_kwrest && p.block().is_none()
    {
        return false;
    }
    has_restarg && !any_keywords && !has_kwrest
}

fn def_arguments_allowed(def: &ruby_prism::DefNode) -> bool {
    match def.parameters() {
        Some(p) => parameters_allowed(&p),
        None => false,
    }
}

fn block_arguments_allowed(block: &ruby_prism::BlockNode) -> bool {
    let params = match block.parameters() {
        Some(p) => p,
        None => return false,
    };
    if let Some(bp) = params.as_block_parameters_node() {
        match bp.parameters() {
            Some(inner) => parameters_allowed(&inner),
            None => false,
        }
    } else {
        // numbered or it block → not allowed
        false
    }
}

fn matches_define_method<'a>(
    call: &CallNode<'a>,
    method_name: &str,
) -> Option<ruby_prism::BlockNode<'a>> {
    let cname = String::from_utf8_lossy(call.name().as_slice());
    if cname != "define_method" {
        return None;
    }
    let args = call.arguments()?;
    let first = args.arguments().iter().next()?;
    let sym = first.as_symbol_node()?;
    let val = sym.unescaped();
    let val_str = String::from_utf8_lossy(val);
    if val_str != method_name {
        return None;
    }
    let block = call.block()?;
    block.as_block_node()
}

enum DefOrBlock<'a> {
    Def(ruby_prism::DefNode<'a>),
    Block(ruby_prism::BlockNode<'a>),
}

fn find_def_with_name<'a>(
    siblings_stack: &[Vec<Node<'a>>],
    method_name: &str,
) -> Option<DefOrBlock<'a>> {
    for siblings in siblings_stack.iter().rev() {
        for s in siblings.iter() {
            if let Some(d) = s.as_def_node() {
                let dname = String::from_utf8_lossy(d.name().as_slice()).to_string();
                if dname == method_name {
                    return Some(DefOrBlock::Def(d));
                }
            }
            if let Some(c) = s.as_call_node() {
                if let Some(b) = matches_define_method(&c, method_name) {
                    return Some(DefOrBlock::Block(b));
                }
            }
        }
    }
    None
}

enum ScopeFrame<'src> {
    Stmts(ruby_prism::StatementsNode<'src>),
    Program(ruby_prism::ProgramNode<'src>),
}

struct StackedVisitor<'a, 'src> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    frames: Vec<ScopeFrame<'src>>,
}

impl<'a, 'src> StackedVisitor<'a, 'src> {
    fn collect_siblings(&self) -> Vec<Vec<Node<'src>>> {
        self.frames.iter().map(|f| match f {
            ScopeFrame::Stmts(s) => s.body().iter().collect(),
            ScopeFrame::Program(p) => p.statements().body().iter().collect(),
        }).collect()
    }

    fn check_call(&mut self, call: &CallNode<'src>) {
        if call.receiver().is_some() {
            return;
        }
        let cname = String::from_utf8_lossy(call.name().as_slice());
        if cname != "ruby2_keywords" {
            return;
        }
        let args = match call.arguments() {
            Some(a) => a,
            None => return,
        };
        let first = match args.arguments().iter().next() {
            Some(a) => a,
            None => return,
        };

        if let Some(def) = first.as_def_node() {
            if def_arguments_allowed(&def) {
                return;
            }
            if let Some(s) = call.message_loc() {
                let method_name = String::from_utf8_lossy(def.name().as_slice());
                self.offenses.push(self.ctx.offense_with_range(
                    "Lint/UselessRuby2Keywords",
                    &format!("`ruby2_keywords` is unnecessary for method `{}`.", method_name),
                    Severity::Warning,
                    s.start_offset(),
                    s.end_offset(),
                ));
            }
        } else if let Some(sym) = first.as_symbol_node() {
            let method_name_bytes = sym.unescaped();
            let method_name = String::from_utf8_lossy(method_name_bytes).to_string();
            let stack = self.collect_siblings();
            let target = match find_def_with_name(&stack, &method_name) {
                Some(t) => t,
                None => return,
            };
            let allowed = match &target {
                DefOrBlock::Def(d) => def_arguments_allowed(d),
                DefOrBlock::Block(b) => block_arguments_allowed(b),
            };
            if allowed {
                return;
            }
            let loc = call.location();
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/UselessRuby2Keywords",
                &format!("`ruby2_keywords` is unnecessary for method `{}`.", method_name),
                Severity::Warning,
                loc.start_offset(),
                loc.end_offset(),
            ));
        }
    }
}

/// SAFETY: ruby-prism nodes are pointer wrappers; ptr::read is safe while parser lives.
fn reborrow_program<'src>(p: &ruby_prism::ProgramNode<'src>) -> ruby_prism::ProgramNode<'src> {
    unsafe { std::ptr::read(p) }
}
fn reborrow_stmts<'src>(p: &ruby_prism::StatementsNode<'src>) -> ruby_prism::StatementsNode<'src> {
    unsafe { std::ptr::read(p) }
}

impl<'a, 'src> Visit<'src> for StackedVisitor<'a, 'src> {
    fn visit_program_node(&mut self, node: &ruby_prism::ProgramNode<'src>) {
        self.frames.push(ScopeFrame::Program(reborrow_program(node)));
        ruby_prism::visit_program_node(self, node);
        self.frames.pop();
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode<'src>) {
        let pushed = if let Some(body) = node.body() {
            if let Some(stmts) = body.as_statements_node() {
                self.frames.push(ScopeFrame::Stmts(reborrow_stmts(&stmts)));
                true
            } else { false }
        } else { false };
        ruby_prism::visit_class_node(self, node);
        if pushed { self.frames.pop(); }
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode<'src>) {
        let pushed = if let Some(body) = node.body() {
            if let Some(stmts) = body.as_statements_node() {
                self.frames.push(ScopeFrame::Stmts(reborrow_stmts(&stmts)));
                true
            } else { false }
        } else { false };
        ruby_prism::visit_module_node(self, node);
        if pushed { self.frames.pop(); }
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'src>) {
        let pushed = if let Some(body) = node.body() {
            if let Some(stmts) = body.as_statements_node() {
                self.frames.push(ScopeFrame::Stmts(reborrow_stmts(&stmts)));
                true
            } else { false }
        } else { false };
        ruby_prism::visit_def_node(self, node);
        if pushed { self.frames.pop(); }
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'src>) {
        self.check_call(node);
        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Lint/UselessRuby2Keywords", |_cfg| {
    Some(Box::new(UselessRuby2Keywords::new()))
});
