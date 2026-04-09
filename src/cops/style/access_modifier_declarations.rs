//! Style/AccessModifierDeclarations cop

use crate::cops::{CheckContext, Cop};
use crate::helpers::access_modifier::ACCESS_MODIFIERS;
use crate::offense::{Location, Offense, Severity};
use ruby_prism::Visit;
const ATTR_METHODS: &[&str] = &["attr", "attr_reader", "attr_writer", "attr_accessor"];

#[derive(Debug, Clone, PartialEq)]
pub enum EnforcedStyle {
    Group,
    Inline,
}

pub struct AccessModifierDeclarations {
    enforced_style: EnforcedStyle,
    allow_modifiers_on_symbols: bool,
    allow_modifiers_on_attrs: bool,
    allow_modifiers_on_alias_method: bool,
}

impl AccessModifierDeclarations {
    pub fn new(style: EnforcedStyle) -> Self {
        Self {
            enforced_style: style,
            allow_modifiers_on_symbols: true,
            allow_modifiers_on_attrs: true,
            allow_modifiers_on_alias_method: true,
        }
    }

    pub fn with_config(
        style: EnforcedStyle,
        allow_modifiers_on_symbols: bool,
        allow_modifiers_on_attrs: bool,
        allow_modifiers_on_alias_method: bool,
    ) -> Self {
        Self {
            enforced_style: style,
            allow_modifiers_on_symbols,
            allow_modifiers_on_attrs,
            allow_modifiers_on_alias_method,
        }
    }
}

#[derive(Debug, Clone)]
struct ModifierInfo {
    modifier_name: String,
    line: u32,
    column_start: u32,
    column_end: u32,
    has_arguments: bool,
    arg_kind: ModifierArgKind,
    inside_block: bool,
    is_hash_value: bool,
    inside_if: bool,
    scope_depth: usize,
    scope_id: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum ModifierArgKind {
    None,
    DefNode,
    Symbol,
    Splat,
    AttrMethod,
    AliasMethod,
    Other,
}

struct ModifierCollector {
    source: String,
    modifiers: Vec<ModifierInfo>,
    block_depth: usize,
    scope_depth: usize,
    current_scope_id: usize,
    next_scope_id: usize,
}

impl ModifierCollector {
    fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
            modifiers: Vec::new(),
            block_depth: 0,
            scope_depth: 0,
            current_scope_id: 0,
            next_scope_id: 1,
        }
    }

    fn classify_arguments(node: &ruby_prism::CallNode, source: &str) -> (bool, ModifierArgKind) {
        if let Some(args) = node.arguments() {
            let args_list: Vec<_> = args.arguments().iter().collect();
            if args_list.is_empty() { return (false, ModifierArgKind::None); }

            let first_arg = &args_list[0];
            if first_arg.as_def_node().is_some() { return (true, ModifierArgKind::DefNode); }
            if first_arg.as_symbol_node().is_some() { return (true, ModifierArgKind::Symbol); }
            if first_arg.as_splat_node().is_some() { return (true, ModifierArgKind::Splat); }
            if let Some(call) = first_arg.as_call_node() {
                let call_name = node_name!(call);
                if ATTR_METHODS.contains(&call_name.as_ref()) { return (true, ModifierArgKind::AttrMethod); }
                if call_name == "alias_method" { return (true, ModifierArgKind::AliasMethod); }
                return (true, ModifierArgKind::Other);
            }
            return (true, ModifierArgKind::Other);
        }

        let loc = node.location();
        if loc.end_offset() <= source.len() && source[loc.start_offset()..loc.end_offset()].contains('(') {
            return (true, ModifierArgKind::Other);
        }
        (false, ModifierArgKind::None)
    }

    fn check_is_hash_value(&self, node: &ruby_prism::CallNode) -> bool {
        if node.arguments().is_some() {
            return false;
        }

        let loc = node.location();
        let start = loc.start_offset();

        if start >= 2 {
            let before = &self.source[..start];
            let trimmed = before.trim_end();
            if trimmed.ends_with(':') {
                return true;
            }
        }

        false
    }

    fn check_inside_if(&self, node: &ruby_prism::CallNode) -> bool {
        let loc = node.location();
        let end_offset = loc.end_offset();

        if end_offset < self.source.len() {
            let after = &self.source[end_offset..];
            let eol = after.find('\n').unwrap_or(after.len());
            let rest_of_line = after[..eol].trim();
            if rest_of_line.starts_with("if ") || rest_of_line.starts_with("unless ") {
                return true;
            }
        }

        false
    }
}

impl Visit<'_> for ModifierCollector {
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode) {
        self.block_depth += 1;
        ruby_prism::visit_block_node(self, node);
        self.block_depth -= 1;
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode) {
        self.block_depth += 1;
        ruby_prism::visit_lambda_node(self, node);
        self.block_depth -= 1;
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        let prev_scope_id = self.current_scope_id;
        self.current_scope_id = self.next_scope_id;
        self.next_scope_id += 1;
        self.scope_depth += 1;
        ruby_prism::visit_class_node(self, node);
        self.scope_depth -= 1;
        self.current_scope_id = prev_scope_id;
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        let prev_scope_id = self.current_scope_id;
        self.current_scope_id = self.next_scope_id;
        self.next_scope_id += 1;
        self.scope_depth += 1;
        ruby_prism::visit_module_node(self, node);
        self.scope_depth -= 1;
        self.current_scope_id = prev_scope_id;
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        let prev_scope_id = self.current_scope_id;
        self.current_scope_id = self.next_scope_id;
        self.next_scope_id += 1;
        self.scope_depth += 1;
        ruby_prism::visit_singleton_class_node(self, node);
        self.scope_depth -= 1;
        self.current_scope_id = prev_scope_id;
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let name_str = node_name!(node).to_string();

        if ACCESS_MODIFIERS.contains(&name_str.as_str()) {
            let msg_loc = node.message_loc().unwrap();
            let start_offset = msg_loc.start_offset();
            let end_offset = msg_loc.end_offset();
            let loc = Location::from_offsets(&self.source, start_offset, end_offset);

            let (has_arguments, arg_kind) = Self::classify_arguments(node, &self.source);

            let inside_block = self.block_depth > 0;
            let is_hash_value = self.check_is_hash_value(node);
            let inside_if = self.check_inside_if(node);

            let info = ModifierInfo {
                modifier_name: name_str,
                line: loc.line,
                column_start: loc.column,
                column_end: loc.last_column,
                has_arguments,
                arg_kind,
                inside_block,
                is_hash_value,
                inside_if,
                scope_depth: self.scope_depth,
                scope_id: self.current_scope_id,
            };

            self.modifiers.push(info);
        }

        // Continue visiting children
        ruby_prism::visit_call_node(self, node);
    }
}

