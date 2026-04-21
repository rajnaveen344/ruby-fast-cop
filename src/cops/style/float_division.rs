//! Style/FloatDivision - enforce consistent `to_f` placement in division.
//!
//! Ported from:
//! https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/float_division.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Offense, Severity};
use ruby_prism::Node;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    LeftCoerce,
    RightCoerce,
    SingleCoerce,
    Fdiv,
}

pub struct FloatDivision {
    style: EnforcedStyle,
}

impl FloatDivision {
    pub fn new(style: EnforcedStyle) -> Self {
        Self { style }
    }
}

impl Default for FloatDivision {
    fn default() -> Self {
        Self::new(EnforcedStyle::SingleCoerce)
    }
}

/// Is `node` a `.to_f` call with a real receiver (not implicit self)?
fn is_to_f(n: &Node) -> bool {
    if let Node::CallNode { .. } = n {
        let c = n.as_call_node().unwrap();
        if node_name!(c) != "to_f" {
            return false;
        }
        if c.receiver().is_none() {
            return false;
        }
        if c.arguments().is_some() {
            return false;
        }
        true
    } else {
        false
    }
}

/// (start_offset, end_offset) of the receiver of a `.to_f` call.
fn to_f_receiver_range(n: &Node) -> Option<(usize, usize)> {
    if let Node::CallNode { .. } = n {
        let c = n.as_call_node().unwrap();
        let r = c.receiver()?;
        Some((r.location().start_offset(), r.location().end_offset()))
    } else {
        None
    }
}

/// Whether the receiver of `.to_f` is a Regexp.last_match(int) or nth-ref ($1).
fn to_f_inner_is_regexp_last_match_or_nth_ref(n: &Node) -> bool {
    if let Node::CallNode { .. } = n {
        let c = n.as_call_node().unwrap();
        if let Some(inner) = c.receiver() {
            return is_regexp_last_match_or_nth_ref(&inner);
        }
    }
    false
}

fn is_regexp_last_match_or_nth_ref(n: &Node) -> bool {
    match n {
        Node::NumberedReferenceReadNode { .. } => true,
        Node::CallNode { .. } => {
            let c = n.as_call_node().unwrap();
            if node_name!(c) != "last_match" {
                return false;
            }
            let args = match c.arguments() {
                Some(a) => a,
                None => return false,
            };
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if arg_list.len() != 1 {
                return false;
            }
            if !matches!(&arg_list[0], Node::IntegerNode { .. }) {
                return false;
            }
            let recv = match c.receiver() {
                Some(r) => r,
                None => return false,
            };
            match &recv {
                Node::ConstantReadNode { .. } => {
                    let cr = recv.as_constant_read_node().unwrap();
                    node_name!(cr) == "Regexp"
                }
                Node::ConstantPathNode { .. } => {
                    let cp = recv.as_constant_path_node().unwrap();
                    cp.name()
                        .map(|nm| nm.as_slice() == b"Regexp")
                        .unwrap_or(false)
                }
                _ => false,
            }
        }
        _ => false,
    }
}

impl Cop for FloatDivision {
    fn name(&self) -> &'static str {
        "Style/FloatDivision"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        if node_name!(node) != "/" {
            return vec![];
        }
        let recv = match node.receiver() {
            Some(r) => r,
            None => return vec![],
        };
        let args = match node.arguments() {
            Some(a) => a,
            None => return vec![],
        };
        let arg_list: Vec<_> = args.arguments().iter().collect();
        if arg_list.len() != 1 {
            return vec![];
        }
        let right = &arg_list[0];

        let left_coerce = is_to_f(&recv);
        let right_coerce = is_to_f(right);

        if left_coerce && to_f_inner_is_regexp_last_match_or_nth_ref(&recv) {
            return vec![];
        }
        if right_coerce && to_f_inner_is_regexp_last_match_or_nth_ref(right) {
            return vec![];
        }

        let offense_triggered = match self.style {
            EnforcedStyle::LeftCoerce => right_coerce,
            EnforcedStyle::RightCoerce => left_coerce,
            EnforcedStyle::SingleCoerce => left_coerce && right_coerce,
            EnforcedStyle::Fdiv => left_coerce || right_coerce,
        };

