//! Style/IfInsideElse cop
//!
//! If the else branch of a conditional consists solely of an if node,
//! it can be combined with the else to become an elsif.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Convert `if` nested inside `else` to `elsif`.";
const COP_NAME: &str = "Style/IfInsideElse";

pub struct IfInsideElse {
    allow_if_modifier: bool,
}

impl IfInsideElse {
    pub fn new(allow_if_modifier: bool) -> Self {
        Self { allow_if_modifier }
    }
}

impl Default for IfInsideElse {
    fn default() -> Self {
        Self::new(false)
    }
}

impl Cop for IfInsideElse {
    fn name(&self) -> &'static str {
        COP_NAME
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = IfInsideElseVisitor { ctx, cop: self, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct IfInsideElseVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a IfInsideElse,
    offenses: Vec<Offense>,
}

impl<'a> IfInsideElseVisitor<'a> {
    fn kw_src(&self, loc: &ruby_prism::Location) -> &str {
        &self.ctx.source[loc.start_offset()..loc.end_offset()]
    }

    fn is_ternary(&self, node: &ruby_prism::IfNode) -> bool {
        if let Some(then_loc) = node.then_keyword_loc() {
            self.kw_src(&then_loc) == "?"
        } else {
            false
        }
    }

    fn is_modifier_if(&self, node: &ruby_prism::IfNode) -> bool {
        if let Some(kw_loc) = node.if_keyword_loc() {
            if let Some(stmts) = node.statements() {
                return kw_loc.start_offset() > stmts.location().start_offset();
            }
        }
        false
    }

    fn has_then_keyword(&self, node: &ruby_prism::IfNode) -> bool {
        if let Some(then_loc) = node.then_keyword_loc() {
            self.kw_src(&then_loc) == "then"
        } else {
            false
        }
    }

    fn build_correction(
        &self,
        else_node: &ruby_prism::ElseNode,
        inner_if: &ruby_prism::IfNode,
        is_modifier: bool,
    ) -> Option<Correction> {
        if self.has_then_keyword(inner_if) {
            let is_single_line = !self.ctx.source[inner_if.location().start_offset()..inner_if.location().end_offset()].contains('\n');
            if is_single_line && inner_if_has_final_else(inner_if) {
                // Combined: else → elsif + IfThenCorrector body (single-line only)
                self.correct_then_form_with_elsif(else_node, inner_if)
            } else {
                self.correct_then_form(inner_if)
            }
        } else if is_modifier {
            self.correct_modifier_form(else_node, inner_if)
        } else {
            self.correct_standard_form(else_node, inner_if)
        }
    }

