//! Assignment: represents a single assignment to a variable (mirrors RuboCop's Assignment).

use super::branch::Branch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentKind {
    Simple,
    MultipleAssignment,
    OperatorAssignment,
    OrAssignment,
    AndAssignment,
    RegexpNamedCapture,
}

/// Represents one assignment to a variable.
pub struct Assignment {
    /// Name of the variable
    pub name: String,
    /// Byte offset of the name location (for offense reporting)
    pub name_start: usize,
    pub name_end: usize,
    /// Kind of assignment
    pub kind: AssignmentKind,
    /// Operator string for op-assign (e.g., "+", "||", "&&")
    pub op: Option<String>,
    /// For regexp captures: the regexp node location
    pub regexp_start: usize,
    pub regexp_end: usize,
    /// Whether this assignment has been referenced
    pub referenced: bool,
    /// Whether this assignment has been reassigned (superseded by a later assignment
    /// on the same branch before being referenced)
    pub reassigned: bool,
    /// Branch info for this assignment
    pub branch: Option<Branch>,
    /// Byte offset of the assignment node (for identity)
    pub node_offset: usize,
    /// Scope start offset (for determining which scope this belongs to)
    pub scope_offset: usize,
    /// Whether this assignment is inside a modifier conditional's condition.
    /// In `puts a if (a = 123)`, `a = 123` is in a modifier conditional.
    /// This affects reference walk: we skip the break check for these.
    pub in_modifier_conditional: bool,
}

impl Assignment {
    pub fn new(
        name: String,
        name_start: usize,
        name_end: usize,
        kind: AssignmentKind,
        op: Option<String>,
        node_offset: usize,
        scope_offset: usize,
    ) -> Self {
        Self {
            name,
            name_start,
            name_end,
            kind,
            op,
            regexp_start: 0,
            regexp_end: 0,
            referenced: false,
            reassigned: false,
            branch: None,
            node_offset,
            scope_offset,
            in_modifier_conditional: false,
        }
    }

    pub fn reference(&mut self) {
        self.referenced = true;
    }

    pub fn reassigned(&mut self) {
        if !self.referenced {
            self.reassigned = true;
        }
    }

    /// An assignment is "used" if it's referenced, or if it hasn't been
    /// reassigned and the variable was captured by a block.
    pub fn used(&self, captured_by_block: bool) -> bool {
        self.referenced || (!self.reassigned && captured_by_block)
    }

    /// Check if this assignment runs exclusively with another branch context.
    pub fn runs_exclusively_with(&self, other_branch: &Option<Branch>) -> bool {
        match (&self.branch, other_branch) {
            (Some(a), Some(b)) => a.exclusive_with(b),
            _ => false,
        }
    }
}
