//! Style/HashAsLastArrayItem cop
//!
//! Checks for presence/absence of braces around hash literal as last array item.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
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
                        vec![ctx.offense_with_range(
                            self.name(),
                            "Wrap hash in `{` and `}`.",
                            self.severity(),
                            start,
                            end,
                        )]
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
                        vec![ctx.offense_with_range(
                            self.name(),
                            "Omit the braces around the hash.",
                            self.severity(),
                            start,
                            end,
                        )]
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
