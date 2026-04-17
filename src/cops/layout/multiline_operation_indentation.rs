use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Aligned,
    Indented,
}

pub struct MultilineOperationIndentation {
    style: Style,
    indentation_width: usize,
}

impl MultilineOperationIndentation {
    pub fn new(style: Style, width: Option<usize>) -> Self {
        Self { style, indentation_width: width.unwrap_or(2) }
    }
}

impl Cop for MultilineOperationIndentation {
    fn name(&self) -> &'static str { "Layout/MultilineOperationIndentation" }
    fn severity(&self) -> Severity { Severity::Convention }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = OpVisitor {
            ctx,
            style: self.style,
            indentation_width: self.indentation_width,
            offenses: Vec::new(),
            ancestors: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

/// Ancestor context for determining indentation rules
#[derive(Debug, Clone, Copy)]
enum Ancestor {
    /// Inside a keyword condition (if/unless/while/until/for)
    KeywordCondition { kw_offset: usize, kw_len: usize, is_for: bool },
    /// Inside a block body (do..end or { })
    BlockBody,
    /// Inside an assignment RHS
    Assignment { rhs_begins_its_line: bool },
    /// Inside a parenthesized grouping expression
    GroupedExpression,
    /// Inside parenthesized method call arguments
    ParenthesizedArgs,
    /// Inside method call arguments (without parentheses, NOT a def modifier)
    MethodCallArgs,
    /// Inside an array literal (resets assignment context)
    ArrayLiteral,
}

struct OpVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: Style,
    indentation_width: usize,
    offenses: Vec<Offense>,
    ancestors: Vec<Ancestor>,
}

impl<'a> OpVisitor<'a> {
    fn check_operation(&mut self, lhs: &Node, rhs_off: usize, rhs_end: usize) {
        // Only check if RHS begins its own line
        if !self.ctx.begins_its_line(rhs_off) { return; }

        let lhs_off = lhs.location().start_offset();
        if self.ctx.line_of(lhs_off) == self.ctx.line_of(rhs_off) { return; }

        // Skip operations inside grouped expressions or parenthesized method call args
        if self.is_inside_grouped_or_paren_args() { return; }

        let rhs_col = self.ctx.col_of(rhs_off);
        let kw_ancestor = self.find_keyword_ancestor();
        let assign_ancestor = self.find_assignment_ancestor();
        let in_method_args = self.is_in_method_call_args();
        let should_align = self.should_align(&kw_ancestor, &assign_ancestor, in_method_args);

        if should_align {
            let correct_col = self.ctx.col_of(lhs_off);
            let delta = correct_col as isize - rhs_col as isize;
            if delta == 0 { return; }

            let what = self.operation_description(&kw_ancestor, &assign_ancestor);
            let msg = format!("Align the operands of {} spanning multiple lines.", what);
            self.offense(rhs_off, rhs_end, &msg, delta);
        } else {
            let correct_col = if let Some(ref kw) = kw_ancestor {
                if !kw.is_modifier {
                    let base_indent = self.ctx.indentation_of(kw.kw_offset);
                    base_indent + self.indentation_width + 2
                } else {
                    self.ctx.indentation_of(self.ctx.line_start(lhs_off)) + self.indentation_width
                }
            } else {
                self.ctx.indentation_of(self.ctx.line_start(lhs_off)) + self.indentation_width
            };

            let delta = correct_col as isize - rhs_col as isize;
            if delta == 0 { return; }

            let what = self.operation_description(&kw_ancestor, &assign_ancestor);
            let li = self.ctx.indentation_of(self.ctx.line_start(lhs_off));

            let msg = if kw_ancestor.as_ref().map_or(false, |k| !k.is_modifier) {
                format!(
                    "Use {} (not {}) spaces for indenting {} spanning multiple lines.",
                    correct_col, rhs_col, what,
                )
            } else {
                let used = rhs_col as isize - li as isize;
                format!(
                    "Use {} (not {}) spaces for indenting {} spanning multiple lines.",
                    self.indentation_width, used, what,
                )
            };
            self.offense(rhs_off, rhs_end, &msg, delta);
        }
    }

    fn should_align(&self, kw: &Option<KwInfo>, assign: &Option<AssignAncestor>, in_method_args: bool) -> bool {
        if let Some(a) = assign {
            if a.rhs_begins_its_line { return true; }
        }

        if self.style != Style::Aligned { return false; }

        if kw.as_ref().map_or(false, |k| !k.is_modifier) { return true; }
        if assign.is_some() { return true; }
        if in_method_args { return true; }

        false
    }

    fn operation_description(&self, kw: &Option<KwInfo>, assign: &Option<AssignAncestor>) -> String {
        if let Some(k) = kw {
            if !k.is_modifier {
                return kw_message_tail(&k.keyword);
            }
        }
        if assign.is_some() {
            return "an expression in an assignment".to_string();
        }
        "an expression".to_string()
    }

