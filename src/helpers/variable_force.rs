//! Variable liveness analysis (mirrors RuboCop's VariableForce).
//!
//! Provides scope-based tracking of local variable writes and reads,
//! determining which assignments are "useless" (never read before being
//! overwritten or going out of scope).
//!
//! Used by `Lint/UselessAssignment` and can be reused by other cops like
//! `Lint/ShadowedArgument`, `Lint/UnusedBlockArgument`, etc.

use crate::cops::CheckContext;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

// ── Types ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteKind {
    Simple,
    MultiAssign,
    OpAssign,   // +=, -=, etc.
    AndAssign,  // &&=
    OrAssign,   // ||=
    RegexpCapture,
}

#[derive(Debug, Clone)]
pub struct WriteInfo {
    pub name: String,
    pub name_start: usize,
    pub name_end: usize,
    pub kind: WriteKind,
    pub op: Option<String>,
    pub regexp_start: usize,
    pub regexp_end: usize,
}

pub struct ScopeInfo {
    pub params: HashSet<String>,
    pub has_bare_super: bool,
    pub method_calls: HashSet<String>,
    pub all_var_names: HashSet<String>,
    pub all_reads: HashSet<String>,
}

impl ScopeInfo {
    fn new() -> Self {
        Self {
            params: HashSet::new(),
            has_bare_super: false,
            method_calls: HashSet::new(),
            all_var_names: HashSet::new(),
            all_reads: HashSet::new(),
        }
    }
}

pub struct ScopeAnalyzer<'a> {
    ctx: &'a CheckContext<'a>,
    pub offenses: Vec<Offense>,
}

impl<'a> ScopeAnalyzer<'a> {
    pub fn new(ctx: &'a CheckContext<'a>) -> Self {
        Self {
            ctx,
            offenses: Vec::new(),
        }
    }

    pub fn analyze_program(&mut self, node: &ruby_prism::ProgramNode) {
        let mut scope = ScopeInfo::new();
        let stmts = node.statements();
        let body: Vec<_> = stmts.body().iter().collect();
        // Collect scope info first
        self.collect_scope_info(&body, &mut scope);
        // Analyze
        let live_out = HashSet::new();
        let useless = self.analyze_stmts_for_useless(&body, &live_out, &mut scope);
        self.report_useless(&useless, &scope);
    }

    fn analyze_scope(&mut self, body: &Option<Node>, params: HashSet<String>) {
        let mut scope = ScopeInfo::new();
        scope.params = params;
        if let Some(body_node) = body {
            self.analyze_body_as_scope(body_node, &mut scope);
        }
    }

    fn analyze_body_as_scope(&mut self, body_node: &Node, scope: &mut ScopeInfo) {
        if let Some(stmts_node) = body_node.as_statements_node() {
            let body: Vec<_> = stmts_node.body().iter().collect();
            self.collect_scope_info(&body, scope);
            let live_out = HashSet::new();
            let useless = self.analyze_stmts_for_useless(&body, &live_out, scope);
            self.report_useless(&useless, scope);
        } else {
            // Single node body - wrap in a 1-element slice
            // We need to handle this without clone. Use analyze_node_reverse directly.
            let mut info_collector = ScopeInfoCollector { scope };
            info_collector.visit(body_node);
            let live_out = HashSet::new();
            let mut live = live_out;
            let mut useless = Vec::new();
            self.analyze_node_reverse(body_node, &mut live, &mut useless, scope);
            self.report_useless(&useless, scope);
        }
    }

    /// Collect scope-level info: bare super, method calls, variable names
    fn collect_scope_info(&self, stmts: &[Node], scope: &mut ScopeInfo) {
        let mut collector = ScopeInfoCollector { scope };
        for stmt in stmts {
            collector.visit(stmt);
        }
    }

    /// Analyze statements for useless assignments using reverse-flow analysis.
    /// `live_out`: variables that are live at the end of these statements.
    /// Returns list of useless writes.
    fn analyze_stmts_for_useless(
        &mut self,
        stmts: &[Node],
        live_out: &HashSet<String>,
        scope: &mut ScopeInfo,
    ) -> Vec<WriteInfo> {
        let mut live = live_out.clone();
        let mut useless = Vec::new();

        // Process statements in reverse order
        for stmt in stmts.iter().rev() {
            self.analyze_node_reverse(stmt, &mut live, &mut useless, scope);
        }

        useless
    }

