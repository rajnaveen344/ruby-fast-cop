//! Style/BisectedAttrAccessor cop
//!
//! Detects `attr_reader :x` + `attr_writer :x` pairs → suggest `attr_accessor :x`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{Node, Visit};
use std::collections::HashMap;

const MSG: &str = "Combine both accessors into `attr_accessor %s`.";

#[derive(Default)]
pub struct BisectedAttrAccessor;

impl BisectedAttrAccessor {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for BisectedAttrAccessor {
    fn name(&self) -> &'static str {
        "Style/BisectedAttrAccessor"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut visitor = AttrVisitor {
            cop: self,
            ctx,
            offenses: Vec::new(),
            scope_stack: Vec::new(),
        };
        ruby_prism::visit_program_node(&mut visitor, &result.node().as_program_node().unwrap());
        visitor.offenses
    }
}

/// Represents one `attr_reader`/`attr_writer`/`attr` call with its attributes.
#[derive(Debug, Clone)]
struct AttrCall {
    kind: AttrKind, // reader or writer
    attrs: Vec<AttrArg>, // each attribute argument
    call_start: usize,
    call_end: usize,
    /// Column offsets of each attribute argument (for offense reporting)
    attr_arg_ranges: Vec<(usize, usize)>, // (start, end) per attr
}

#[derive(Debug, Clone, PartialEq)]
enum AttrKind {
    Reader, // attr_reader or attr
    Writer, // attr_writer
}

/// An attribute argument (symbol or splat)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum AttrArg {
    Symbol(String),   // :foo
    Splat(String),    // *ATTRS
}

impl AttrArg {
    fn display(&self) -> String {
        match self {
            AttrArg::Symbol(s) => format!(":{}", s),
            AttrArg::Splat(s) => format!("*{}", s),
        }
    }
}

/// Visibility scope bucket.
#[derive(Debug, Default)]
struct VisibilityBucket {
    readers: Vec<AttrCall>,
    writers: Vec<AttrCall>,
}

/// A scope (class/module/eigenclass) with visibility-separated attr calls.
#[derive(Debug, Default)]
struct Scope {
    /// visibility → bucket
    buckets: Vec<(String, VisibilityBucket)>,
    current_visibility: String,
}

impl Scope {
    fn new() -> Self {
        Self {
            buckets: vec![("public".to_string(), VisibilityBucket::default())],
            current_visibility: "public".to_string(),
        }
    }

    fn set_visibility(&mut self, vis: &str) {
        self.current_visibility = vis.to_string();
        if !self.buckets.iter().any(|(v, _)| v == vis) {
            self.buckets.push((vis.to_string(), VisibilityBucket::default()));
        }
    }

    fn add_reader(&mut self, call: AttrCall) {
        let vis = self.current_visibility.clone();
        if let Some(b) = self.buckets.iter_mut().find(|(v, _)| *v == vis) {
            b.1.readers.push(call);
        } else {
            self.buckets.push((vis.clone(), VisibilityBucket { readers: vec![call], writers: vec![] }));
        }
    }

    fn add_writer(&mut self, call: AttrCall) {
        let vis = self.current_visibility.clone();
        if let Some(b) = self.buckets.iter_mut().find(|(v, _)| *v == vis) {
            b.1.writers.push(call);
        } else {
            self.buckets.push((vis.clone(), VisibilityBucket { readers: vec![], writers: vec![call] }));
        }
    }
}

struct AttrVisitor<'a> {
    cop: &'a BisectedAttrAccessor,
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
    scope_stack: Vec<Scope>,
}