    fn is_inside_grouped_or_paren_args(&self) -> bool {
        for a in self.ancestors.iter().rev() {
            match a {
                Ancestor::GroupedExpression | Ancestor::ParenthesizedArgs => return true,
                Ancestor::BlockBody => return false,
                _ => {}
            }
        }
        false
    }

    fn is_in_method_call_args(&self) -> bool {
        for a in self.ancestors.iter().rev() {
            match a {
                Ancestor::MethodCallArgs => return true,
                Ancestor::BlockBody => return false,
                _ => {}
            }
        }
        false
    }

    fn find_keyword_ancestor(&self) -> Option<KwInfo> {
        for a in self.ancestors.iter().rev() {
            match a {
                Ancestor::KeywordCondition { kw_offset, kw_len, is_for } => {
                    let kw_text = if *is_for {
                        "for".to_string()
                    } else {
                        let s = self.ctx.bytes();
                        let end = (*kw_offset + kw_len).min(s.len());
                        String::from_utf8_lossy(&s[*kw_offset..end]).trim().to_string()
                    };
                    return Some(KwInfo {
                        kw_offset: *kw_offset,
                        keyword: kw_text,
                        is_modifier: false,
                    });
                }
                Ancestor::BlockBody => return None,
                _ => {}
            }
        }
        None
    }

    fn find_assignment_ancestor(&self) -> Option<AssignAncestor> {
        for a in self.ancestors.iter().rev() {
            match a {
                Ancestor::Assignment { rhs_begins_its_line } => {
                    return Some(AssignAncestor { rhs_begins_its_line: *rhs_begins_its_line });
                }
                Ancestor::BlockBody | Ancestor::KeywordCondition { .. } | Ancestor::ArrayLiteral => return None,
                _ => {}
            }
        }
        None
    }

    fn offense(&mut self, rhs_off: usize, rhs_end: usize, msg: &str, delta: isize) {
        let off = self.ctx.offense_with_range(
            "Layout/MultilineOperationIndentation", msg,
            Severity::Convention, rhs_off, rhs_end,
        );
        let ls = self.ctx.line_start(rhs_off);
        let cur = rhs_off - ls;
        let new = (cur as isize + delta).max(0) as usize;
        let edits = vec![Edit {
            start_offset: ls, end_offset: rhs_off, replacement: " ".repeat(new),
        }];
        self.offenses.push(off.with_correction(Correction { edits }));
    }

    fn with_ancestor<F>(&mut self, kind: Ancestor, f: F)
    where F: FnOnce(&mut Self) {
        self.ancestors.push(kind);
        f(self);
        self.ancestors.pop();
    }

    fn rhs_begins_its_line(&self, operator_end: usize, rhs_start: usize) -> bool {
        self.ctx.line_of(operator_end) != self.ctx.line_of(rhs_start)
    }

    fn check_send_node(&mut self, node: &ruby_prism::CallNode) {
        let receiver = match node.receiver() {
            Some(r) => r,
            None => return,
        };

        let name = String::from_utf8_lossy(node.name().as_slice());
        if name.as_ref() == "[]" || name.as_ref() == "[]=" { return; }
        if node.arguments().is_none() && (name.as_ref() == "-@" || name.as_ref() == "+@" || name.as_ref() == "~") {
            return;
        }

        // Must NOT have a dot operator — this cop handles binary operators, not method calls
        if node.call_operator_loc().is_some() { return; }

        let rhs = match node.arguments() {
            Some(args) => {
                let arguments = args.arguments();
                if arguments.is_empty() { return; }
                arguments.iter().next().unwrap()
            }
            None => return,
        };

        let rhs_off = rhs.location().start_offset();
        let rhs_end = rhs.location().end_offset();
        self.check_operation(&receiver, rhs_off, rhs_end);
    }

