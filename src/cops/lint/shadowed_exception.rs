//! Lint/ShadowedException - Detects rescue clauses that mask each other.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/lint/shadowed_exception.rb

use crate::cops::{CheckContext, Cop};
use crate::node_name;
use crate::offense::{Offense, Severity};
use ruby_prism::{Node, Visit};

const MSG: &str = "Do not shadow rescued Exceptions.";

#[derive(Default)]
pub struct ShadowedException;

impl ShadowedException {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for ShadowedException {
    fn name(&self) -> &'static str {
        "Lint/ShadowedException"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = Visitor { ctx, offenses: Vec::new() };
        visitor.visit_program_node(node);
        visitor.offenses
    }
}

struct Visitor<'a> {
    ctx: &'a CheckContext<'a>,
    offenses: Vec<Offense>,
}

/// Represents an exception class for ancestry comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ExClass {
    /// A known exception class with depth in the ancestry tree (Exception=0, StandardError=1, etc.)
    Known(&'static str),
    /// Errno::* subclass (special handling)
    Errno(String),
    /// Unknown / non-const expression
    Unknown,
}

impl<'a> Visitor<'a> {
    /// Walk the rescue chain and collect (group, location) pairs.
    fn collect_rescue_data(&self, first: &ruby_prism::RescueNode) -> Vec<(Vec<ExClass>, (usize, usize))> {
        let mut out = Vec::new();
        let group = self.group_for(first);
        let loc = first.location();
        out.push((group, (loc.start_offset(), loc.end_offset())));
        let mut next = first.subsequent();
        while let Some(r) = next {
            let group = self.group_for(&r);
            let loc = r.location();
            out.push((group, (loc.start_offset(), loc.end_offset())));
            next = r.subsequent();
        }
        out
    }

    /// Convert exception expression node into ExClass.
    fn classify(&self, node: &Node) -> ExClass {
        match node {
            Node::ConstantReadNode { .. } => {
                let name = node_name!(node.as_constant_read_node().unwrap()).to_string();
                Self::lookup(&name)
            }
            Node::ConstantPathNode { .. } => {
                let path = node.as_constant_path_node().unwrap();
                let parent = path.parent();
                let name = path
                    .name()
                    .map(|id| String::from_utf8_lossy(id.as_slice()).to_string())
                    .unwrap_or_default();
                // Errno::* detection
                if let Some(p) = parent {
                    if let Node::ConstantReadNode { .. } = &p {
                        let parent_name = node_name!(p.as_constant_read_node().unwrap()).to_string();
                        if parent_name == "Errno" {
                            return ExClass::Errno(name);
                        }
                    }
                }
                Self::lookup(&name)
            }
            _ => ExClass::Unknown,
        }
    }

    /// Look up known exception class.
    fn lookup(name: &str) -> ExClass {
        if exception_depth(name).is_some() {
            ExClass::Known(KNOWN_NAME_LOOKUP.iter().find(|n| **n == name).copied().unwrap_or(""))
        } else {
            ExClass::Unknown
        }
    }

    /// Get all rescued exceptions for one rescue clause (its `exceptions` list).
    /// Empty list = bare `rescue` → treat as `[StandardError]`.
    fn group_for(&self, rescue: &ruby_prism::RescueNode) -> Vec<ExClass> {
        let exceptions: Vec<Node> = rescue.exceptions().iter().collect();
        if exceptions.is_empty() {
            return vec![ExClass::Known("StandardError")];
        }
        // Skip splat/array/etc — treat as Unknown.
        exceptions.iter().map(|e| self.classify(e)).collect()
    }

    /// Compare two exception classes.
    /// Returns `Some(Ordering)` if comparable, `None` if unrelated.
    /// `Equal` means same class (including duplicates).
    fn compare(a: &ExClass, b: &ExClass) -> Option<std::cmp::Ordering> {
        use std::cmp::Ordering;
        match (a, b) {
            (ExClass::Known(x), ExClass::Known(y)) => {
                if x == y { return Some(Ordering::Equal); }
                let dx = exception_depth(x)?;
                let dy = exception_depth(y)?;
                if is_ancestor_of(x, y) { Some(Ordering::Less) }
                else if is_ancestor_of(y, x) { Some(Ordering::Greater) }
                else if dx == dy { None } else { None }
            }
            (ExClass::Errno(_), ExClass::Errno(_)) => {
                // Special case: assume different Errno codes → not shadowed.
                // (RuboCop checks runtime const equality; we conservatively say no relation.)
                None
            }
            _ => None,
        }
    }

    /// Does this group contain multiple levels of exceptions (i.e., one is ancestor of another, or duplicates)?
    fn contains_multiple_levels(group: &[ExClass]) -> bool {
        // "Always treat Exception as the highest" → if group has Exception + something else, true.
        let has_exception = group.iter().any(|e| matches!(e, ExClass::Known(s) if *s == "Exception"));
        if group.len() > 1 && has_exception {
            return true;
        }
        // Check pairs
        for i in 0..group.len() {
            for j in (i + 1)..group.len() {
                if Self::compare(&group[i], &group[j]).is_some() {
                    return true;
                }
            }
        }
        false
    }

