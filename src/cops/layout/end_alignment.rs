use crate::cops::{CheckContext, Cop};
use crate::helpers::source::*;
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndAlignmentStyle {
    Keyword,
    Variable,
    StartOfLine,
}

pub struct EndAlignment {
    style: EndAlignmentStyle,
}

impl EndAlignment {
    pub fn new(style: EndAlignmentStyle) -> Self {
        Self { style }
    }
}

impl Cop for EndAlignment {
    fn name(&self) -> &'static str {
        "Layout/EndAlignment"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = EndAlignmentVisitor {
            ctx,
            style: self.style,
            offenses: Vec::new(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct EndAlignmentVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: EndAlignmentStyle,
    offenses: Vec<Offense>,
}

fn only_whitespace_before_keyword(source: &str, keyword_offset: usize) -> bool {
    let ls = line_start_offset(source, keyword_offset);
    source[ls..keyword_offset]
        .chars()
        .all(|c| c.is_whitespace() || c == '\u{FEFF}')
}

fn has_assignment_before_keyword(source: &str, keyword_offset: usize) -> bool {
    let ls = line_start_offset(source, keyword_offset);
    let before = &source[ls..keyword_offset];
    let bytes = before.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        let ch = bytes[i];
        if ch == b'=' {
            let prev = if i > 0 { bytes[i - 1] } else { 0 };
            let next = if i + 1 < len { bytes[i + 1] } else { 0 };

            if next == b'=' || next == b'~' || next == b'>' {
                i += 2;
                continue;
            }
            if prev == b'!' {
                i += 1;
                continue;
            }
            if prev == b'<' {
                if i >= 2 && bytes[i - 2] == b'<' {
                    return true; // <<=
                }
                i += 1;
                continue; // <=
            }
            if prev == b'>' {
                if i >= 2 && bytes[i - 2] == b'>' {
                    return true; // >>=
                }
                i += 1;
                continue; // >=
            }
            return true;
        } else if ch == b'<' && i + 1 < len && bytes[i + 1] == b'<' {
            if i + 2 < len && bytes[i + 2] == b'=' {
                i += 3;
                continue;
            }
            let after = if i + 2 < len { bytes[i + 2] } else { 0 };
            if after == b' ' || after == b'\t' {
                return true;
            }
            i += 2;
            continue;
        }

        if ch == b'\'' || ch == b'"' {
            let quote = ch;
            i += 1;
            while i < len && bytes[i] != quote {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            if i < len {
                i += 1;
            }
            continue;
        }

        i += 1;
    }

    false
}

fn is_in_call_or_operator_context(source: &str, keyword_offset: usize) -> bool {
    let ls = line_start_offset(source, keyword_offset);
    let before = &source[ls..keyword_offset];
    let trimmed = before.trim_end();

    if trimmed.is_empty() || trimmed.chars().all(|c| c.is_whitespace() || c == '\u{FEFF}') {
        return false;
    }

    if let Some(semi_pos) = before.rfind(';') {
        if before[semi_pos + 1..].trim().is_empty() {
            return false;
        }
    }

    !has_assignment_before_keyword(source, keyword_offset)
}

impl<'a> EndAlignmentVisitor<'a> {
    fn check_keyword_end(&mut self, keyword: &str, kw_loc: &ruby_prism::Location, end_loc: &ruby_prism::Location) {
        let source = self.ctx.source;
        let kw_off = kw_loc.start_offset();
        let end_off = end_loc.start_offset();
        let kw_line = line_at_offset(source, kw_off);
        let end_line = line_at_offset(source, end_off);

        if kw_line == end_line {
            return;
        }

        let kw_col = col_at_offset(source, kw_off);
        let end_col = col_at_offset(source, end_off);

        let expected_col = match self.style {
            EndAlignmentStyle::Keyword => kw_col,
            EndAlignmentStyle::Variable => {
                if only_whitespace_before_keyword(source, kw_off) {
                    kw_col
                } else if has_assignment_before_keyword(source, kw_off)
                    || is_in_call_or_operator_context(source, kw_off)
                {
                    first_non_ws_col(source, kw_off)
                } else {
                    kw_col
                }
            }
            EndAlignmentStyle::StartOfLine => first_non_ws_col(source, kw_off),
        };

        if end_col != expected_col {
            let align_target = self.build_align_target(keyword, kw_off, source);
            let message = format!(
                "`end` at {}, {} is not aligned with `{}` at {}, {}.",
                end_line, end_col, align_target, kw_line, expected_col
            );
            let location = crate::offense::Location::from_offsets(source, end_off, end_loc.end_offset());
            self.offenses.push(Offense::new(
                "Layout/EndAlignment",
                message,
                Severity::Convention,
                location,
                self.ctx.filename,
            ));
        }
    }

    fn build_align_target(&self, keyword: &str, kw_off: usize, source: &str) -> String {
        match self.style {
            EndAlignmentStyle::Keyword => keyword.to_string(),
            EndAlignmentStyle::Variable => {
                if only_whitespace_before_keyword(source, kw_off) {
                    return keyword.to_string();
                }
                let ls = line_start_offset(source, kw_off);
                let before = &source[ls..kw_off];
                if let Some(semi_pos) = before.rfind(';') {
                    if before[semi_pos + 1..].trim().is_empty() {
                        return keyword.to_string();
                    }
                }
                let first_nw = source[ls..]
                    .chars()
                    .position(|c| !c.is_whitespace() && c != '\u{FEFF}')
                    .unwrap_or(0);
                source[ls + first_nw..kw_off + keyword.len()].to_string()
            }
            EndAlignmentStyle::StartOfLine => {
                let line_text = get_line_text(source, kw_off);
                line_text
                    .trim_start_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}')
                    .trim_end()
                    .to_string()
            }
        }
    }
}

macro_rules! visit_keyword_end {
    ($self:ident, $node:ident, $keyword:expr, $kw_loc:expr, $end_loc:expr, $visit_fn:path) => {{
        $self.check_keyword_end($keyword, &$kw_loc, &$end_loc);
        $visit_fn($self, $node);
    }};
}

impl Visit<'_> for EndAlignmentVisitor<'_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode) {
        if let Some(kw_loc) = node.if_keyword_loc() {
            if let Some(end_loc) = node.end_keyword_loc() {
                let kw_text = std::str::from_utf8(kw_loc.as_slice()).unwrap_or("if");
                if kw_text == "if" {
                    self.check_keyword_end("if", &kw_loc, &end_loc);
                }
            }
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode) {
        if let Some(end_loc) = node.end_keyword_loc() {
            visit_keyword_end!(self, node, "unless", node.keyword_loc(), end_loc, ruby_prism::visit_unless_node);
        } else {
            ruby_prism::visit_unless_node(self, node);
        }
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode) {
        if let Some(end_loc) = node.closing_loc() {
            visit_keyword_end!(self, node, "while", node.keyword_loc(), end_loc, ruby_prism::visit_while_node);
        } else {
            ruby_prism::visit_while_node(self, node);
        }
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode) {
        if let Some(end_loc) = node.closing_loc() {
            visit_keyword_end!(self, node, "until", node.keyword_loc(), end_loc, ruby_prism::visit_until_node);
        } else {
            ruby_prism::visit_until_node(self, node);
        }
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode) {
        visit_keyword_end!(self, node, "case", node.case_keyword_loc(), node.end_keyword_loc(), ruby_prism::visit_case_node);
    }

    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode) {
        visit_keyword_end!(self, node, "case", node.case_keyword_loc(), node.end_keyword_loc(), ruby_prism::visit_case_match_node);
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        visit_keyword_end!(self, node, "class", node.class_keyword_loc(), node.end_keyword_loc(), ruby_prism::visit_class_node);
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        visit_keyword_end!(self, node, "class", node.class_keyword_loc(), node.end_keyword_loc(), ruby_prism::visit_singleton_class_node);
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        visit_keyword_end!(self, node, "module", node.module_keyword_loc(), node.end_keyword_loc(), ruby_prism::visit_module_node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        if let Some(end_loc) = node.end_keyword_loc() {
            visit_keyword_end!(self, node, "def", node.def_keyword_loc(), end_loc, ruby_prism::visit_def_node);
        } else {
            ruby_prism::visit_def_node(self, node);
        }
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        if let Some(kw_loc) = node.begin_keyword_loc() {
            if let Some(end_loc) = node.end_keyword_loc() {
                self.check_keyword_end("begin", &kw_loc, &end_loc);
            }
        }
        ruby_prism::visit_begin_node(self, node);
    }

    fn visit_for_node(&mut self, node: &ruby_prism::ForNode) {
        visit_keyword_end!(self, node, "for", node.for_keyword_loc(), node.end_keyword_loc(), ruby_prism::visit_for_node);
    }
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style_align_with: String,
}
impl Default for Cfg {
    fn default() -> Self { Self { enforced_style_align_with: "keyword".into() } }
}

crate::register_cop!("Layout/EndAlignment", |cfg| {
    let c: Cfg = cfg.typed("Layout/EndAlignment");
    let align_style = match c.enforced_style_align_with.as_str() {
        "variable" => EndAlignmentStyle::Variable,
        "start_of_line" => EndAlignmentStyle::StartOfLine,
        _ => EndAlignmentStyle::Keyword,
    };
    Some(Box::new(EndAlignment::new(align_style)))
});
