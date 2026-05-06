//! Style/ExplicitBlockArgument cop
//!
//! Enforces the use of explicit block arguments instead of passing arguments
//! through intermediate blocks via yield.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Consider using explicit block argument in the surrounding method's signature over `yield`.";

#[derive(Default)]
pub struct ExplicitBlockArgument;

impl ExplicitBlockArgument {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for ExplicitBlockArgument {
    fn name(&self) -> &'static str {
        "Style/ExplicitBlockArgument"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = ExplicitBlockArgumentVisitor {
            ctx,
            offenses: Vec::new(),
            def_stack: Vec::new(),
            def_edited: std::collections::HashSet::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

/// Info about the enclosing def node, needed to build corrections.
struct DefInfo {
    /// name_loc end offset (right after the method name)
    name_end: usize,
    /// lparen offset (if any)
    lparen: Option<usize>,
    /// rparen offset (if any)
    rparen: Option<usize>,
    /// Existing block param name (if already has `&blk`)
    existing_block_param: Option<String>,
    /// Whether there are any non-block params
    has_params: bool,
    /// Last regular param end offset (for inserting `, &block`)
    last_param_end: Option<usize>,
    /// For zsuper: list of param source representations
    zsuper_args: Option<Vec<String>>,
}

struct ExplicitBlockArgumentVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    /// Stack of enclosing def info
    def_stack: Vec<DefInfo>,
    /// Set of def name_end offsets for which we've already emitted a def edit
    def_edited: std::collections::HashSet<usize>,
}

impl<'a> ExplicitBlockArgumentVisitor<'a> {
    fn param_name(node: &Node) -> Option<Vec<u8>> {
        match node {
            Node::RequiredParameterNode { .. } => {
                Some(node.as_required_parameter_node().unwrap().name().as_slice().to_vec())
            }
            _ => None,
        }
    }

    fn arg_name(node: &Node) -> Option<Vec<u8>> {
        match node {
            Node::LocalVariableReadNode { .. } => {
                Some(node.as_local_variable_read_node().unwrap().name().as_slice().to_vec())
            }
            _ => None,
        }
    }

    /// Check if a BlockNode contains only `{ yield args... }` matching block params.
    fn block_is_pure_yield(block: &ruby_prism::BlockNode) -> bool {
        let block_params: Vec<_> = if let Some(params) = block.parameters() {
            match params {
                Node::BlockParametersNode { .. } => {
                    let bp = params.as_block_parameters_node().unwrap();
                    if let Some(inner) = bp.parameters() {
                        inner.requireds().iter().collect()
                    } else {
                        vec![]
                    }
                }
                _ => return false,
            }
        } else {
            vec![]
        };

        let body = match block.body() {
            Some(b) => b,
            None => return false,
        };

        let yield_node_raw = if let Some(stmts) = body.as_statements_node() {
            let mut iter = stmts.body().iter();
            let first = match iter.next() { Some(n) => n, None => return false };
            if iter.next().is_some() { return false; }
            first
        } else {
            body
        };

        let yield_node = match yield_node_raw.as_yield_node() {
            Some(y) => y,
            None => return false,
        };

        let yield_args: Vec<_> = if let Some(args) = yield_node.arguments() {
            args.arguments().iter().collect()
        } else {
            vec![]
        };

        if block_params.is_empty() && yield_args.is_empty() {
            return true;
        }

        if block_params.len() != yield_args.len() {
            return false;
        }

        block_params.iter().zip(yield_args.iter()).all(|(bp, ya)| {
            let bp_name = Self::param_name(bp);
            let ya_name = Self::arg_name(ya);
            bp_name.is_some() && ya_name.is_some() && bp_name == ya_name
        })
    }

    fn get_block_from_opt(block_opt: Option<Node>) -> Option<ruby_prism::BlockNode> {
        block_opt?.as_block_node()
    }

