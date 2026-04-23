//! Data-driven test runner for RuboCop parity tests.
//!
//! This module discovers TOML test fixtures and runs the corresponding cops
//! against the source code, comparing actual offenses with expected offenses.

use glob::glob;
use ruby_fast_cop::{
    Config, Location, Offense, Severity, apply_corrections, check_source_with_cop_config_version_and_path,
    check_source_with_peers,
};
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
    /// Optional Unix file mode (e.g. 0o644, 0o755) — creates a real tempfile at that mode
    /// so cops that call `fs::metadata` can observe it. Used by `Lint/ScriptPermission`.
    /// The tempfile's basename replaces `__FILE__` placeholders in expected messages.
    #[serde(default)]
    file_mode: Option<u32>,
    /// Optional list of synthetic peer-cop offenses to inject into the peer-pass.
    /// Used exclusively by `Lint/RedundantCopDisableDirective` fixtures, which
    /// mirror RuboCop specs that stub `FakeLocation`-backed offenses to represent
    /// peer-cop hits that our real peer cops cannot reproduce.
    #[serde(default)]
    peer_offenses: Vec<InjectedPeerOffense>,
}

/// A synthetic peer offense used by the `Lint/RedundantCopDisableDirective` tests
/// to stand in for offenses that RuboCop's own specs mock via `FakeLocation`.
#[derive(Debug, Deserialize)]
struct InjectedPeerOffense {
    cop_name: String,
    line: u32,
    #[serde(default)]
    column: u32,
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
    file_basename: Option<&str>,
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
    // Substitute __FILE__ placeholder with the tempfile basename when provided.
    let expected_msg_owned;
    let expected_msg: &str = if let Some(b) = file_basename {
        expected_msg_owned = expected_msg.replace("__FILE__", b);
        &expected_msg_owned
    } else {
        expected_msg
    };
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

    // Use test-specified filename or default to "(string)" (matches RuboCop's default
    // when source is provided as a string without a file path)
    let test_filename_owned: String;
    let test_filename: &str = test_case.filename.as_deref().unwrap_or("(string)");

    // If file_mode is set, create a real tempfile at that mode so cops reading
    // filesystem metadata (e.g. Lint/ScriptPermission) work end-to-end.
    // The tempfile is auto-deleted when `_tmp_guard` drops.
    let (tmp_path, tmp_basename, _tmp_guard) = if let Some(mode) = test_case.file_mode {
        let guard = TmpFileGuard::create(&source, mode)
            .expect("failed to create tempfile for file_mode test");
        let basename = guard
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        (Some(guard.path.clone()), Some(basename), Some(guard))
    } else {
        (None, None, None)
    };

    // Override filename to the tempfile path when we made one.
    let test_filename = if let Some(ref p) = tmp_path {
        test_filename_owned = p.to_string_lossy().into_owned();
        test_filename_owned.as_str()
    } else {
        test_filename
    };

    // Run the linter with the test-specific config and Ruby version.
    // `Lint/RedundantCopDisableDirective` needs peer-cop data to judge whether a
    // `# rubocop:disable` directive actually silences a real offense, so take the
    // peer-pass path and merge in any fixture-injected synthetic offenses.
    let offenses = if cop_name == "Lint/RedundantCopDisableDirective" {
        let extras: Vec<Offense> = test_case
            .peer_offenses
            .iter()
            .map(|p| {
                Offense::new(
                    &p.cop_name,
                    "",
                    Severity::Convention,
                    Location::new(p.line, p.column, p.line, p.column + 1),
                    test_filename,
                )
            })
            .collect();
        check_source_with_peers(
            &source,
            test_filename,
            cop_name,
            &config,
            ruby_version,
            tmp_path.as_deref(),
            extras,
        )
    } else {
        check_source_with_cop_config_version_and_path(
            &source,
            test_filename,
            cop_name,
            &config,
            ruby_version,
            tmp_path.as_deref(),
        )
    };

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
            .then(a.location.last_column.cmp(&b.location.last_column))
    });

    let mut sorted_expected: Vec<_> = test_case.offenses.iter().collect();
    sorted_expected.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then(a.column_start.cmp(&b.column_start))
            .then(a.column_end.cmp(&b.column_end))
    });

    for (actual, expected) in sorted_actual.iter().zip(sorted_expected.iter()) {
        errors.extend(compare_offense(
            actual,
            expected,
            &test_case.name,
            cop_name,
            tmp_basename.as_deref(),
        ));
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

/// Small RAII tempfile helper so we avoid pulling in the `tempfile` crate.
/// Writes `contents` to a uniquely-named file in `std::env::temp_dir()` with `mode` bits,
/// removes it on drop.
struct TmpFileGuard {
    path: PathBuf,
}

impl TmpFileGuard {
    fn create(contents: &str, mode: u32) -> std::io::Result<Self> {
        use std::io::Write;
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("rfc-sp-{pid}-{ts}-{n}.rb"));
        let mut f = std::fs::File::create(&path)?;
        f.write_all(contents.as_bytes())?;
        drop(f);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))?;
        }
        #[cfg(not(unix))]
        let _ = mode;
        Ok(Self { path })
    }
}

impl Drop for TmpFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
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