impl Cop for AccessModifierDeclarations {
    fn name(&self) -> &'static str {
        "Style/AccessModifierDeclarations"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut collector = ModifierCollector::new(ctx.source);
        collector.visit_program_node(node);

        let modifiers = collector.modifiers;

        let mut offenses = Vec::new();

        match self.enforced_style {
            EnforcedStyle::Group => {
                self.check_group_style(&modifiers, ctx, &mut offenses);
            }
            EnforcedStyle::Inline => {
                self.check_inline_style(&modifiers, ctx, &mut offenses);
            }
        }

        offenses
    }
}

impl AccessModifierDeclarations {
    fn check_group_style(
        &self,
        modifiers: &[ModifierInfo],
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
    ) {
        for (i, info) in modifiers.iter().enumerate() {
            if info.inside_block || info.is_hash_value || info.inside_if { continue; }
            if !info.has_arguments { continue; }
            if info.scope_depth == 0 && is_symbol_like_arg(&info.arg_kind) { continue; }
            if self.should_allow_for_group(info) { continue; }
            if self.has_right_sibling_same_modifier_in_scope(modifiers, i) { continue; }

            let message = format!(
                "`{}` should not be inlined in method definitions.",
                info.modifier_name
            );

            offenses.push(Offense::new(
                self.name(),
                &message,
                self.severity(),
                Location::new(info.line, info.column_start, info.line, info.column_end),
                ctx.filename,
            ));
        }
    }

    fn check_inline_style(
        &self,
        modifiers: &[ModifierInfo],
        ctx: &CheckContext,
        offenses: &mut Vec<Offense>,
    ) {
        for info in modifiers {
            if info.inside_block || info.is_hash_value { continue; }
            if info.has_arguments { continue; }
            if !self.has_following_def_for_inline(info, ctx.source) { continue; }

            let message = format!(
                "`{}` should be inlined in method definitions.",
                info.modifier_name
            );

            offenses.push(Offense::new(
                self.name(),
                &message,
                self.severity(),
                Location::new(info.line, info.column_start, info.line, info.column_end),
                ctx.filename,
            ));
        }
    }

    fn should_allow_for_group(&self, info: &ModifierInfo) -> bool {
        match info.arg_kind {
            ModifierArgKind::Symbol | ModifierArgKind::Splat => self.allow_modifiers_on_symbols,
            ModifierArgKind::AttrMethod => self.allow_modifiers_on_attrs,
            ModifierArgKind::AliasMethod => self.allow_modifiers_on_alias_method,
            _ => false,
        }
    }

    fn has_right_sibling_same_modifier_in_scope(
        &self,
        modifiers: &[ModifierInfo],
        current_idx: usize,
    ) -> bool {
        let current = &modifiers[current_idx];

        for j in (current_idx + 1)..modifiers.len() {
            let sibling = &modifiers[j];
            if sibling.scope_id != current.scope_id { continue; }
            if sibling.modifier_name != current.modifier_name { continue; }
            if sibling.inside_block || sibling.is_hash_value || sibling.inside_if { continue; }
            if !sibling.has_arguments { continue; }
            if sibling.scope_depth == 0 && is_symbol_like_arg(&sibling.arg_kind) { continue; }
            if self.should_allow_for_group(sibling) { continue; }
            return true;
        }
        false
    }

    fn has_following_def_for_inline(&self, info: &ModifierInfo, source: &str) -> bool {
        let lines: Vec<&str> = source.lines().collect();
        let modifier_line_idx = (info.line as usize).saturating_sub(1);

        if modifier_line_idx >= lines.len() { return false; }

        let modifier_line = lines[modifier_line_idx];
        let col_end = info.column_end as usize;
        if col_end < modifier_line.len() {
            let rest = &modifier_line[col_end..];
            for part in rest.split(';') {
                if part.trim().starts_with("def ") { return true; }
            }
        }

        for i in (modifier_line_idx + 1)..lines.len() {
            let trimmed = lines[i].trim();
            if trimmed.is_empty() {
                continue;
            }

            if trimmed.starts_with("def ") {
                return true;
            }

            // If we find another bare access modifier, stop
            let is_bare_modifier = ACCESS_MODIFIERS.iter().any(|m| {
                trimmed == *m || trimmed.starts_with(&format!("{} #", m))
            });
            if is_bare_modifier {
                return false;
            }

            if trimmed == "end" {
                return false;
            }

            // Any other content = stop looking
            break;
        }
        false
    }
}

fn is_symbol_like_arg(kind: &ModifierArgKind) -> bool {
    matches!(
        kind,
        ModifierArgKind::Symbol | ModifierArgKind::Splat | ModifierArgKind::Other
    )
}
