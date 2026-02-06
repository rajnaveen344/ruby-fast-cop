//! Data-driven test runner for RuboCop parity tests.
//!
//! This module discovers YAML test fixtures and runs the corresponding cops
//! against the source code, comparing actual offenses with expected offenses.

use glob::glob;
use ruby_fast_cop::{check_source_with_cop_config_and_version, Config, Offense};
use serde::Deserialize;
use std::path::PathBuf;

/// Default Ruby version for tests without a specified version
const DEFAULT_RUBY_VERSION: f64 = 2.5;

/// Represents a complete test file for a cop
#[derive(Debug, Deserialize)]
struct CopTestFile {
    /// The full cop name (e.g., "Lint/Debugger")
    cop: String,
    /// The department (e.g., "lint", "style")
    department: String,
    /// The severity level (e.g., "warning", "convention")
    severity: String,
    /// Whether this cop is implemented
    implemented: bool,
    /// List of test cases
    tests: Vec<TestCase>,
}

/// A single test case within a cop test file
#[derive(Debug, Deserialize)]
struct TestCase {
    /// Name of the test case
    name: String,
    /// Ruby source code to check
    source: String,
    /// Expected offenses
    offenses: Vec<ExpectedOffense>,
    /// Optional corrected source (for autocorrect tests)
    #[serde(default)]
    corrected: Option<String>,
    /// Optional cop-specific configuration
    #[serde(default)]
    config: serde_yaml::Value,
    /// Optional Ruby version requirement (e.g., ">= 3.1")
    #[serde(default)]
    ruby_version: Option<String>,
    /// Whether this test contains unresolved Ruby string interpolation
    #[serde(default)]
    interpolated: bool,
    /// Whether an interpolated test has been manually verified/fixed
    #[serde(default)]
    verified: bool,
}

/// An expected offense in a test case
#[derive(Debug, Deserialize)]
struct ExpectedOffense {
    /// Line number (1-indexed)
    line: u32,
    /// Starting column (1-indexed)
    column_start: u32,
    /// Ending column (1-indexed)
    column_end: u32,
    /// Expected message (can be a substring)
    message: String,
}

/// Find all YAML test fixture files
fn discover_test_files() -> Vec<PathBuf> {
    let pattern = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/**/*.yaml"
    );

    let mut files: Vec<PathBuf> = glob(pattern)
        .expect("Failed to read glob pattern")
        .filter_map(|entry| entry.ok())
        .collect();

    // Also check for .yml extension
    let pattern_yml = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/**/*.yml"
    );

    let yml_files: Vec<PathBuf> = glob(pattern_yml)
        .expect("Failed to read glob pattern")
        .filter_map(|entry| entry.ok())
        .collect();

    files.extend(yml_files);
    files.sort();
    files
}

/// Load and parse a YAML test file
fn load_test_file(path: &PathBuf) -> Result<CopTestFile, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

    serde_yaml::from_str(&content)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

/// Compare an actual offense with an expected offense
fn compare_offense(
    actual: &Offense,
    expected: &ExpectedOffense,
    test_name: &str,
    cop_name: &str,
) -> Vec<String> {
    let mut errors = Vec::new();

    if actual.location.line != expected.line {
        errors.push(format!(
            "[{}] {}: Line mismatch - expected {}, got {}",
            cop_name, test_name, expected.line, actual.location.line
        ));
    }

    if actual.location.column != expected.column_start {
        errors.push(format!(
            "[{}] {}: Column start mismatch - expected {}, got {}",
            cop_name, test_name, expected.column_start, actual.location.column
        ));
    }

    if actual.location.last_column != expected.column_end {
        errors.push(format!(
            "[{}] {}: Column end mismatch - expected {}, got {}",
            cop_name, test_name, expected.column_end, actual.location.last_column
        ));
    }

    if !actual.message.contains(&expected.message) {
        errors.push(format!(
            "[{}] {}: Message mismatch - expected to contain '{}', got '{}'",
            cop_name, test_name, expected.message, actual.message
        ));
    }

    errors
}