    /// Process a single node in reverse. Updates `live` set and collects useless writes.
    fn analyze_node_reverse(
        &mut self,
        node: &Node,
        live: &mut HashSet<String>,
        useless: &mut Vec<WriteInfo>,
        scope: &mut ScopeInfo,
    ) {
        match node {
            Node::LocalVariableWriteNode { .. } => {
                let write = node.as_local_variable_write_node().unwrap();
                let name = name_str(&write.name());
                if name.starts_with('_') {
                    self.process_rhs_for_scopes(&write.value());
                    self.collect_reads(&write.value(), live);
                    return;
                }
                self.process_rhs_for_scopes(&write.value());

                // In reverse analysis: first handle the WRITE, then the RHS READS.
                // For simple writes (foo = bar = expr), we DON'T flag inner writes.
                // The outer write handles both.

                // Handle the write: check if the variable is needed after this point
                if live.contains(&name) {
                    live.remove(&name);
                } else {
                    useless.push(WriteInfo {
                        name: name.clone(),
                        name_start: write.name_loc().start_offset(),
                        name_end: write.name_loc().end_offset(),
                        kind: WriteKind::Simple,
                        op: None,
                        regexp_start: 0,
                        regexp_end: 0,
                    });
                }

                // Now collect reads from the RHS - these make earlier writes live
                self.collect_reads(&write.value(), live);
                // Process any nested writes in the RHS (e.g., foo = [1, bar = 2])
                self.process_nested_writes(&write.value(), live, useless, scope);
                // Process blocks in the RHS (e.g., foo = do_something { bar = ... })
                self.process_blocks_in_rhs(&write.value(), live, useless, scope);

                scope.all_var_names.insert(name);
            }

            Node::LocalVariableOperatorWriteNode { .. } => {
                let write = node.as_local_variable_operator_write_node().unwrap();
                let name = name_str(&write.name());
                if name.starts_with('_') {
                    self.process_rhs_for_scopes(&write.value());
                    self.collect_reads(&write.value(), live);
                    return;
                }
                self.process_rhs_for_scopes(&write.value());
                let op = String::from_utf8_lossy(write.binary_operator().as_slice()).to_string();

                // Handle inner writes to same variable (e.g., foo += foo = 2)
                self.collect_same_var_rhs_writes(&name, &write.value(), live, useless, scope);

                // Reverse order: write first, then read
                if live.contains(&name) {
                    live.remove(&name);
                } else {
                    useless.push(WriteInfo {
                        name: name.clone(),
                        name_start: write.name_loc().start_offset(),
                        name_end: write.name_loc().end_offset(),
                        kind: WriteKind::OpAssign,
                        op: Some(op),
                        regexp_start: 0,
                        regexp_end: 0,
                    });
                }
                // Op-assign always reads the variable
                live.insert(name.clone());
                // And reads from the RHS expression
                self.collect_reads(&write.value(), live);
                scope.all_var_names.insert(name);
            }

            Node::LocalVariableAndWriteNode { .. } => {
                let write = node.as_local_variable_and_write_node().unwrap();
                let name = name_str(&write.name());
                if name.starts_with('_') {
                    self.process_rhs_for_scopes(&write.value());
                    self.collect_reads(&write.value(), live);
                    return;
                }
                self.process_rhs_for_scopes(&write.value());

                if live.contains(&name) {
                    live.remove(&name);
                } else {
                    useless.push(WriteInfo {
                        name: name.clone(),
                        name_start: write.name_loc().start_offset(),
                        name_end: write.name_loc().end_offset(),
                        kind: WriteKind::AndAssign,
                        op: Some("&&".to_string()),
                        regexp_start: 0,
                        regexp_end: 0,
                    });
                }
                live.insert(name.clone());
                self.collect_reads(&write.value(), live);
                scope.all_var_names.insert(name);
            }

            Node::LocalVariableOrWriteNode { .. } => {
                let write = node.as_local_variable_or_write_node().unwrap();
                let name = name_str(&write.name());
                if name.starts_with('_') {
                    self.process_rhs_for_scopes(&write.value());
                    self.collect_reads(&write.value(), live);
                    return;
                }
                self.process_rhs_for_scopes(&write.value());

                if live.contains(&name) {
                    live.remove(&name);
                } else {
                    useless.push(WriteInfo {
                        name: name.clone(),
                        name_start: write.name_loc().start_offset(),
                        name_end: write.name_loc().end_offset(),
                        kind: WriteKind::OrAssign,
                        op: Some("||".to_string()),
                        regexp_start: 0,
                        regexp_end: 0,
                    });
                }
                live.insert(name.clone());
                self.collect_reads(&write.value(), live);
                scope.all_var_names.insert(name);
            }

            Node::LocalVariableReadNode { .. } => {
                let read = node.as_local_variable_read_node().unwrap();
                let name = name_str(&read.name());
                live.insert(name);
            }

            Node::MultiWriteNode { .. } => {
                let multi = node.as_multi_write_node().unwrap();
                self.process_rhs_for_scopes(&multi.value());
                // Reverse: handle writes FIRST, then reads from RHS
                let rights: Vec<_> = multi.rights().iter().collect();
                for target in rights.iter().rev() {
                    self.handle_multi_target(target, live, useless, scope);
                }
                if let Some(rest) = multi.rest() {
                    self.handle_multi_target(&rest, live, useless, scope);
                }
                let lefts: Vec<_> = multi.lefts().iter().collect();
                for target in lefts.iter().rev() {
                    self.handle_multi_target(target, live, useless, scope);
                }
                // Now collect reads from RHS
                self.collect_reads(&multi.value(), live);
                // Process nested writes in RHS (e.g., a, b = func(c = 3))
                self.process_nested_writes(&multi.value(), live, useless, scope);
            }

            Node::MatchWriteNode { .. } => {
                let mw = node.as_match_write_node().unwrap();
                let call = mw.call();
                // Handle named capture writes FIRST (reverse order)
                let targets: Vec<_> = mw.targets().iter().collect();
                let regexp_loc = call.receiver().map(|r| (r.location().start_offset(), r.location().end_offset()));
                for target in targets.iter().rev() {
                    if let Some(lv) = target.as_local_variable_target_node() {
                        let name = name_str(&lv.name());
                        if name.starts_with('_') { continue; }
                        let (rs, re) = regexp_loc.unwrap_or((lv.location().start_offset(), lv.location().end_offset()));
                        if live.contains(&name) {
                            live.remove(&name);
                        } else {
                            useless.push(WriteInfo {
                                name: name.clone(),
                                name_start: lv.location().start_offset(),
                                name_end: lv.location().end_offset(),
                                kind: WriteKind::RegexpCapture,
                                op: None,
                                regexp_start: rs,
                                regexp_end: re,
                            });
                        }
                        scope.all_var_names.insert(name);
                    }
                }
                // Now collect reads from the RHS
                if let Some(args) = call.arguments() {
                    for arg in args.arguments().iter() {
                        self.collect_reads(&arg, live);
                    }
                }
                if let Some(recv) = call.receiver() {
                    self.collect_reads(&recv, live);
                }
            }

            // ── Scope-creating nodes ──
            Node::DefNode { .. } => {
                let def = node.as_def_node().unwrap();
                // Receiver is in outer scope
                if let Some(recv) = def.receiver() {
                    self.collect_reads(&recv, live);
                }
                let params = extract_param_names(&def);
                self.analyze_scope(&def.body(), params);
            }

            Node::ClassNode { .. } => {
                let class = node.as_class_node().unwrap();
                if let Some(superclass) = class.superclass() {
                    self.collect_reads(&superclass, live);
                }
                self.analyze_scope(&class.body(), HashSet::new());
            }

            Node::ModuleNode { .. } => {
                let module = node.as_module_node().unwrap();
                self.collect_reads(&module.constant_path(), live);
                self.analyze_scope(&module.body(), HashSet::new());
            }

            Node::SingletonClassNode { .. } => {
                let sc = node.as_singleton_class_node().unwrap();
                self.collect_reads(&sc.expression(), live);
                self.analyze_scope(&sc.body(), HashSet::new());
            }

            // ── Block/Lambda (captures outer variables) ──
            Node::BlockNode { .. } => {
                let block = node.as_block_node().unwrap();
                self.analyze_block_for_outer(block, live, useless, scope);
            }

            Node::LambdaNode { .. } => {
                let lambda = node.as_lambda_node().unwrap();
                let mut collector = VarRefCollector::new();
                collector.visit_lambda_node(&lambda);
                // Variables read in lambda make outer writes live
                for var in &collector.referenced_vars {
                    live.insert(var.clone());
                }
                // Variables written in lambda also make outer writes live
                // (the lambda captures the binding)
                for var in &collector.written_vars {
                    live.insert(var.clone());
                }
            }

            // ── If/Unless ──
            Node::IfNode { .. } => {
                let if_node = node.as_if_node().unwrap();
                self.analyze_if_reverse(if_node, live, useless, scope);
            }

            Node::UnlessNode { .. } => {
                let unless = node.as_unless_node().unwrap();
                self.analyze_unless_reverse(unless, live, useless, scope);
            }

            // ── Loops ──
            Node::WhileNode { .. } => {
                let w = node.as_while_node().unwrap();
                self.analyze_while_reverse(w, live, useless, scope);
            }

            Node::UntilNode { .. } => {
                let u = node.as_until_node().unwrap();
                self.analyze_until_reverse(u, live, useless, scope);
            }

            Node::ForNode { .. } => {
                let f = node.as_for_node().unwrap();
                self.analyze_for_reverse(f, live, useless, scope);
            }

            // ── Begin/Rescue ──
            Node::BeginNode { .. } => {
                let b = node.as_begin_node().unwrap();
                self.analyze_begin_reverse(b, live, useless, scope);
            }

            // ── Case ──
            Node::CaseNode { .. } => {
                let c = node.as_case_node().unwrap();
                self.analyze_case_reverse(c, live, useless, scope);
            }

            Node::CaseMatchNode { .. } => {
                let c = node.as_case_match_node().unwrap();
                self.analyze_case_match_reverse(c, live, useless, scope);
            }

            // ── And/Or (short-circuit: right side may not execute) ──
            Node::AndNode { .. } => {
                let and = node.as_and_node().unwrap();
                let live_after = live.clone();
                // Right side may or may not execute
                let mut right_live = live_after.clone();
                self.analyze_node_reverse(&and.right(), &mut right_live, useless, scope);
                // Left side always executes; union with the path where right doesn't
                *live = right_live;
                for var in &live_after {
                    live.insert(var.clone());
                }
                self.analyze_node_reverse(&and.left(), live, useless, scope);
            }

            Node::OrNode { .. } => {
                let or = node.as_or_node().unwrap();
                let live_after = live.clone();
                let mut right_live = live_after.clone();
                self.analyze_node_reverse(&or.right(), &mut right_live, useless, scope);
                *live = right_live;
                for var in &live_after {
                    live.insert(var.clone());
                }
                self.analyze_node_reverse(&or.left(), live, useless, scope);
            }

            // ── Super ──
            Node::ForwardingSuperNode { .. } => {
                scope.has_bare_super = true;
            }

            // ── Pattern matching ──
            Node::MatchPredicateNode { .. } => {
                let mp = node.as_match_predicate_node().unwrap();
                self.collect_pattern_writes(&mp.pattern(), live, useless, scope);
                self.collect_reads(&mp.value(), live);
            }

            Node::MatchRequiredNode { .. } => {
                let mr = node.as_match_required_node().unwrap();
                self.collect_pattern_writes(&mr.pattern(), live, useless, scope);
                self.collect_reads(&mr.value(), live);
            }

            // ── Call nodes (may have blocks) ──
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                // Process the block first (reverse order)
                if let Some(block_node) = call.block() {
                    self.analyze_node_reverse(&block_node, live, useless, scope);
                }
                // Collect reads from arguments and receiver
                if let Some(args) = call.arguments() {
                    for arg in args.arguments().iter() {
                        self.analyze_node_reverse(&arg, live, useless, scope);
                    }
                }
                if let Some(recv) = call.receiver() {
                    self.collect_reads(&recv, live);
                }
                // Track variable-like method calls for "did you mean"
                if call.receiver().is_none() {
                    if let Some(msg_loc) = call.message_loc() {
                        let name = String::from_utf8_lossy(msg_loc.as_slice()).to_string();
                        let has_args = if let Some(args) = call.arguments() {
                            args.arguments().len() > 0
                        } else {
                            false
                        };
                        if !has_args && call.block().is_none() {
                            scope.method_calls.insert(name.clone());
                        }
                        if call.is_variable_call() {
                            scope.method_calls.insert(name);
                        }
                    }
                }
            }

            // ── Parentheses ──
            Node::ParenthesesNode { .. } => {
                let parens = node.as_parentheses_node().unwrap();
                if let Some(body) = parens.body() {
                    if let Some(stmts) = body.as_statements_node() {
                        let items: Vec<_> = stmts.body().iter().collect();
                        for stmt in items.iter().rev() {
                            self.analyze_node_reverse(stmt, live, useless, scope);
                        }
                    } else {
                        self.analyze_node_reverse(&body, live, useless, scope);
                    }
                }
            }

            // ── Everything else: just collect reads ──
            _ => {
                self.process_rhs_for_scopes(node);
                self.collect_reads(node, live);
            }
        }
    }

    fn handle_multi_target(
        &mut self,
        target: &Node,
        live: &mut HashSet<String>,
        useless: &mut Vec<WriteInfo>,
        scope: &mut ScopeInfo,
    ) {
        match target {
            Node::LocalVariableTargetNode { .. } => {
                let lv = target.as_local_variable_target_node().unwrap();
                let name = name_str(&lv.name());
                if name.starts_with('_') { return; }
                if live.contains(&name) {
                    live.remove(&name);
                } else {
                    useless.push(WriteInfo {
                        name: name.clone(),
                        name_start: lv.location().start_offset(),
                        name_end: lv.location().end_offset(),
                        kind: WriteKind::MultiAssign,
                        op: None,
                        regexp_start: 0,
                        regexp_end: 0,
                    });
                }
                scope.all_var_names.insert(name);
            }
            Node::SplatNode { .. } => {
                let splat = target.as_splat_node().unwrap();
                if let Some(expr) = splat.expression() {
                    self.handle_multi_target(&expr, live, useless, scope);
                }
            }
            Node::MultiTargetNode { .. } => {
                let mt = target.as_multi_target_node().unwrap();
                let rights: Vec<_> = mt.rights().iter().collect();
                for t in rights.iter().rev() {
                    self.handle_multi_target(t, live, useless, scope);
                }
                if let Some(rest) = mt.rest() {
                    self.handle_multi_target(&rest, live, useless, scope);
                }
                let lefts: Vec<_> = mt.lefts().iter().collect();
                for t in lefts.iter().rev() {
                    self.handle_multi_target(t, live, useless, scope);
                }
            }
            _ => {}
        }
    }

    fn collect_pattern_writes(
        &mut self,
        pattern: &Node,
        live: &mut HashSet<String>,
        useless: &mut Vec<WriteInfo>,
        scope: &mut ScopeInfo,
    ) {
        match pattern {
            Node::LocalVariableTargetNode { .. } => {
                let lv = pattern.as_local_variable_target_node().unwrap();
                let name = name_str(&lv.name());
                if name.starts_with('_') { return; }
                if live.contains(&name) {
                    live.remove(&name);
                } else {
                    useless.push(WriteInfo {
                        name: name.clone(),
                        name_start: lv.location().start_offset(),
                        name_end: lv.location().end_offset(),
                        kind: WriteKind::Simple,
                        op: None,
                        regexp_start: 0,
                        regexp_end: 0,
                    });
                }
                scope.all_var_names.insert(name);
            }
            Node::HashPatternNode { .. } => {
                let hp = pattern.as_hash_pattern_node().unwrap();
                for elem in hp.elements().iter() {
                    self.collect_pattern_writes(&elem, live, useless, scope);
                }
                if let Some(rest) = hp.rest() {
                    self.collect_pattern_writes(&rest, live, useless, scope);
                }
            }
            Node::ArrayPatternNode { .. } => {
                let ap = pattern.as_array_pattern_node().unwrap();
                for elem in ap.requireds().iter() {
                    self.collect_pattern_writes(&elem, live, useless, scope);
                }
                if let Some(rest) = ap.rest() {
                    self.collect_pattern_writes(&rest, live, useless, scope);
                }
                for elem in ap.posts().iter() {
                    self.collect_pattern_writes(&elem, live, useless, scope);
                }
            }
            Node::AssocNode { .. } => {
                let assoc = pattern.as_assoc_node().unwrap();
                self.collect_pattern_writes(&assoc.value(), live, useless, scope);
            }
            Node::CapturePatternNode { .. } => {
                let cp = pattern.as_capture_pattern_node().unwrap();
                let target = cp.target();
                let name = name_str(&target.name());
                if !name.starts_with('_') {
                    if live.contains(&name) {
                        live.remove(&name);
                    } else {
                        useless.push(WriteInfo {
                            name: name.clone(),
                            name_start: target.location().start_offset(),
                            name_end: target.location().end_offset(),
                            kind: WriteKind::Simple,
                            op: None,
                            regexp_start: 0,
                            regexp_end: 0,
                        });
                    }
                    scope.all_var_names.insert(name);
                }
                self.collect_pattern_writes(&cp.value(), live, useless, scope);
            }
            Node::SplatNode { .. } => {
                let splat = pattern.as_splat_node().unwrap();
                if let Some(expr) = splat.expression() {
                    self.collect_pattern_writes(&expr, live, useless, scope);
                }
            }
            Node::FindPatternNode { .. } => {
                let fp = pattern.as_find_pattern_node().unwrap();
                let left = fp.left();
                if let Some(expr) = left.expression() {
                    self.collect_pattern_writes(&expr, live, useless, scope);
                }
                for elem in fp.requireds().iter() {
                    self.collect_pattern_writes(&elem, live, useless, scope);
                }
                let right = fp.right();
                if let Some(splat) = right.as_splat_node() {
                    if let Some(expr) = splat.expression() {
                        self.collect_pattern_writes(&expr, live, useless, scope);
                    }
                }
            }
            Node::PinnedVariableNode { .. } => {
                let pv = pattern.as_pinned_variable_node().unwrap();
                self.collect_reads(&pv.variable(), live);
            }
            _ => {}
        }
    }

    fn analyze_if_reverse(
        &mut self,
        if_node: ruby_prism::IfNode,
        live: &mut HashSet<String>,
        useless: &mut Vec<WriteInfo>,
        scope: &mut ScopeInfo,
    ) {
        // For if/elsif/else: the live-out set is what's live after the whole if.
        // Each branch gets analyzed with the same live-out set.
        // The live-in is the union of all branches' live-in sets + condition reads.

        let is_modifier = if let Some(kw_loc) = if_node.if_keyword_loc() {
            kw_loc.start_offset() > if_node.predicate().location().start_offset()
        } else {
            false
        };

        if is_modifier {
            // Modifier if: condition and body are in same flow.
            // `foo = 1 if cond` -> cond is evaluated first, then maybe body
            // In reverse: body first (may or may not execute), then condition
            if let Some(stmts) = if_node.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                // The body may or may not execute, so we union its live set
                let mut branch_live = live.clone();
                for stmt in body.iter().rev() {
                    self.analyze_node_reverse(stmt, &mut branch_live, useless, scope);
                }
                // Union with the main live set
                for var in branch_live {
                    live.insert(var);
                }
            }
            self.collect_reads(&if_node.predicate(), live);
            return;
        }

        // Regular if: collect live sets from all branches
        let live_after = live.clone();

        // Analyze if branch
        let mut if_live = live_after.clone();
        if let Some(stmts) = if_node.statements() {
            let body: Vec<_> = stmts.body().iter().collect();
            for stmt in body.iter().rev() {
                self.analyze_node_reverse(stmt, &mut if_live, useless, scope);
            }
        }

        // Analyze else/elsif branch
        let mut else_live = live_after.clone();
        if let Some(subsequent) = if_node.subsequent() {
            self.analyze_subsequent_reverse(&subsequent, &mut else_live, useless, scope);
        }

        // The live set before the if is the union of both branches
        *live = if_live;
        for var in else_live {
            live.insert(var);
        }

        // Analyze the condition
        self.collect_reads(&if_node.predicate(), live);
    }

    fn analyze_subsequent_reverse(
        &mut self,
        node: &Node,
        live: &mut HashSet<String>,
        useless: &mut Vec<WriteInfo>,
        scope: &mut ScopeInfo,
    ) {
        if let Some(if_node) = node.as_if_node() {
            self.analyze_if_reverse(if_node, live, useless, scope);
        } else if let Some(else_node) = node.as_else_node() {
            if let Some(stmts) = else_node.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                for stmt in body.iter().rev() {
                    self.analyze_node_reverse(stmt, live, useless, scope);
                }
            }
        }
    }

    fn analyze_unless_reverse(
        &mut self,
        unless: ruby_prism::UnlessNode,
        live: &mut HashSet<String>,
        useless: &mut Vec<WriteInfo>,
        scope: &mut ScopeInfo,
    ) {
        let kw_loc = unless.keyword_loc();
        let is_modifier = kw_loc.start_offset() > unless.predicate().location().start_offset();

        if is_modifier {
            if let Some(stmts) = unless.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                let mut branch_live = live.clone();
                for stmt in body.iter().rev() {
                    self.analyze_node_reverse(stmt, &mut branch_live, useless, scope);
                }
                for var in branch_live {
                    live.insert(var);
                }
            }
            self.collect_reads(&unless.predicate(), live);
            return;
        }

        let live_after = live.clone();

        let mut unless_live = live_after.clone();
        if let Some(stmts) = unless.statements() {
            let body: Vec<_> = stmts.body().iter().collect();
            for stmt in body.iter().rev() {
                self.analyze_node_reverse(stmt, &mut unless_live, useless, scope);
            }
        }

        let mut else_live = live_after.clone();
        if let Some(else_clause) = unless.else_clause() {
            if let Some(stmts) = else_clause.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                for stmt in body.iter().rev() {
                    self.analyze_node_reverse(stmt, &mut else_live, useless, scope);
                }
            }
        }

        *live = unless_live;
        for var in else_live {
            live.insert(var);
        }

        self.collect_reads(&unless.predicate(), live);
    }

    fn analyze_while_reverse(
        &mut self,
        while_node: ruby_prism::WhileNode,
        live: &mut HashSet<String>,
        useless: &mut Vec<WriteInfo>,
        scope: &mut ScopeInfo,
    ) {
        // Loops: analyze body, then union the body's live-in with the loop's live-out.
        // Iterate until fixed point because writes in the body might be read by
        // earlier statements in the next iteration.

        // Collect reads in condition
        self.collect_reads(&while_node.predicate(), live);

        // Collect variables read in the loop condition - these must stay live
        // throughout the loop body because the condition is re-evaluated each iteration
        let mut cond_reads = HashSet::new();
        self.collect_all_reads(&while_node.predicate(), &mut cond_reads);

        if let Some(stmts) = while_node.statements() {
            let body: Vec<_> = stmts.body().iter().collect();
            // Iterate to fixed point
            for _ in 0..3 {
                let prev_live = live.clone();
                let mut body_live = live.clone();
                let mut temp_useless = Vec::new();
                for stmt in body.iter().rev() {
                    self.analyze_node_reverse(stmt, &mut body_live, &mut temp_useless, scope);
                    // Re-add condition reads after each statement
                    for var in &cond_reads {
                        body_live.insert(var.clone());
                    }
                }
                for var in body_live {
                    live.insert(var);
                }
                self.collect_reads(&while_node.predicate(), live);
                if *live == prev_live {
                    break;
                }
            }
            // Final pass to actually record useless writes
            let mut body_live = live.clone();
            for stmt in body.iter().rev() {
                self.analyze_node_reverse(stmt, &mut body_live, useless, scope);
                for var in &cond_reads {
                    body_live.insert(var.clone());
                }
            }
            for var in body_live {
                live.insert(var);
            }
        }

        self.collect_reads(&while_node.predicate(), live);
    }

    fn analyze_until_reverse(
        &mut self,
        until: ruby_prism::UntilNode,
        live: &mut HashSet<String>,
        useless: &mut Vec<WriteInfo>,
        scope: &mut ScopeInfo,
    ) {
        self.collect_reads(&until.predicate(), live);

        let mut cond_reads = HashSet::new();
        self.collect_all_reads(&until.predicate(), &mut cond_reads);

        if let Some(stmts) = until.statements() {
            let body: Vec<_> = stmts.body().iter().collect();
            for _ in 0..3 {
                let prev_live = live.clone();
                let mut body_live = live.clone();
                let mut temp_useless = Vec::new();
                for stmt in body.iter().rev() {
                    self.analyze_node_reverse(stmt, &mut body_live, &mut temp_useless, scope);
                    for var in &cond_reads {
                        body_live.insert(var.clone());
                    }
                }
                for var in body_live {
                    live.insert(var);
                }
                self.collect_reads(&until.predicate(), live);
                if *live == prev_live { break; }
            }
            let mut body_live = live.clone();
            for stmt in body.iter().rev() {
                self.analyze_node_reverse(stmt, &mut body_live, useless, scope);
                for var in &cond_reads {
                    body_live.insert(var.clone());
                }
            }
            for var in body_live {
                live.insert(var);
            }
        }

        self.collect_reads(&until.predicate(), live);
    }

    fn analyze_for_reverse(
        &mut self,
        for_node: ruby_prism::ForNode,
        live: &mut HashSet<String>,
        useless: &mut Vec<WriteInfo>,
        scope: &mut ScopeInfo,
    ) {
        // For loop body
        if let Some(stmts) = for_node.statements() {
            let body: Vec<_> = stmts.body().iter().collect();
            for _ in 0..3 {
                let prev_live = live.clone();
                let mut body_live = live.clone();
                let mut temp_useless = Vec::new();
                for stmt in body.iter().rev() {
                    self.analyze_node_reverse(stmt, &mut body_live, &mut temp_useless, scope);
                }
                for var in body_live {
                    live.insert(var);
                }
                if *live == prev_live { break; }
            }
            let mut body_live = live.clone();
            for stmt in body.iter().rev() {
                self.analyze_node_reverse(stmt, &mut body_live, useless, scope);
            }
            for var in body_live {
                live.insert(var);
            }
        }

        // For index variables - writes (reverse: handle writes before reads)
        self.handle_for_index(&for_node.index(), live, useless, scope);

        // Collection reads (evaluated before the loop, so in reverse comes after)
        self.collect_reads(&for_node.collection(), live);
    }

    fn handle_for_index(
        &mut self,
        node: &Node,
        live: &mut HashSet<String>,
        useless: &mut Vec<WriteInfo>,
        scope: &mut ScopeInfo,
    ) {
        match node {
            Node::LocalVariableTargetNode { .. } => {
                let lv = node.as_local_variable_target_node().unwrap();
                let name = name_str(&lv.name());
                if name.starts_with('_') { return; }
                if live.contains(&name) {
                    live.remove(&name);
                } else {
                    useless.push(WriteInfo {
                        name: name.clone(),
                        name_start: lv.location().start_offset(),
                        name_end: lv.location().end_offset(),
                        kind: WriteKind::MultiAssign,
                        op: None,
                        regexp_start: 0,
                        regexp_end: 0,
                    });
                }
                scope.all_var_names.insert(name);
            }
            Node::MultiTargetNode { .. } => {
                let mt = node.as_multi_target_node().unwrap();
                let rights: Vec<_> = mt.rights().iter().collect();
                for t in rights.iter().rev() {
                    self.handle_for_index(t, live, useless, scope);
                }
                if let Some(rest) = mt.rest() {
                    self.handle_for_index(&rest, live, useless, scope);
                }
                let lefts: Vec<_> = mt.lefts().iter().collect();
                for t in lefts.iter().rev() {
                    self.handle_for_index(t, live, useless, scope);
                }
            }
            _ => {}
        }
    }

    fn analyze_begin_reverse(
        &mut self,
        begin: ruby_prism::BeginNode,
        live: &mut HashSet<String>,
        useless: &mut Vec<WriteInfo>,
        scope: &mut ScopeInfo,
    ) {
        let has_retry = begin_has_retry(&begin);

        // With rescue/ensure, a variable written in the main body may be read in
        // rescue/ensure if an exception occurs at any point. So all writes in
        // the main body that are read in rescue/ensure/else are live.

        // First, collect all reads in rescue/else/ensure
        let mut rescue_reads = HashSet::new();
        let mut rescue_clause = begin.rescue_clause();
        while let Some(rc) = rescue_clause {
            if let Some(stmts) = rc.statements() {
                for stmt in stmts.body().iter() {
                    self.collect_all_reads(&stmt, &mut rescue_reads);
                }
            }
            rescue_clause = rc.subsequent();
        }
        if let Some(else_clause) = begin.else_clause() {
            if let Some(stmts) = else_clause.statements() {
                for stmt in stmts.body().iter() {
                    self.collect_all_reads(&stmt, &mut rescue_reads);
                }
            }
        }
        let mut ensure_reads = HashSet::new();
        if let Some(ensure) = begin.ensure_clause() {
            if let Some(stmts) = ensure.statements() {
                for stmt in stmts.body().iter() {
                    self.collect_all_reads(&stmt, &mut ensure_reads);
                }
            }
        }

        // Check if ensure has writes that override begin/rescue writes
        let mut ensure_writes = HashSet::new();
        if let Some(ensure) = begin.ensure_clause() {
            if let Some(stmts) = ensure.statements() {
                for stmt in stmts.body().iter() {
                    collect_all_writes_in_node(&stmt, &mut ensure_writes);
                }
            }
        }

        // Analyze ensure clause first (executes last)
        if let Some(ensure) = begin.ensure_clause() {
            if let Some(stmts) = ensure.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                for stmt in body.iter().rev() {
                    self.analyze_node_reverse(stmt, live, useless, scope);
                }
            }
        }

        // If ensure unconditionally writes a variable, writes in begin/rescue/else
        // that are the last write before ensure are useless
        // We handle this by NOT adding rescue_reads to live for variables that ensure overwrites.

        // Analyze else clause
        let mut else_live = live.clone();
        if let Some(else_clause) = begin.else_clause() {
            if let Some(stmts) = else_clause.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                for stmt in body.iter().rev() {
                    self.analyze_node_reverse(stmt, &mut else_live, useless, scope);
                }
            }
        }

        // Analyze rescue clauses - each is a separate branch (like if/elsif)
        let mut rescue_combined_live = HashSet::new();
        let mut rescue_clause = begin.rescue_clause();
        while let Some(rc) = rescue_clause {
            let mut branch_live = live.clone();
            // For retry: all reads in rescue bodies become always-live within rescue
            if has_retry {
                for var in &rescue_reads {
                    branch_live.insert(var.clone());
                }
            }
            if let Some(stmts) = rc.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                for stmt in body.iter().rev() {
                    self.analyze_node_reverse(stmt, &mut branch_live, useless, scope);
                    if has_retry {
                        for var in &rescue_reads {
                            branch_live.insert(var.clone());
                        }
                    }
                }
            }
            // Rescue exception variable
            if let Some(reference) = rc.reference() {
                if let Some(lv) = reference.as_local_variable_target_node() {
                    let name = name_str(&lv.name());
                    if !name.starts_with('_') {
                        if branch_live.contains(&name) {
                            branch_live.remove(&name);
                        } else {
                            useless.push(WriteInfo {
                                name: name.clone(),
                                name_start: lv.location().start_offset(),
                                name_end: lv.location().end_offset(),
                                kind: WriteKind::Simple,
                                op: None,
                                regexp_start: 0,
                                regexp_end: 0,
                            });
                        }
                        scope.all_var_names.insert(name);
                    }
                }
            }
            // Union this branch's live set
            for var in branch_live {
                rescue_combined_live.insert(var);
            }
            rescue_clause = rc.subsequent();
        }

        // For retry: if rescue has retry, the begin body is a loop
        let _has_rescue = begin.rescue_clause().is_some();

        // Union live sets from rescue and else branches
        for var in rescue_combined_live {
            live.insert(var);
        }
        for var in else_live {
            live.insert(var);
        }
        // Variables read in rescue/ensure that aren't overwritten by ensure
        // make writes in the begin body live
        for var in &rescue_reads {
            if !ensure_writes.contains(var) {
                live.insert(var.clone());
            }
        }
        for var in &ensure_reads {
            live.insert(var.clone());
        }

        // Variables that are read in rescue/ensure - these must stay live
        // throughout the entire begin body because an exception can occur
        // between any two statements.
        let mut always_live_in_begin: HashSet<String> = HashSet::new();
        if begin.rescue_clause().is_some() || begin.ensure_clause().is_some() {
            for var in &rescue_reads {
                if !ensure_writes.contains(var) {
                    always_live_in_begin.insert(var.clone());
                }
            }
            for var in &ensure_reads {
                always_live_in_begin.insert(var.clone());
            }
            // Also include variables that are live after the begin (they're read
            // after the begin, so any write before an exception point needs to be kept)
            for var in live.iter() {
                always_live_in_begin.insert(var.clone());
            }
        }

        // Analyze main body
        if let Some(stmts) = begin.statements() {
            let body: Vec<_> = stmts.body().iter().collect();
            if has_retry {
                // With retry, the begin body acts like a loop
                for _ in 0..3 {
                    let prev_live = live.clone();
                    let mut body_live = live.clone();
                    let mut temp_useless = Vec::new();
                    for stmt in body.iter().rev() {
                        self.analyze_node_reverse(stmt, &mut body_live, &mut temp_useless, scope);
                        // Re-add always-live variables
                        for var in &always_live_in_begin {
                            body_live.insert(var.clone());
                        }
                    }
                    for var in body_live {
                        live.insert(var);
                    }
                    if *live == prev_live { break; }
                }
                let mut body_live = live.clone();
                for stmt in body.iter().rev() {
                    self.analyze_node_reverse(stmt, &mut body_live, useless, scope);
                    for var in &always_live_in_begin {
                        body_live.insert(var.clone());
                    }
                }
                for var in body_live {
                    live.insert(var);
                }
            } else {
                for stmt in body.iter().rev() {
                    self.analyze_node_reverse(stmt, live, useless, scope);
                    // Re-add always-live variables after each statement
                    for var in &always_live_in_begin {
                        live.insert(var.clone());
                    }
                }
            }
        }
    }

    fn analyze_case_reverse(
        &mut self,
        case: ruby_prism::CaseNode,
        live: &mut HashSet<String>,
        useless: &mut Vec<WriteInfo>,
        scope: &mut ScopeInfo,
    ) {
        let live_after = live.clone();
        let mut combined_live = HashSet::new();

        for when in case.conditions().iter() {
            if let Some(when_node) = when.as_when_node() {
                let mut branch_live = live_after.clone();
                if let Some(stmts) = when_node.statements() {
                    let body: Vec<_> = stmts.body().iter().collect();
                    for stmt in body.iter().rev() {
                        self.analyze_node_reverse(stmt, &mut branch_live, useless, scope);
                    }
                }
                for cond in when_node.conditions().iter() {
                    self.collect_reads(&cond, &mut branch_live);
                }
                for var in branch_live {
                    combined_live.insert(var);
                }
            }
        }

        if let Some(else_clause) = case.else_clause() {
            let mut branch_live = live_after.clone();
            if let Some(stmts) = else_clause.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                for stmt in body.iter().rev() {
                    self.analyze_node_reverse(stmt, &mut branch_live, useless, scope);
                }
            }
            for var in branch_live {
                combined_live.insert(var);
            }
        } else {
            // No else means the fall-through path is possible
            for var in &live_after {
                combined_live.insert(var.clone());
            }
        }

        *live = combined_live;

        if let Some(pred) = case.predicate() {
            self.collect_reads(&pred, live);
        }
    }

    fn analyze_case_match_reverse(
        &mut self,
        case_match: ruby_prism::CaseMatchNode,
        live: &mut HashSet<String>,
        useless: &mut Vec<WriteInfo>,
        scope: &mut ScopeInfo,
    ) {
        let live_after = live.clone();
        let mut combined_live = HashSet::new();

        for in_node in case_match.conditions().iter() {
            if let Some(in_n) = in_node.as_in_node() {
                let mut branch_live = live_after.clone();
                if let Some(stmts) = in_n.statements() {
                    let body: Vec<_> = stmts.body().iter().collect();
                    for stmt in body.iter().rev() {
                        self.analyze_node_reverse(stmt, &mut branch_live, useless, scope);
                    }
                }
                // Pattern can have writes
                self.collect_pattern_writes(&in_n.pattern(), &mut branch_live, useless, scope);
                for var in branch_live {
                    combined_live.insert(var);
                }
            }
        }

        if let Some(else_clause) = case_match.else_clause() {
            let mut branch_live = live_after.clone();
            if let Some(stmts) = else_clause.statements() {
                let body: Vec<_> = stmts.body().iter().collect();
                for stmt in body.iter().rev() {
                    self.analyze_node_reverse(stmt, &mut branch_live, useless, scope);
                }
            }
            for var in branch_live {
                combined_live.insert(var);
            }
        } else {
            for var in &live_after {
                combined_live.insert(var.clone());
            }
        }

        *live = combined_live;

        if let Some(pred) = case_match.predicate() {
            self.collect_reads(&pred, live);
        }
    }

    /// Analyze a block node. Determines which variables are captured from
    /// the outer scope and which are block-local. Reports useless assignments
    /// for block-local variables and updates the outer live set for captured ones.
    fn analyze_block_for_outer(
        &mut self,
        block: ruby_prism::BlockNode,
        live: &mut HashSet<String>,
        useless: &mut Vec<WriteInfo>,
        scope: &mut ScopeInfo,
    ) {
        // Collect all variable refs in the block
        let mut collector = VarRefCollector::new();
        collector.visit_block_node(&block);

        // Collect block parameter names (these shadow outer variables)
        let mut block_params: HashSet<String> = HashSet::new();
        if let Some(params) = block.parameters() {
            if let Some(bp) = params.as_block_parameters_node() {
                // Regular block params
                if let Some(parameters) = bp.parameters() {
                    for p in parameters.requireds().iter() {
                        if let Some(rp) = p.as_required_parameter_node() {
                            block_params.insert(name_str(&rp.name()));
                        }
                    }
                    for p in parameters.optionals().iter() {
                        if let Some(op) = p.as_optional_parameter_node() {
                            block_params.insert(name_str(&op.name()));
                        }
                    }
                    if let Some(rest) = parameters.rest() {
                        if let Some(rp) = rest.as_rest_parameter_node() {
                            if let Some(name_loc) = rp.name_loc() {
                                block_params.insert(String::from_utf8_lossy(name_loc.as_slice()).to_string());
                            }
                        }
                    }
                    for p in parameters.posts().iter() {
                        if let Some(rp) = p.as_required_parameter_node() {
                            block_params.insert(name_str(&rp.name()));
                        }
                    }
                }
                // Block-local vars (after ;)
                for local in bp.locals().iter() {
                    if let Some(blv) = local.as_block_local_variable_node() {
                        let name = name_str(&blv.name());
                        block_params.insert(name);
                    }
                }
            }
        }

        // Determine which variables are outer-scope captures vs block-local:
        // - If a variable is in the outer scope's live set or all_var_names, it's captured
        // - Block parameters shadow outer variables (not captured)
        // - Everything else is block-local

        // For captured variables (read or written in block, exist in outer scope):
        // Add to live to keep outer writes alive
        for var in &collector.referenced_vars {
            if !block_params.contains(var) {
                live.insert(var.clone());
            }
        }
        for var in &collector.written_vars {
            if !block_params.contains(var) {
                // If the variable exists in outer scope, it's a capture
                if live.contains(var) || scope.all_var_names.contains(var) {
                    live.insert(var.clone());
                }
            }
        }

        // Analyze block body for block-local useless assignments
        if let Some(body) = block.body() {
            let stmts = if let Some(sn) = body.as_statements_node() {
                sn.body().iter().collect::<Vec<_>>()
            } else {
                vec![body]
            };

            let mut block_scope = ScopeInfo::new();
            self.collect_scope_info(&stmts, &mut block_scope);

            let live_out = HashSet::new();
            let block_useless = self.analyze_stmts_for_useless(&stmts, &live_out, &mut block_scope);

            // Report useless writes for block-local variables
            for w in block_useless {
                let is_captured = !block_params.contains(&w.name) &&
                    (live.contains(&w.name) || scope.all_var_names.contains(&w.name));
                if !is_captured {
                    self.report_single_useless(&w, &block_scope);
                }
            }
        }
    }

    /// Collect all reads in a node tree into the live set
    fn collect_reads(&self, node: &Node, live: &mut HashSet<String>) {
        let mut collector = ReadCollector { live };
        // Use the Visit trait's dispatch method to walk the tree
        collector.visit(node);
    }

    /// Collect all reads including in nested scopes (for rescue/ensure analysis)
    fn collect_all_reads(&self, node: &Node, reads: &mut HashSet<String>) {
        let mut collector = AllReadCollector { reads };
        collector.visit(node);
    }

    /// Process RHS of assignments for scope-creating nodes (def, class, etc.)
    fn process_rhs_for_scopes(&mut self, node: &Node) {
        match node {
            Node::DefNode { .. } => {
                let def = node.as_def_node().unwrap();
                let params = extract_param_names(&def);
                self.analyze_scope(&def.body(), params);
            }
            Node::ClassNode { .. } => {
                let class = node.as_class_node().unwrap();
                self.analyze_scope(&class.body(), HashSet::new());
            }
            Node::ModuleNode { .. } => {
                let module = node.as_module_node().unwrap();
                self.analyze_scope(&module.body(), HashSet::new());
            }
            Node::SingletonClassNode { .. } => {
                let sc = node.as_singleton_class_node().unwrap();
                self.analyze_scope(&sc.body(), HashSet::new());
            }
            _ => {
                // Look for nested scope-creating nodes via Visit
                // We can't easily use Visit here because we need &mut self,
                // so we use a manual traversal approach: just walk the common cases.
                // The Visit-based approach in analyze_node_reverse handles most cases.
            }
        }
    }

    /// Process nested writes in RHS of assignments.
    /// For example, in `foo = [1, bar = 2]`, the `bar = 2` is a nested write.
    /// Skips direct chained assignments (foo = bar = expr) since those are handled
    /// by the outer write.
    fn process_nested_writes(
        &mut self,
        node: &Node,
        live: &mut HashSet<String>,
        useless: &mut Vec<WriteInfo>,
        scope: &mut ScopeInfo,
    ) {
        // Skip direct chained assignment (the node itself is a write)
        if matches!(node, Node::LocalVariableWriteNode { .. }) {
            return;
        }
        let mut finder = NestedWriteFinder::new();
        finder.visit(node);
        // Process found writes in reverse offset order
        let mut writes = finder.writes;
        writes.sort_by(|a, b| b.0.cmp(&a.0)); // Sort by offset descending
        for (_, name, name_start, name_end) in writes {
            if name.starts_with('_') { continue; }
            if live.contains(&name) {
                live.remove(&name);
            } else {
                useless.push(WriteInfo {
                    name: name.clone(),
                    name_start,
                    name_end,
                    kind: WriteKind::Simple,
                    op: None,
                    regexp_start: 0,
                    regexp_end: 0,
                });
            }
            scope.all_var_names.insert(name);
        }
    }

    /// Process blocks found in RHS values for useless assignment analysis
    fn process_blocks_in_rhs(
        &mut self,
        node: &Node,
        live: &mut HashSet<String>,
        useless: &mut Vec<WriteInfo>,
        scope: &mut ScopeInfo,
    ) {
        match node {
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                if let Some(block_node) = call.block() {
                    self.analyze_node_reverse(&block_node, live, useless, scope);
                }
                // Recurse into receiver and arguments
                if let Some(recv) = call.receiver() {
                    self.process_blocks_in_rhs(&recv, live, useless, scope);
                }
                if let Some(args) = call.arguments() {
                    for arg in args.arguments().iter() {
                        self.process_blocks_in_rhs(&arg, live, useless, scope);
                    }
                }
            }
            Node::BlockNode { .. } => {
                let block = node.as_block_node().unwrap();
                self.analyze_block_for_outer(block, live, useless, scope);
            }
            _ => {}
        }
    }

    /// Handle writes to the SAME variable nested in RHS (e.g., `foo += foo = 2`)
    /// The inner write is always useless because the outer op-assign overwrites it.
    fn collect_same_var_rhs_writes(
        &mut self,
        outer_name: &str,
        node: &Node,
        _live: &mut HashSet<String>,
        useless: &mut Vec<WriteInfo>,
        scope: &mut ScopeInfo,
    ) {
        if let Some(write) = node.as_local_variable_write_node() {
            let name = name_str(&write.name());
            if name == outer_name && !name.starts_with('_') {
                // This inner write to the same variable is always useless
                // because the outer op-assign will overwrite it
                useless.push(WriteInfo {
                    name: name.clone(),
                    name_start: write.name_loc().start_offset(),
                    name_end: write.name_loc().end_offset(),
                    kind: WriteKind::Simple,
                    op: None,
                    regexp_start: 0,
                    regexp_end: 0,
                });
                scope.all_var_names.insert(name);
            }
        }
    }

    fn report_useless(&mut self, writes: &[WriteInfo], scope: &ScopeInfo) {
        for w in writes {
            self.report_single_useless(w, scope);
        }
    }

    fn report_single_useless(&mut self, w: &WriteInfo, scope: &ScopeInfo) {
        // Skip if bare super and this is a method param
        if scope.has_bare_super && scope.params.contains(&w.name) {
            return;
        }

        let mut message = format!("Useless assignment to variable - `{}`.", w.name);

        // For multi-assign (except for-loop index), always use underscore suggestion
        if w.kind == WriteKind::MultiAssign {
            // Check for "Did you mean" first - only from method calls (for-loop collections)
            if let Some(suggestion) = find_suggestion_from_methods(&w.name, scope) {
                message = format!(
                    "Useless assignment to variable - `{}`. Did you mean `{}`?",
                    w.name, suggestion
                );
            } else {
                message = format!(
                    "Useless assignment to variable - `{}`. Use `_` or `_{}` as a variable name to indicate that it won't be used.",
                    w.name, w.name
                );
            }
        } else if let Some(suggestion) = find_suggestion(&w.name, scope) {
            message = format!(
                "Useless assignment to variable - `{}`. Did you mean `{}`?",
                w.name, suggestion
            );
        } else if let Some(ref op) = w.op {
            // Only suggest operator replacement if it's a trailing useless op-assign
            // For op-assign that's not at scope end, don't add the suggestion
            // We need to check if this is the "last expression" of scope
            // For simplicity: if it's op/and/or assign, we always add the suggestion
            // when there's no "did you mean" - this matches RuboCop behavior
            // (RuboCop adds it when the op-assign is useless regardless of position)
            message = format!(
                "Useless assignment to variable - `{}`. Use `{}` instead of `{}=`.",
                w.name, op, op
            );
        }

        let (start, end) = if w.kind == WriteKind::RegexpCapture {
            (w.regexp_start, w.regexp_end)
        } else {
            (w.name_start, w.name_end)
        };

        self.offenses.push(self.ctx.offense_with_range(
            "Lint/UselessAssignment",
            &message,
            Severity::Warning,
            start,
            end,
        ));
    }
}

