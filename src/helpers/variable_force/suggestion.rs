//! "Did you mean?" suggestion logic using Levenshtein distance.

use super::scope::Scope;
use std::collections::HashSet;

pub fn levenshtein(a: &str, b: &str) -> usize {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let a_len = a_bytes.len();
    let b_len = b_bytes.len();

    if a_len == 0 { return b_len; }
    if b_len == 0 { return a_len; }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        for j in 1..=b_len {
            let cost = if a_bytes[i - 1] == b_bytes[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

/// Find a suggestion for a variable name from variable names and method calls
/// found in the source around the scope.
pub fn find_suggestion(name: &str, scope: &Scope, source: &str) -> Option<String> {
    let threshold = (name.len() + 2) / 3;
    let mut best: Option<(String, usize)> = None;

    let check = |other: &str, best: &mut Option<(String, usize)>| {
        if other == name || other.starts_with('_') { return; }
        let dist = levenshtein(name, other);
        if dist > 0 && dist <= threshold {
            if best.is_none() || dist < best.as_ref().unwrap().1 {
                *best = Some((other.to_string(), dist));
            }
        }
    };

    // Check variable names in scope
    for var_name in scope.variables.keys() {
        check(var_name, &mut best);
    }

    // Check method calls in source (scope region)
    let scope_source = if scope.node_end_offset <= source.len() {
        &source[scope.node_offset..scope.node_end_offset]
    } else {
        source
    };

    // Simple heuristic: find bare word method calls in the scope
    let method_calls = extract_method_calls(scope_source);
    for method_name in &method_calls {
        check(method_name, &mut best);
    }

    // Also check variable reads in the source
    let var_reads = extract_local_var_reads(scope_source);
    for var_name in &var_reads {
        check(var_name, &mut best);
    }

    best.map(|(s, _)| s)
}

/// Find suggestion only from method calls (used for multi-assign / for-loop).
/// Excludes variable names from the scope to only suggest method-like names.
pub fn find_suggestion_from_methods(name: &str, scope: &Scope, source: &str) -> Option<String> {
    let threshold = (name.len() + 2) / 3;
    let mut best: Option<(String, usize)> = None;

    let check = |other: &str, best: &mut Option<(String, usize)>| {
        if other == name || other.starts_with('_') { return; }
        let dist = levenshtein(name, other);
        if dist > 0 && dist <= threshold {
            if best.is_none() || dist < best.as_ref().unwrap().1 {
                *best = Some((other.to_string(), dist));
            }
        }
    };

    let scope_source = if scope.node_end_offset <= source.len() {
        &source[scope.node_offset..scope.node_end_offset]
    } else {
        source
    };

    let method_calls = extract_method_calls(scope_source);
    // Exclude names that are actually variables in this scope
    for method_name in &method_calls {
        if scope.variables.contains_key(method_name.as_str()) {
            continue;
        }
        check(method_name, &mut best);
    }

    best.map(|(s, _)| s)
}

/// Extract method call names from source text (simple heuristic).
fn extract_method_calls(source: &str) -> HashSet<String> {
    let mut calls = HashSet::new();
    // Look for bare identifiers that could be method calls
    // This is a rough heuristic - looks for identifier-like tokens
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if is_ident_start(bytes[i]) {
            let start = i;
            while i < len && is_ident_char(bytes[i]) {
                i += 1;
            }
            let word = &source[start..i];
            // Skip Ruby keywords
            if !is_ruby_keyword(word) && !word.starts_with(|c: char| c.is_uppercase()) {
                calls.insert(word.to_string());
            }
        } else {
            i += 1;
        }
    }
    calls
}

/// Extract local variable read names from source text (identifiers not followed by =).
fn extract_local_var_reads(source: &str) -> HashSet<String> {
    let mut reads = HashSet::new();
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if is_ident_start(bytes[i]) && !bytes[i].is_ascii_uppercase() {
            let start = i;
            while i < len && is_ident_char(bytes[i]) {
                i += 1;
            }
            let word = &source[start..i];
            if !is_ruby_keyword(word) {
                reads.insert(word.to_string());
            }
        } else {
            i += 1;
        }
    }
    reads
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_lowercase() || b == b'_'
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_ruby_keyword(word: &str) -> bool {
    matches!(
        word,
        "def" | "end" | "if" | "else" | "elsif" | "unless" | "while" | "until"
        | "for" | "do" | "begin" | "rescue" | "ensure" | "raise" | "return"
        | "yield" | "class" | "module" | "self" | "super" | "true" | "false"
        | "nil" | "and" | "or" | "not" | "in" | "then" | "case" | "when"
        | "break" | "next" | "redo" | "retry" | "defined" | "puts" | "print"
        | "require" | "require_relative" | "include" | "extend" | "prepend"
        | "attr_reader" | "attr_writer" | "attr_accessor" | "private" | "public"
        | "protected" | "alias" | "alias_method"
    )
}
