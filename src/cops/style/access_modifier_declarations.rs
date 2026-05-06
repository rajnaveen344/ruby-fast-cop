//! Style/AccessModifierDeclarations cop

use crate::cops::{CheckContext, Cop};
use crate::helpers::access_modifier::ACCESS_MODIFIERS;
use crate::offense::{Correction, Edit, Location, Offense, Severity};
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
    // byte offsets for the whole call node (modifier + args)
    node_start: usize,
    node_end: usize,
    // byte offsets for the argument node (def/call/symbol)
    arg_start: usize,
    arg_end: usize,
    // symbol names from symbol args (for `private :foo, :bar`)
    symbol_names: Vec<String>,
    // end offset of the enclosing class/module/sclass scope (0 if top-level)
    scope_end_offset: usize,
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

// Info about a def node found in the source
#[derive(Debug, Clone)]
struct DefInfo {
    name: String,
    start: usize,
    end: usize,
    scope_id: usize,
}

struct ModifierCollector {
    source: String,
    modifiers: Vec<ModifierInfo>,
    def_nodes: Vec<DefInfo>,
    // bare modifiers (no args): name, line, scope_id, node_start, node_end
    bare_modifiers: Vec<BareModifierInfo>,
    block_depth: usize,
    scope_depth: usize,
    current_scope_id: usize,
    next_scope_id: usize,
    // stack for scope boundaries
    scope_end_offsets: Vec<usize>,
}

#[derive(Debug, Clone)]
struct BareModifierInfo {
    name: String,
    scope_id: usize,
    node_start: usize,
    node_end: usize,
    line: u32,
}

