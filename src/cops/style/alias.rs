//! Style/Alias — Enforces use of `alias` vs `alias_method`.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/alias.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    PreferAlias,
    PreferAliasMethod,
}

impl Default for EnforcedStyle {
    fn default() -> Self {
        EnforcedStyle::PreferAlias
    }
}

pub struct Alias {
    style: EnforcedStyle,
}

impl Default for Alias {
    fn default() -> Self {
        Self { style: EnforcedStyle::PreferAlias }
    }
}

impl Alias {
    pub fn new(style: EnforcedStyle) -> Self {
        Self { style }
    }
}

impl Cop for Alias {
    fn name(&self) -> &'static str {
        "Style/Alias"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = AliasVisitor {
            ctx,
            style: self.style,
            offenses: Vec::new(),
            // scope stack: 0=toplevel, 1=class/module, def_depth counts method defs
            scope_stack: vec![ScopeKind::TopLevel],
            in_instance_eval: false,
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    TopLevel,
    ClassBody,
    ModuleBody,
    DefBody,
    BlockBody,
    SingletonDefBody,
}

struct AliasVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: EnforcedStyle,
    offenses: Vec<Offense>,
    scope_stack: Vec<ScopeKind>,
    in_instance_eval: bool,
}

impl<'a> AliasVisitor<'a> {
    fn current_scope(&self) -> ScopeKind {
        *self.scope_stack.last().unwrap_or(&ScopeKind::TopLevel)
    }

    fn in_def(&self) -> bool {
        self.scope_stack.iter().any(|s| matches!(s, ScopeKind::DefBody | ScopeKind::SingletonDefBody))
    }

    fn in_block(&self) -> bool {
        self.scope_stack.iter().rev().any(|s| {
            if matches!(s, ScopeKind::ClassBody | ScopeKind::ModuleBody | ScopeKind::DefBody) {
                return false; // stop at class/module/def boundary
            }
            matches!(s, ScopeKind::BlockBody)
        })
    }

    /// `alias :new_name :old_name` or `alias new_name old_name`
    fn check_alias_method_node(&mut self, node: &ruby_prism::AliasMethodNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();

        let new_name_loc = node.new_name().location();
        let old_name_loc = node.old_name().location();

        let new_name_src = &self.ctx.source[new_name_loc.start_offset()..new_name_loc.end_offset()];
        let old_name_src = &self.ctx.source[old_name_loc.start_offset()..old_name_loc.end_offset()];

        // Skip global variable aliases
        if new_name_src.starts_with('$') || old_name_src.starts_with('$') {
            return;
        }

        // Skip if inside instance_eval block
        if self.in_instance_eval {
            return;
        }

        // Extract bare name (without leading `:`)
        let new_bare = symbol_to_bare(new_name_src);
        let old_bare = symbol_to_bare(old_name_src);

        // Skip if interpolated symbol (contains `#{`)
        if new_name_src.contains("#{") || old_name_src.contains("#{") {
            match self.style {
                EnforcedStyle::PreferAlias => return,
                EnforcedStyle::PreferAliasMethod => {
                    // still flag: use alias_method
                    let msg = "Use `alias_method` instead of `alias`.";
                    let offense_end = start + 5; // "alias"
                    let correction = format!("alias_method {}, {}", new_name_src, old_name_src);
                    let offense = self.ctx.offense_with_range(
                        "Style/Alias", msg, Severity::Convention, start, start + 5,
                    ).with_correction(Correction::replace(start, end, correction));
                    self.offenses.push(offense);
                    return;
                }
            }
        }

        match self.style {
            EnforcedStyle::PreferAlias => {
                let scope = self.current_scope();
                // Inside a regular def — alias is allowed (no offense)
                let in_regular_def = matches!(scope, ScopeKind::DefBody);
                // Inside singleton def (def obj.foo) — alias should use alias_method
                let in_singleton_def = matches!(scope, ScopeKind::SingletonDefBody)
                    || self.scope_stack.iter().any(|s| matches!(s, ScopeKind::SingletonDefBody));
                // Check if inside a block that's not a class/module body
                let in_block_scope = self.scope_stack.iter().rev()
                    .take_while(|s| !matches!(s, ScopeKind::ClassBody | ScopeKind::ModuleBody | ScopeKind::TopLevel))
                    .any(|s| matches!(s, ScopeKind::BlockBody));

                if in_regular_def && !in_singleton_def {
                    // Inside a regular def — no offense for alias (allowed)
                    return;
                }

                if in_singleton_def || in_block_scope {
                    // Inside a singleton def or block (not class/module) — flag alias, use alias_method
                    let msg = "Use `alias_method` instead of `alias`.";
                    let correction = format!("alias_method :{}, :{}", new_bare, old_bare);
                    let offense = self.ctx.offense_with_range(
                        "Style/Alias", msg, Severity::Convention, start, start + 5,
                    ).with_correction(Correction::replace(start, end, correction));
                    self.offenses.push(offense);
                    return;
                }

                // At class/module/toplevel with symbol args → prefer bareword
                if new_name_src.starts_with(':') && old_name_src.starts_with(':') {
                    // Both are symbols — convert to bareword alias
                    let msg = format!("Use `alias {} {}` instead of `alias {} {}`.",
                        new_bare, old_bare, new_name_src, old_name_src);
                    let args_start = new_name_loc.start_offset();
                    let correction = format!("{} {}", new_bare, old_bare);
                    let offense = self.ctx.offense_with_range(
                        "Style/Alias", &msg, Severity::Convention, args_start, end,
                    ).with_correction(Correction::replace(args_start, end, correction));
                    self.offenses.push(offense);
                }
                // If bareword args already — no offense
            }
            EnforcedStyle::PreferAliasMethod => {
                let msg = "Use `alias_method` instead of `alias`.";
                let correction = format!("alias_method :{}, :{}", new_bare, old_bare);
                let offense = self.ctx.offense_with_range(
                    "Style/Alias", msg, Severity::Convention, start, start + 5,
                ).with_correction(Correction::replace(start, end, correction));
                self.offenses.push(offense);
            }
        }
    }