// ── Helper: read a ConstantId as String ──

fn name_str(id: &ruby_prism::ConstantId) -> String {
    String::from_utf8_lossy(id.as_slice()).to_string()
}

// ── ReadCollector: find all local variable reads in a subtree ──

struct ReadCollector<'a> {
    live: &'a mut HashSet<String>,
}

impl Visit<'_> for ReadCollector<'_> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let name = name_str(&node.name());
        self.live.insert(name);
    }

    // Don't descend into scope-creating nodes
    fn visit_def_node(&mut self, _node: &ruby_prism::DefNode) {}
    fn visit_class_node(&mut self, _node: &ruby_prism::ClassNode) {}
    fn visit_module_node(&mut self, _node: &ruby_prism::ModuleNode) {}
    fn visit_singleton_class_node(&mut self, _node: &ruby_prism::SingletonClassNode) {}
}

// ── AllReadCollector: reads including those across rescue boundaries ──

struct AllReadCollector<'a> {
    reads: &'a mut HashSet<String>,
}

impl Visit<'_> for AllReadCollector<'_> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let name = name_str(&node.name());
        self.reads.insert(name);
    }
    // Op-assign/and-assign/or-assign also read the variable
    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        let name = name_str(&node.name());
        self.reads.insert(name);
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode) {
        let name = name_str(&node.name());
        self.reads.insert(name);
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        let name = name_str(&node.name());
        self.reads.insert(name);
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
    fn visit_def_node(&mut self, _node: &ruby_prism::DefNode) {}
    fn visit_class_node(&mut self, _node: &ruby_prism::ClassNode) {}
    fn visit_module_node(&mut self, _node: &ruby_prism::ModuleNode) {}
    fn visit_singleton_class_node(&mut self, _node: &ruby_prism::SingletonClassNode) {}
}

