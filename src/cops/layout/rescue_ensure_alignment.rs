use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use ruby_prism::Visit;

/// Tracks the alignment context for rescue/ensure keywords.
#[derive(Debug, Clone)]
struct AlignInfo {
    /// 0-indexed column to align against
    col: usize,
    /// Source text for the message (e.g., "begin", "def test", "class C")
    source: String,
    /// 1-indexed line of the alignment target
    line: usize,
}

pub struct RescueEnsureAlignment {
    begin_end_alignment_style: Option<String>,
}

impl RescueEnsureAlignment {
    pub fn new() -> Self {
        Self {
            begin_end_alignment_style: None,
        }
    }

    pub fn with_begin_end_style(style: Option<String>) -> Self {
        Self {
            begin_end_alignment_style: style,
        }
    }
}

impl Default for RescueEnsureAlignment {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for RescueEnsureAlignment {
    fn name(&self) -> &'static str {
        "Layout/RescueEnsureAlignment"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(
        &self,
        node: &ruby_prism::ProgramNode,
        ctx: &CheckContext,
    ) -> Vec<Offense> {
        let mut visitor = RescueEnsureVisitor {
            ctx,
            offenses: Vec::new(),
            begin_end_alignment_style: self.begin_end_alignment_style.clone(),
        };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct RescueEnsureVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    begin_end_alignment_style: Option<String>,
}

impl<'a> RescueEnsureVisitor<'a> {
    /// Check alignment of a rescue/ensure keyword against expected alignment.
    fn check_keyword(
        &mut self,
        kw: &str,
        kw_offset: usize,
        kw_end_offset: usize,
        align: &AlignInfo,
    ) {
        let kw_col = self.ctx.col_of(kw_offset);
        let kw_line = self.ctx.line_of(kw_offset);

        // Same line — no alignment check
        if kw_line == align.line {
            return;
        }

        if kw_col == align.col {
            return;
        }

        let message = format!(
            "`{}` at {}, {} is not aligned with `{}` at {}, {}.",
            kw, kw_line, kw_col, align.source, align.line, align.col
        );

        let location =
            crate::offense::Location::from_offsets(self.ctx.source, kw_offset, kw_end_offset);
        self.offenses.push(Offense::new(
            "Layout/RescueEnsureAlignment",
            message,
            Severity::Convention,
            location,
            self.ctx.filename,
        ));
    }

    /// Build AlignInfo for a begin (kwbegin) node.
    fn align_info_for_begin(&self, node: &ruby_prism::BeginNode) -> Option<AlignInfo> {
        let begin_loc = node.begin_keyword_loc()?;
        let begin_off = begin_loc.start_offset();

        let style = self.begin_end_alignment_style.as_deref();

        match style {
            Some("start_of_line") => {
                let line_text = self.ctx.line_text(begin_off);
                let trimmed = line_text.trim_start();
                let indent = line_text.len() - trimmed.len();
                Some(AlignInfo {
                    col: indent,
                    source: trimmed.trim_end().to_string(),
                    line: self.ctx.line_of(begin_off),
                })
            }
            Some("begin") => Some(AlignInfo {
                col: self.ctx.col_of(begin_off),
                source: "begin".to_string(),
                line: self.ctx.line_of(begin_off),
            }),
            _ => {
                // Default: align with begin keyword
                Some(AlignInfo {
                    col: self.ctx.col_of(begin_off),
                    source: "begin".to_string(),
                    line: self.ctx.line_of(begin_off),
                })
            }
        }
    }

    /// Build AlignInfo for a def node.
    fn align_info_for_def(&self, node: &ruby_prism::DefNode) -> AlignInfo {
        let def_off = node.def_keyword_loc().start_offset();
        let name_end = node.name_loc().end_offset();
        let source = &self.ctx.source[def_off..name_end];
        AlignInfo {
            col: self.ctx.col_of(def_off),
            source: source.to_string(),
            line: self.ctx.line_of(def_off),
        }
    }