    /// Build def info from a DefNode
    fn build_def_info(node: &ruby_prism::DefNode, source: &str) -> DefInfo {
        let name_end = node.name_loc().end_offset();
        let lparen = node.lparen_loc().map(|l| l.start_offset());
        let rparen = node.rparen_loc().map(|l| l.start_offset());

        let mut existing_block_param = None;
        let mut has_params = false;
        let mut last_param_end = None;
        let mut zsuper_args: Vec<String> = Vec::new();

        if let Some(params) = node.parameters() {
            // Check for existing block param
            if let Some(bp) = params.block() {
                let name_src = source[bp.location().start_offset()..bp.location().end_offset()].to_string();
                // Strip leading `&`
                existing_block_param = Some(name_src.trim_start_matches('&').to_string());
            }

            // Gather all non-block params for has_params and last_param_end
            let requireds: Vec<_> = params.requireds().iter().collect();
            let optionals: Vec<_> = params.optionals().iter().collect();
            let rest = params.rest();
            let posts: Vec<_> = params.posts().iter().collect();
            let keywords: Vec<_> = params.keywords().iter().collect();
            let keyword_rest = params.keyword_rest();

            // Build zsuper arg list (how to convert ForwardingSuperNode to explicit)
            for r in &requireds {
                let s = source[r.location().start_offset()..r.location().end_offset()].trim().to_string();
                // For required params, just use the name
                let name = param_to_arg_src(&s);
                zsuper_args.push(name);
            }
            for o in &optionals {
                let s = source[o.location().start_offset()..o.location().end_offset()].trim().to_string();
                // optional: `x = val` → just `x`
                let name = if let Some(eq_pos) = s.find(" = ") { s[..eq_pos].to_string() } else { s };
                zsuper_args.push(name);
            }
            if let Some(r) = rest.as_ref() {
                let s = source[r.location().start_offset()..r.location().end_offset()].trim().to_string();
                zsuper_args.push(s);
            }
            for p in &posts {
                let s = source[p.location().start_offset()..p.location().end_offset()].trim().to_string();
                zsuper_args.push(param_to_arg_src(&s));
            }
            for k in &keywords {
                let s = source[k.location().start_offset()..k.location().end_offset()].trim().to_string();
                // keyword: `foo:` or `foo: val` → just `foo:`
                let kname = if let Some(colon_pos) = s.find(':') { s[..=colon_pos].to_string() } else { s };
                zsuper_args.push(kname);
            }
            if let Some(kr) = keyword_rest.as_ref() {
                let s = source[kr.location().start_offset()..kr.location().end_offset()].trim().to_string();
                zsuper_args.push(s);
            }

            // Determine has_params and last_param_end (excluding block param)
            let all_non_block: Vec<_> = {
                let mut v: Vec<_> = requireds.iter().map(|n| n.location().end_offset()).collect();
                v.extend(optionals.iter().map(|n| n.location().end_offset()));
                if let Some(r) = rest.as_ref() { v.push(r.location().end_offset()); }
                v.extend(posts.iter().map(|n| n.location().end_offset()));
                v.extend(keywords.iter().map(|n| n.location().end_offset()));
                if let Some(kr) = keyword_rest.as_ref() { v.push(kr.location().end_offset()); }
                v
            };
            has_params = !all_non_block.is_empty();
            last_param_end = all_non_block.into_iter().max();
        }

        DefInfo {
            name_end,
            lparen,
            rparen,
            existing_block_param,
            has_params,
            last_param_end,
            zsuper_args: if zsuper_args.is_empty() { None } else { Some(zsuper_args) },
        }
    }

    /// Build the def edit: add `&block_name` to def params.
    /// Returns None if def already has a block param.
    fn build_def_edit(info: &DefInfo, block_name: &str) -> Option<Edit> {
        if info.existing_block_param.is_some() {
            return None; // already has block param, no change
        }

        let edit = if let Some(rparen) = info.rparen {
            if info.has_params {
                // `def m(x, ...)` → insert `, &block` before `)`
                Edit { start_offset: rparen, end_offset: rparen, replacement: format!(", &{}", block_name) }
            } else {
                // `def m()` → replace `()` with `(&block)`
                let lparen = info.lparen.unwrap_or(info.name_end);
                // rparen is the position of `)`
                Edit { start_offset: lparen, end_offset: rparen + 1, replacement: format!("(&{})", block_name) }
            }
        } else {
            // `def m` with no parens → insert `(&block)` after name
            Edit { start_offset: info.name_end, end_offset: info.name_end, replacement: format!("(&{})", block_name) }
        };
        Some(edit)
    }