// ── ScopeInfoCollector ──

struct ScopeInfoCollector<'a> {
    scope: &'a mut ScopeInfo,
}

impl Visit<'_> for ScopeInfoCollector<'_> {
    fn visit_forwarding_super_node(&mut self, _node: &ruby_prism::ForwardingSuperNode) {
        self.scope.has_bare_super = true;
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Track variable-like method calls
        if node.receiver().is_none() {
            if let Some(msg_loc) = node.message_loc() {
                let name = String::from_utf8_lossy(msg_loc.as_slice()).to_string();
                let has_args = if let Some(args) = node.arguments() {
                    args.arguments().len() > 0
                } else {
                    false
                };
                if !has_args && node.block().is_none() {
                    self.scope.method_calls.insert(name);
                }
            }
        }
        if node.is_variable_call() {
            if let Some(msg_loc) = node.message_loc() {
                let name = String::from_utf8_lossy(msg_loc.as_slice()).to_string();
                self.scope.method_calls.insert(name);
            }
        }
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let name = name_str(&node.name());
        self.scope.all_var_names.insert(name);
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let name = name_str(&node.name());
        self.scope.all_reads.insert(name);
    }

    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        let name = name_str(&node.name());
        self.scope.all_var_names.insert(name.clone());
        self.scope.all_reads.insert(name);
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }

    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode) {
        let name = name_str(&node.name());
        self.scope.all_var_names.insert(name.clone());
        self.scope.all_reads.insert(name);
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }

    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        let name = name_str(&node.name());
        self.scope.all_var_names.insert(name.clone());
        self.scope.all_reads.insert(name);
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }

    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode) {
        let name = name_str(&node.name());
        self.scope.all_var_names.insert(name);
    }

    // Don't descend into scope-creating nodes or blocks
    fn visit_def_node(&mut self, _node: &ruby_prism::DefNode) {}
    fn visit_class_node(&mut self, _node: &ruby_prism::ClassNode) {}
    fn visit_module_node(&mut self, _node: &ruby_prism::ModuleNode) {}
    fn visit_singleton_class_node(&mut self, _node: &ruby_prism::SingletonClassNode) {}
    fn visit_block_node(&mut self, _node: &ruby_prism::BlockNode) {}
    fn visit_lambda_node(&mut self, _node: &ruby_prism::LambdaNode) {}
}

