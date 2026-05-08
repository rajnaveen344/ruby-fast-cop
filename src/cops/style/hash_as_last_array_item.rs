//! Style/HashAsLastArrayItem cop
//!
//! Checks for presence/absence of braces around hash literal as last array item.

use crate::cops::{CheckContext, Cop};
use crate::helpers::source::{col_at_offset, line_at_offset};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::{ArrayNode, Node};

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Braces,
    NoBraces,
}

pub struct HashAsLastArrayItem {
    style: Style,
}

impl Default for HashAsLastArrayItem {
    fn default() -> Self {
        Self { style: Style::Braces }
    }
}

impl HashAsLastArrayItem {
    pub fn new(style: Style) -> Self {
        Self { style }
    }

    /// True if array uses explicit square brackets (not implicit)
    fn is_explicit_array(node: &ArrayNode) -> bool {
        node.opening_loc().is_some()
    }

    /// All elements are hashes already in the correct style → skip (RuboCop ignores this case)
    fn all_hashes_correct_style(elements: &[Node], braces_expected: bool) -> bool {
        elements.iter().all(|n| match n {
            Node::HashNode { .. } => braces_expected,         // has braces, wanted braces
            Node::KeywordHashNode { .. } => !braces_expected, // no braces, wanted no braces
            _ => false,
        })
    }

    fn check_array(&self, node: &ArrayNode, ctx: &CheckContext) -> Vec<Offense> {
        if !Self::is_explicit_array(node) {
            return vec![];
        }

        let elements: Vec<Node> = node.elements().iter().collect();
        if elements.is_empty() {
            return vec![];
        }

        let last = elements.last().unwrap();

        // Skip if second-to-last is also a hash (multiple consecutive hashes)
        if elements.len() >= 2 {
            let second_last = &elements[elements.len() - 2];
            if matches!(second_last, Node::HashNode { .. } | Node::KeywordHashNode { .. }) {
                return vec![];
            }
        }

        match self.style {
            Style::Braces => {
                // Flag if last element is KeywordHashNode (no braces)
                match last {
                    Node::KeywordHashNode { .. } => {
                        // Skip if it has a kwsplat
                        let kh = last.as_keyword_hash_node().unwrap();
                        let has_kwsplat = kh.elements().iter().any(|e| {
                            matches!(e, Node::AssocSplatNode { .. })
                        });
                        if has_kwsplat {
                            return vec![];
                        }
                        // Skip if all elements already have correct style (all braced)
                        if Self::all_hashes_correct_style(&elements, true) {
                            return vec![];
                        }

                        let start = last.location().start_offset();
                        let end = last.location().end_offset();
                        let array_start = node.location().start_offset();
                        let hash_line = line_at_offset(ctx.source, start);
                        let array_line = line_at_offset(ctx.source, array_start);
                        let is_single_line = hash_line == line_at_offset(ctx.source, end.saturating_sub(1));
                        let same_line_as_array = hash_line == array_line;

                        let correction = if is_single_line || same_line_as_array {
                            // wrap with { and }
                            Correction {
                                edits: vec![
                                    Edit { start_offset: start, end_offset: start, replacement: "{".into() },
                                    Edit { start_offset: end, end_offset: end, replacement: "}".into() },
                                ],
                            }
                        } else {
                            // multiline: wrap with {\n  indent and \n  indent}
                            let col = col_at_offset(ctx.source, start) as usize;
                            let indent = " ".repeat(col);
                            Correction {
                                edits: vec![
                                    Edit { start_offset: start, end_offset: start, replacement: format!("{{\n{}", indent) },
                                    Edit { start_offset: end, end_offset: end, replacement: format!("\n{}}}", indent) },
                                ],
                            }
                        };

                        vec![ctx.offense_with_range(
                            self.name(),
                            "Wrap hash in `{` and `}`.",
                            self.severity(),
                            start,
                            end,
                        ).with_correction(correction)]
                    }
                    Node::HashNode { .. } => {
                        // Already has braces — ok
                        vec![]
                    }
                    _ => vec![],
                }
            }
            Style::NoBraces => {
                // Flag if last element is HashNode (with braces)
                match last {
                    Node::HashNode { .. } => {
                        let hash = last.as_hash_node().unwrap();
                        // Empty hash cannot be unbraced
                        if hash.elements().iter().count() == 0 {
                            return vec![];
                        }
                        // Skip if all elements already have correct style (all unbraced)
                        if Self::all_hashes_correct_style(&elements, false) {
                            return vec![];
                        }

                        let start = last.location().start_offset();
                        let end = last.location().end_offset();

                        // Build correction: remove { and }, plus trailing comma after }
                        // HashNode opening_loc/closing_loc return Location directly (not Option)
                        let open_loc = hash.opening_loc();
                        let close_loc = hash.closing_loc();
                        let correction = {
                            let open = open_loc;
                            let close = close_loc;
                            let mut edits = Vec::new();
                            // Remove opening brace
                            edits.push(Edit {
                                start_offset: open.start_offset(),
                                end_offset: open.end_offset(),
                                replacement: String::new(),
                            });
                            // Remove closing brace
                            edits.push(Edit {
                                start_offset: close.start_offset(),
                                end_offset: close.end_offset(),
                                replacement: String::new(),
                            });
                            // Remove trailing comma after the hash (if present)
                            // scan byte after close.end for comma with surrounding space
                            let source_bytes = ctx.source.as_bytes();
                            let mut pos = close.end_offset();
                            // skip whitespace
                            while pos < source_bytes.len() && (source_bytes[pos] == b' ' || source_bytes[pos] == b'\t') {
                                pos += 1;
                            }
                            // Actually: RuboCop removes only the immediate trailing `,` after the hash element
                            // (range_with_surrounding_space from hash's last child end, side: right, resize 1)
                            // For our purposes: scan after close brace for `,`
                            if pos < source_bytes.len() && source_bytes[pos] == b',' {
                                edits.push(Edit {
                                    start_offset: pos,
                                    end_offset: pos + 1,
                                    replacement: String::new(),
                                });
                            }
                            Correction { edits }
                        };

                        vec![ctx.offense_with_range(
                            self.name(),
                            "Omit the braces around the hash.",
                            self.severity(),
                            start,
                            end,
                        ).with_correction(correction)]
                    }
                    _ => vec![],
                }
            }
        }
    }
}

impl Cop for HashAsLastArrayItem {
    fn name(&self) -> &'static str {
        "Style/HashAsLastArrayItem"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_array(&self, node: &ArrayNode, ctx: &CheckContext) -> Vec<Offense> {
        self.check_array(node, ctx)
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: Option<String>,
}

crate::register_cop!("Style/HashAsLastArrayItem", |cfg| {
    let c: Cfg = cfg.typed("Style/HashAsLastArrayItem");
    let style = match c.enforced_style.as_deref() {
        Some("no_braces") => Style::NoBraces,
        _ => Style::Braces,
    };
    Some(Box::new(HashAsLastArrayItem::new(style)))
});
