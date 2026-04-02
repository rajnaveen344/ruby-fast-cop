//! Branch tracking for conditional control flow (mirrors RuboCop's Branch + Branchable).
//!
//! A Branch identifies which arm of a conditional a node lives in.
//! Two branches are "exclusive" if they are different arms of the same conditional.

/// Identifies a branch: the control node's offset + which child arm.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Branch {
    /// Offset of the control node (if/case/rescue/etc.)
    pub control_offset: usize,
    /// Which child index this branch corresponds to
    pub child_index: usize,
    /// The type of branching construct
    pub kind: BranchKind,
    /// Parent branch (for nested conditionals)
    pub parent: Option<Box<Branch>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BranchKind {
    If,
    Case,
    CaseMatch,
    Rescue,
    Ensure,
    And,
    Or,
    AndAssign,
    OrAssign,
    OpAssign,
    While,
    Until,
    WhilePost,
    UntilPost,
    For,
}

impl Branch {
    pub fn new(control_offset: usize, child_index: usize, kind: BranchKind) -> Self {
        Self {
            control_offset,
            child_index,
            kind,
            parent: None,
        }
    }

    pub fn with_parent(mut self, parent: Branch) -> Self {
        self.parent = Some(Box::new(parent));
        self
    }

    /// Two branches are exclusive if they share a control node but have
    /// different child indices. Also checks ancestor branches.
    pub fn exclusive_with(&self, other: &Branch) -> bool {
        if self.may_jump_to_other_branch() {
            return false;
        }

        // Check if other or any of its ancestors share our control node
        let mut other_cursor = Some(other);
        while let Some(ob) = other_cursor {
            if self.control_offset == ob.control_offset {
                return self.child_index != ob.child_index;
            }
            other_cursor = ob.parent.as_deref();
        }

        // Check our parent against the other branch
        if let Some(ref parent) = self.parent {
            return parent.exclusive_with(other);
        }

        false
    }

    /// Rescue main body may jump to rescue clause on exception.
    fn may_jump_to_other_branch(&self) -> bool {
        matches!(self.kind, BranchKind::Rescue | BranchKind::Ensure)
            && self.child_index == 0
    }

    /// Rescue main body may run incompletely (exception at any point).
    pub fn may_run_incompletely(&self) -> bool {
        matches!(self.kind, BranchKind::Rescue | BranchKind::Ensure)
            && self.child_index == 0
    }
}

/// Determine the branch for a node at the given offset within a scope.
/// This walks up the "virtual parent chain" encoded in the branch stack.
///
/// The branch_stack is maintained by the dispatcher as it enters/exits
/// branching constructs.
pub fn current_branch(branch_stack: &[Branch]) -> Option<Branch> {
    branch_stack.last().cloned()
}
