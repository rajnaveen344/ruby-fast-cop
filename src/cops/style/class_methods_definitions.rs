//! Style/ClassMethodsDefinitions cop

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::{col_at_offset, line_start_offset};
use crate::node_name;
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::Node;

const MSG_SCLASS: &str = "Do not define public methods within class << self.";
const MSG_DEF_SELF: &str = "Use `class << self` to define a class method.";

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    #[default]
    DefSelf,
    SelfClass,
}

pub struct ClassMethodsDefinitions {
    style: EnforcedStyle,
}

impl ClassMethodsDefinitions {
    pub fn new() -> Self {
        Self { style: EnforcedStyle::DefSelf }
    }

    pub fn with_style(style: EnforcedStyle) -> Self {
        Self { style }
    }

    fn class_elements<'a>(body: Option<Node<'a>>) -> Vec<Node<'a>> {
        let body = match body {
            Some(b) => b,
            None => return vec![],
        };
        if let Some(stmts) = body.as_statements_node() {
            stmts.body().iter().collect()
        } else {
            vec![body]
        }
    }

    /// Compute current visibility for each child via siblings sweep. Returns
    /// "public"/"private"/"protected" for the target node.
    fn node_visibility<'a>(elements: &[Node<'a>], target: &Node<'a>) -> &'static str {
        let mut current = "public";
        for el in elements {
            if let Some(call) = el.as_call_node() {
                if call.receiver().is_none() && call.arguments().is_none() && call.block().is_none() {
                    let m = node_name!(call);
                    match m.as_ref() {
                        "private" => current = "private",
                        "protected" => current = "protected",
                        "public" => current = "public",
                        _ => {}
                    }
                }
            }
            if el.location().start_offset() == target.location().start_offset() {
                return match current {
                    "private" => "private",
                    "protected" => "protected",
                    _ => "public",
                };
            }
        }
        "public"
    }

    /// Instance defs only (no `def self.x`). Mirrors RuboCop `def_type?` (not defs_type).
    fn def_nodes<'a>(sclass: &ruby_prism::SingletonClassNode<'a>) -> Vec<Node<'a>> {
        let body = match sclass.body() {
            Some(b) => b,
            None => return vec![],
        };
        let is_instance_def = |n: &Node| -> bool {
            n.as_def_node().map(|d| d.receiver().is_none()).unwrap_or(false)
        };
        // Single DefNode body
        if is_instance_def(&body) {
            return vec![body];
        }
        // StatementsNode children, only instance defs
        if let Some(stmts) = body.as_statements_node() {
            return stmts
                .body()
                .iter()
                .filter(|n| is_instance_def(n))
                .collect();
        }
        vec![]
    }
}

impl Default for ClassMethodsDefinitions {
    fn default() -> Self {
        Self::new()
    }
}

impl Cop for ClassMethodsDefinitions {
    fn name(&self) -> &'static str {
        "Style/ClassMethodsDefinitions"
    }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        if self.style != EnforcedStyle::SelfClass {
            return vec![];
        }
        // Receiver must be self
        if !matches!(node.receiver(), Some(Node::SelfNode { .. })) {
            return vec![];
        }
        // Range = `def self.name` — `def` + ` ` + `self.` + name
        let loc = node.location();
        let name_loc = node.name_loc();
        let start = loc.start_offset();
        let end = name_loc.end_offset();
        vec![ctx.offense_with_range(self.name(), MSG_DEF_SELF, Severity::Convention, start, end)]
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        if self.style != EnforcedStyle::DefSelf {
            return vec![];
        }
        use ruby_prism::Visit;
        let mut visitor = SClassVisitor { ctx, cop: self, offenses: vec![] };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

/// Get the source range including preceding comment lines.
fn source_with_preceding_comments(source: &str, node_start: usize, node_end: usize) -> (usize, usize) {
    let node_line_start = line_start_offset(source, node_start);
    let source_bytes = source.as_bytes();
    let mut cursor = node_line_start;
    loop {
        if cursor == 0 { break; }
        let prev_line_end = cursor - 1;
        let prev_line_start = line_start_offset(source, prev_line_end);
        let prev_line = &source[prev_line_start..prev_line_end];
        let trimmed = prev_line.trim();
        if trimmed.starts_with('#') {
            cursor = prev_line_start;
        } else {
            break;
        }
    }
    (cursor, node_end)
}