// ── VarRefCollector: collect variable references in blocks ──

struct VarRefCollector {
    referenced_vars: HashSet<String>,
    written_vars: HashSet<String>,
}

impl VarRefCollector {
    fn new() -> Self {
        Self {
            referenced_vars: HashSet::new(),
            written_vars: HashSet::new(),
        }
    }
}

impl Visit<'_> for VarRefCollector {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode) {
        let name = name_str(&node.name());
        self.referenced_vars.insert(name);
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let name = name_str(&node.name());
        self.written_vars.insert(name);
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode) {
        let name = name_str(&node.name());
        self.written_vars.insert(name);
    }

    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        let name = name_str(&node.name());
        self.referenced_vars.insert(name.clone());
        self.written_vars.insert(name);
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }

    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode) {
        let name = name_str(&node.name());
        self.referenced_vars.insert(name.clone());
        self.written_vars.insert(name);
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }

    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode) {
        let name = name_str(&node.name());
        self.referenced_vars.insert(name.clone());
        self.written_vars.insert(name);
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }

    fn visit_def_node(&mut self, _node: &ruby_prism::DefNode) {}
    fn visit_class_node(&mut self, _node: &ruby_prism::ClassNode) {}
    fn visit_module_node(&mut self, _node: &ruby_prism::ModuleNode) {}
    fn visit_singleton_class_node(&mut self, _node: &ruby_prism::SingletonClassNode) {}
}

