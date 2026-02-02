//! Data-driven test runner for RuboCop parity tests.
//!
//! This module discovers YAML test fixtures and runs the corresponding cops
//! against the source code, comparing actual offenses with expected offenses.

use glob::glob;
use ruby_fast_cop::{check_source_with_cop_config, Config, Offense};
use serde::Deserialize;
use std::path::PathBuf;

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

/// Run a single test case and return any errors
fn run_test_case(
    test_case: &TestCase,
    cop_name: &str,
) -> Vec<String> {
    let mut errors = Vec::new();

    // Build config from test case's config field
    let config = Config::from_cop_yaml(cop_name, &test_case.config);

    // Run the linter with the test-specific config
    let offenses = check_source_with_cop_config(&test_case.source, "test.rb", cop_name, &config);

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

/// Run all tests from a single test file
fn run_test_file(test_file: &CopTestFile, file_path: &PathBuf) -> Vec<String> {
    let mut errors = Vec::new();

    // Skip unimplemented cops
    if !test_file.implemented {
        println!(
            "  Skipping {} (not implemented)",
            test_file.cop
        );
        return errors;
    }

    println!(
        "  Testing {} ({} test cases)",
        test_file.cop,
        test_file.tests.len()
    );

    for test_case in &test_file.tests {
        let test_errors = run_test_case(test_case, &test_file.cop);
        if !test_errors.is_empty() {
            errors.push(format!(
                "Failures in {}:",
                file_path.display()
            ));
            errors.extend(test_errors);
        }
    }

    errors
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
    let mut skipped_cops = 0;

    for file_path in &test_files {
        match load_test_file(file_path) {
            Ok(test_file) => {
                if !test_file.implemented {
                    skipped_cops += 1;
                }
                total_tests += test_file.tests.len();
                let errors = run_test_file(&test_file, file_path);
                all_errors.extend(errors);
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
    println!("  Skipped (unimplemented): {}", skipped_cops);
    println!("  Total test cases: {}", total_tests);

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