    /// Build AlignInfo from a keyword offset and name end offset (for class/module nodes).
    fn align_info_for_keyword_range(&self, kw_off: usize, name_end: usize) -> AlignInfo {
        let source = &self.ctx.source[kw_off..name_end];
        AlignInfo {
            col: self.ctx.col_of(kw_off),
            source: source.to_string(),
            line: self.ctx.line_of(kw_off),
        }
    }

    /// Build AlignInfo for a singleton class (class << self).
    fn align_info_for_sclass(&self, node: &ruby_prism::SingletonClassNode) -> AlignInfo {
        let kw_off = node.class_keyword_loc().start_offset();
        let expr_end = node.expression().location().end_offset();
        let source = &self.ctx.source[kw_off..expr_end];
        AlignInfo {
            col: self.ctx.col_of(kw_off),
            source: source.to_string(),
            line: self.ctx.line_of(kw_off),
        }
    }

    /// Build AlignInfo for a do...end block called on a method.
    fn align_info_for_block_call(
        &self,
        call_node: &ruby_prism::CallNode,
        block_node: &ruby_prism::BlockNode,
    ) -> AlignInfo {
        let call_off = call_node.location().start_offset();
        let open_loc = block_node.opening_loc();
        let do_off = open_loc.start_offset();
        let do_line = self.ctx.line_of(do_off);
        let call_line = self.ctx.line_of(call_off);

        // Check for assignment context on same line
        if let Some(assign) = self.find_assignment_context(call_off) {
            if assign.line == call_line {
                return assign;
            }
        }

        if do_line != call_line {
            // do keyword on a different line than call start
            let do_line_text = self.ctx.line_text(do_off);
            let do_line_trimmed = do_line_text.trim_start();
            let do_indent = do_line_text.len() - do_line_trimmed.len();

            // Leading dot pattern or trailing dot — align with the `do` line
            let line_start = self.ctx.line_start(do_off);
            let do_end = open_loc.end_offset();
            let source_text = self.ctx.src(line_start + do_indent, do_end).trim_end().to_string();
            return AlignInfo {
                col: do_indent,
                source: source_text,
                line: do_line,
            };
        }

        // Call and `do` on the same line — align with call start (including receiver)
        // Use receiver start if receiver is present (e.g., [1,2,3].each uses [1,2,3]'s start)
        let effective_start = if let Some(recv) = call_node.receiver() {
            recv.location().start_offset()
        } else {
            call_off
        };
        let do_end = open_loc.end_offset();
        let source_text = self.ctx.src(effective_start, do_end).trim_end().to_string();
        AlignInfo {
            col: self.ctx.col_of(effective_start),
            source: source_text,
            line: self.ctx.line_of(effective_start),
        }
    }

    /// Process rescue clauses within a container.
    fn check_rescues(&mut self, rescue: &ruby_prism::RescueNode, align: &AlignInfo) {
        let kw_loc = rescue.keyword_loc();
        self.check_keyword("rescue", kw_loc.start_offset(), kw_loc.end_offset(), align);

        if let Some(subsequent) = rescue.subsequent() {
            self.check_rescues(&subsequent, align);
        }
    }

    /// Process ensure clause.
    fn check_ensure(&mut self, ensure_node: &ruby_prism::EnsureNode, align: &AlignInfo) {
        let kw_loc = ensure_node.ensure_keyword_loc();
        self.check_keyword("ensure", kw_loc.start_offset(), kw_loc.end_offset(), align);
    }

    /// Process a begin node's rescue/ensure clauses.
    fn process_begin_node(&mut self, node: &ruby_prism::BeginNode, align: &AlignInfo) {
        if let Some(rescue) = node.rescue_clause() {
            self.check_rescues(&rescue, align);
        }
        if let Some(ensure) = node.ensure_clause() {
            self.check_ensure(&ensure, align);
        }
    }

    /// Check rescue/ensure in a def node.
    fn process_def_node(&mut self, node: &ruby_prism::DefNode, align_override: Option<AlignInfo>) {
        let align = align_override.unwrap_or_else(|| self.align_info_for_def(node));
        if let Some(body) = node.body() {
            if let Some(begin) = body.as_begin_node() {
                self.process_begin_node(&begin, &align);
            }
        }
    }