impl AttrVisitor<'_> {
    fn enter_scope(&mut self) {
        self.scope_stack.push(Scope::new());
    }

    fn exit_scope(&mut self) {
        if let Some(scope) = self.scope_stack.pop() {
            let offenses = self.check_scope(scope);
            self.offenses.extend(offenses);
        }
    }

    fn current_scope_mut(&mut self) -> Option<&mut Scope> {
        self.scope_stack.last_mut()
    }

    fn parse_attr_call(&self, node: &ruby_prism::CallNode) -> Option<AttrCall> {
        let method_name = crate::node_name!(node);
        let kind = match method_name.as_ref() {
            "attr_reader" | "attr" => AttrKind::Reader,
            "attr_writer" => AttrKind::Writer,
            _ => return None,
        };

        // Must have no explicit receiver (bare call)
        if node.receiver().is_some() {
            return None;
        }

        let args = node.arguments()?;
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.is_empty() {
            return None;
        }

        let mut attrs = Vec::new();
        let mut attr_arg_ranges = Vec::new();

        for arg in &arg_list {
            let loc = arg.location();
            let src = &self.ctx.source[loc.start_offset()..loc.end_offset()];
            match arg {
                Node::SymbolNode { .. } => {
                    let sym = arg.as_symbol_node().unwrap();
                    let name = String::from_utf8_lossy(sym.unescaped().as_ref()).to_string();
                    attrs.push(AttrArg::Symbol(name));
                    attr_arg_ranges.push((loc.start_offset(), loc.end_offset()));
                }
                Node::SplatNode { .. } => {
                    // `*ATTRIBUTES` — treat splat as a unit
                    let splat = arg.as_splat_node().unwrap();
                    let inner_src = &self.ctx.source[loc.start_offset()..loc.end_offset()];
                    let inner = inner_src.trim_start_matches('*');
                    attrs.push(AttrArg::Splat(inner.to_string()));
                    attr_arg_ranges.push((loc.start_offset(), loc.end_offset()));
                }
                _ => return None, // Unknown arg type
            }
        }

        Some(AttrCall {
            kind,
            attrs,
            call_start: node.location().start_offset(),
            call_end: node.location().end_offset(),
            attr_arg_ranges,
        })
    }

    fn get_indent(source: &str, node_start: usize) -> String {
        let bytes = source.as_bytes();
        let mut line_start = node_start;
        while line_start > 0 && bytes[line_start - 1] != b'\n' {
            line_start -= 1;
        }
        let mut indent = String::new();
        for &b in &bytes[line_start..node_start] {
            if b == b' ' || b == b'\t' {
                indent.push(b as char);
            } else {
                break;
            }
        }
        indent
    }

    /// Returns the range [start, end) including the trailing newline
    fn range_with_newline(source: &str, start: usize, end: usize) -> (usize, usize) {
        let bytes = source.as_bytes();
        // Find the line start (to include leading indent in the range we remove)
        let mut line_start = start;
        while line_start > 0 && bytes[line_start - 1] != b'\n' {
            line_start -= 1;
        }
        // Find end of line (past the newline)
        let mut line_end = end;
        while line_end < bytes.len() && bytes[line_end] != b'\n' {
            line_end += 1;
        }
        if line_end < bytes.len() && bytes[line_end] == b'\n' {
            line_end += 1;
        }
        (line_start, line_end)
    }

    fn check_scope(&self, scope: Scope) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let source = self.ctx.source;

        for (_vis, bucket) in &scope.buckets {
            let readers = &bucket.readers;
            let writers = &bucket.writers;

            if readers.is_empty() || writers.is_empty() {
                continue;
            }

            // Build attr → positions maps
            let mut reader_map: HashMap<AttrArg, Vec<(usize, usize)>> = HashMap::new();
            for (ci, call) in readers.iter().enumerate() {
                for (ai, attr) in call.attrs.iter().enumerate() {
                    reader_map.entry(attr.clone()).or_default().push((ci, ai));
                }
            }
            let mut writer_map: HashMap<AttrArg, Vec<(usize, usize)>> = HashMap::new();
            for (ci, call) in writers.iter().enumerate() {
                for (ai, attr) in call.attrs.iter().enumerate() {
                    writer_map.entry(attr.clone()).or_default().push((ci, ai));
                }
            }

            // Find bisected attrs (in original order they appear in first reader)
            let all_reader_attrs: Vec<AttrArg> = readers.iter()
                .flat_map(|r| r.attrs.iter().cloned())
                .collect();
            let bisected_set: std::collections::HashSet<AttrArg> = reader_map.keys()
                .filter(|a| writer_map.contains_key(a))
                .cloned()
                .collect();
            // Preserve order from readers
            let mut seen = std::collections::HashSet::new();
            let bisected_attrs: Vec<AttrArg> = all_reader_attrs.into_iter()
                .filter(|a| bisected_set.contains(a) && seen.insert(a.clone()))
                .collect();

            if bisected_attrs.is_empty() {
                continue;
            }

            // Collect all offense positions
            let mut all_offense_positions: Vec<(usize, usize, String)> = Vec::new(); // (start, end, msg)

            for attr in &bisected_attrs {
                let msg = MSG.replacen("%s", &attr.display(), 1);

                if let Some(positions) = reader_map.get(attr) {
                    for &(ci, ai) in positions {
                        let call = &readers[ci];
                        let (arg_start, arg_end) = call.attr_arg_ranges[ai];
                        all_offense_positions.push((arg_start, arg_end, msg.clone()));
                    }
                }
                if let Some(positions) = writer_map.get(attr) {
                    for &(ci, ai) in positions {
                        let call = &writers[ci];
                        let (arg_start, arg_end) = call.attr_arg_ranges[ai];
                        all_offense_positions.push((arg_start, arg_end, msg.clone()));
                    }
                }
            }

            // Sort offenses by position
            all_offense_positions.sort_by_key(|&(s, _, _)| s);

            // Build the multi-edit correction
            // Strategy: for each affected reader call and writer call, build new source
            let mut edits: Vec<Edit> = Vec::new();

            // Group bisected by reader call index
            for (ci, call) in readers.iter().enumerate() {
                let bisected_in_this: Vec<AttrArg> = call.attrs.iter()
                    .filter(|a| bisected_set.contains(a))
                    .cloned()
                    .collect();
                if bisected_in_this.is_empty() { continue; }

                let remaining: Vec<AttrArg> = call.attrs.iter()
                    .filter(|a| !bisected_set.contains(a))
                    .cloned()
                    .collect();

                let all_bisected = remaining.is_empty();
                let indent = Self::get_indent(source, call.call_start);

                let (line_start, line_end) = Self::range_with_newline(source, call.call_start, call.call_end);

                // accessor line: attr_accessor :x, :y, ...
                let accessor_names: Vec<String> = bisected_in_this.iter().map(|a| a.display()).collect();
                let accessor_line = format!("{}attr_accessor {}\n", indent, accessor_names.join(", "));

                if all_bisected {
                    // Replace whole line (including newline) with attr_accessor line
                    edits.push(Edit {
                        start_offset: line_start,
                        end_offset: line_end,
                        replacement: accessor_line,
                    });
                } else {
                    // Insert accessor line before, then replace node with remaining reader
                    let remaining_names: Vec<String> = remaining.iter().map(|a| a.display()).collect();
                    let reader_replacement = format!("attr_reader {}", remaining_names.join(", "));
                    // Insert accessor line before this node
                    edits.push(Edit {
                        start_offset: line_start,
                        end_offset: call.call_start,
                        replacement: format!("{}{}\n", indent, accessor_names.iter().map(|n| format!("attr_accessor {}", n)).collect::<Vec<_>>().join(&format!("\n{}", indent))),
                    });
                    // Actually for multiple bisected attrs in one reader call, we emit one attr_accessor line
                    // Let me reconsider - RuboCop emits one `attr_accessor :x, :y` line
                    // then the remaining reader. So replace the whole call with remaining reader.
                    // But we already emitted an insert above - need to redo this.
                    // Clear the last edit and do it properly:
                    edits.pop();
                    // Replace from line_start to call_start with accessor_line
                    edits.push(Edit {
                        start_offset: call.call_start,
                        end_offset: call.call_end,
                        replacement: reader_replacement,
                    });
                    // Insert accessor_line before (prepend to line)
                    edits.push(Edit {
                        start_offset: line_start,
                        end_offset: line_start,
                        replacement: accessor_line,
                    });
                }
            }

            // Process writers
            for call in writers.iter() {
                let bisected_in_this: Vec<AttrArg> = call.attrs.iter()
                    .filter(|a| bisected_set.contains(a))
                    .cloned()
                    .collect();
                if bisected_in_this.is_empty() { continue; }

                let remaining: Vec<AttrArg> = call.attrs.iter()
                    .filter(|a| !bisected_set.contains(a))
                    .cloned()
                    .collect();
                let all_bisected = remaining.is_empty();

                let (line_start, line_end) = Self::range_with_newline(source, call.call_start, call.call_end);

                if all_bisected {
                    // Remove whole line
                    edits.push(Edit {
                        start_offset: line_start,
                        end_offset: line_end,
                        replacement: String::new(),
                    });
                } else {
                    // Replace with remaining writer
                    let remaining_names: Vec<String> = remaining.iter().map(|a| a.display()).collect();
                    let writer_replacement = format!("attr_writer {}", remaining_names.join(", "));
                    edits.push(Edit {
                        start_offset: call.call_start,
                        end_offset: call.call_end,
                        replacement: writer_replacement,
                    });
                }
            }

            if edits.is_empty() {
                // Emit offenses without corrections
                for (start, end, msg) in all_offense_positions {
                    offenses.push(self.ctx.offense_with_range(
                        self.cop.name(), &msg, self.cop.severity(), start, end));
                }
            } else {
                // Sort edits by start_offset (apply_corrections expects sorted or will sort)
                edits.sort_by_key(|e| e.start_offset);

                let correction = Correction { edits };

                // Attach correction to first offense, rest get no correction
                let mut first = true;
                for (start, end, msg) in all_offense_positions {
                    let off = self.ctx.offense_with_range(
                        self.cop.name(), &msg, self.cop.severity(), start, end);
                    if first {
                        offenses.push(off.with_correction(correction.clone()));
                        first = false;
                    } else {
                        offenses.push(off);
                    }
                }
            }
        }

        offenses
    }
}