/// Parse a Ruby version requirement string like ">= 3.0" into a version number
/// Returns the version number if parseable, or None
fn parse_ruby_version(version_str: &str) -> Option<f64> {
    // Handle formats like ">= 3.0", ">= 2.7", "3.1", etc.
    let version_str = version_str.trim();

    // Strip comparison operators
    let version_num = version_str
        .trim_start_matches(">=")
        .trim_start_matches(">")
        .trim_start_matches("<=")
        .trim_start_matches("<")
        .trim_start_matches("==")
        .trim_start_matches("~>")
        .trim();

    // Parse as f64
    version_num.parse::<f64>().ok()
}

/// Decode source from YAML format
/// - Converts ‹TAB› back to actual tabs
/// - Restores base indentation from ‹BASE›N‹/BASE› markers
fn decode_source(source: &str) -> String {
    let source = source.replace("‹TAB›", "\t");

    // Check for base indentation marker
    // Note: ‹ and › are multi-byte UTF-8 characters (3 bytes each)
    const BASE_PREFIX: &str = "‹BASE›";
    const BASE_SUFFIX: &str = "‹/BASE›";

    if source.starts_with(BASE_PREFIX) {
        if let Some(end_marker) = source.find(BASE_SUFFIX) {
            // Extract the indent number between ‹BASE› and ‹/BASE›
            let prefix_len = BASE_PREFIX.len();
            let base_indent: usize = source[prefix_len..end_marker].parse().unwrap_or(0);

            // Find the start of content after ‹/BASE› and newline
            let suffix_end = end_marker + BASE_SUFFIX.len();
            let rest = if source[suffix_end..].starts_with('\n') {
                &source[suffix_end + 1..]
            } else {
                &source[suffix_end..]
            };

            let indent_str = " ".repeat(base_indent);

            return rest
                .lines()
                .map(|line| {
                    if line.trim().is_empty() {
                        String::new()
                    } else {
                        format!("{}{}", indent_str, line)
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
    }

    source
}

/// Run a single test case and return any errors
fn run_test_case(
    test_case: &TestCase,
    cop_name: &str,
) -> Vec<String> {
    let mut errors = Vec::new();

    // Build config from test case's config field
    let config = Config::from_cop_yaml(cop_name, &test_case.config);

    // Decode source (converts ‹TAB› markers back to actual tabs)
    let source = decode_source(&test_case.source);

    // Get Ruby version from test case, or use default
    let ruby_version = test_case
        .ruby_version
        .as_ref()
        .and_then(|v| parse_ruby_version(v))
        .unwrap_or(DEFAULT_RUBY_VERSION);

    // Run the linter with the test-specific config and Ruby version
    let offenses = check_source_with_cop_config_and_version(
        &source,
        "test.rb",
        cop_name,
        &config,
        ruby_version,
    );

    // Check offense count
    if offenses.len() != test_case.offenses.len() {
        errors.push(format!(
            "[{}] {}: Offense count mismatch - expected {}, got {}",
            cop_name, test_case.name, test_case.offenses.len(), offenses.len()
        ));

        // Print actual offenses for debugging
        if !offenses.is_empty() {
            errors.push(format!(
                "[{}] {}: Actual offenses:",
                cop_name, test_case.name
            ));
            for offense in &offenses {
                errors.push(format!(
                    "  - line {}, col {}-{}: {}",
                    offense.location.line,
                    offense.location.column,
                    offense.location.last_column,
                    offense.message
                ));
            }
        }

        // Print expected offenses for debugging
        if !test_case.offenses.is_empty() {
            errors.push(format!(
                "[{}] {}: Expected offenses:",
                cop_name, test_case.name
            ));
            for expected in &test_case.offenses {
                errors.push(format!(
                    "  - line {}, col {}-{}: {}",
                    expected.line,
                    expected.column_start,
                    expected.column_end,
                    expected.message
                ));
            }
        }

        return errors;
    }

    // Compare each offense
    // Sort both by line then column for consistent comparison
    let mut sorted_actual: Vec<_> = offenses.iter().collect();
    sorted_actual.sort_by(|a, b| {
        a.location.line.cmp(&b.location.line)
            .then(a.location.column.cmp(&b.location.column))
    });

    let mut sorted_expected: Vec<_> = test_case.offenses.iter().collect();
    sorted_expected.sort_by(|a, b| {
        a.line.cmp(&b.line)
            .then(a.column_start.cmp(&b.column_start))
    });

    for (actual, expected) in sorted_actual.iter().zip(sorted_expected.iter()) {
        errors.extend(compare_offense(actual, expected, &test_case.name, cop_name));
    }

    errors
}

/// Check if a test case has $UNRESOLVED config values
fn has_unresolved_config(test_case: &TestCase) -> bool {
    // Convert config to string and check for $UNRESOLVED
    let config_str = serde_yaml::to_string(&test_case.config).unwrap_or_default();
    config_str.contains("$UNRESOLVED")
}

/// Result of running tests for a single file
struct TestFileResult {
    errors: Vec<String>,
    skipped_interpolated: usize,
    skipped_unresolved: usize,
    ran: usize,
}

/// Run all tests from a single test file
fn run_test_file(test_file: &CopTestFile, file_path: &PathBuf) -> TestFileResult {
    let mut result = TestFileResult {
        errors: Vec::new(),
        skipped_interpolated: 0,
        skipped_unresolved: 0,
        ran: 0,
    };

    // Skip unimplemented cops
    if !test_file.implemented {
        println!(
            "  Skipping {} (not implemented)",
            test_file.cop
        );
        return result;
    }

    // Filter out tests that are:
    // 1. Interpolated but not verified
    // 2. Have $UNRESOLVED config values
    let runnable_tests: Vec<_> = test_file.tests.iter()
        .filter(|t| {
            let is_interpolated_unverified = t.interpolated && !t.verified;
            let has_unresolved = has_unresolved_config(t);
            !is_interpolated_unverified && !has_unresolved
        })
        .collect();

    let skipped_interp = test_file.tests.iter()
        .filter(|t| t.interpolated && !t.verified)
        .count();
    let skipped_unres = test_file.tests.iter()
        .filter(|t| has_unresolved_config(t))
        .count();

    result.skipped_interpolated = skipped_interp;
    result.skipped_unresolved = skipped_unres;

    println!(
        "  Testing {} ({} test cases, {} skipped interpolated, {} skipped unresolved config)",
        test_file.cop,
        runnable_tests.len(),
        skipped_interp,
        skipped_unres
    );

    for test_case in runnable_tests {
        result.ran += 1;
        let test_errors = run_test_case(test_case, &test_file.cop);
        if !test_errors.is_empty() {
            result.errors.push(format!(
                "Failures in {}:",
                file_path.display()
            ));
            result.errors.extend(test_errors);
        }
    }

    result
}

#[test]
fn rubocop_parity_tests() {
    let test_files = discover_test_files();

    if test_files.is_empty() {
        println!("No test fixtures found in tests/fixtures/");
        return;
    }

    println!("Discovered {} test fixture(s)", test_files.len());

    let mut all_errors = Vec::new();
    let mut total_tests = 0;
    let mut tests_ran = 0;
    let mut skipped_cops = 0;
    let mut skipped_interpolated = 0;
    let mut skipped_unresolved = 0;

    for file_path in &test_files {
        match load_test_file(file_path) {
            Ok(test_file) => {
                if !test_file.implemented {
                    skipped_cops += 1;
                }
                total_tests += test_file.tests.len();
                let result = run_test_file(&test_file, file_path);
                all_errors.extend(result.errors);
                skipped_interpolated += result.skipped_interpolated;
                skipped_unresolved += result.skipped_unresolved;
                tests_ran += result.ran;
            }
            Err(e) => {
                // Check if this file is likely unimplemented by looking for the marker
                // This allows us to skip YAML parse errors for unimplemented cops
                let content = std::fs::read_to_string(file_path).unwrap_or_default();
                if content.contains("implemented: false") {
                    skipped_cops += 1;
                    println!(
                        "  Skipping {} (unimplemented, YAML has parse issues)",
                        file_path.display()
                    );
                } else {
                    // Only count as error if file appears to be implemented
                    all_errors.push(e);
                }
            }
        }
    }

    println!();
    println!("Summary:");
    println!("  Total test files: {}", test_files.len());
    println!("  Skipped cops (unimplemented): {}", skipped_cops);
    println!("  Total test cases: {}", total_tests);
    println!("  Tests ran: {}", tests_ran);
    println!("  Skipped (unverified interpolated): {}", skipped_interpolated);
    println!("  Skipped (unresolved config): {}", skipped_unresolved);

    if !all_errors.is_empty() {
        println!();
        println!("Errors:");
        for error in &all_errors {
            println!("  {}", error);
        }
        panic!(
            "RuboCop parity tests failed with {} error(s)",
            all_errors.len()
        );
    }

    println!("  All tests passed!");
}
