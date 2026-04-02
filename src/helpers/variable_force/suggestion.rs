//! "Did you mean?" suggestion logic using Levenshtein distance.

use super::types::ScopeInfo;

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

/// Find suggestion only from method calls (used for multi-assign / for-loop).
pub fn find_suggestion_from_methods(name: &str, scope: &ScopeInfo) -> Option<String> {
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

    for other in &scope.method_calls {
        check(other, &mut best);
    }

    best.map(|(s, _)| s)
}

pub fn find_suggestion(name: &str, scope: &ScopeInfo) -> Option<String> {
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

    for other in &scope.all_var_names {
        check(other, &mut best);
    }
    for other in &scope.method_calls {
        check(other, &mut best);
    }
    for other in &scope.all_reads {
        check(other, &mut best);
    }

    best.map(|(s, _)| s)
}