    /// Sorted check — consecutive group pairs must have x <= y (lower-level before higher-level).
    /// Returns false if NOT sorted (i.e., some shadowing exists).
    fn sorted(groups: &[Vec<ExClass>]) -> bool {
        use std::cmp::Ordering;
        for pair in groups.windows(2) {
            let (x, y) = (&pair[0], &pair[1]);
            let x_has_exception = x.iter().any(|e| matches!(e, ExClass::Known(s) if *s == "Exception"));
            let y_has_exception = y.iter().any(|e| matches!(e, ExClass::Known(s) if *s == "Exception"));

            if x_has_exception {
                return false;
            }
            if y_has_exception {
                continue; // sorted
            }
            // If a group is empty or all-Unknown → treat as sorted.
            let x_all_unknown = x.iter().all(|e| matches!(e, ExClass::Unknown));
            let y_all_unknown = y.iter().all(|e| matches!(e, ExClass::Unknown));
            if x.is_empty() || y.is_empty() || x_all_unknown || y_all_unknown {
                continue;
            }

            // Compare across pairs (any x[i] vs y[j])
            let mut any_known_pair = false;
            let mut violated = false;
            for xi in x {
                for yj in y {
                    if let Some(ord) = Self::compare(xi, yj) {
                        any_known_pair = true;
                        if ord == Ordering::Greater {
                            violated = true;
                        }
                    }
                }
            }
            if any_known_pair && violated {
                return false;
            }
        }
        true
    }

    /// Find which rescue is the shadowing one.
    fn find_shadowing(groups: &[Vec<ExClass>]) -> Option<usize> {
        for (i, g) in groups.iter().enumerate() {
            if Self::contains_multiple_levels(g) {
                return Some(i);
            }
        }
        for (i, win) in groups.windows(2).enumerate() {
            if !Self::sorted(win) {
                return Some(i);
            }
        }
        None
    }

    fn check_rescue(&mut self, first: &ruby_prism::RescueNode) {
        let data = self.collect_rescue_data(first);
        let groups: Vec<Vec<ExClass>> = data.iter().map(|(g, _)| g.clone()).collect();

        let any_multilevel = groups.iter().any(|g| Self::contains_multiple_levels(g));
        if !any_multilevel && Self::sorted(&groups) {
            return;
        }

        if let Some(idx) = Self::find_shadowing(&groups) {
            let (start, end) = data[idx].1;
            self.offenses.push(self.ctx.offense_with_range(
                "Lint/ShadowedException",
                MSG,
                Severity::Warning,
                start,
                end,
            ));
        }
    }
}

impl<'a> Visit<'_> for Visitor<'a> {
    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode) {
        if let Some(rescue) = node.rescue_clause() {
            self.check_rescue(&rescue);
        }
        ruby_prism::visit_begin_node(self, node);
    }
}

// ── Built-in Ruby exception ancestry ──

/// Names list (for &'static str references in classify).
const KNOWN_NAME_LOOKUP: &[&str] = &[
    "Exception", "StandardError", "ScriptError", "SecurityError",
    "SignalException", "SystemExit", "SystemStackError", "NoMemoryError",
    "ArgumentError", "EncodingError", "FiberError", "IOError", "EOFError",
    "IndexError", "KeyError", "StopIteration", "LocalJumpError", "NameError",
    "NoMethodError", "RangeError", "FloatDomainError", "RegexpError",
    "RuntimeError", "FrozenError", "SystemCallError", "ThreadError",
    "TypeError", "ZeroDivisionError", "LoadError", "NotImplementedError",
    "SyntaxError", "Interrupt",
];

/// Depth from Exception (Exception=0, StandardError/ScriptError/etc=1, deeper subclasses=2+).
fn exception_depth(name: &str) -> Option<usize> {
    Some(match name {
        "Exception" => 0,
        "StandardError" | "ScriptError" | "SecurityError" | "SignalException"
        | "SystemExit" | "SystemStackError" | "NoMemoryError" => 1,
        "ArgumentError" | "EncodingError" | "FiberError" | "IOError"
        | "IndexError" | "LocalJumpError" | "NameError" | "RangeError"
        | "RegexpError" | "RuntimeError" | "SystemCallError" | "ThreadError"
        | "TypeError" | "ZeroDivisionError"
        | "LoadError" | "NotImplementedError" | "SyntaxError"
        | "Interrupt" => 2,
        "EOFError" | "KeyError" | "StopIteration" | "NoMethodError"
        | "FloatDomainError" | "FrozenError" => 3,
        _ => return None,
    })
}

/// Is `ancestor` an ancestor of `descendant` in Ruby's exception hierarchy?
fn is_ancestor_of(ancestor: &str, descendant: &str) -> bool {
    if ancestor == descendant { return false; }
    if ancestor == "Exception" {
        return exception_depth(descendant).is_some();
    }
    let mut cur = parent_of(descendant);
    while let Some(p) = cur {
        if p == ancestor { return true; }
        cur = parent_of(p);
    }
    false
}

fn parent_of(name: &str) -> Option<&'static str> {
    Some(match name {
        "StandardError" | "ScriptError" | "SecurityError" | "SignalException"
        | "SystemExit" | "SystemStackError" | "NoMemoryError" => "Exception",

        "ArgumentError" | "EncodingError" | "FiberError" | "IOError"
        | "IndexError" | "LocalJumpError" | "NameError" | "RangeError"
        | "RegexpError" | "RuntimeError" | "SystemCallError" | "ThreadError"
        | "TypeError" | "ZeroDivisionError" => "StandardError",

        "EOFError" => "IOError",
        "KeyError" | "StopIteration" => "IndexError",
        "NoMethodError" => "NameError",
        "FloatDomainError" => "RangeError",
        "FrozenError" => "RuntimeError",

        "LoadError" | "NotImplementedError" | "SyntaxError" => "ScriptError",

        "Interrupt" => "SignalException",

        _ => return None,
    })
}