impl ModifierCollector {
    fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
            modifiers: Vec::new(),
            def_nodes: Vec::new(),
            bare_modifiers: Vec::new(),
            block_depth: 0,
            scope_depth: 0,
            current_scope_id: 0,
            next_scope_id: 1,
            scope_end_offsets: Vec::new(),
        }
    }

    fn classify_arguments(node: &ruby_prism::CallNode, source: &str) -> (bool, ModifierArgKind, usize, usize, Vec<String>) {
        if let Some(args) = node.arguments() {
            let args_list: Vec<_> = args.arguments().iter().collect();
            if args_list.is_empty() { return (false, ModifierArgKind::None, 0, 0, vec![]); }

            let first_arg = &args_list[0];
            let arg_start = first_arg.location().start_offset();
            // end = last arg's end
            let last_arg = &args_list[args_list.len() - 1];
            let arg_end = last_arg.location().end_offset();

            if first_arg.as_def_node().is_some() { return (true, ModifierArgKind::DefNode, arg_start, arg_end, vec![]); }

            // Collect symbol names
            let mut symbol_names = Vec::new();
            for a in &args_list {
                if let Some(sym) = a.as_symbol_node() {
                    let bytes = sym.unescaped();
                    let name = String::from_utf8_lossy(&bytes).to_string();
                    symbol_names.push(name);
                }
            }

            if first_arg.as_symbol_node().is_some() { return (true, ModifierArgKind::Symbol, arg_start, arg_end, symbol_names); }
            if first_arg.as_splat_node().is_some() { return (true, ModifierArgKind::Splat, arg_start, arg_end, vec![]); }
            if let Some(call) = first_arg.as_call_node() {
                let call_name = node_name!(call);
                if ATTR_METHODS.contains(&call_name.as_ref()) { return (true, ModifierArgKind::AttrMethod, arg_start, arg_end, vec![]); }
                if call_name == "alias_method" { return (true, ModifierArgKind::AliasMethod, arg_start, arg_end, vec![]); }
                return (true, ModifierArgKind::Other, arg_start, arg_end, vec![]);
            }
            return (true, ModifierArgKind::Other, arg_start, arg_end, vec![]);
        }

        let loc = node.location();
        if loc.end_offset() <= source.len() && source[loc.start_offset()..loc.end_offset()].contains('(') {
            return (true, ModifierArgKind::Other, 0, 0, vec![]);
        }
        (false, ModifierArgKind::None, 0, 0, vec![])
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
        let end_off = node.location().end_offset();
        self.scope_end_offsets.push(end_off);
        ruby_prism::visit_class_node(self, node);
        self.scope_end_offsets.pop();
        self.scope_depth -= 1;
        self.current_scope_id = prev_scope_id;
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        let prev_scope_id = self.current_scope_id;
        self.current_scope_id = self.next_scope_id;
        self.next_scope_id += 1;
        self.scope_depth += 1;
        let end_off = node.location().end_offset();
        self.scope_end_offsets.push(end_off);
        ruby_prism::visit_module_node(self, node);
        self.scope_end_offsets.pop();
        self.scope_depth -= 1;
        self.current_scope_id = prev_scope_id;
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        let prev_scope_id = self.current_scope_id;
        self.current_scope_id = self.next_scope_id;
        self.next_scope_id += 1;
        self.scope_depth += 1;
        let end_off = node.location().end_offset();
        self.scope_end_offsets.push(end_off);
        ruby_prism::visit_singleton_class_node(self, node);
        self.scope_end_offsets.pop();
        self.scope_depth -= 1;
        self.current_scope_id = prev_scope_id;
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        // Only track top-level of current scope (not nested defs)
        let name = node_name!(node).to_string();
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        self.def_nodes.push(DefInfo {
            name,
            start,
            end,
            scope_id: self.current_scope_id,
        });
        // Don't recurse into def body for def_nodes collection (keep shallow)
        // but we do need to visit for nested classes. Actually don't visit
        // to avoid false def detections inside methods.
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let name_str = node_name!(node).to_string();

        if ACCESS_MODIFIERS.contains(&name_str.as_str()) {
            let msg_loc = node.message_loc().unwrap();
            let start_offset = msg_loc.start_offset();
            let end_offset = msg_loc.end_offset();
            let loc = Location::from_offsets(&self.source, start_offset, end_offset);

            let (has_arguments, arg_kind, arg_start, arg_end, symbol_names) = Self::classify_arguments(node, &self.source);

            let inside_block = self.block_depth > 0;
            let is_hash_value = self.check_is_hash_value(node);
            let inside_if = self.check_inside_if(node);

            let node_loc = node.location();
            let node_start = node_loc.start_offset();
            let node_end = node_loc.end_offset();

            if !has_arguments {
                // bare modifier
                if !inside_block && !is_hash_value {
                    let msg_line = loc.line;
                    self.bare_modifiers.push(BareModifierInfo {
                        name: name_str.clone(),
                        scope_id: self.current_scope_id,
                        node_start,
                        node_end,
                        line: msg_line,
                    });
                }
            }

            let scope_end_offset = self.scope_end_offsets.last().copied().unwrap_or(0);

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
                node_start,
                node_end,
                arg_start,
                arg_end,
                symbol_names,
                scope_end_offset,
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
        let def_nodes = collector.def_nodes;
        let bare_modifiers = collector.bare_modifiers;

        let mut offenses = Vec::new();

        match self.enforced_style {
            EnforcedStyle::Group => {
                self.check_group_style(&modifiers, &def_nodes, &bare_modifiers, ctx, &mut offenses);
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
        def_nodes: &[DefInfo],
        bare_modifiers: &[BareModifierInfo],
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

            // Collect ALL sibling inline modifiers of same name in this scope (including self)
            let siblings = self.collect_all_siblings(modifiers, i);
            let correction = self.build_group_correction(info, &siblings, def_nodes, bare_modifiers, ctx.source);

            let mut offense = Offense::new(
                self.name(),
                &message,
                self.severity(),
                Location::new(info.line, info.column_start, info.line, info.column_end),
                ctx.filename,
            );
            if let Some(corr) = correction {
                offense = offense.with_correction(corr);
            }
            offenses.push(offense);
        }
    }

    /// Collect all sibling inline modifiers (same name, scope, not skipped) including self.
    fn collect_all_siblings<'a>(&self, modifiers: &'a [ModifierInfo], current_idx: usize) -> Vec<&'a ModifierInfo> {
        let current = &modifiers[current_idx];
        let mut result = Vec::new();

        for (i, m) in modifiers.iter().enumerate() {
            if m.scope_id != current.scope_id { continue; }
            if m.modifier_name != current.modifier_name { continue; }
            if m.inside_block || m.is_hash_value || m.inside_if { continue; }
            if !m.has_arguments { continue; }
            if m.scope_depth == 0 && is_symbol_like_arg(&m.arg_kind) { continue; }
            if self.should_allow_for_group(m) { continue; }
            // Include this sibling only if it comes before or IS current (i.e. was suppressed or is current)
            // Actually: collect all in scope with same name; the "last" one is current_idx
            let _ = i;
            result.push(m);
        }
        result
    }

    fn build_group_correction(
        &self,
        info: &ModifierInfo,
        siblings: &[&ModifierInfo],
        def_nodes: &[DefInfo],
        bare_modifiers: &[BareModifierInfo],
        source: &str,
    ) -> Option<Correction> {
        match &info.arg_kind {
            ModifierArgKind::DefNode => {
                self.correct_inline_def_all(info, siblings, bare_modifiers, source)
            }
            ModifierArgKind::Symbol | ModifierArgKind::Splat | ModifierArgKind::Other |
            ModifierArgKind::AttrMethod | ModifierArgKind::AliasMethod => {
                self.correct_inline_non_def(info, def_nodes, bare_modifiers, source)
            }
            ModifierArgKind::None => None,
        }
    }

    /// Fix all sibling inline-def modifiers by grouping their defs together.
    fn correct_inline_def_all(
        &self,
        info: &ModifierInfo,
        siblings: &[&ModifierInfo],
        bare_modifiers: &[BareModifierInfo],
        source: &str,
    ) -> Option<Correction> {
        // Sort siblings by node_start
        let mut sorted_siblings: Vec<&ModifierInfo> = siblings.to_vec();
        sorted_siblings.sort_by_key(|s| s.node_start);

        // Build combined def sources with optional preceding comments
        let mut def_blocks: Vec<String> = Vec::new();
        for sib in &sorted_siblings {
            let preceding_comment = self.find_preceding_comment(sib.node_start, source);
            let arg_source = &source[sib.arg_start..sib.arg_end];
            if let Some((cmt_start, _)) = preceding_comment {
                let cmt_src = self.line_source_trimmed(cmt_start, source);
                def_blocks.push(format!("{}\n{}", cmt_src, arg_source));
            } else {
                def_blocks.push(arg_source.to_string());
            }
        }

        let bare = bare_modifiers.iter().find(|b| {
            b.name == info.modifier_name && b.scope_id == info.scope_id
        });

        if let Some(bare_mod) = bare {
            // Insert all defs after the bare modifier; delete all inline sibling nodes
            let defs_text: Vec<String> = def_blocks.iter().map(|d| format!("\n\n{}", d)).collect();
            let insert_text = defs_text.join("");

            let mut edits = vec![
                Edit {
                    start_offset: bare_mod.node_end,
                    end_offset: bare_mod.node_end,
                    replacement: insert_text,
                },
            ];

            // Delete all sibling inline nodes
            for sib in &sorted_siblings {
                let preceding_comment = self.find_preceding_comment(sib.node_start, source);
                let del_start = if let Some((cmt_start, _)) = preceding_comment {
                    self.line_start(cmt_start, source)
                } else {
                    self.line_start(sib.node_start, source)
                };
                let del_end = self.line_end_inc(sib.node_end, source);
                edits.push(Edit {
                    start_offset: del_start,
                    end_offset: del_end,
                    replacement: String::new(),
                });
            }

            Some(Correction { edits })
        } else if info.scope_end_offset > 0 {
            // Has enclosing class/module/sclass → insert before class `end`
            // `corrector.insert_before(ancestor.loc.end, "modifier\n\ndefs\n")`
            // The class `end` offset points to the `end` keyword start.
            let scope_end = info.scope_end_offset;
            // Find start of the `end` keyword line (the last `end` of the class)
            // scope_end is the end of the class node = after the final `\n` or the `end` keyword.
            // Actually scope_end is the byte after the `end` keyword.
            // We want to insert BEFORE the `end` line.
            let end_keyword_start = self.line_start(scope_end.saturating_sub(1), source);

            let all_defs = def_blocks.join("\n\n");
            let insert_text = format!("{}\n\n{}\n", info.modifier_name, all_defs);

            let mut edits = vec![
                Edit {
                    start_offset: end_keyword_start,
                    end_offset: end_keyword_start,
                    replacement: insert_text,
                },
            ];

            // Delete all sibling inline nodes
            for sib in &sorted_siblings {
                let preceding_comment = self.find_preceding_comment(sib.node_start, source);
                let del_start = if let Some((cmt_start, _)) = preceding_comment {
                    self.line_start(cmt_start, source)
                } else {
                    self.line_start(sib.node_start, source)
                };
                let del_end = self.line_end_inc(sib.node_end, source);
                edits.push(Edit {
                    start_offset: del_start,
                    end_offset: del_end,
                    replacement: String::new(),
                });
            }

            Some(Correction { edits })
        } else {
            // Top-level (no class) → replace the only sibling
            // `corrector.replace(node, "modifier\n\ndef_source")`
            if sorted_siblings.is_empty() { return None; }

            // Only one sibling at top level
            let first = sorted_siblings[0];
            let preceding_comment_first = self.find_preceding_comment(first.node_start, source);

            let first_line_start = if let Some((cmt_start, _)) = preceding_comment_first {
                self.line_start(cmt_start, source)
            } else {
                self.line_start(first.node_start, source)
            };
            let first_line_end = self.line_end_inc(first.node_end, source);

            let replacement = format!("{}\n\n{}\n", info.modifier_name, def_blocks.join("\n\n"));

            let mut edits = vec![
                Edit {
                    start_offset: first_line_start,
                    end_offset: first_line_end,
                    replacement,
                },
            ];

            // Delete remaining siblings (2nd onwards) if any
            for sib in sorted_siblings.iter().skip(1) {
                let preceding_comment = self.find_preceding_comment(sib.node_start, source);
                let del_start = if let Some((cmt_start, _)) = preceding_comment {
                    self.line_start(cmt_start, source)
                } else {
                    self.line_start(sib.node_start, source)
                };
                let del_end = self.line_end_inc(sib.node_end, source);
                edits.push(Edit {
                    start_offset: del_start,
                    end_offset: del_end,
                    replacement: String::new(),
                });
            }

            Some(Correction { edits })
        }
    }

    fn correct_inline_non_def(
        &self,
        info: &ModifierInfo,
        def_nodes: &[DefInfo],
        bare_modifiers: &[BareModifierInfo],
        source: &str,
    ) -> Option<Correction> {
        let arg_source = &source[info.arg_start..info.arg_end];

        let preceding_comment = self.find_preceding_comment(info.node_start, source);

        if info.arg_kind == ModifierArgKind::Symbol && !info.symbol_names.is_empty() {
            // `private :foo, :bar` → find defs named foo, bar; move to group
            let found_defs: Vec<&DefInfo> = info.symbol_names.iter()
                .filter_map(|name| {
                    def_nodes.iter().find(|d| d.name == *name && d.scope_id == info.scope_id)
                })
                .collect();

            if found_defs.len() != info.symbol_names.len() {
                // Not all defs found → no autocorrect
                return None;
            }

            return self.correct_symbol_list(info, &found_defs, bare_modifiers, source, preceding_comment);
        }

        // For AttrMethod / AliasMethod / Other: split modifier from arg
        let bare = bare_modifiers.iter().find(|b| {
            b.name == info.modifier_name && b.scope_id == info.scope_id
        });

        if let Some(bare_mod) = bare {
            // Insert after bare modifier, delete inline call
            let insert_text = if let Some((cmt_start, _)) = preceding_comment {
                let cmt_src = self.line_source_trimmed(cmt_start, source);
                format!("\n\n{}\n{}", cmt_src, arg_source)
            } else {
                format!("\n\n{}", arg_source)
            };

            let del_start = if let Some((cmt_start, _)) = preceding_comment {
                self.line_start(cmt_start, source)
            } else {
                self.line_start(info.node_start, source)
            };
            let del_end = self.line_end_inc(info.node_end, source);

            let edits = vec![
                Edit {
                    start_offset: bare_mod.node_end,
                    end_offset: bare_mod.node_end,
                    replacement: insert_text,
                },
                Edit {
                    start_offset: del_start,
                    end_offset: del_end,
                    replacement: String::new(),
                },
            ];
            Some(Correction { edits })
        } else if info.scope_end_offset > 0 {
            // Has class ancestor → insert before class end, delete inline node
            let scope_end = info.scope_end_offset;
            let end_keyword_start = self.line_start(scope_end.saturating_sub(1), source);

            let insert_text = if let Some((cmt_start, _)) = preceding_comment {
                let cmt_src = self.line_source_trimmed(cmt_start, source);
                format!("{}\n\n{}\n{}\n", info.modifier_name, cmt_src, arg_source)
            } else {
                format!("{}\n\n{}\n", info.modifier_name, arg_source)
            };

            let del_start = if let Some((cmt_start, _)) = preceding_comment {
                self.line_start(cmt_start, source)
            } else {
                self.line_start(info.node_start, source)
            };
            let del_end = self.line_end_inc(info.node_end, source);

            let edits = vec![
                Edit {
                    start_offset: end_keyword_start,
                    end_offset: end_keyword_start,
                    replacement: insert_text,
                },
                Edit {
                    start_offset: del_start,
                    end_offset: del_end,
                    replacement: String::new(),
                },
            ];
            Some(Correction { edits })
        } else {
            // Top-level: replace in place
            let line_start = if let Some((cmt_start, _)) = preceding_comment {
                self.line_start(cmt_start, source)
            } else {
                self.line_start(info.node_start, source)
            };
            let line_end = self.line_end_inc(info.node_end, source);

            let replacement = if let Some((cmt_start, _)) = preceding_comment {
                let cmt_src = self.line_source_trimmed(cmt_start, source);
                format!("{}\n\n{}\n{}\n", info.modifier_name, cmt_src, arg_source)
            } else {
                format!("{}\n\n{}\n", info.modifier_name, arg_source)
            };

            Some(Correction::replace(line_start, line_end, replacement))
        }
    }

    fn correct_symbol_list(
        &self,
        info: &ModifierInfo,
        found_defs: &[&DefInfo],
        bare_modifiers: &[BareModifierInfo],
        source: &str,
        preceding_comment: Option<(usize, usize)>,
    ) -> Option<Correction> {
        // Build def sources in order they appear in source
        let mut sorted_defs = found_defs.to_vec();
        sorted_defs.sort_by_key(|d| d.start);

        let def_sources: Vec<&str> = sorted_defs.iter().map(|d| &source[d.start..d.end]).collect();

        let bare = bare_modifiers.iter().find(|b| {
            b.name == info.modifier_name && b.scope_id == info.scope_id
        });

        // Build the def block text (preceded by optional comment)
        let defs_joined = def_sources.join("\n");

        if let Some(bare_mod) = bare {
            // Insert defs after bare modifier, delete inline symbol call + original defs
            let insert_text = if let Some((cmt_start, _)) = preceding_comment {
                let cmt_src = self.line_source_trimmed(cmt_start, source);
                format!("\n\n{}\n{}", cmt_src, defs_joined)
            } else {
                format!("\n\n{}", defs_joined)
            };

            let mut edits = vec![
                Edit {
                    start_offset: bare_mod.node_end,
                    end_offset: bare_mod.node_end,
                    replacement: insert_text,
                },
            ];

            // Delete inline modifier node
            let del_start = if let Some((cmt_start, _)) = preceding_comment {
                self.line_start(cmt_start, source)
            } else {
                self.line_start(info.node_start, source)
            };
            let del_end = self.line_end_inc(info.node_end, source);
            edits.push(Edit {
                start_offset: del_start,
                end_offset: del_end,
                replacement: String::new(),
            });

            // Delete original def nodes
            for def in &sorted_defs {
                let ds = self.line_start(def.start, source);
                let de = self.line_end_inc(def.end, source);
                edits.push(Edit {
                    start_offset: ds,
                    end_offset: de,
                    replacement: String::new(),
                });
            }

            Some(Correction { edits })
        } else if info.scope_end_offset > 0 {
            // Has class ancestor → insert before class end, delete inline modifier + original defs
            let scope_end = info.scope_end_offset;
            let end_keyword_start = self.line_start(scope_end.saturating_sub(1), source);

            let insert_text = if let Some((cmt_start, _)) = preceding_comment {
                let cmt_src = self.line_source_trimmed(cmt_start, source);
                format!("{}\n\n{}\n{}\n", info.modifier_name, cmt_src, defs_joined)
            } else {
                format!("{}\n\n{}\n", info.modifier_name, defs_joined)
            };

            let del_start = if let Some((cmt_start, _)) = preceding_comment {
                self.line_start(cmt_start, source)
            } else {
                self.line_start(info.node_start, source)
            };
            let del_end = self.line_end_inc(info.node_end, source);

            let mut edits = vec![
                Edit {
                    start_offset: end_keyword_start,
                    end_offset: end_keyword_start,
                    replacement: insert_text,
                },
                Edit {
                    start_offset: del_start,
                    end_offset: del_end,
                    replacement: String::new(),
                },
            ];

            // Delete original def nodes
            for def in &sorted_defs {
                let ds = self.line_start(def.start, source);
                let de = self.line_end_inc(def.end, source);
                edits.push(Edit {
                    start_offset: ds,
                    end_offset: de,
                    replacement: String::new(),
                });
            }

            Some(Correction { edits })
        } else {
            // Top-level: replace inline modifier call with `modifier\n\ndefs`
            // Also delete original def nodes
            let replacement = if let Some((cmt_start, _)) = preceding_comment {
                let cmt_src = self.line_source_trimmed(cmt_start, source);
                format!("{}\n\n{}\n{}\n", info.modifier_name, cmt_src, defs_joined)
            } else {
                format!("{}\n\n{}\n", info.modifier_name, defs_joined)
            };

            let line_start = if let Some((cmt_start, _)) = preceding_comment {
                self.line_start(cmt_start, source)
            } else {
                self.line_start(info.node_start, source)
            };
            let line_end = self.line_end_inc(info.node_end, source);

            let mut edits = vec![
                Edit {
                    start_offset: line_start,
                    end_offset: line_end,
                    replacement,
                },
            ];

            // Delete original def nodes
            for def in &sorted_defs {
                let ds = self.line_start(def.start, source);
                let de = self.line_end_inc(def.end, source);
                edits.push(Edit {
                    start_offset: ds,
                    end_offset: de,
                    replacement: String::new(),
                });
            }

            Some(Correction { edits })
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

            let correction = self.build_inline_correction(info, ctx.source);

            let mut offense = Offense::new(
                self.name(),
                &message,
                self.severity(),
                Location::new(info.line, info.column_start, info.line, info.column_end),
                ctx.filename,
            );
            if let Some(corr) = correction {
                offense = offense.with_correction(corr);
            }
            offenses.push(offense);
        }
    }

    fn build_inline_correction(&self, info: &ModifierInfo, source: &str) -> Option<Correction> {
        // Find all def lines in the scope that this bare modifier owns.
        // Then: delete the modifier line; prefix each def with `modifier_indent modifier_name `.
        let lines: Vec<&str> = source.lines().collect();
        let modifier_line_idx = (info.line as usize).saturating_sub(1);
        if modifier_line_idx >= lines.len() { return None; }

        let modifier_line = lines[modifier_line_idx];
        let modifier_indent = {
            let trimmed_start = modifier_line.len() - modifier_line.trim_start().len();
            &modifier_line[..trimmed_start]
        };
        let modifier_indent_len = modifier_indent.len();

        // Find def lines that follow at same indent level.
        // We skip blank lines and lines whose content is inside defs (deeper indent or `end`).
        // Strategy: track nesting depth; defs at modifier_indent_len are our targets.
        let mut def_line_indices: Vec<usize> = Vec::new();
        let mut depth: usize = 0;
        let mut i = modifier_line_idx + 1;

        // Handle case where modifier line has semicolon: `private; def foo; end`
        // or `private;` followed by defs on next lines
        let mod_line_after_modifier = {
            let col_end = info.column_end as usize;
            if col_end < modifier_line.len() {
                modifier_line[col_end..].trim()
            } else {
                ""
            }
        };

        // If there's content on the same line after modifier (e.g. `private; def foo; end`)
        // Those are handled by `has_following_def_for_inline` — but we need to handle them in correction too.
        // Detect: if modifier_line contains `; def ` after the modifier keyword
        if mod_line_after_modifier.starts_with(';') {
            let rest = mod_line_after_modifier[1..].trim();
            if rest.starts_with("def ") || rest.starts_with("def\t") {
                // Same-line: `private; def foo...` → `private def foo...`
                // Delete from end of modifier keyword to start of `def`
                let mod_line_start: usize = source.lines().take(modifier_line_idx).map(|l| l.len() + 1).sum();
                // The part after `private` is `; def foo...`
                // Find the start of `def` in the original line
                let after_modifier_start = mod_line_start + info.column_end as usize;
                // `; def` — find where `def` starts from the semicolon
                let chars_to_def = mod_line_after_modifier.find("def").unwrap();
                // del_start = after modifier keyword, del_end = start of `def`
                let del_start = after_modifier_start; // position of `;`
                let del_end = after_modifier_start + chars_to_def; // position of `d` in `def`
                // Insert space + find all following defs on same line and prefix them too
                // For now, handle the simple case: delete `; ` between modifier and each def
                // Also handle multiple defs: `private; def foo; end; def bar; end`
                // → `private def foo; end; private def bar; end`
                // Strategy: replace `;` separators before each `def` with `; private `
                let full_line = modifier_line;
                let col_end = info.column_end as usize;
                let rest_of_line = &full_line[col_end..]; // `; def foo; end; def bar; end`
                // Build corrected rest: for each `; def` → `; private def`, first `; def` → ` def`
                let mut corrected_rest = String::new();
                let mut remaining = rest_of_line;
                let mut first = true;
                loop {
                    if let Some(pos) = remaining.find("; def ") {
                        let before = &remaining[..pos];
                        corrected_rest.push_str(before);
                        if first {
                            corrected_rest.push(' '); // replace `; ` with ` `
                            first = false;
                        } else {
                            corrected_rest.push_str("; ");
                            corrected_rest.push_str(&info.modifier_name);
                            corrected_rest.push(' ');
                        }
                        remaining = &remaining[pos + 2..]; // skip `; ` → `def ...`
                    } else {
                        corrected_rest.push_str(remaining);
                        break;
                    }
                }
                // Replace rest of line
                let _ = (del_start, del_end);
                return Some(Correction::replace(
                    mod_line_start + col_end,
                    mod_line_start + full_line.len(),
                    corrected_rest,
                ));
            }
            // `private;\n` form — the semicolon is the only thing after modifier on the line
            // Fall through to scan next lines, but we need to delete the `;` from the modifier line
            // AND find all following defs.
        }

        while i < lines.len() {
            let trimmed = lines[i].trim();
            if trimmed.is_empty() {
                i += 1;
                continue;
            }
            let line_indent_len = lines[i].len() - lines[i].trim_start().len();

            if depth == 0 && line_indent_len == modifier_indent_len {
                if trimmed.starts_with("def ") || trimmed.starts_with("def\t") {
                    def_line_indices.push(i);
                    // Single-line def: `def foo; end` or `def foo(...) = expr`
                    let is_single_line = trimmed.contains("; end")
                        || trimmed.ends_with("; end")
                        || (trimmed.starts_with("def ") && trimmed.contains(" = ") && !trimmed.contains("\n"));
                    if !is_single_line {
                        depth += 1; // entering def body
                    }
                } else {
                    // Another statement at same level — stop
                    break;
                }
            } else if depth > 0 {
                if trimmed == "end" && line_indent_len == modifier_indent_len {
                    depth -= 1; // closing def
                }
                // else: inside def body, keep going
            }
            i += 1;
        }

        if def_line_indices.is_empty() { return None; }

        // Compute byte offsets for line starts
        let line_starts: Vec<usize> = {
            let mut offsets = Vec::with_capacity(lines.len() + 1);
            let mut off = 0;
            for l in &lines {
                offsets.push(off);
                off += l.len() + 1; // +1 for '\n'
            }
            offsets.push(off);
            offsets
        };

        let mod_line_start = line_starts[modifier_line_idx];
        let first_def_line_start = line_starts[def_line_indices[0]];

        // Delete from start of modifier line to start of first def line
        let del_end = first_def_line_start;

        let mut edits = vec![
            Edit {
                start_offset: mod_line_start,
                end_offset: del_end,
                replacement: String::new(),
            },
        ];

        for def_idx in &def_line_indices {
            let def_line_start = line_starts[*def_idx];
            let def_line = lines[*def_idx];
            let def_trimmed_offset = def_line.len() - def_line.trim_start().len();
            // Replace leading whitespace with modifier_indent + modifier_name + " "
            edits.push(Edit {
                start_offset: def_line_start,
                end_offset: def_line_start + def_trimmed_offset,
                replacement: format!("{}{} ", modifier_indent, info.modifier_name),
            });
        }

        Some(Correction { edits })
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

    // Find a preceding comment line immediately before the node_start
    fn find_preceding_comment(&self, node_start: usize, source: &str) -> Option<(usize, usize)> {
        let line_start = self.line_start(node_start, source);
        if line_start == 0 { return None; }
        // Look at line before
        let prev_line_end = line_start.saturating_sub(1); // skip the \n
        let prev_line_start = self.line_start(prev_line_end.saturating_sub(1), source);
        let prev_line = &source[prev_line_start..prev_line_end];
        if prev_line.trim().starts_with('#') {
            Some((prev_line_start, prev_line_end))
        } else {
            None
        }
    }

    fn line_start(&self, offset: usize, source: &str) -> usize {
        if offset == 0 { return 0; }
        let before = &source[..offset];
        match before.rfind('\n') {
            Some(nl) => nl + 1,
            None => 0,
        }
    }

    fn line_end_inc(&self, offset: usize, source: &str) -> usize {
        // Returns offset after the newline that ends the line containing `offset`
        let after = &source[offset..];
        match after.find('\n') {
            Some(nl) => offset + nl + 1,
            None => source.len(),
        }
    }

    fn line_source_at<'s>(&self, offset: usize, source: &'s str) -> &'s str {
        let start = self.line_start(offset, source);
        let end_nl = source[offset..].find('\n').map(|n| offset + n).unwrap_or(source.len());
        source[start..end_nl].trim_end()
    }

    // Returns the trimmed (no leading whitespace) content of the line at offset
    fn line_source_trimmed<'s>(&self, offset: usize, source: &'s str) -> &'s str {
        self.line_source_at(offset, source).trim_start()
    }
}

fn is_symbol_like_arg(kind: &ModifierArgKind) -> bool {
    matches!(
        kind,
        ModifierArgKind::Symbol | ModifierArgKind::Splat | ModifierArgKind::Other
    )
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
    allow_modifiers_on_symbols: bool,
    allow_modifiers_on_attrs: bool,
    allow_modifiers_on_alias_method: bool,
}
impl Default for Cfg {
    fn default() -> Self {
        Self {
            enforced_style: String::new(),
            allow_modifiers_on_symbols: true,
            allow_modifiers_on_attrs: true,
            allow_modifiers_on_alias_method: true,
        }
    }
}

crate::register_cop!("Style/AccessModifierDeclarations", |cfg| {
    let c: Cfg = cfg.typed("Style/AccessModifierDeclarations");
    let style = match c.enforced_style.as_str() {
        "inline" => EnforcedStyle::Inline,
        _ => EnforcedStyle::Group,
    };
    Some(Box::new(AccessModifierDeclarations::with_config(
        style,
        c.allow_modifiers_on_symbols,
        c.allow_modifiers_on_attrs,
        c.allow_modifiers_on_alias_method,
    )))
});
