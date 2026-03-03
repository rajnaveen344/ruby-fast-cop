//! Data-driven test runner for RuboCop parity tests.
//!
//! This module discovers TOML test fixtures and runs the corresponding cops
//! against the source code, comparing actual offenses with expected offenses.

use glob::glob;
use ruby_fast_cop::{Config, Offense, apply_corrections, check_source_with_cop_config_and_version};
use serde::Deserialize;
use std::path::PathBuf;

/// Default Ruby version for tests without a specified version.
/// Matches RuboCop's TargetRuby::DEFAULT_VERSION (2.7).
const DEFAULT_RUBY_VERSION: f64 = 2.7;

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
    #[serde(default = "default_toml_table")]
    config: toml::Value,
    /// Optional base indentation to restore (replaces ‹BASE›N‹/BASE› markers)
    #[serde(default)]
    base_indent: Option<usize>,
    /// Optional Ruby version requirement (e.g., ">= 3.1")
    #[serde(default)]
    ruby_version: Option<String>,
    /// Optional filename override (e.g., "Gemfile", "config.ru")
    #[serde(default)]
    filename: Option<String>,
    /// If true, strip the trailing newline from source (TOML ''' always adds one)
    #[serde(default)]
    strip_trailing_newline: bool,
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

fn default_toml_table() -> toml::Value {
    toml::Value::Table(toml::map::Map::new())
}

/// Find all TOML test fixture files
fn discover_test_files() -> Vec<PathBuf> {
    let pattern = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/**/*.toml");

    let mut files: Vec<PathBuf> = glob(pattern)
        .expect("Failed to read glob pattern")
        .filter_map(|entry| entry.ok())
        .collect();

    files.sort();
    files
}

/// Load and parse a TOML test file
fn load_test_file(path: &PathBuf) -> Result<CopTestFile, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

    toml::from_str(&content).map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