        if !offense_triggered {
            return vec![];
        }

        let message = match self.style {
            EnforcedStyle::LeftCoerce => "Prefer using `.to_f` on the left side.",
            EnforcedStyle::RightCoerce => "Prefer using `.to_f` on the right side.",
            EnforcedStyle::SingleCoerce => "Prefer using `.to_f` on one side only.",
            EnforcedStyle::Fdiv => "Prefer using `fdiv` for float divisions.",
        };

        let recv_range = (recv.location().start_offset(), recv.location().end_offset());
        let right_range = (
            right.location().start_offset(),
            right.location().end_offset(),
        );
        let left_inner = if left_coerce {
            to_f_receiver_range(&recv).unwrap_or(recv_range)
        } else {
            recv_range
        };
        let right_inner = if right_coerce {
            to_f_receiver_range(right).unwrap_or(right_range)
        } else {
            right_range
        };

        let correction = self.build_correction(
            node, recv_range, right_range, left_inner, right_inner, left_coerce, right_coerce, ctx,
        );

        let mut offense = ctx.offense(self.name(), message, self.severity(), &node.location());
        if let Some(c) = correction {
            offense = offense.with_correction(c);
        }
        vec![offense]
    }
}

impl FloatDivision {
    #[allow(clippy::too_many_arguments)]
    fn build_correction(
        &self,
        node: &ruby_prism::CallNode,
        recv_range: (usize, usize),
        right_range: (usize, usize),
        left_inner: (usize, usize),
        right_inner: (usize, usize),
        left_coerce: bool,
        right_coerce: bool,
        ctx: &CheckContext,
    ) -> Option<Correction> {
        match self.style {
            EnforcedStyle::LeftCoerce | EnforcedStyle::SingleCoerce => {
                let mut edits: Vec<Edit> = Vec::new();
                if !left_coerce {
                    edits.push(Edit {
                        start_offset: recv_range.1,
                        end_offset: recv_range.1,
                        replacement: ".to_f".to_string(),
                    });
                }
                if right_coerce {
                    edits.push(Edit {
                        start_offset: right_inner.1,
                        end_offset: right_range.1,
                        replacement: String::new(),
                    });
                }
                if edits.is_empty() {
                    return None;
                }
                Some(Correction { edits })
            }
            EnforcedStyle::RightCoerce => {
                let mut edits: Vec<Edit> = Vec::new();
                if left_coerce {
                    edits.push(Edit {
                        start_offset: left_inner.1,
                        end_offset: recv_range.1,
                        replacement: String::new(),
                    });
                }
                if !right_coerce {
                    edits.push(Edit {
                        start_offset: right_range.1,
                        end_offset: right_range.1,
                        replacement: ".to_f".to_string(),
                    });
                }
                if edits.is_empty() {
                    return None;
                }
                Some(Correction { edits })
            }
            EnforcedStyle::Fdiv => {
                let recv_src = &ctx.source[left_inner.0..left_inner.1];
                let arg_src = &ctx.source[right_inner.0..right_inner.1];
                let arg_wrapped = if arg_src.starts_with('(') && arg_src.ends_with(')') {
                    arg_src.to_string()
                } else {
                    format!("({})", arg_src)
                };
                let replacement = format!("{}.fdiv{}", recv_src, arg_wrapped);
                Some(Correction::replace(
                    node.location().start_offset(),
                    node.location().end_offset(),
                    replacement,
                ))
            }
        }
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg { enforced_style: String }

crate::register_cop!("Style/FloatDivision", |cfg| {
    let c: Cfg = cfg.typed("Style/FloatDivision");
    let style = match c.enforced_style.as_str() {
        "left_coerce" => EnforcedStyle::LeftCoerce,
        "right_coerce" => EnforcedStyle::RightCoerce,
        "fdiv" => EnforcedStyle::Fdiv,
        _ => EnforcedStyle::SingleCoerce,
    };
    Some(Box::new(FloatDivision::new(style)))
});
