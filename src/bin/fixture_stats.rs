//! Statistics tool for TOML test fixtures.
//!
//! Reads all TOML fixtures and provides deterministic statistics about test coverage.
//!
//! Usage:
//!   cargo run --bin fixture-stats
//!   cargo run --bin fixture-stats -- --verbose
//!   cargo run --bin fixture-stats -- --department style

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
}

/// Statistics for a single cop
#[derive(Debug, Default)]
struct CopStats {
    total_tests: usize,
    implemented: bool,
}

/// Overall statistics
#[derive(Debug, Default)]
struct OverallStats {
    total_files: usize,
    total_tests: usize,
    implemented_cops: usize,
    unimplemented_cops: usize,
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

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let verbose = args.contains(&"--verbose".to_string()) || args.contains(&"-v".to_string());
    let department_filter = args.iter()
        .position(|a| a == "--department" || a == "-d")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.to_lowercase());

    let test_files = discover_test_files();

    let mut overall = OverallStats::default();
    let mut by_department: BTreeMap<String, OverallStats> = BTreeMap::new();
    let mut by_cop: BTreeMap<String, CopStats> = BTreeMap::new();

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

        for _test in &test_file.tests {
            overall.total_tests += 1;
            dept_stats.total_tests += 1;
            cop_stats.total_tests += 1;
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
    println!();

    // Print by department
    println!("By Department");
    println!("───────────────────────────────────────────────────────────────");
    println!("{:<20} {:>6} {:>6} {:>10}",
        "Department", "Cops", "Tests", "Implemented");
    println!("{}", "─".repeat(50));

    for (dept, stats) in &by_department {
        println!("{:<20} {:>6} {:>6} {:>10}",
            dept,
            stats.total_files,
            stats.total_tests,
            stats.implemented_cops);
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
        println!("{:<35} {:>6}",
            "Cop", "Tests");
        println!("{}", "─".repeat(45));
        for (cop, stats) in &implemented {
            println!("{:<35} {:>6}",
                cop, stats.total_tests);
        }
    }
    println!();

    // Verbose: show top cops by test count
    if verbose {
        println!("Top 20 Cops by Test Count");
        println!("───────────────────────────────────────────────────────────────");

        let mut cops_by_tests: Vec<_> = by_cop.iter().collect();
        cops_by_tests.sort_by(|a, b| b.1.total_tests.cmp(&a.1.total_tests));

        println!("{:<40} {:>6}",
            "Cop", "Tests");
        println!("{}", "─".repeat(50));
        for (cop, stats) in cops_by_tests.iter().take(20) {
            println!("{:<40} {:>6}",
                cop, stats.total_tests);
        }
        println!();
    }
}