// ── NestedWriteFinder: find writes nested in expressions ──

struct NestedWriteFinder {
    // (offset, name, name_start, name_end)
    writes: Vec<(usize, String, usize, usize)>,
    in_container: bool,
}

impl NestedWriteFinder {
    fn new() -> Self {
        Self { writes: Vec::new(), in_container: false }
    }
}

impl Visit<'_> for NestedWriteFinder {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        // Only record if we're inside a container (array, hash, arguments)
        // not at the top level or in a simple chain
        if self.in_container {
            let name = name_str(&node.name());
            self.writes.push((
                node.location().start_offset(),
                name,
                node.name_loc().start_offset(),
                node.name_loc().end_offset(),
            ));
        }
        // Recurse into the value to find deeper nested writes
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    // Track when we're inside a container
    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        let was = self.in_container;
        self.in_container = true;
        ruby_prism::visit_array_node(self, node);
        self.in_container = was;
    }

    fn visit_arguments_node(&mut self, node: &ruby_prism::ArgumentsNode) {
        let was = self.in_container;
        self.in_container = true;
        ruby_prism::visit_arguments_node(self, node);
        self.in_container = was;
    }

    // Don't descend into scope-creating nodes
    fn visit_def_node(&mut self, _node: &ruby_prism::DefNode) {}
    fn visit_class_node(&mut self, _node: &ruby_prism::ClassNode) {}
    fn visit_module_node(&mut self, _node: &ruby_prism::ModuleNode) {}
    fn visit_singleton_class_node(&mut self, _node: &ruby_prism::SingletonClassNode) {}
    fn visit_block_node(&mut self, _node: &ruby_prism::BlockNode) {}
    fn visit_lambda_node(&mut self, _node: &ruby_prism::LambdaNode) {}
}