/// Build correction for `class << self ... end` → `def self.x; ...; end` transformations.
fn build_sclass_correction(
    sclass: &ruby_prism::SingletonClassNode,
    def_nodes: &[ruby_prism::Node],
    source: &str,
) -> Option<Correction> {
    // Compute sclass column (for indentation diff)
    let sclass_node_start = sclass.location().start_offset();
    let sclass_col = col_at_offset(source, sclass_node_start) as usize;
    let sclass_end = sclass.location().end_offset();
    // Edit range starts at the beginning of the sclass line (includes leading indent)
    let sclass_start = line_start_offset(source, sclass_node_start);

    // Check if sclass only has methods (all children are the def_nodes)
    let only_methods = sclass_only_methods(sclass);

    // For each def: build rewritten source
    let mut rewritten_defs: Vec<String> = Vec::new();
    let mut def_edits: Vec<Edit> = Vec::new(); // to remove defs from sclass

    for def_node in def_nodes {
        let def = def_node.as_def_node().unwrap();
        let (range_start, range_end) = source_with_preceding_comments(
            source,
            def_node.location().start_offset(),
            def_node.location().end_offset(),
        );
        let def_src = &source[range_start..range_end];

        // Replace `def foo` with `def self.foo`
        let def_name = String::from_utf8_lossy(def.name().as_slice()).into_owned();
        let rewritten = def_src.replacen(&format!("def {}", def_name), &format!("def self.{}", def_name), 1);

        // Un-indent by sclass_col + 2 spaces (the additional indent inside class << self)
        let indent_to_remove = sclass_col + 2;
        let prefix: String = " ".repeat(indent_to_remove);
        let unindented: String = rewritten.lines()
            .map(|line| {
                if line.starts_with(&prefix) {
                    &line[indent_to_remove..]
                } else {
                    line.trim_start_matches(' ')
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let trimmed = unindented.trim_end_matches('\n').to_string();
        rewritten_defs.push(trimmed);

        if !only_methods {
            // Remove the def (including preceding comments) from sclass body
            // We need to also remove the trailing newline
            let remove_end = if range_end < source.len() && source.as_bytes()[range_end] == b'\n' {
                range_end + 1
            } else {
                range_end
            };
            def_edits.push(Edit {
                start_offset: range_start,
                end_offset: remove_end,
                replacement: String::new(),
            });
        }
    }

    if rewritten_defs.is_empty() {
        return None;
    }

    let indent = " ".repeat(sclass_col);
    let mut edits: Vec<Edit> = Vec::new();

    if only_methods {
        // Replace entire sclass with rewritten defs
        // First def gets its leading whitespace stripped
        if let Some(first) = rewritten_defs.first_mut() {
            *first = first.trim_start().to_string();
        }
        // Add indent to each def
        let indented_defs: Vec<String> = rewritten_defs.iter()
            .map(|d| {
                d.lines()
                    .map(|l| if l.is_empty() { String::new() } else { format!("{}{}", indent, l) })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect();

        let replacement = indented_defs.join(&format!("\n\n"));
        edits.push(Edit {
            start_offset: sclass_start,
            end_offset: sclass_end,
            replacement,
        });
    } else {
        // Keep sclass (with defs removed), insert rewritten defs after sclass
        edits.extend(def_edits);

        // Build inserted text
        let indented_defs: Vec<String> = rewritten_defs.iter()
            .map(|d| {
                d.lines()
                    .map(|l| if l.is_empty() { String::new() } else { format!("{}{}", indent, l) })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect();

        let insert_text = format!("\n\n{}", indented_defs.join(&format!("\n\n")));
        edits.push(Edit {
            start_offset: sclass_end,
            end_offset: sclass_end,
            replacement: insert_text,
        });
    }

    Some(Correction { edits })
}

fn sclass_only_methods(sclass: &ruby_prism::SingletonClassNode) -> bool {
    let body = match sclass.body() {
        Some(b) => b,
        None => return false,
    };
    // Single def (no receiver)
    if let Some(d) = body.as_def_node() {
        return d.receiver().is_none();
    }
    // Statements — all must be defs (no receiver)
    if let Some(stmts) = body.as_statements_node() {
        return stmts.body().iter().all(|n| {
            n.as_def_node().map(|d| d.receiver().is_none()).unwrap_or(false)
        });
    }
    false
}

struct SClassVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    cop: &'a ClassMethodsDefinitions,
    offenses: Vec<Offense>,
}

impl<'a> ruby_prism::Visit<'_> for SClassVisitor<'a> {
    fn visit_singleton_class_node(&mut self, node: &ruby_prism::SingletonClassNode) {
        // Only `class << self`
        if matches!(node.expression(), Node::SelfNode { .. }) {
            let defs = ClassMethodsDefinitions::def_nodes(node);
            if !defs.is_empty() {
                let elements = ClassMethodsDefinitions::class_elements(node.body());
                let all_public = defs.iter().all(|d| {
                    ClassMethodsDefinitions::node_visibility(&elements, d) == "public"
                });
                if all_public {
                    let kw = node.class_keyword_loc();
                    let expr = node.expression();
                    let start = kw.start_offset();
                    let end = expr.location().end_offset();
                    let correction = build_sclass_correction(node, &defs, self.ctx.source);
                    let offense = self.ctx.offense_with_range(
                        self.cop.name(),
                        MSG_SCLASS,
                        Severity::Convention,
                        start,
                        end,
                    );
                    self.offenses.push(if let Some(c) = correction {
                        offense.with_correction(c)
                    } else {
                        offense
                    });
                }
            }
        }
        ruby_prism::visit_singleton_class_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: Option<String>,
}

crate::register_cop!("Style/ClassMethodsDefinitions", |cfg| {
    let c: Cfg = cfg.typed("Style/ClassMethodsDefinitions");
    let style = match c.enforced_style.as_deref() {
        Some("self_class") => EnforcedStyle::SelfClass,
        _ => EnforcedStyle::DefSelf,
    };
    Some(Box::new(ClassMethodsDefinitions::with_style(style)))
});
