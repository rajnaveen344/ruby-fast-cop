//! VariableTable: scope stack + variable lookup (mirrors RuboCop's VariableTable).

use super::assignment::{Assignment, AssignmentKind};
use super::branch::Branch;
use super::scope::{Scope, ScopeType};
use super::variable::Variable;

/// Manages the lifetime of all scopes and local variables.
pub struct VariableTable {
    pub scope_stack: Vec<Scope>,
    /// Current branch stack for tracking conditional context.
    /// Each branch has its parent set to the previous top of the stack.
    pub branch_stack: Vec<Branch>,
}

impl VariableTable {
    /// Push a branch onto the stack, automatically setting its parent
    /// to the current top of the stack.
    pub fn push_branch(&mut self, mut branch: Branch) {
        if let Some(current) = self.branch_stack.last() {
            branch.parent = Some(Box::new(current.clone()));
        }
        self.branch_stack.push(branch);
    }

    pub fn pop_branch(&mut self) -> Option<Branch> {
        self.branch_stack.pop()
    }
}

impl VariableTable {
    pub fn new() -> Self {
        Self {
            scope_stack: Vec::new(),
            branch_stack: Vec::new(),
        }
    }

    pub fn push_scope(
        &mut self,
        node_offset: usize,
        node_end_offset: usize,
        scope_type: ScopeType,
    ) -> &Scope {
        let scope = Scope::new(node_offset, node_end_offset, scope_type);
        self.scope_stack.push(scope);
        self.scope_stack.last().unwrap()
    }

    pub fn pop_scope(&mut self) -> Option<Scope> {
        self.scope_stack.pop()
    }

    pub fn current_scope(&self) -> Option<&Scope> {
        self.scope_stack.last()
    }

    pub fn current_scope_mut(&mut self) -> Option<&mut Scope> {
        self.scope_stack.last_mut()
    }

    /// Declare a variable in the current scope.
    pub fn declare_variable(
        &mut self,
        name: &str,
        is_argument: bool,
        is_method_argument: bool,
    ) {
        if let Some(scope) = self.scope_stack.last_mut() {
            let variable = Variable::new(name.to_string(), is_argument, is_method_argument);
            scope.variables.insert(name.to_string(), variable);
        }
    }

    /// Check if a variable exists in accessible scopes.
    pub fn variable_exist(&self, name: &str) -> bool {
        for scope in self.scope_stack.iter().rev() {
            if scope.variables.contains_key(name) {
                return true;
            }
            if !scope.is_block() {
                break;
            }
        }
        false
    }

    /// Assign to a variable. Creates an Assignment and adds it to the variable.
    pub fn assign_to_variable(
        &mut self,
        name: &str,
        name_start: usize,
        name_end: usize,
        kind: AssignmentKind,
        op: Option<String>,
        node_offset: usize,
    ) {
        // Get current branch and scope offset before mutable borrow
        let branch = self.branch_stack.last().cloned();
        let scope_offset = self.scope_stack.last().map(|s| s.node_offset).unwrap_or(0);

        // Find the variable (need to check capture)
        let current_scope_offset = self.scope_stack.last().map(|s| s.node_offset);
        let is_block_scope = self.scope_stack.last().map(|s| s.is_block()).unwrap_or(false);

        // Find which scope has the variable and check if capture is needed
        let mut var_scope_idx = None;
        for (i, scope) in self.scope_stack.iter().enumerate().rev() {
            if scope.variables.contains_key(name) {
                var_scope_idx = Some(i);
                break;
            }
            if !scope.is_block() {
                break;
            }
        }

        if let Some(idx) = var_scope_idx {
            // Check if captured by block
            if is_block_scope && Some(self.scope_stack[idx].node_offset) != current_scope_offset {
                self.scope_stack[idx]
                    .variables
                    .get_mut(name)
                    .unwrap()
                    .capture_with_block();
            }

            let mut assignment = Assignment::new(
                name.to_string(),
                name_start,
                name_end,
                kind,
                op,
                node_offset,
                scope_offset,
            );
            assignment.branch = branch;
            self.scope_stack[idx]
                .variables
                .get_mut(name)
                .unwrap()
                .assign(assignment);
        }
    }

    /// Reference a variable by name.
    pub fn reference_variable(&mut self, name: &str) {
        let branch = self.branch_stack.last().cloned();
        let current_scope_offset = self.scope_stack.last().map(|s| s.node_offset);
        let is_block_scope = self.scope_stack.last().map(|s| s.is_block()).unwrap_or(false);

        // Find which scope has the variable
        let mut var_scope_idx = None;
        for (i, scope) in self.scope_stack.iter().enumerate().rev() {
            if scope.variables.contains_key(name) {
                var_scope_idx = Some(i);
                break;
            }
            if !scope.is_block() {
                break;
            }
        }

        if let Some(idx) = var_scope_idx {
            // Check if captured by block
            if is_block_scope && Some(self.scope_stack[idx].node_offset) != current_scope_offset {
                self.scope_stack[idx]
                    .variables
                    .get_mut(name)
                    .unwrap()
                    .capture_with_block();
            }

            self.scope_stack[idx]
                .variables
                .get_mut(name)
                .unwrap()
                .reference(&branch);
        }
    }

    /// Get all accessible variables (current scope + outer block scopes).
    pub fn accessible_variables(&self) -> Vec<&Variable> {
        let mut vars = Vec::new();
        for scope in self.scope_stack.iter().rev() {
            vars.extend(scope.variables.values());
            if !scope.is_block() {
                break;
            }
        }
        vars
    }

    /// Get all accessible variables mutably.
    pub fn accessible_variables_mut(&mut self) -> Vec<&mut Variable> {
        let mut vars = Vec::new();
        let mut stop_after = false;
        for scope in self.scope_stack.iter_mut().rev() {
            let is_block = scope.is_block();
            vars.extend(scope.variables.values_mut());
            if !is_block || stop_after {
                break;
            }
            if !is_block {
                stop_after = true;
            }
        }
        vars
    }
}