// ── Helper: check if begin has retry ──

fn begin_has_retry(begin: &ruby_prism::BeginNode) -> bool {
    let mut checker = RetryChecker { has_retry: false };
    let mut rescue = begin.rescue_clause();
    while let Some(rc) = rescue {
        if let Some(stmts) = rc.statements() {
            for stmt in stmts.body().iter() {
                checker.visit(&stmt);
            }
        }
        if checker.has_retry { return true; }
        rescue = rc.subsequent();
    }
    false
}

struct RetryChecker {
    has_retry: bool,
}

impl Visit<'_> for RetryChecker {
    fn visit_retry_node(&mut self, _node: &ruby_prism::RetryNode) {
        self.has_retry = true;
    }
    fn visit_begin_node(&mut self, _node: &ruby_prism::BeginNode) {}
}

// ── Helper: collect all writes in a node ──

fn collect_all_writes_in_node(node: &Node, writes: &mut HashSet<String>) {
    let mut collector = WriteCollector { writes };
    collector.visit(node);
}

struct WriteCollector<'a> {
    writes: &'a mut HashSet<String>,
}

impl Visit<'_> for WriteCollector<'_> {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let name = name_str(&node.name());
        self.writes.insert(name);
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode) {
        let name = name_str(&node.name());
        self.writes.insert(name);
    }
    fn visit_def_node(&mut self, _node: &ruby_prism::DefNode) {}
    fn visit_class_node(&mut self, _node: &ruby_prism::ClassNode) {}
    fn visit_module_node(&mut self, _node: &ruby_prism::ModuleNode) {}
    fn visit_singleton_class_node(&mut self, _node: &ruby_prism::SingletonClassNode) {}
}