    /// Process a block node (do...end) with rescue/ensure.
    fn process_block_body(&mut self, block_node: &ruby_prism::BlockNode, align: &AlignInfo) {
        if let Some(body) = block_node.body() {
            if let Some(begin) = body.as_begin_node() {
                self.process_begin_node(&begin, align);
            }
        }
    }

    /// Try to find an assignment context on the line of the given offset.
    fn find_assignment_context(&self, offset: usize) -> Option<AlignInfo> {
        let line_start = self.ctx.line_start(offset);
        let before = &self.ctx.source[line_start..offset];
        let trimmed = before.trim_end();
        if trimmed.is_empty() {
            return None;
        }
        if !contains_assignment(trimmed) {
            return None;
        }
        let target = extract_assignment_target(trimmed);
        if target.is_empty() {
            return None;
        }
        let indent = before.len() - before.trim_start().len();
        Some(AlignInfo {
            col: indent,
            source: target,
            line: self.ctx.line_of(offset),
        })
    }

    /// Check if a block has rescue/ensure in its body.
    fn block_has_rescue_or_ensure(&self, block_node: &ruby_prism::BlockNode) -> bool {
        if let Some(body) = block_node.body() {
            if let Some(begin) = body.as_begin_node() {
                return begin.rescue_clause().is_some() || begin.ensure_clause().is_some();
            }
        }
        false
    }
}

impl Visit<'_> for RescueEnsureVisitor<'_> {
    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        if node.begin_keyword_loc().is_some() {
            // Explicit begin...end block
            if let Some(align) = self.align_info_for_begin(node) {
                self.process_begin_node(node, &align);
            }
        }
        ruby_prism::visit_begin_node(self, node);
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode) {
        // Only handle standalone def nodes (not those preceded by access modifiers)
        // Access-modifier-prefixed defs are handled in visit_call_node
        let def_off = node.def_keyword_loc().start_offset();
        if self.ctx.begins_its_line(def_off) {
            self.process_def_node(node, None);
        }
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        let align = self.align_info_for_keyword_range(
            node.class_keyword_loc().start_offset(),
            node.constant_path().location().end_offset(),
        );
        if let Some(body) = node.body() {
            if let Some(begin) = body.as_begin_node() {
                self.process_begin_node(&begin, &align);
            }
        }
        ruby_prism::visit_class_node(self, node);
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        let align = self.align_info_for_keyword_range(
            node.module_keyword_loc().start_offset(),
            node.constant_path().location().end_offset(),
        );
        if let Some(body) = node.body() {
            if let Some(begin) = body.as_begin_node() {
                self.process_begin_node(&begin, &align);
            }
        }
        ruby_prism::visit_module_node(self, node);
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        let align = self.align_info_for_sclass(node);
        if let Some(body) = node.body() {
            if let Some(begin) = body.as_begin_node() {
                self.process_begin_node(&begin, &align);
            }
        }
        ruby_prism::visit_singleton_class_node(self, node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        // Handle do...end blocks with rescue/ensure
        if let Some(block_ref) = node.block() {
            if let Some(block_node) = block_ref.as_block_node() {
                let open_text =
                    std::str::from_utf8(block_node.opening_loc().as_slice()).unwrap_or("");
                if open_text == "do" && self.block_has_rescue_or_ensure(&block_node) {
                    let align = self.align_info_for_block_call(node, &block_node);
                    self.process_block_body(&block_node, &align);
                }
            }
        }

        // Handle access modifier + def (private def foo / private_class_method def foo)
        if node.receiver().is_none() {
            let call_name = String::from_utf8_lossy(node.name().as_slice());
            let is_access_mod = matches!(
                call_name.as_ref(),
                "private"
                    | "protected"
                    | "public"
                    | "private_class_method"
                    | "public_class_method"
            );
            if is_access_mod {
                if let Some(args) = node.arguments() {
                    for arg in args.arguments().iter() {
                        if let Some(def_node) = arg.as_def_node() {
                            let call_off = node.location().start_offset();
                            let name_end = def_node.name_loc().end_offset();
                            let source = self.ctx.src(call_off, name_end).to_string();
                            let align = AlignInfo {
                                col: self.ctx.col_of(call_off),
                                source,
                                line: self.ctx.line_of(call_off),
                            };
                            self.process_def_node(&def_node, Some(align));
                        }
                    }
                }
            }
        }

        ruby_prism::visit_call_node(self, node);
    }

    fn visit_forwarding_super_node(&mut self, node: &ruby_prism::ForwardingSuperNode) {
        // `super do ... rescue ... end`
        if let Some(block_node) = node.block() {
            let open_text =
                std::str::from_utf8(block_node.opening_loc().as_slice()).unwrap_or("");
            if open_text == "do" && self.block_has_rescue_or_ensure(&block_node) {
                let super_off = node.location().start_offset();
                let do_end = block_node.opening_loc().end_offset();
                let source = self.ctx.src(super_off, do_end).trim_end().to_string();
                let align = AlignInfo {
                    col: self.ctx.col_of(super_off),
                    source,
                    line: self.ctx.line_of(super_off),
                };
                self.process_block_body(&block_node, &align);
            }
        }
        ruby_prism::visit_forwarding_super_node(self, node);
    }
}

/// Check if a string contains an assignment operator.
fn contains_assignment(s: &str) -> bool {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let q = bytes[i];
            i += 1;
            while i < len && bytes[i] != q {
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
        if bytes[i] == b'=' {
            let prev = if i > 0 { bytes[i - 1] } else { 0 };
            let next = if i + 1 < len { bytes[i + 1] } else { 0 };
            if next == b'=' || next == b'~' || next == b'>' {
                i += 2;
                continue;
            }
            if prev == b'!' || prev == b'<' || prev == b'>' {
                i += 1;
                continue;
            }
            return true;
        }
        i += 1;
    }
    false
}

/// Extract the assignment target from a string like "result = [1, 2, 3].map"
fn extract_assignment_target(s: &str) -> String {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut eq_pos = None;

    while i < len {
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let q = bytes[i];
            i += 1;
            while i < len && bytes[i] != q {
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
        if bytes[i] == b'=' {
            let prev = if i > 0 { bytes[i - 1] } else { 0 };
            let next = if i + 1 < len { bytes[i + 1] } else { 0 };
            if next == b'=' || next == b'~' || next == b'>' {
                i += 2;
                continue;
            }
            if prev == b'!' || prev == b'<' || prev == b'>' {
                i += 1;
                continue;
            }
            eq_pos = Some(i);
            break;
        }
        i += 1;
    }

    match eq_pos {
        Some(pos) => {
            let mut start = pos;
            if start > 0
                && matches!(
                    bytes[start - 1],
                    b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^'
                )
            {
                start -= 1;
                if start > 0
                    && bytes[start] == bytes[start - 1]
                    && (bytes[start] == b'&' || bytes[start] == b'|')
                {
                    start -= 1;
                }
            }
            let target = s[..start].trim();
            // For object attribute assignment (obj.attr =), use just the receiver (obj)
            // This matches RuboCop's `node.receiver.source_range`
            if let Some(dot_pos) = target.rfind('.') {
                // Check this looks like a method call, not a constant or class variable
                let after_dot = &target[dot_pos + 1..];
                let before_dot = &target[..dot_pos];
                if !before_dot.is_empty()
                    && after_dot.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && after_dot.chars().next().map_or(false, |c| c.is_lowercase() || c == '_')
                {
                    return before_dot.trim().to_string();
                }
            }
            target.to_string()
        }
        None => String::new(),
    }
}

crate::register_cop!("Layout/RescueEnsureAlignment", |cfg| {
    let begin_end_style = cfg.get_cop_config("Layout/BeginEndAlignment")
        .and_then(|c| {
            let enabled = c.raw.get("Enabled").and_then(|v| v.as_bool()).unwrap_or(true);
            if enabled {
                c.raw.get("EnforcedStyleAlignWith").and_then(|v| v.as_str().map(|s| s.to_string()))
            } else {
                None
            }
        });
    Some(Box::new(RescueEnsureAlignment::with_begin_end_style(begin_end_style)))
});