/// Convert a toml::Value to serde_yaml::Value for Config::from_cop_yaml()
fn toml_to_yaml_value(value: &toml::Value) -> serde_yaml::Value {
    match value {
        toml::Value::String(s) => serde_yaml::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_yaml::Value::Number((*i).into()),
        toml::Value::Float(f) => serde_yaml::Value::Number(serde_yaml::Number::from(*f)),
        toml::Value::Boolean(b) => serde_yaml::Value::Bool(*b),
        toml::Value::Datetime(dt) => serde_yaml::Value::String(dt.to_string()),
        toml::Value::Array(arr) => {
            serde_yaml::Value::Sequence(arr.iter().map(toml_to_yaml_value).collect())
        }
        toml::Value::Table(table) => {
            let mut mapping = serde_yaml::Mapping::new();
            for (k, v) in table {
                mapping.insert(serde_yaml::Value::String(k.clone()), toml_to_yaml_value(v));
            }
            serde_yaml::Value::Mapping(mapping)
        }
    }
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

    // Strip RuboCop autocorrect annotation prefix "{} " from expected messages
    let expected_msg = expected
        .message
        .strip_prefix("{} ")
        .unwrap_or(&expected.message);
    if !actual.message.contains(expected_msg) {
        errors.push(format!(
            "[{}] {}: Message mismatch - expected to contain '{}', got '{}'",
            cop_name, test_name, expected_msg, actual.message
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

/// Decode source from TOML format
/// - Converts ‹TAB› back to actual tabs (kept for edge cases)
/// - Restores base indentation from base_indent field
fn decode_source(source: &str, base_indent: Option<usize>) -> String {
    let source = source.replace("‹TAB›", "\t");

    if let Some(indent) = base_indent {
        if indent > 0 {
            let indent_str = " ".repeat(indent);
            return source
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

/// Result of running a single test case
struct TestCaseResult {
    errors: Vec<String>,
    correction_validated: bool,
}

/// Run a single test case and return any errors
fn run_test_case(test_case: &TestCase, cop_name: &str) -> TestCaseResult {
    let mut errors = Vec::new();

    // Build config from test case's config field (convert TOML to YAML for the library API)
    let yaml_config = toml_to_yaml_value(&test_case.config);
    let config = Config::from_cop_toml(cop_name, &yaml_config);

    // Decode source
    let mut source = decode_source(&test_case.source, test_case.base_indent);

    // Strip trailing newline if requested (TOML ''' always adds a trailing newline)
    if test_case.strip_trailing_newline {
        if source.ends_with('\n') {
            source.pop();
        }
    }

    // Get Ruby version from test case, or use default
    let ruby_version = test_case
        .ruby_version
        .as_ref()
        .and_then(|v| parse_ruby_version(v))
        .unwrap_or(DEFAULT_RUBY_VERSION);

    // Use test-specified filename or default to "test.rb"
    let test_filename = test_case.filename.as_deref().unwrap_or("test.rb");

    // Run the linter with the test-specific config and Ruby version
    let offenses = check_source_with_cop_config_and_version(
        &source,
        test_filename,
        cop_name,
        &config,
        ruby_version,
    );

    // Check offense count
    if offenses.len() != test_case.offenses.len() {
        errors.push(format!(
            "[{}] {}: Offense count mismatch - expected {}, got {}",
            cop_name,
            test_case.name,
            test_case.offenses.len(),
            offenses.len()
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
                    expected.line, expected.column_start, expected.column_end, expected.message
                ));
            }
        }

        return TestCaseResult { errors, correction_validated: false };
    }

    // Compare each offense
    // Sort both by line then column for consistent comparison
    let mut sorted_actual: Vec<_> = offenses.iter().collect();
    sorted_actual.sort_by(|a, b| {
        a.location
            .line
            .cmp(&b.location.line)
            .then(a.location.column.cmp(&b.location.column))
    });

    let mut sorted_expected: Vec<_> = test_case.offenses.iter().collect();
    sorted_expected.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then(a.column_start.cmp(&b.column_start))
    });

    for (actual, expected) in sorted_actual.iter().zip(sorted_expected.iter()) {
        errors.extend(compare_offense(actual, expected, &test_case.name, cop_name));
    }

    // Correction validation: if TOML has `corrected` and offenses have corrections, compare
    let mut correction_validated = false;
    if let Some(ref corrected_toml) = test_case.corrected {
        let has_corrections = offenses.iter().any(|o| o.correction.is_some());
        if has_corrections {
            let mut expected_corrected = decode_source(corrected_toml, test_case.base_indent);
            if test_case.strip_trailing_newline && expected_corrected.ends_with('\n') {
                expected_corrected.pop();
            }
            let actual_corrected = apply_corrections(&source, &offenses);
            if actual_corrected != expected_corrected {
                errors.push(format!(
                    "[{}] {}: Correction mismatch",
                    cop_name, test_case.name
                ));
                errors.push(format!("  Expected corrected:\n{}", indent_block(&expected_corrected)));
                errors.push(format!("  Actual corrected:\n{}", indent_block(&actual_corrected)));
            } else {
                correction_validated = true;
            }
        }
        // If TOML has `corrected` but offenses have no corrections, skip silently
        // (cop hasn't implemented corrections yet)
    }

    TestCaseResult { errors, correction_validated }
}

/// Indent a block of text for debug display
fn indent_block(text: &str) -> String {
    text.lines()
        .map(|line| format!("    |{}", line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Result of running tests for a single file
struct TestFileResult {
    errors: Vec<String>,
    ran: usize,
    corrections_validated: usize,
}

/// Run all tests from a single test file
fn run_test_file(test_file: &CopTestFile, file_path: &PathBuf) -> TestFileResult {
    let mut result = TestFileResult {
        errors: Vec::new(),
        ran: 0,
        corrections_validated: 0,
    };

    // Skip unimplemented cops
    if !test_file.implemented {
        println!("  Skipping {} (not implemented)", test_file.cop);
        return result;
    }

    println!(
        "  Testing {} ({} test cases)",
        test_file.cop,
        test_file.tests.len()
    );

    for test_case in &test_file.tests {
        result.ran += 1;
        let tc_result = run_test_case(test_case, &test_file.cop);
        if !tc_result.errors.is_empty() {
            result
                .errors
                .push(format!("Failures in {}:", file_path.display()));
            result.errors.extend(tc_result.errors);
        }
        if tc_result.correction_validated {
            result.corrections_validated += 1;
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
    let mut corrections_validated = 0;

    for file_path in &test_files {
        match load_test_file(file_path) {
            Ok(test_file) => {
                if !test_file.implemented {
                    skipped_cops += 1;
                }
                total_tests += test_file.tests.len();
                let result = run_test_file(&test_file, file_path);
                all_errors.extend(result.errors);
                tests_ran += result.ran;
                corrections_validated += result.corrections_validated;
            }
            Err(e) => {
                // Check if this file is likely unimplemented by looking for the marker
                let content = std::fs::read_to_string(file_path).unwrap_or_default();
                if content.contains("implemented = false") {
                    skipped_cops += 1;
                    println!(
                        "  Skipping {} (unimplemented, TOML has parse issues)",
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
    println!("  Corrections validated: {}", corrections_validated);

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