impl Visit<'_> for AttrVisitor<'_> {
    fn visit_class_node(&mut self, node: &ruby_prism::ClassNode) {
        self.enter_scope();
        ruby_prism::visit_class_node(self, node);
        self.exit_scope();
    }

    fn visit_module_node(&mut self, node: &ruby_prism::ModuleNode) {
        self.enter_scope();
        ruby_prism::visit_module_node(self, node);
        self.exit_scope();
    }

    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        self.enter_scope();
        ruby_prism::visit_singleton_class_node(self, node);
        self.exit_scope();
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode) {
        let scope = match self.scope_stack.last_mut() {
            Some(s) => s,
            None => {
                ruby_prism::visit_call_node(self, node);
                return;
            }
        };

        let method = crate::node_name!(node);
        match method.as_ref() {
            "private" | "protected" | "public" => {
                // Check if it's a bare visibility change (no args)
                if node.arguments().is_none() && node.receiver().is_none() {
                    scope.set_visibility(method.as_ref());
                }
            }
            "attr_reader" | "attr" | "attr_writer" => {
                if let Some(call) = self.parse_attr_call(node) {
                    match call.kind {
                        AttrKind::Reader => {
                            if let Some(s) = self.scope_stack.last_mut() {
                                s.add_reader(call);
                            }
                        }
                        AttrKind::Writer => {
                            if let Some(s) = self.scope_stack.last_mut() {
                                s.add_writer(call);
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        ruby_prism::visit_call_node(self, node);
    }
}

crate::register_cop!("Style/BisectedAttrAccessor", |_cfg| {
    Some(Box::new(BisectedAttrAccessor::new()))
});