    /// Visit a call node's arguments with appropriate ancestor context.
    /// If the call has parenthesized args, push ParenthesizedArgs.
    /// If the call has unparenthesized args, push MethodCallArgs.
    fn visit_call_with_arg_tracking(&mut self, node: &ruby_prism::CallNode) {
        self.check_send_node(node);

        // Visit receiver normally
        if let Some(recv) = node.receiver() {
            self.visit(&recv);
        }

        // Visit arguments with context
        if let Some(args) = node.arguments() {
            let is_setter = is_setter_method(node);
            if is_setter {
                // Setter methods are assignments
                let last_arg = args.arguments().iter().last();
                if let Some(last) = last_arg {
                    let rhs_begins = if let Some(op) = node.message_loc() {
                        self.ctx.line_of(op.end_offset()) != self.ctx.line_of(last.location().start_offset())
                    } else {
                        false
                    };
                    self.with_ancestor(Ancestor::Assignment { rhs_begins_its_line: rhs_begins }, |this| {
                        this.visit_arguments_node(&args);
                    });
                } else {
                    self.visit_arguments_node(&args);
                }
            } else if is_def_modifier(node) {
                // def modifier (private/protected/public/module_function def ...)
                // Don't wrap in MethodCallArgs context
                self.visit_arguments_node(&args);
            } else if node.opening_loc().is_some() {
                // Has parentheses
                self.with_ancestor(Ancestor::ParenthesizedArgs, |this| {
                    this.visit_arguments_node(&args);
                });
            } else {
                // No parentheses — method call args
                self.with_ancestor(Ancestor::MethodCallArgs, |this| {
                    this.visit_arguments_node(&args);
                });
            }
        }

        // Visit block with BlockBody context
        if let Some(block) = node.block() {
            self.with_ancestor(Ancestor::BlockBody, |this| {
                this.visit(&block);
            });
        }
    }
}

struct KwInfo {
    kw_offset: usize,
    keyword: String,
    is_modifier: bool,
}

struct AssignAncestor {
    rhs_begins_its_line: bool,
}

fn kw_message_tail(keyword: &str) -> String {
    let kind = if keyword == "for" { "collection" } else { "condition" };
    let article = if keyword.starts_with('i') || keyword.starts_with('u') { "an" } else { "a" };
    format!("a {} in {} `{}` statement", kind, article, keyword)
}

fn is_def_modifier(node: &ruby_prism::CallNode) -> bool {
    // Check if this is a call like `private def ...`, `protected def ...`, etc.
    if node.receiver().is_some() { return false; }
    let name = String::from_utf8_lossy(node.name().as_slice());
    let is_access = matches!(name.as_ref(), "private" | "protected" | "public" | "module_function");
    if !is_access { return false; }
    // Check if the first argument is a DefNode
    if let Some(args) = node.arguments() {
        let arguments = args.arguments();
        if let Some(first) = arguments.iter().next() {
            return matches!(first, Node::DefNode { .. });
        }
    }
    false
}

fn is_setter_method(node: &ruby_prism::CallNode) -> bool {
    let name = String::from_utf8_lossy(node.name().as_slice());
    name.ends_with('=') && name.as_ref() != "==" && name.as_ref() != "!="
        && name.as_ref() != "<=" && name.as_ref() != ">="
}

fn is_ternary(node: &ruby_prism::IfNode) -> bool {
    node.end_keyword_loc().is_none() && node.if_keyword_loc().is_some()
}

fn is_modifier_form(ctx: &CheckContext, kw_off: usize) -> bool {
    let ls = ctx.line_start(kw_off);
    let before = &ctx.source[ls..kw_off];
    !before.trim().is_empty()
}

// ---- Visitor ----

impl<'a> Visit<'_> for OpVisitor<'a> {
    fn visit_and_node(&mut self, node: &ruby_prism::AndNode) {
        let rhs = node.right();
        self.check_operation(&node.left(), rhs.location().start_offset(), rhs.location().end_offset());
        ruby_prism::visit_and_node(self, node);
    }

