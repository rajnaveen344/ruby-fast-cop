//! Layout/ElseAlignment - aligns `else`/`elsif` with the base keyword (if/unless/case/begin/def/rescue).

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::col_at_offset;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

pub struct ElseAlignment {
    end_align_style: String, // "keyword" or "variable" (affects variable-alignment logic)
}

impl ElseAlignment {
    pub fn new() -> Self {
        Self {
            end_align_style: "keyword".to_string(),
        }
    }

    pub fn with_end_align_style(style: String) -> Self {
        Self { end_align_style: style }
    }
}

impl Default for ElseAlignment {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for ElseAlignment {
    fn name(&self) -> &'static str {
        "Layout/ElseAlignment"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut v = ElseVisitor {
            ctx,
            end_align_style: self.end_align_style.as_str(),
            offenses: Vec::new(),
        };
        v.visit_program_node(node);
        v.offenses
    }
}

/// Base alignment information for the else/elsif keyword.
#[derive(Clone)]
struct Base {
    col: usize,
    /// Displayed keyword-ish text (first non-whitespace token of the base range).
    text: String,
    /// If this base keyword is on the same line as an assignment LHS, store LHS col+text.
    /// RuboCop uses `node.loc` vs `rhs.loc` variable_alignment? to pick variable vs keyword.
    lhs: Option<(usize, String)>,
}

struct ElseVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    end_align_style: &'a str,
    offenses: Vec<Offense>,
}

impl<'a> ElseVisitor<'a> {
    fn begins_its_line(&self, off: usize) -> bool {
        let s = self.ctx.source.as_bytes();
        let mut i = off;
        while i > 0 {
            i -= 1;
            let b = s[i];
            if b == b'\n' {
                return true;
            }
            // Skip BOM bytes (0xEF 0xBB 0xBF)
            if b == 0xBF && i >= 2 && s[i - 1] == 0xBB && s[i - 2] == 0xEF {
                i -= 2;
                continue;
            }
            if b == 0xBB && i >= 1 && s[i - 1] == 0xEF {
                i -= 1;
                continue;
            }
            if b == 0xEF {
                continue;
            }
            if b != b' ' && b != b'\t' {
                return false;
            }
        }
        true
    }

    fn check_align(
        &mut self,
        kw_text: &str,
        kw_loc: &ruby_prism::Location,
        base: &Base,
    ) {
        let off = kw_loc.start_offset();
        if !self.begins_its_line(off) {
            return;
        }
        let col = col_at_offset(self.ctx.source, off) as usize;

        // Determine the target: if end_align_style == "variable" and lhs present, align with LHS.
        // Otherwise align with base keyword col.
        let (target_col, target_text) = if self.end_align_style == "variable" {
            if let Some((lcol, ltext)) = &base.lhs {
                (*lcol, ltext.clone())
            } else {
                (base.col, base.text.clone())
            }
        } else {
            (base.col, base.text.clone())
        };

        if col == target_col {
            return;
        }

        let message = format!("Align `{}` with `{}`.", kw_text, target_text);
        let location = crate::offense::Location::from_offsets(
            self.ctx.source,
            off,
            kw_loc.end_offset(),
        );
        self.offenses.push(Offense::new(
            "Layout/ElseAlignment",
            message,
            Severity::Convention,
            location,
            self.ctx.filename,
        ));
    }

    /// Build a Base from a keyword location, looking for a same-line assignment LHS.
    fn base_from_keyword(&self, kw_off: usize, kw_text: String) -> Base {
        let kw_col = col_at_offset(self.ctx.source, kw_off) as usize;
        let line_start = self.ctx.line_start(kw_off);
        let before = &self.ctx.source[line_start..kw_off];

        // Check for assignment: like `var = ...if`, `foo.bar = if`, `foo[bar] = if`,
        // also op_asgn `var += if`, etc.
        if let Some((indent, lhs_text)) = extract_lhs_if_assignment(before) {
            return Base {
                col: kw_col,
                text: kw_text,
                lhs: Some((indent, lhs_text)),
            };
        }

        Base {
            col: kw_col,
            text: kw_text,
            lhs: None,
        }
    }