// ── Helper: extract method parameter names ──

fn extract_param_names(def: &ruby_prism::DefNode) -> HashSet<String> {
    let mut params = HashSet::new();
    if let Some(parameters) = def.parameters() {
        for p in parameters.requireds().iter() {
            if let Some(rp) = p.as_required_parameter_node() {
                params.insert(name_str(&rp.name()));
            }
        }
        for p in parameters.optionals().iter() {
            if let Some(op) = p.as_optional_parameter_node() {
                params.insert(name_str(&op.name()));
            }
        }
        if let Some(rest) = parameters.rest() {
            if let Some(rp) = rest.as_rest_parameter_node() {
                if let Some(name_loc) = rp.name_loc() {
                    params.insert(String::from_utf8_lossy(name_loc.as_slice()).to_string());
                }
            }
        }
        for p in parameters.keywords().iter() {
            if let Some(kp) = p.as_required_keyword_parameter_node() {
                let name = name_str(&kp.name());
                params.insert(name.trim_end_matches(':').to_string());
            } else if let Some(kp) = p.as_optional_keyword_parameter_node() {
                let name = name_str(&kp.name());
                params.insert(name.trim_end_matches(':').to_string());
            }
        }
        if let Some(kr) = parameters.keyword_rest() {
            if let Some(krp) = kr.as_keyword_rest_parameter_node() {
                if let Some(name_loc) = krp.name_loc() {
                    params.insert(String::from_utf8_lossy(name_loc.as_slice()).to_string());
                }
            }
        }
        if let Some(block_param) = parameters.block() {
            if let Some(name_loc) = block_param.name_loc() {
                params.insert(String::from_utf8_lossy(name_loc.as_slice()).to_string());
            }
        }
        for p in parameters.posts().iter() {
            if let Some(rp) = p.as_required_parameter_node() {
                params.insert(name_str(&rp.name()));
            }
        }
    }
    params
}

// ── Levenshtein distance ──

pub fn levenshtein(a: &str, b: &str) -> usize {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let a_len = a_bytes.len();
    let b_len = b_bytes.len();

    if a_len == 0 { return b_len; }
    if b_len == 0 { return a_len; }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        for j in 1..=b_len {
            let cost = if a_bytes[i - 1] == b_bytes[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

/// Find suggestion only from method calls (used for multi-assign / for-loop)
pub fn find_suggestion_from_methods(name: &str, scope: &ScopeInfo) -> Option<String> {
    let threshold = (name.len() + 2) / 3;
    let mut best: Option<(String, usize)> = None;

    let check = |other: &str, best: &mut Option<(String, usize)>| {
        if other == name || other.starts_with('_') { return; }
        let dist = levenshtein(name, other);
        if dist > 0 && dist <= threshold {
            if best.is_none() || dist < best.as_ref().unwrap().1 {
                *best = Some((other.to_string(), dist));
            }
        }
    };

    for other in &scope.method_calls {
        check(other, &mut best);
    }

    best.map(|(s, _)| s)
}

pub fn find_suggestion(name: &str, scope: &ScopeInfo) -> Option<String> {
    let threshold = (name.len() + 2) / 3;
    let mut best: Option<(String, usize)> = None;

    let check = |other: &str, best: &mut Option<(String, usize)>| {
        if other == name || other.starts_with('_') { return; }
        let dist = levenshtein(name, other);
        if dist > 0 && dist <= threshold {
            if best.is_none() || dist < best.as_ref().unwrap().1 {
                *best = Some((other.to_string(), dist));
            }
        }
    };

    for other in &scope.all_var_names {
        check(other, &mut best);
    }
    for other in &scope.method_calls {
        check(other, &mut best);
    }
    for other in &scope.all_reads {
        check(other, &mut best);
    }

    best.map(|(s, _)| s)
}