    /// `alias_method :new, :old`
    fn check_alias_method_call(&mut self, node: &ruby_prism::CallNode) {
        let method = node_name!(node);
        if method != "alias_method" { return; }

        // Must have no explicit receiver (or explicit non-Kernel receiver disqualifies)
        if node.receiver().is_some() { return; }

        // Must have exactly 2 symbol args
        let args = match node.arguments() {
            Some(a) => a,
            None => return,
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() < 2 { return; }

        // Both args must be symbol literals (not constant, not method call)
        let new_sym = extract_symbol(&arg_list[0], self.ctx.source);
        let old_sym = extract_symbol(&arg_list[1], self.ctx.source);
        let (new_bare, old_bare) = match (new_sym, old_sym) {
            (Some(n), Some(o)) => (n, o),
            _ => return,
        };

        match self.style {
            EnforcedStyle::PreferAliasMethod => return, // alias_method is preferred
            EnforcedStyle::PreferAlias => {
                // alias_method is preferred inside defs/blocks/singleton-defs
                let in_singleton_def = self.scope_stack.iter()
                    .any(|s| matches!(s, ScopeKind::SingletonDefBody));
                if in_singleton_def { return; }

                // Inside a non-class/module block → alias_method ok
                let in_block_scope = self.scope_stack.iter().rev()
                    .take_while(|s| !matches!(s, ScopeKind::ClassBody | ScopeKind::ModuleBody | ScopeKind::TopLevel))
                    .any(|s| matches!(s, ScopeKind::BlockBody));
                if in_block_scope { return; }

                let scope = self.current_scope();
                let location_desc = match scope {
                    ScopeKind::ClassBody => "in a class body",
                    ScopeKind::ModuleBody => "in a module body",
                    ScopeKind::TopLevel => "at the top level",
                    _ => return,
                };
                let msg = format!("Use `alias` instead of `alias_method` {}.", location_desc);
                let start = node.location().start_offset();
                let end = node.location().end_offset();
                let name_end = start + "alias_method".len();
                let correction = format!("alias {} {}", new_bare, old_bare);
                let offense = self.ctx.offense_with_range(
                    "Style/Alias", &msg, Severity::Convention, start, name_end,
                ).with_correction(Correction::replace(start, end, correction));
                self.offenses.push(offense);
            }
        }
    }
}

fn symbol_to_bare(s: &str) -> &str {
    s.trim_start_matches(':')
}

/// Extract symbol bare name from node (SymbolNode only).
fn extract_symbol<'a>(node: &Node, source: &'a str) -> Option<&'a str> {
    if let Some(sym) = node.as_symbol_node() {
        let loc = sym.location();
        let src = &source[loc.start_offset()..loc.end_offset()];
        // src is like `:foo`
        Some(src.trim_start_matches(':'))
    } else {
        None
    }
}

impl<'a> Visit<'_> for AliasVisitor<'a> {
    fn visit_alias_method_node(&mut self, node: &ruby_prism::AliasMethodNode) {
        self.check_alias_method_node(node);
        ruby_prism::visit_alias_method_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.check_alias_method_call(node);
        // Check if it's instance_eval
        let method = node_name!(node);
        let was_instance_eval = self.in_instance_eval;
        if method == "instance_eval" {
            self.in_instance_eval = true;
        }
        ruby_prism::visit_call_node(self, node);
        self.in_instance_eval = was_instance_eval;
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        self.scope_stack.push(ScopeKind::ClassBody);
        ruby_prism::visit_class_node(self, node);
        self.scope_stack.pop();
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        self.scope_stack.push(ScopeKind::ModuleBody);
        ruby_prism::visit_module_node(self, node);
        self.scope_stack.pop();
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let kind = if node.receiver().is_some() {
            ScopeKind::SingletonDefBody
        } else {
            ScopeKind::DefBody
        };
        self.scope_stack.push(kind);
        ruby_prism::visit_def_node(self, node);
        self.scope_stack.pop();
    }

    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        self.scope_stack.push(ScopeKind::BlockBody);
        ruby_prism::visit_block_node(self, node);
        self.scope_stack.pop();
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style: String }

crate::register_cop!("Style/Alias", |cfg| {
    let c: Cfg = cfg.typed("Style/Alias");
    let style = match c.enforced_style.as_str() {
        "prefer_alias_method" => EnforcedStyle::PreferAliasMethod,
        _ => EnforcedStyle::PreferAlias,
    };
    Some(Box::new(Alias::new(style)))
});