    /// Process a top-level if/unless and its else / elsif chain.
    fn process_if_chain(&mut self, node: &ruby_prism::IfNode, base: &Base) {
        // Check subsequent: ElseNode (else) or IfNode (elsif).
        if let Some(sub) = node.subsequent() {
            match &sub {
                Node::ElseNode { .. } => {
                    let else_n = sub.as_else_node().unwrap();
                    let else_kw = else_n.else_keyword_loc();
                    self.check_align("else", &else_kw, base);
                }
                Node::IfNode { .. } => {
                    let elsif_n = sub.as_if_node().unwrap();
                    if let Some(kw_loc) = elsif_n.if_keyword_loc() {
                        let kw_text = std::str::from_utf8(kw_loc.as_slice()).unwrap_or("elsif");
                        if kw_text == "elsif" {
                            self.check_align("elsif", &kw_loc, base);
                        }
                    }
                    // Recurse into elsif's own chain with same base
                    self.process_if_chain(&elsif_n, base);
                }
                _ => {}
            }
        }
    }
}

impl Visit<'_> for ElseVisitor<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        if let Some(kw_loc) = node.if_keyword_loc() {
            let kw_text = std::str::from_utf8(kw_loc.as_slice()).unwrap_or("if");
            if kw_text == "if" {
                // Top-level if (not an elsif)
                let base = self.base_from_keyword(kw_loc.start_offset(), "if".to_string());
                self.process_if_chain(node, &base);
            }
        }
        // Descend to visit nested structures inside branches
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        let kw_loc = node.keyword_loc();
        let base = self.base_from_keyword(kw_loc.start_offset(), "unless".to_string());
        if let Some(else_n) = node.else_clause() {
            let else_kw = else_n.else_keyword_loc();
            self.check_align("else", &else_kw, &base);
        }
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        if let Some(else_n) = node.else_clause() {
            // base: the last when's keyword
            let last_when_kw = node
                .conditions()
                .iter()
                .filter_map(|c| c.as_when_node().map(|w| w.keyword_loc()))
                .last();
            if let Some(when_kw) = last_when_kw {
                let base = Base {
                    col: col_at_offset(self.ctx.source, when_kw.start_offset()) as usize,
                    text: "when".to_string(),
                    lhs: None,
                };
                let else_kw = else_n.else_keyword_loc();
                self.check_align("else", &else_kw, &base);
            }
        }
        ruby_prism::visit_case_node(self, node);
    }

    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        if let Some(else_n) = node.else_clause() {
            let last_in_kw = node
                .conditions()
                .iter()
                .filter_map(|c| c.as_in_node().map(|p| p.in_loc()))
                .last();
            if let Some(in_kw) = last_in_kw {
                let base = Base {
                    col: col_at_offset(self.ctx.source, in_kw.start_offset()) as usize,
                    text: "in".to_string(),
                    lhs: None,
                };
                let else_kw = else_n.else_keyword_loc();
                self.check_align("else", &else_kw, &base);
            }
        }
        ruby_prism::visit_case_match_node(self, node);
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        // For kwbegin (begin..end), else base is begin keyword.
        // For def/block implicit begin: else base is the def/block keyword — handled by ancestor.
        if let Some(begin_kw) = node.begin_keyword_loc() {
            let base = Base {
                col: col_at_offset(self.ctx.source, begin_kw.start_offset()) as usize,
                text: "begin".to_string(),
                lhs: None,
            };
            if node.rescue_clause().is_some() {
                if let Some(else_n) = node.else_clause() {
                    let else_kw = else_n.else_keyword_loc();
                    self.check_align("else", &else_kw, &base);
                }
            }
        }
        ruby_prism::visit_begin_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        let def_kw = node.def_keyword_loc();
        let def_off = def_kw.start_offset();
        // Skip if wrapped by a same-line access modifier (handled in visit_call_node).
        if !self.ctx.begins_its_line(def_off) {
            ruby_prism::visit_def_node(self, node);
            return;
        }
        let base = Base {
            col: col_at_offset(self.ctx.source, def_off) as usize,
            text: "def".to_string(),
            lhs: None,
        };
        if let Some(body) = node.body() {
            if let Some(begin) = body.as_begin_node() {
                if begin.rescue_clause().is_some() {
                    if let Some(else_n) = begin.else_clause() {
                        let else_kw = else_n.else_keyword_loc();
                        self.check_align("else", &else_kw, &base);
                    }
                }
            }
        }
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Handle "private def foo\n...\nelse\n..." — base = "private" keyword on caller line.
        if node.receiver().is_none() {
            let call_name = String::from_utf8_lossy(node.name().as_slice());
            let is_access_mod = matches!(
                call_name.as_ref(),
                "private" | "protected" | "public" | "private_class_method" | "public_class_method"
            );
            if is_access_mod {
                if let Some(args) = node.arguments() {
                    for arg in args.arguments().iter() {
                        if let Some(def_node) = arg.as_def_node() {
                            let call_off = node.location().start_offset();
                            let base = Base {
                                col: col_at_offset(self.ctx.source, call_off) as usize,
                                text: call_name.to_string(),
                                lhs: None,
                            };
                            if let Some(body) = def_node.body() {
                                if let Some(begin) = body.as_begin_node() {
                                    if begin.rescue_clause().is_some() {
                                        if let Some(else_n) = begin.else_clause() {
                                            let else_kw = else_n.else_keyword_loc();
                                            self.check_align("else", &else_kw, &base);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Handle block with rescue/else
        if let Some(block_ref) = node.block() {
            if let Some(block_node) = block_ref.as_block_node() {
                if let Some(body) = block_node.body() {
                    if let Some(begin) = body.as_begin_node() {
                        if begin.rescue_clause().is_some() {
                            if let Some(else_n) = begin.else_clause() {
                                let base = block_base_from_call(self.ctx, node);
                                let else_kw = else_n.else_keyword_loc();
                                self.check_align("else", &else_kw, &base);
                            }
                        }
                    }
                }
            }
        }

        ruby_prism::visit_call_node(self, node);
    }
}

/// Extract the LHS text for an assignment ending at `before.len()`.
/// Returns (indent_col, lhs_text) if `before` ends with `= ` (assignment operator).
fn extract_lhs_if_assignment(before: &str) -> Option<(usize, String)> {
    let trimmed = before.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    let bytes = trimmed.as_bytes();
    // Must end in a `=` that is an assignment: last `=` not followed by =,~,>
    let last = *bytes.last()?;
    if last != b'=' {
        return None;
    }
    // disallow ==, <=, >=, !=, =~, => etc. at end
    if bytes.len() >= 2 {
        let prev = bytes[bytes.len() - 2];
        if prev == b'=' || prev == b'!' || prev == b'<' || prev == b'>' {
            return None;
        }
    }
    // Find start of LHS text: position of first non-whitespace on line
    let indent = before.len() - before.trim_start().len();
    // Strip compound-assign op: op= becomes just op (e.g., `foo += ` → lhs is `foo`)
    let mut end = bytes.len() - 1; // position of `=`
    if end > 0 {
        let prev = bytes[end - 1];
        if matches!(prev, b'+' | b'-' | b'*' | b'/' | b'%' | b'|' | b'&' | b'^') {
            end -= 1;
            if end > 0 && (bytes[end] == bytes[end - 1]) && (bytes[end] == b'|' || bytes[end] == b'&') {
                end -= 1;
            }
        }
    }
    let lhs = trimmed[..end].trim().to_string();
    if lhs.is_empty() {
        return None;
    }
    // For attr writer: `foo.bar = ` → LHS text is `foo.bar` but RuboCop aligns with receiver.
    // Tests use `foo.bar = if baz` expecting "Align with foo.bar" — check:
    // fixture line 256: message = "Align `elsif` with `foo.bar`."
    // fixture line 282: "Align `else` with `foo[bar]`."
    // So we use the full LHS text.
    Some((indent, lhs))
}

/// Compute base for `else` inside a do-end block's implicit begin.
/// Mirror of RescueEnsureAlignment.align_info_for_block_call (simplified).
fn block_base_from_call(ctx: &CheckContext, call: &ruby_prism::CallNode) -> Base {
    let call_off = call.location().start_offset();
    let call_line = ctx.line_of(call_off);
    let call_col = col_at_offset(ctx.source, call_off) as usize;

    // If there's assignment on the same line before the call, use LHS.
    let line_start = ctx.line_start(call_off);
    let before = &ctx.source[line_start..call_off];
    if let Some((indent, lhs_text)) = extract_lhs_if_assignment(before) {
        let _ = call_line;
        return Base {
            col: call_col,
            text: "block".to_string(),
            lhs: Some((indent, lhs_text)),
        };
    }

    // Otherwise use call's receiver + name (the "foo.bar" or just name)
    let eff_start = call
        .receiver()
        .map(|r| r.location().start_offset())
        .unwrap_or(call_off);
    let eff_col = col_at_offset(ctx.source, eff_start) as usize;
    let name_end = call.message_loc().map(|l| l.end_offset()).unwrap_or(call_off + 1);
    let text = ctx.source[eff_start..name_end].trim_end().to_string();
    Base {
        col: eff_col,
        text,
        lhs: None,
    }
}