    fn visit_or_node(&mut self, node: &ruby_prism::OrNode) {
        let rhs = node.right();
        self.check_operation(&node.left(), rhs.location().start_offset(), rhs.location().end_offset());
        ruby_prism::visit_or_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        self.visit_call_with_arg_tracking(node);
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        if let Some(ref kw) = node.if_keyword_loc() {
            let kw_off = kw.start_offset();
            if !is_modifier_form(self.ctx, kw_off) && !is_ternary(node) {
                let kw_len = kw.end_offset() - kw_off;
                self.with_ancestor(
                    Ancestor::KeywordCondition { kw_offset: kw_off, kw_len, is_for: false },
                    |this| { this.visit(&node.predicate()); },
                );
                if let Some(stmts) = node.statements() {
                    self.visit_statements_node(&stmts);
                }
                if let Some(subs) = node.subsequent() {
                    self.visit(&subs);
                }
                return;
            }
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        let kw_off = node.keyword_loc().start_offset();
        if !is_modifier_form(self.ctx, kw_off) {
            let kw_len = node.keyword_loc().end_offset() - kw_off;
            self.with_ancestor(
                Ancestor::KeywordCondition { kw_offset: kw_off, kw_len, is_for: false },
                |this| { this.visit(&node.predicate()); },
            );
            if let Some(stmts) = node.statements() { self.visit_statements_node(&stmts); }
            if let Some(ec) = node.else_clause() { self.visit_else_node(&ec); }
            return;
        }
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        let kw_off = node.keyword_loc().start_offset();
        if !is_modifier_form(self.ctx, kw_off) {
            let kw_len = node.keyword_loc().end_offset() - kw_off;
            self.with_ancestor(
                Ancestor::KeywordCondition { kw_offset: kw_off, kw_len, is_for: false },
                |this| { this.visit(&node.predicate()); },
            );
            if let Some(stmts) = node.statements() { self.visit_statements_node(&stmts); }
            return;
        }
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        let kw_off = node.keyword_loc().start_offset();
        if !is_modifier_form(self.ctx, kw_off) {
            let kw_len = node.keyword_loc().end_offset() - kw_off;
            self.with_ancestor(
                Ancestor::KeywordCondition { kw_offset: kw_off, kw_len, is_for: false },
                |this| { this.visit(&node.predicate()); },
            );
            if let Some(stmts) = node.statements() { self.visit_statements_node(&stmts); }
            return;
        }
        ruby_prism::visit_until_node(self, node);
    }

    fn visit_for_node(&mut self, node: &ruby_prism::ForNode) {
        let kw_off = node.for_keyword_loc().start_offset();
        let kw_len = node.for_keyword_loc().end_offset() - kw_off;
        self.with_ancestor(
            Ancestor::KeywordCondition { kw_offset: kw_off, kw_len, is_for: true },
            |this| { this.visit(&node.collection()); },
        );
        self.visit(&node.index());
        if let Some(stmts) = node.statements() { self.visit_statements_node(&stmts); }
    }

    fn visit_parentheses_node(&mut self, node: &ruby_prism::ParenthesesNode) {
        self.with_ancestor(Ancestor::GroupedExpression, |this| {
            ruby_prism::visit_parentheses_node(this, node);
        });
    }

    fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode) {
        self.with_ancestor(Ancestor::ArrayLiteral, |this| {
            ruby_prism::visit_array_node(this, node);
        });
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode) {
        let value = node.value();
        let rhs_begins = self.rhs_begins_its_line(node.name_loc().end_offset(), value.location().start_offset());
        self.with_ancestor(Ancestor::Assignment { rhs_begins_its_line: rhs_begins }, |this| {
            this.visit(&value);
        });
    }

    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode) {
        let value = node.value();
        let rhs_begins = self.rhs_begins_its_line(node.name_loc().end_offset(), value.location().start_offset());
        self.with_ancestor(Ancestor::Assignment { rhs_begins_its_line: rhs_begins }, |this| {
            this.visit(&value);
        });
    }

    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode) {
        let value = node.value();
        let rhs_begins = self.rhs_begins_its_line(node.name_loc().end_offset(), value.location().start_offset());
        self.with_ancestor(Ancestor::Assignment { rhs_begins_its_line: rhs_begins }, |this| {
            this.visit(&value);
        });
    }

    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode) {
        let value = node.value();
        let rhs_begins = self.rhs_begins_its_line(node.name_loc().end_offset(), value.location().start_offset());
        self.with_ancestor(Ancestor::Assignment { rhs_begins_its_line: rhs_begins }, |this| {
            this.visit(&value);
        });
    }

    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode) {
        let value = node.value();
        let rhs_begins = self.rhs_begins_its_line(node.name_loc().end_offset(), value.location().start_offset());
        self.with_ancestor(Ancestor::Assignment { rhs_begins_its_line: rhs_begins }, |this| {
            this.visit(&value);
        });
    }

    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode) {
        let value = node.value();
        let rhs_begins = self.rhs_begins_its_line(node.binary_operator_loc().end_offset(), value.location().start_offset());
        self.with_ancestor(Ancestor::Assignment { rhs_begins_its_line: rhs_begins }, |this| {
            this.visit(&value);
        });
    }

    fn visit_constant_path_write_node(&mut self, node: &ruby_prism::ConstantPathWriteNode) {
        let value = node.value();
        let rhs_begins = self.rhs_begins_its_line(node.operator_loc().end_offset(), value.location().start_offset());
        self.with_ancestor(Ancestor::Assignment { rhs_begins_its_line: rhs_begins }, |this| {
            this.visit(&value);
        });
    }

    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode) {
        let value = node.value();
        let rhs_begins = self.rhs_begins_its_line(node.operator_loc().end_offset(), value.location().start_offset());
        self.with_ancestor(Ancestor::Assignment { rhs_begins_its_line: rhs_begins }, |this| {
            this.visit(&value);
        });
    }
}


crate::register_cop!("Layout/MultilineOperationIndentation", |cfg| {
    let cop_config = cfg.get_cop_config("Layout/MultilineOperationIndentation");
    let style = cop_config
        .and_then(|c| c.enforced_style.as_ref())
        .map(|s| match s.as_str() {
            "indented" => Style::Indented,
            _ => Style::Aligned,
        })
        .unwrap_or(Style::Aligned);
    let width = cop_config
        .and_then(|c| c.raw.get("IndentationWidth"))
        .and_then(|v| v.as_i64())
        .map(|v| v as usize);
    Some(Box::new(MultilineOperationIndentation::new(style, width)))
});
