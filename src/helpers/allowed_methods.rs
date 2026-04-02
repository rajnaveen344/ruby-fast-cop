//! Shared helper for checking allowed methods/patterns.
//!
//! Mirrors RuboCop's `AllowedMethods` + `AllowedPattern` mixins — exact name
//! matching, qualified name matching, and regex pattern matching (both `/pat/`
//! delimited and bare regex).

/// Check if a method is allowed by name or pattern.
///
/// - `allowed_methods`: exact names or `/regex/` patterns
/// - `allowed_patterns`: regex patterns (bare or `/`-delimited)
/// - `method_name`: the simple method name (e.g. `"foo"`)
/// - `qualified_name`: optional qualified name (e.g. `"Foo.bar"`) — checked in addition to `method_name`
pub fn is_method_allowed(
    allowed_methods: &[String],
    allowed_patterns: &[String],
    method_name: &str,
    qualified_name: Option<&str>,
) -> bool {
    allowed_methods.iter().any(|allowed| {
        if is_regex_pattern(allowed) {
            let pat = strip_regex_delimiters(allowed);
            match_regex(pat, method_name, qualified_name)
        } else {
            allowed == method_name || qualified_name.map_or(false, |qn| allowed == qn)
        }
    }) || allowed_patterns.iter().any(|pat| {
        let pat = strip_regex_delimiters(pat);
        match_regex(pat, method_name, qualified_name)
    })
}

fn is_regex_pattern(s: &str) -> bool {
    (s.starts_with('/') && s.ends_with('/') && s.len() > 2) || s.starts_with("(?")
}

fn strip_regex_delimiters(s: &str) -> &str {
    if s.starts_with('/') && s.ends_with('/') && s.len() > 2 {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn match_regex(pattern: &str, method_name: &str, qualified_name: Option<&str>) -> bool {
    regex::Regex::new(pattern).map_or(false, |re| {
        re.is_match(method_name) || qualified_name.map_or(false, |qn| re.is_match(qn))
    })
}
