//! Variable: represents a local variable's lifetime (mirrors RuboCop's Variable).

use super::assignment::Assignment;
use super::branch::Branch;

/// Represents a local variable within a scope.
pub struct Variable {
    pub name: String,
    /// Whether this variable was declared as a method/block argument
    pub is_argument: bool,
    /// Whether this variable is a method argument (as opposed to block argument)
    pub is_method_argument: bool,
    /// Whether this is a keyword argument (kwarg, kwoptarg)
    pub is_keyword_argument: bool,
    /// Whether this is a block parameter (&block)
    pub is_block_arg_type: bool,
    /// Whether this is a block-local variable (declared after ; in |a; b|)
    pub is_block_local_variable: bool,
    /// Byte offset of the declaration name (for offense reporting)
    pub declaration_start: usize,
    pub declaration_end: usize,
    /// All assignments to this variable
    pub assignments: Vec<Assignment>,
    /// Whether any explicit reference exists
    pub reference_count: usize,
    /// Whether the variable has been captured by a block/lambda
    pub captured_by_block: bool,
}

impl Variable {
    pub fn new(name: String, is_argument: bool, is_method_argument: bool) -> Self {
        Self {
            name,
            is_argument,
            is_method_argument,
            is_keyword_argument: false,
            is_block_arg_type: false,
            is_block_local_variable: false,
            declaration_start: 0,
            declaration_end: 0,
            assignments: Vec::new(),
            reference_count: 0,
            captured_by_block: false,
        }
    }

    /// Whether this is a block argument (argument in a block scope, not a method scope)
    pub fn is_block_argument(&self) -> bool {
        self.is_argument && !self.is_method_argument
    }

    /// Add an assignment to this variable.
    /// The assignment's branch must already be set by the caller.
    pub fn assign(&mut self, assignment: Assignment) {
        // Mark last assignment as reassigned if on same branch and not captured
        if !self.captured_by_block {
            if let Some(last) = self.assignments.last() {
                if last.branch == assignment.branch {
                    let idx = self.assignments.len() - 1;
                    self.assignments[idx].reassigned();
                }
            }
        }
        self.assignments.push(assignment);
    }

    /// Reference this variable: mark the current assignment(s) as used.
    pub fn reference(&mut self, ref_branch: &Option<Branch>) {
        self.reference_count += 1;

        let mut consumed_branches: Vec<Branch> = Vec::new();
        // First pass: determine which assignments to reference and when to stop
        let mut to_reference: Vec<usize> = Vec::new();
        let mut stop_at: Option<usize> = None;

        for i in (0..self.assignments.len()).rev() {
            let assignment = &self.assignments[i];

            // Skip if we've already consumed this branch
            if let Some(ref ab) = assignment.branch {
                if consumed_branches.iter().any(|cb| cb == ab) {
                    continue;
                }
            }

            // Don't reference if assignment runs exclusively with this reference
            if !assignment.runs_exclusively_with(ref_branch) {
                to_reference.push(i);
            }

            // Modifier conditional assignments: skip the break check.
            // In `puts a if (a = 123)`, the assignment in the condition
            // should not prevent earlier assignments from being referenced.
            if assignment.in_modifier_conditional {
                continue;
            }

            // If no branch or same branch as reference, we're done
            match (&assignment.branch, ref_branch) {
                (None, _) => { stop_at = Some(i); break; }
                (Some(ab), Some(rb)) if ab == rb => { stop_at = Some(i); break; }
                (Some(ab), _) => {
                    if !ab.may_run_incompletely() {
                        consumed_branches.push(ab.clone());
                    }
                }
            }
        }

        // Second pass: apply references
        for i in to_reference {
            self.assignments[i].reference();
        }

        let _ = stop_at;
    }

    pub fn should_be_unused(&self) -> bool {
        self.name.starts_with('_')
    }

    pub fn referenced(&self) -> bool {
        self.reference_count > 0
    }

    pub fn used(&self) -> bool {
        self.captured_by_block || self.referenced()
    }

    pub fn capture_with_block(&mut self) {
        self.captured_by_block = true;
    }
}