    /// Build the call edit: replace `call { |params| yield args }` with `call(&block_name)`.
    /// Returns the replacement for the whole call node.
    fn build_call_replacement(call: &ruby_prism::CallNode, block_name: &str, source: &str) -> String {
        // We need to keep the call's receiver + method + args but replace the block with `&block_name`.
        // Strategy: find the block start offset and remove everything from there onwards,
        // then insert `(&block_name)` or `, &block_name)` or ` &block_name`.

        let block = match Self::get_block_from_opt(call.block()) {
            Some(b) => b,
            None => return source[call.location().start_offset()..call.location().end_offset()].to_string(),
        };

        let block_arg_ref = format!("&{}", block_name);

        // Source of call without block: everything before block start
        let call_start = call.location().start_offset();
        let block_start = block.location().start_offset();
        let before_block = &source[call_start..block_start].trim_end();

        // Check if the call has a closing paren before the block
        // e.g., `foo(a, b) { block }` — before_block ends with `)`
        // We need to place `&block_name` inside the arg list.
        let has_closing_paren = before_block.ends_with(')');
        let has_empty_parens = has_closing_paren && before_block.ends_with("()");

        if has_empty_parens {
            // `foo() { yield }` → `foo(&block)`
            let base = &before_block[..before_block.len() - 2];
            format!("({}&{})", base, block_name)
                // Actually: `foo()` ends with `()`, so base = `foo`, result = `foo(&block)`
                .replacen("(", "", 1) // this is wrong, let me redo
        } else if has_closing_paren {
            // Check for trailing comma: `foo(a, b,)` → `foo(a, b, &block)`
            let inner_end = before_block.len() - 1; // position of `)`
            let inner = &before_block[..inner_end];
            if inner.trim_end().ends_with(',') {
                format!("{} {})", inner, block_arg_ref)
            } else {
                format!("{}, {})", inner, block_arg_ref)
            }
        } else {
            // No parens — add parens: `foo { yield }` → `foo(&block)` — but there might be a space before block
            // Also: `foo a, b { yield }` — but ExplicitBlockArgument only fires for single-expr blocks
            // For `foo` with no args and no parens: just `foo(&block)`
            // For `foo a { yield }` — not valid Ruby for this pattern
            format!("({}{})", before_block, block_arg_ref)
                // Actually before_block includes method name + space after it, but no - let me reconsider
        }
    }

    /// Build correction for a qualifying call.
    fn build_call_correction(
        &self,
        call_start: usize,
        call_end: usize,
        call: Option<&ruby_prism::CallNode>,
        super_node_type: SuperType,
        block: &ruby_prism::BlockNode,
    ) -> Correction {
        let def_info = self.def_stack.last().unwrap();
        let block_name = def_info.existing_block_param.as_deref().unwrap_or("block");

        // Build def edit
        let def_edit = Self::build_def_edit(def_info, block_name);

        // Build call replacement
        let call_replacement = self.build_call_replacement_for(
            call_start, call_end, call, super_node_type, block, block_name, def_info,
        );

        let mut edits = vec![
            Edit { start_offset: call_start, end_offset: call_end, replacement: call_replacement },
        ];
        if let Some(de) = def_edit {
            edits.push(de);
        }

        Correction { edits }
    }

    fn build_call_replacement_for(
        &self,
        call_start: usize,
        call_end: usize,
        call: Option<&ruby_prism::CallNode>,
        super_node_type: SuperType,
        block: &ruby_prism::BlockNode,
        block_name: &str,
        def_info: &DefInfo,
    ) -> String {
        let source = self.ctx.source;
        let block_arg_ref = format!("&{}", block_name);
        let block_start = block.location().start_offset();

        match super_node_type {
            SuperType::NotSuper => {
                // Regular call node
                let call = call.unwrap();
                // Source before the block
                let before_block_raw = &source[call_start..block_start];
                let before_block = before_block_raw.trim_end();

                let has_closing_paren = before_block.ends_with(')');

                if has_closing_paren {
                    // Has parens — check if empty or has content
                    let inner_end = before_block.rfind(')').unwrap();
                    let inner = &before_block[..inner_end];
                    let has_lparen = call.opening_loc().is_some();
                    let _ = has_lparen;

                    let content_inside = {
                        // find the matching open paren
                        let paren_content = find_paren_content(inner);
                        paren_content.trim()
                    };

                    if content_inside.is_empty() {
                        // `foo()` → `foo(&block)`
                        let base = &inner[..inner.rfind('(').map(|p| p).unwrap_or(inner.len())];
                        format!("{}(&{})", base, block_name)
                    } else if content_inside.ends_with(',') {
                        // trailing comma: `foo(a, b,)` → `foo(a, b, &block)`
                        format!("{} {})", &before_block[..inner_end], block_arg_ref)
                    } else {
                        format!("{}, {})", &before_block[..inner_end], block_arg_ref)
                    }
                } else {
                    // No parens before block
                    // `items.something { yield }` → `items.something(&block)`
                    format!("{}({})", before_block, block_arg_ref)
                }
            }
            SuperType::Super => {
                // `super(args) { yield }` or `super() { yield }` → `super(&block)` or `super(args, &block)`
                let before_block_raw = &source[call_start..block_start];
                let before_block = before_block_raw.trim_end();
                let has_closing_paren = before_block.ends_with(')');

                if has_closing_paren {
                    let inner_end = before_block.rfind(')').unwrap();
                    let inner = &before_block[..inner_end];
                    let content_inside = find_paren_content(inner).trim().to_string();

                    if content_inside.is_empty() {
                        // `super()` → `super(&block)`
                        let base = &inner[..inner.rfind('(').unwrap_or(inner.len())];
                        format!("{}(&{})", base, block_name)
                    } else {
                        format!("{}, {})", &before_block[..inner_end], block_arg_ref)
                    }
                } else {
                    // `super { yield }` (SuperNode without args/parens?)
                    format!("{}({})", before_block, block_arg_ref)
                }
            }
            SuperType::ForwardingSuper => {
                // `super { yield }` in zsuper context → explicit super(args, &block)
                let before_block_raw = &source[call_start..block_start];
                let before_block = before_block_raw.trim_end();

                if let Some(ref args) = def_info.zsuper_args {
                    let mut arg_list = args.clone();
                    arg_list.push(format!("&{}", block_name));
                    format!("{}({})", before_block, arg_list.join(", "))
                } else {
                    // No params in def → `super(&block)`
                    format!("{}(&{})", before_block, block_name)
                }
            }
        }
    }

