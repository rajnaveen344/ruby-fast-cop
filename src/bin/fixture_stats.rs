//! Statistics tool for TOML test fixtures.
//!
//! Reads all TOML fixtures and provides deterministic statistics about test coverage,
//! interpolation status, and verification progress.
//!
//! Usage:
//!   cargo run --bin fixture-stats
//!   cargo run --bin fixture-stats -- --verbose
//!   cargo run --bin fixture-stats -- --department style
//!   cargo run --bin fixture-stats -- --unresolved

use glob::glob;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Represents a complete test file for a cop
#[derive(Debug, Deserialize)]
struct CopTestFile {
    cop: String,
    department: String,
    #[serde(default)]
    implemented: bool,
    #[serde(default)]
    tests: Vec<TestCase>,
}

/// A single test case
#[derive(Debug, Deserialize)]
struct TestCase {
    #[allow(dead_code)]
    name: String,
    #[serde(default = "default_toml_table")]
    config: toml::Value,
    #[serde(default)]
    interpolated: bool,
    #[serde(default)]
    verified: bool,
}

fn default_toml_table() -> toml::Value {
    toml::Value::Table(toml::map::Map::new())
}

/// Statistics for a single cop
#[derive(Debug, Default)]
struct CopStats {
    total_tests: usize,
    interpolated_tests: usize,
    verified_tests: usize,
    unresolved_config_count: usize,
    implemented: bool,
}

/// Overall statistics
#[derive(Debug, Default)]
struct OverallStats {
    total_files: usize,
    total_tests: usize,
    implemented_cops: usize,
    unimplemented_cops: usize,
    interpolated_tests: usize,
    verified_tests: usize,
    runnable_tests: usize,
    skipped_tests: usize,
    unresolved_config_values: usize,
    tests_with_unresolved_config: usize,
}

/// Find all TOML test fixture files
fn discover_test_files() -> Vec<PathBuf> {
    let pattern = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/**/*.toml"
    );

    let mut files: Vec<PathBuf> = glob(pattern)
        .expect("Failed to read glob pattern")
        .filter_map(|entry| entry.ok())
        .collect();

    files.sort();
    files
}