    /// Combined: else → elsif + IfThenCorrector body (for then-forms with final else)
    fn correct_then_form_with_elsif(
        &self,
        else_node: &ruby_prism::ElseNode,
        inner_if: &ruby_prism::IfNode,
    ) -> Option<Correction> {
        let source = self.ctx.source;
        let else_kw_loc = else_node.else_keyword_loc();

        let if_kw_loc = inner_if.if_keyword_loc()?;
        let inner_start = if_kw_loc.start_offset();
        let inner_end = inner_if.location().end_offset();

        // col_indent = spaces before inner `if`
        let line_start = source[..inner_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let col_indent = " ".repeat(inner_start - line_start);

        let cond_src = {
            let pred = inner_if.predicate();
            source[pred.location().start_offset()..pred.location().end_offset()].to_string()
        };

        let if_branch_src = if let Some(stmts) = inner_if.statements() {
            source[stmts.location().start_offset()..stmts.location().end_offset()].to_string()
        } else {
            "nil".to_string()
        };

        // Build: elsif {cond}\n{col_indent}{body}\n{subsequent}
        // No trailing `end` — the outer `end` is preserved in the source.
        let mut replacement = format!("elsif {}\n{}{}\n", cond_src, col_indent, if_branch_src);
        let subsequent_part = self.rewrite_else_branch_no_end(inner_if.subsequent(), source, &col_indent);
        replacement.push_str(&subsequent_part);

        // Replace from else_kw start to inner_if end (including trailing newline)
        let replace_start = else_kw_loc.start_offset();
        let replace_end = if inner_end < source.len() && source.as_bytes()[inner_end] == b'\n' {
            inner_end + 1
        } else {
            inner_end
        };

        Some(Correction::replace(replace_start, replace_end, replacement))
    }

    /// `if cond then body end` → multiline form (IfThenCorrector with indentation: 0)
    /// Mirrors RuboCop's IfThenCorrector.new(node, indentation: 0)
    fn correct_then_form(&self, inner_if: &ruby_prism::IfNode) -> Option<Correction> {
        let source = self.ctx.source;
        let then_loc = inner_if.then_keyword_loc()?;
        if self.kw_src(&then_loc) != "then" { return None; }

        let if_kw_loc = inner_if.if_keyword_loc()?;
        let inner_start = if_kw_loc.start_offset();
        let inner_end = inner_if.location().end_offset();

        // Get column of inner `if` (= indentation of the if's line)
        let line_start = source[..inner_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
        // col_indent = number of spaces before `if` on its line
        let col_indent = " ".repeat(inner_start - line_start);

        let cond_src = {
            let pred = inner_if.predicate();
            &source[pred.location().start_offset()..pred.location().end_offset()]
        };

        // IfThenCorrector.replacement with indentation=0:
        // branch_body_indentation = ''
        // indentation = col_indent
        // if_branch_source = statements source (or 'nil')
        let if_branch_src = if let Some(stmts) = inner_if.statements() {
            source[stmts.location().start_offset()..stmts.location().end_offset()].to_string()
        } else {
            "nil".to_string()
        };

        let mut replacement = format!("if {}\n{}{}\n", cond_src, col_indent, if_branch_src);

        // Subsequent (elsif or else)
        let else_replacement = self.rewrite_else_branch(inner_if.subsequent(), source, &col_indent);
        replacement.push_str(&else_replacement);

        Some(Correction::replace(inner_start, inner_end, replacement))
    }

    /// Mirrors IfThenCorrector.rewrite_else_branch(else_branch, indentation)
    fn rewrite_else_branch(
        &self,
        subsequent: Option<ruby_prism::Node>,
        source: &str,
        indent: &str,
    ) -> String {
        match subsequent {
            None => "end".to_string(),
            Some(sub) => {
                if let Some(else_node) = sub.as_else_node() {
                    // Standard else
                    let else_body = if let Some(stmts) = else_node.statements() {
                        source[stmts.location().start_offset()..stmts.location().end_offset()].to_string()
                    } else {
                        "nil".to_string()
                    };
                    format!("{}else\n{}{}\n{}end", indent, indent, else_body, indent)
                } else if let Some(elsif_node) = sub.as_if_node() {
                    // elsif chain — recursively format
                    let cond_src = {
                        let pred = elsif_node.predicate();
                        source[pred.location().start_offset()..pred.location().end_offset()].to_string()
                    };
                    let if_branch_src = if let Some(stmts) = elsif_node.statements() {
                        source[stmts.location().start_offset()..stmts.location().end_offset()].to_string()
                    } else {
                        "nil".to_string()
                    };
                    let mut result = format!("{}elsif {}\n{}{}\n", indent, cond_src, indent, if_branch_src);
                    let rest = self.rewrite_else_branch(elsif_node.subsequent(), source, indent);
                    result.push_str(&rest);
                    result
                } else {
                    "end".to_string()
                }
            }
        }
    }

    /// Modifier: `else\n  foo if cond\nend` → `elsif cond\n  foo\nend`
    fn correct_modifier_form(
        &self,
        else_node: &ruby_prism::ElseNode,
        inner_if: &ruby_prism::IfNode,
    ) -> Option<Correction> {
        let source = self.ctx.source;
        let else_kw_loc = else_node.else_keyword_loc();
        let pred = inner_if.predicate();
        let pred_start = pred.location().start_offset();
        let pred_end = pred.location().end_offset();
        let cond_src = &source[pred_start..pred_end];

        // Get body statement range
        let body_stmts = inner_if.statements()?;
        let body_end = body_stmts.location().end_offset();

        // Extend inner_node_end to include trailing comment on same line
        let inner_node_end = {
            let node_end = inner_if.location().end_offset();
            // Find end of the line (up to \n or EOF)
            source[node_end..].find('\n').map(|p| node_end + p).unwrap_or(source.len())
        };

        // Content of the modifier-if line after removing ` if cond`:
        // source[body_stmts.start..body_end] + source[pred_end..inner_node_end]
        let body_text = &source[body_stmts.location().start_offset()..body_end];
        let after_cond = &source[pred_end..inner_node_end]; // trailing comment if any

        // Build the body line: `{body_text}{after_cond}` but `after_cond` may start with ` `
        let body_line = format!("{}{}", body_text.trim_end(), after_cond.trim_end());

        // Extract lines BEFORE the modifier-if line (from after else\n to before modifier-if line)
        let else_end = else_kw_loc.end_offset();
        // Skip newline after `else`
        let after_else = if else_end < source.len() && source.as_bytes()[else_end] == b'\n' {
            else_end + 1
        } else {
            else_end
        };
        // Line start of modifier-if
        let modifier_line_start = source[..body_stmts.location().start_offset()].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let before_modifier = &source[after_else..modifier_line_start];

        // Indent of modifier-if line
        let modifier_indent_str = &source[modifier_line_start..body_stmts.location().start_offset()];
        let body_indent = " ".repeat(modifier_indent_str.len() - modifier_indent_str.trim_start().len());

        // Replace range: else_kw_start to end of modifier-if line (incl newline)
        let replace_start = else_kw_loc.start_offset();
        let replace_end = if inner_node_end < source.len() && source.as_bytes()[inner_node_end] == b'\n' {
            inner_node_end + 1
        } else {
            inner_node_end
        };

        let mut replacement = format!("elsif {}\n", cond_src);
        replacement.push_str(before_modifier);
        replacement.push_str(&body_indent);
        replacement.push_str(&body_line);
        replacement.push('\n');

        Some(Correction::replace(replace_start, replace_end, replacement))
    }

    /// Like rewrite_else_branch but does NOT add trailing `end`.
    /// Used for single-line then-forms where outer `end` is preserved.
    fn rewrite_else_branch_no_end(
        &self,
        subsequent: Option<ruby_prism::Node>,
        source: &str,
        indent: &str,
    ) -> String {
        match subsequent {
            None => String::new(),
            Some(sub) => {
                if let Some(else_node) = sub.as_else_node() {
                    let else_body = if let Some(stmts) = else_node.statements() {
                        source[stmts.location().start_offset()..stmts.location().end_offset()].to_string()
                    } else {
                        "nil".to_string()
                    };
                    format!("{}else\n{}{}\n", indent, indent, else_body)
                } else if let Some(elsif_node) = sub.as_if_node() {
                    let cond_src = {
                        let pred = elsif_node.predicate();
                        source[pred.location().start_offset()..pred.location().end_offset()].to_string()
                    };
                    let if_branch_src = if let Some(stmts) = elsif_node.statements() {
                        source[stmts.location().start_offset()..stmts.location().end_offset()].to_string()
                    } else {
                        "nil".to_string()
                    };
                    let mut result = format!("{}elsif {}\n{}{}\n", indent, cond_src, indent, if_branch_src);
                    let rest = self.rewrite_else_branch_no_end(elsif_node.subsequent(), source, indent);
                    result.push_str(&rest);
                    result
                } else {
                    String::new()
                }
            }
        }
    }

    /// Standard: `else\n  if cond\n    body\n  end\nend` → `elsif cond\n  body\nend`
    fn correct_standard_form(
        &self,
        else_node: &ruby_prism::ElseNode,
        inner_if: &ruby_prism::IfNode,
    ) -> Option<Correction> {
        let source = self.ctx.source;
        let else_kw_loc = else_node.else_keyword_loc();
        let pred = inner_if.predicate();
        let cond_src = &source[pred.location().start_offset()..pred.location().end_offset()];
        let inner_if_kw = inner_if.if_keyword_loc()?;

        // Compute indent of inner `if` keyword line
        let if_start = inner_if_kw.start_offset();
        let if_line_start = source[..if_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let if_line_indent_str = &source[if_line_start..if_start];
        let if_indent = if_line_indent_str.len() - if_line_indent_str.trim_start().len();

        // Extract if-branch body including leading comments.
        // RuboCop inserts body at position of `if` keyword (if_indent spaces).
        // First line gets if_indent spaces; subsequent lines keep original spacing.
        // Exception: when there are no statements (only comments), keep verbatim (no re-indent).
        let has_stmts = inner_if.statements().is_some();
        let if_body = if has_stmts {
            extract_if_body(source, inner_if, if_indent)
        } else {
            extract_comment_only_body(source, inner_if)
        };

        // Subsequent (else/elsif within inner_if): keep verbatim from line-start
        // to line-start of the inner `end` keyword (i.e. don't include inner `end`)
        let subsequent_raw = if let Some(sub) = inner_if.subsequent() {
            let sub_start = sub.location().start_offset();
            let sub_line_start = source[..sub_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
            if let Some(end_kw) = inner_if.end_keyword_loc() {
                let end_line_start = source[..end_kw.start_offset()].rfind('\n').map(|p| p + 1).unwrap_or(0);
                source[sub_line_start..end_line_start].to_string()
            } else {
                source[sub_line_start..sub.location().end_offset()].to_string()
            }
        } else {
            String::new()
        };

        // Replace range: from else_kw start to end of inner end keyword's line (incl newline)
        let replace_start = else_kw_loc.start_offset();
        let replace_end = if let Some(end_kw) = inner_if.end_keyword_loc() {
            let e = end_kw.end_offset();
            if e < source.len() && source.as_bytes()[e] == b'\n' { e + 1 } else { e }
        } else {
            inner_if.location().end_offset()
        };

        let mut replacement = format!("elsif {}\n", cond_src);
        if !if_body.is_empty() {
            replacement.push_str(&if_body);
            replacement.push('\n');
        }
        if !subsequent_raw.is_empty() {
            replacement.push_str(&subsequent_raw);
        }

        Some(Correction::replace(replace_start, replace_end, replacement))
    }
}

/// Extract comment-only body verbatim (no re-indent). Used when statements() is None.
fn extract_comment_only_body(source: &str, inner_if: &ruby_prism::IfNode) -> String {
    let pred_end = inner_if.predicate().location().end_offset();
    let after_cond_line = if pred_end < source.len() && source.as_bytes()[pred_end] == b'\n' {
        pred_end + 1
    } else {
        source[pred_end..].find('\n').map(|p| pred_end + p + 1).unwrap_or(source.len())
    };
    let next_line_start = if let Some(sub) = inner_if.subsequent() {
        let s = sub.location().start_offset();
        source[..s].rfind('\n').map(|p| p + 1).unwrap_or(0)
    } else if let Some(end_kw) = inner_if.end_keyword_loc() {
        let s = end_kw.start_offset();
        source[..s].rfind('\n').map(|p| p + 1).unwrap_or(0)
    } else {
        return String::new();
    };
    if next_line_start <= after_cond_line { return String::new(); }
    let raw = &source[after_cond_line..next_line_start];
    raw.trim_end_matches('\n').to_string()
}

/// Extract if-branch body (including leading comments) between condition end and subsequent/end.
/// First line is re-indented to `if_indent` spaces; subsequent lines keep original spacing.
/// Returns multi-line string WITHOUT trailing newline.
fn extract_if_body(source: &str, inner_if: &ruby_prism::IfNode, if_indent: usize) -> String {
    let pred_end = inner_if.predicate().location().end_offset();
    // Skip the line ending after condition
    let after_cond_line = if pred_end < source.len() && source.as_bytes()[pred_end] == b'\n' {
        pred_end + 1
    } else {
        // Could have `then` or other delimiter — find next newline
        source[pred_end..].find('\n').map(|p| pred_end + p + 1).unwrap_or(source.len())
    };
    // Find where next keyword (subsequent or end) begins (line start)
    let next_line_start = if let Some(sub) = inner_if.subsequent() {
        let s = sub.location().start_offset();
        source[..s].rfind('\n').map(|p| p + 1).unwrap_or(0)
    } else if let Some(end_kw) = inner_if.end_keyword_loc() {
        let s = end_kw.start_offset();
        source[..s].rfind('\n').map(|p| p + 1).unwrap_or(0)
    } else {
        return String::new();
    };
    if next_line_start <= after_cond_line {
        return String::new();
    }
    let raw = &source[after_cond_line..next_line_start];
    // raw ends with \n (since next_line_start is after a \n); strip it for clean output
    let raw = raw.trim_end_matches('\n');
    if raw.is_empty() {
        return String::new();
    }
    // First line: re-indent to if_indent spaces; subsequent lines: keep as-is
    let mut lines = raw.lines();
    let mut result = String::new();
    if let Some(first) = lines.next() {
        let stripped = first.trim_start();
        result.push_str(&" ".repeat(if_indent));
        result.push_str(stripped);
    }
    for line in lines {
        result.push('\n');
        result.push_str(line);
    }
    result
}

/// Re-indent text: strip `old_indent` spaces from each line's start, add `new_indent`.
fn reindent(text: &str, old_indent: usize, new_indent: usize) -> String {
    text.lines()
        .map(|line| {
            // Count actual leading spaces
            let actual = line.len() - line.trim_start_matches(' ').len();
            if actual >= old_indent {
                format!("{}{}", " ".repeat(new_indent + (actual - old_indent)), &line[actual..])
            } else {
                format!("{}{}", " ".repeat(new_indent), line.trim_start())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Returns true if the IfNode has a terminal `else` branch (not just elsif chains).
fn inner_if_has_final_else(node: &ruby_prism::IfNode) -> bool {
    let mut cur = node.subsequent();
    loop {
        match cur {
            None => return false,
            Some(sub) => {
                if sub.as_else_node().is_some() {
                    return true;
                }
                if let Some(elsif) = sub.as_if_node() {
                    cur = elsif.subsequent();
                } else {
                    return false;
                }
            }
        }
    }
}

fn stmts_with_leading_indent<'a>(source: &str, stmts: &Node<'a>) -> String {
    let loc = stmts.location();
    let stmts_start = loc.start_offset();
    let stmts_end = loc.end_offset();
    let line_start = source[..stmts_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
    source[line_start..stmts_end].to_string()
}

impl<'a> Visit<'_> for IfInsideElseVisitor<'a> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        // Skip ternary
        if self.is_ternary(node) {
            ruby_prism::visit_if_node(self, node);
            return;
        }

        // Skip unless
        if let Some(kw_loc) = node.if_keyword_loc() {
            if self.kw_src(&kw_loc) == "unless" {
                ruby_prism::visit_if_node(self, node);
                return;
            }
        }

        // Check subsequent() — is it an ElseNode?
        if let Some(subsequent) = node.subsequent() {
            if let Some(else_node) = subsequent.as_else_node() {
                // Else body must be a single if node
                if let Some(stmts) = else_node.statements() {
                    let children: Vec<_> = stmts.body().iter().collect();
                    if children.len() == 1 {
                        if let Some(inner_if) = children[0].as_if_node() {
                            // Inner must be `if` not `unless`
                            let inner_kw_src = if let Some(kw) = inner_if.if_keyword_loc() {
                                self.kw_src(&kw).to_string()
                            } else {
                                String::new()
                            };

                            if inner_kw_src == "if" {
                                let is_modifier = self.is_modifier_if(&inner_if);
                                if !(self.cop.allow_if_modifier && is_modifier) {
                                    if let Some(kw_loc) = inner_if.if_keyword_loc() {
                                        let start = kw_loc.start_offset();
                                        let end = kw_loc.end_offset();
                                        let correction = self.build_correction(
                                            &else_node, &inner_if, is_modifier,
                                        );
                                        let offense = self.ctx.offense_with_range(
                                            COP_NAME, MSG, Severity::Convention, start, end,
                                        );
                                        let offense = if let Some(corr) = correction {
                                            offense.with_correction(corr)
                                        } else {
                                            offense
                                        };
                                        self.offenses.push(offense);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        ruby_prism::visit_if_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    allow_if_modifier: bool,
}

crate::register_cop!("Style/IfInsideElse", |cfg| {
    let c: Cfg = cfg.typed("Style/IfInsideElse");
    Some(Box::new(IfInsideElse::new(c.allow_if_modifier)))
});