    fn add_offense_with_correction(
        &mut self,
        start: usize,
        end: usize,
        call: Option<&ruby_prism::CallNode>,
        super_type: SuperType,
        block: &ruby_prism::BlockNode,
    ) {
        if self.def_stack.is_empty() {
            return;
        }
        let def_info = self.def_stack.last().unwrap();
        let block_name = def_info.existing_block_param.as_deref().unwrap_or("block");
        let def_name_end = def_info.name_end;

        // Only include def edit if we haven't already for this def
        let include_def_edit = def_info.existing_block_param.is_none()
            && self.def_edited.insert(def_name_end);

        let call_replacement = self.build_call_replacement_for(
            start, end, call, super_type, block, block_name, def_info,
        );

        let mut edits = vec![
            Edit { start_offset: start, end_offset: end, replacement: call_replacement },
        ];

        if include_def_edit {
            if let Some(de) = Self::build_def_edit(def_info, block_name) {
                edits.push(de);
            }
        }

        let correction = Correction { edits };
        let off = self.ctx.offense_with_range(
            "Style/ExplicitBlockArgument",
            MSG,
            Severity::Convention,
            start,
            end,
        ).with_correction(correction);
        self.offenses.push(off);
    }
}

#[derive(Clone, Copy)]
enum SuperType {
    NotSuper,
    Super,
    ForwardingSuper,
}

/// Given source up to and including the outer `(`, find the content inside the last `(...)`
fn find_paren_content(s: &str) -> &str {
    if let Some(open) = s.rfind('(') {
        &s[open + 1..]
    } else {
        ""
    }
}

/// Convert a param source like `x` → `x`, `*args` → `*args`, `**opts` → `**opts`
fn param_to_arg_src(s: &str) -> String {
    s.to_string()
}

impl<'a> Visit<'_> for ExplicitBlockArgumentVisitor<'a> {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let info = ExplicitBlockArgumentVisitor::build_def_info(node, self.ctx.source);
        self.def_stack.push(info);
        ruby_prism::visit_def_node(self, node);
        self.def_stack.pop();
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        if !self.def_stack.is_empty() {
            let block = match ExplicitBlockArgumentVisitor::get_block_from_opt(node.block()) {
                Some(b) => b,
                None => {
                    ruby_prism::visit_call_node(self, node);
                    return;
                }
            };

            if ExplicitBlockArgumentVisitor::block_is_pure_yield(&block) {
                let start = node.location().start_offset();
                let end = node.location().end_offset();
                self.add_offense_with_correction(start, end, Some(node), SuperType::NotSuper, &block);
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_super_node(&mut self, node: &ruby_prism::SuperNode) {
        if !self.def_stack.is_empty() {
            let block = match ExplicitBlockArgumentVisitor::get_block_from_opt(node.block()) {
                Some(b) => b,
                None => {
                    ruby_prism::visit_super_node(self, node);
                    return;
                }
            };
            if ExplicitBlockArgumentVisitor::block_is_pure_yield(&block) {
                let start = node.location().start_offset();
                let end = node.location().end_offset();
                self.add_offense_with_correction(start, end, None, SuperType::Super, &block);
            }
        }
        ruby_prism::visit_super_node(self, node);
    }

    fn visit_forwarding_super_node(&mut self, node: &ruby_prism::ForwardingSuperNode) {
        if !self.def_stack.is_empty() {
            let block = match node.block() {
                Some(b) => b,
                None => {
                    ruby_prism::visit_forwarding_super_node(self, node);
                    return;
                }
            };
            if ExplicitBlockArgumentVisitor::block_is_pure_yield(&block) {
                let start = node.location().start_offset();
                let end = node.location().end_offset();
                self.add_offense_with_correction(start, end, None, SuperType::ForwardingSuper, &block);
            }
        }
        ruby_prism::visit_forwarding_super_node(self, node);
    }
}

crate::register_cop!("Style/ExplicitBlockArgument", |_cfg| {
    Some(Box::new(ExplicitBlockArgument::new()))
});