/// Count $UNRESOLVED: occurrences in a TOML value
fn count_unresolved(value: &toml::Value) -> usize {
    match value {
        toml::Value::String(s) => {
            if s.starts_with("$UNRESOLVED:") { 1 } else { 0 }
        }
        toml::Value::Array(arr) => {
            arr.iter().map(count_unresolved).sum()
        }
        toml::Value::Table(map) => {
            map.values().map(count_unresolved).sum()
        }
        _ => 0,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let verbose = args.contains(&"--verbose".to_string()) || args.contains(&"-v".to_string());
    let show_unresolved = args.contains(&"--unresolved".to_string());
    let department_filter = args.iter()
        .position(|a| a == "--department" || a == "-d")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.to_lowercase());

    let test_files = discover_test_files();

    let mut overall = OverallStats::default();
    let mut by_department: BTreeMap<String, OverallStats> = BTreeMap::new();
    let mut by_cop: BTreeMap<String, CopStats> = BTreeMap::new();
    let mut unresolved_examples: Vec<(String, String, String)> = Vec::new(); // (cop, test, config_key)

    for file_path in &test_files {
        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Warning: Failed to read {}: {}", file_path.display(), e);
                continue;
            }
        };

        let test_file: CopTestFile = match toml::from_str(&content) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Warning: Failed to parse {}: {}", file_path.display(), e);
                continue;
            }
        };

        // Apply department filter
        if let Some(ref filter) = department_filter {
            if test_file.department.to_lowercase() != *filter {
                continue;
            }
        }

        overall.total_files += 1;

        if test_file.implemented {
            overall.implemented_cops += 1;
        } else {
            overall.unimplemented_cops += 1;
        }

        let dept_stats = by_department.entry(test_file.department.clone()).or_default();
        dept_stats.total_files += 1;
        if test_file.implemented {
            dept_stats.implemented_cops += 1;
        } else {
            dept_stats.unimplemented_cops += 1;
        }

        let cop_stats = by_cop.entry(test_file.cop.clone()).or_default();
        cop_stats.implemented = test_file.implemented;

        for test in &test_file.tests {
            overall.total_tests += 1;
            dept_stats.total_tests += 1;
            cop_stats.total_tests += 1;

            let unresolved_count = count_unresolved(&test.config);
            if unresolved_count > 0 {
                overall.unresolved_config_values += unresolved_count;
                overall.tests_with_unresolved_config += 1;
                dept_stats.unresolved_config_values += unresolved_count;
                dept_stats.tests_with_unresolved_config += 1;
                cop_stats.unresolved_config_count += unresolved_count;

                // Collect examples for --unresolved flag
                if show_unresolved && unresolved_examples.len() < 20 {
                    if let toml::Value::Table(map) = &test.config {
                        for (k, v) in map {
                            if let toml::Value::String(s) = v {
                                if s.starts_with("$UNRESOLVED:") {
                                    unresolved_examples.push((
                                        test_file.cop.clone(),
                                        test.name.clone(),
                                        format!("{}: {}", k, s),
                                    ));
                                }
                            }
                        }
                    }
                }
            }

            if test.interpolated {
                overall.interpolated_tests += 1;
                dept_stats.interpolated_tests += 1;
                cop_stats.interpolated_tests += 1;
            }

            if test.verified {
                overall.verified_tests += 1;
                dept_stats.verified_tests += 1;
                cop_stats.verified_tests += 1;
            }

            // A test is runnable if: cop is implemented AND (not interpolated OR verified)
            let is_runnable = test_file.implemented && (!test.interpolated || test.verified);
            if is_runnable {
                overall.runnable_tests += 1;
                dept_stats.runnable_tests += 1;
            } else if test_file.implemented {
                overall.skipped_tests += 1;
                dept_stats.skipped_tests += 1;
            }
        }
    }

    // Print overall statistics
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║              TOML Test Fixture Statistics                    ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    println!("Overall Summary");
    println!("───────────────────────────────────────────────────────────────");
    println!("  Total fixture files:        {:>6}", overall.total_files);
    println!("  Implemented cops:           {:>6}", overall.implemented_cops);
    println!("  Unimplemented cops:         {:>6}", overall.unimplemented_cops);
    println!();
    println!("  Total test cases:           {:>6}", overall.total_tests);
    println!("  Interpolated (need verify): {:>6} ({:.1}%)",
        overall.interpolated_tests,
        100.0 * overall.interpolated_tests as f64 / overall.total_tests as f64);
    println!("  Verified:                   {:>6} ({:.1}%)",
        overall.verified_tests,
        100.0 * overall.verified_tests as f64 / overall.total_tests.max(1) as f64);
    println!();
    println!("  Tests with $UNRESOLVED:     {:>6}", overall.tests_with_unresolved_config);
    println!("  Total $UNRESOLVED values:   {:>6}", overall.unresolved_config_values);
    println!();
    println!("  Runnable tests:             {:>6}", overall.runnable_tests);
    println!("  Skipped tests:              {:>6}", overall.skipped_tests);
    println!();

    // Print by department
    println!("By Department");
    println!("───────────────────────────────────────────────────────────────");
    println!("{:<20} {:>6} {:>6} {:>6} {:>8} {:>8}",
        "Department", "Cops", "Tests", "Interp", "Verified", "Unresol");
    println!("{}", "─".repeat(63));

    for (dept, stats) in &by_department {
        println!("{:<20} {:>6} {:>6} {:>6} {:>8} {:>8}",
            dept,
            stats.total_files,
            stats.total_tests,
            stats.interpolated_tests,
            stats.verified_tests,
            stats.unresolved_config_values);
    }
    println!();

    // Print implemented cops detail
    println!("Implemented Cops");
    println!("───────────────────────────────────────────────────────────────");
    let implemented: Vec<_> = by_cop.iter()
        .filter(|(_, s)| s.implemented)
        .collect();

    if implemented.is_empty() {
        println!("  (none)");
    } else {
        println!("{:<35} {:>6} {:>8} {:>8}",
            "Cop", "Tests", "Interp", "Verified");
        println!("{}", "─".repeat(63));
        for (cop, stats) in implemented {
            println!("{:<35} {:>6} {:>8} {:>8}",
                cop, stats.total_tests, stats.interpolated_tests, stats.verified_tests);
        }
    }
    println!();

    // Verbose: show all cops with interpolation
    if verbose {
        println!("Cops with Interpolated Tests (Top 20)");
        println!("───────────────────────────────────────────────────────────────");

        let mut cops_with_interp: Vec<_> = by_cop.iter()
            .filter(|(_, s)| s.interpolated_tests > 0)
            .collect();
        cops_with_interp.sort_by(|a, b| b.1.interpolated_tests.cmp(&a.1.interpolated_tests));

        println!("{:<40} {:>6} {:>8}",
            "Cop", "Interp", "Unresol");
        println!("{}", "─".repeat(63));
        for (cop, stats) in cops_with_interp.iter().take(20) {
            println!("{:<40} {:>6} {:>8}",
                cop, stats.interpolated_tests, stats.unresolved_config_count);
        }
        println!();
    }

    // Show unresolved examples
    if show_unresolved && !unresolved_examples.is_empty() {
        println!("Sample $UNRESOLVED Config Values");
        println!("───────────────────────────────────────────────────────────────");
        for (cop, _test, config) in unresolved_examples.iter().take(15) {
            println!("  [{}] {}", cop, config);
        }
        println!();
    }

    // Print legend
    println!("Legend");
    println!("───────────────────────────────────────────────────────────────");
    println!("  Interp    = Tests with interpolated = true (need manual verification)");
    println!("  Verified  = Interpolated tests that have been manually verified");
    println!("  Unresol   = Config values marked $UNRESOLVED:xxx");
    println!("  Runnable  = Tests that will execute (implemented + not interpolated OR verified)");
}
