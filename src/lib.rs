// ── Crate-wide macros (must appear before `mod` declarations) ──

/// Extract a Prism node's name as a `Cow<str>`.
/// Shorthand for `String::from_utf8_lossy(node.name().as_slice())`.
#[macro_export]
macro_rules! node_name {
    ($node:expr) => {
        String::from_utf8_lossy($node.name().as_slice())
    };
}

// ── Modules ──

pub mod config;
pub mod cops;
pub mod correction;
pub mod helpers;
pub mod offense;

pub use config::Config;
pub use correction::{apply_corrections, apply_corrections_detailed, CorrectionResult};
pub use offense::{Correction, Edit, Location, Offense, Severity};

use ruby_prism::parse;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Failed to read file: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Check Ruby source code for offenses using all cops with default config
pub fn check_source(source: &str, filename: &str) -> Vec<Offense> {
    let all_cops = cops::all();
    check_source_with_cops(source, filename, &all_cops)
}

/// Check Ruby source code for offenses using specific cops
pub fn check_source_with_cops(
    source: &str,
    filename: &str,
    cops: &[Box<dyn cops::Cop>],
) -> Vec<Offense> {
    let result = parse(source.as_bytes());
    cops::run_cops(cops, &result, source, filename)
}

/// Check a file for offenses using default cops
pub fn check_file(path: &Path) -> Result<Vec<Offense>> {
    let source = std::fs::read_to_string(path)?;
    let filename = path.to_string_lossy();
    Ok(check_source(&source, &filename))
}

/// Check a file for offenses using configuration
pub fn check_file_with_config(path: &Path, config: &Config) -> Result<Vec<Offense>> {
    // Check if file is globally excluded
    if config.is_excluded(path) {
        return Ok(vec![]);
    }

    let source = std::fs::read_to_string(path)?;
    let filename = path.to_string_lossy();
    let cops = build_cops_from_config(config);

    let result = parse(source.as_bytes());
    let target_ruby_version = config.all_cops.target_ruby_version.unwrap_or(2.5);
    let mut offenses = cops::run_cops_full(
        &cops,
        &result,
        &source,
        &filename,
        target_ruby_version,
        Some(path),
    );

    // Filter out offenses for cops that have this file excluded
    offenses.retain(|offense| !config.is_excluded_for_cop(path, &offense.cop_name));

    Ok(offenses)
}

/// Maximum number of correction iterations before giving up.
/// Ruff uses 10; RuboCop uses 200. We follow Ruff's model.
const MAX_CORRECTION_ITERATIONS: usize = 10;

/// Hash source code for cycle detection during iterative correction.
fn hash_source(source: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

/// Check a file and optionally autocorrect. Returns (offenses, corrected_count).
/// When `autocorrect` is true, iteratively applies corrections (up to 10 passes)
/// until no more fixes are available or a cycle is detected. Writes the file once at end.
pub fn check_and_correct_file(
    path: &Path,
    config: &Config,
    autocorrect: bool,
) -> Result<(Vec<Offense>, usize)> {
    if config.is_excluded(path) {
        return Ok((vec![], 0));
    }

    let source = std::fs::read_to_string(path)?;
    let filename = path.to_string_lossy();
    let cops = build_cops_from_config(config);
    let target_ruby_version = config.all_cops.target_ruby_version.unwrap_or(2.5);

    if !autocorrect {
        // Non-autocorrect path: single parse + lint (unchanged)
        let result = parse(source.as_bytes());
        let mut offenses =
            cops::run_cops_with_version(&cops, &result, &source, &filename, target_ruby_version);
        offenses.retain(|offense| !config.is_excluded_for_cop(path, &offense.cop_name));
        return Ok((offenses, 0));
    }

    let (corrected, offenses, total_applied) =
        check_and_correct_source(&source, &filename, &cops, target_ruby_version);

    // Filter excluded cops from final offenses
    let offenses: Vec<Offense> = offenses
        .into_iter()
        .filter(|o| !config.is_excluded_for_cop(path, &o.cop_name))
        .collect();

    if corrected != source {
        std::fs::write(path, &corrected)?;
    }

    Ok((offenses, total_applied))
}

/// Check source code and iteratively apply corrections in memory.
///
/// Returns `(corrected_source, remaining_offenses, total_corrections_applied)`.
/// Runs up to `MAX_CORRECTION_ITERATIONS` passes, stopping early when:
/// - No corrections are available
/// - No edits were actually applied (all overlapped)
/// - Source didn't change (edits were no-ops)
/// - A cycle is detected (source seen before)
pub fn check_and_correct_source(
    source: &str,
    filename: &str,
    cops: &[Box<dyn cops::Cop>],
    target_ruby_version: f64,
) -> (String, Vec<Offense>, usize) {
    let mut current_source = source.to_string();
    let mut seen_hashes: HashSet<u64> = HashSet::new();
    seen_hashes.insert(hash_source(&current_source));
    let mut total_applied = 0usize;

    for _ in 0..MAX_CORRECTION_ITERATIONS {
        // Run cops in a block so ParseResult's borrow is dropped before we reassign
        let offenses = {
            let result = parse(current_source.as_bytes());
            cops::run_cops_with_version(cops, &result, &current_source, filename, target_ruby_version)
        };

        let has_corrections = offenses.iter().any(|o| o.correction.is_some());
        if !has_corrections {
            return (current_source, offenses, total_applied);
        }

        let cr = correction::apply_corrections_detailed(&current_source, &offenses);
        if cr.applied_count == 0 || cr.output == current_source {
            return (current_source, offenses, total_applied);
        }

        total_applied += cr.applied_count;

        // Cycle detection
        let h = hash_source(&cr.output);
        if !seen_hashes.insert(h) {
            // We've seen this source before — stop to avoid infinite loop
            return (cr.output, offenses, total_applied);
        }

        current_source = cr.output;
    }

    // Exhausted iterations — do one final lint pass on the corrected source
    let offenses = {
        let result = parse(current_source.as_bytes());
        cops::run_cops_with_version(cops, &result, &current_source, filename, target_ruby_version)
    };
    (current_source, offenses, total_applied)
}

/// Normalize Ruby regex syntax to Rust `regex` crate syntax.
/// - Ruby `\p{Word}` → Rust `\w`
/// - Strip Ruby `(?-mix:...)` wrapper
fn normalize_ruby_regex(pat: &str) -> String {
    let mut s = pat.to_string();
    if let Some(inner) = s.strip_prefix("(?-mix:").and_then(|x| x.strip_suffix(")")) {
        s = inner.to_string();
    }
    s = s.replace(r"\p{Word}", r"\w");
    s
}

/// Build cops based on configuration
pub fn build_cops_from_config(config: &Config) -> Vec<Box<dyn cops::Cop>> {
    cops::registry::build_from_config(config)
}


/// Check Ruby source code for offenses using a specific cop with config
/// This is mainly for testing purposes
pub fn check_source_with_cop_config(
    source: &str,
    filename: &str,
    cop_name: &str,
    config: &Config,
) -> Vec<Offense> {
    check_source_with_cop_config_and_version(source, filename, cop_name, config, 2.5)
}

/// Check Ruby source code for offenses using a specific cop with config and Ruby version
/// This is mainly for testing purposes
pub fn check_source_with_cop_config_and_version(
    source: &str,
    filename: &str,
    cop_name: &str,
    config: &Config,
    target_ruby_version: f64,
) -> Vec<Offense> {
    check_source_with_cop_config_version_and_path(
        source,
        filename,
        cop_name,
        config,
        target_ruby_version,
        None,
    )
}

/// Check source for a single cop with an optional real file path.
/// Mainly used by tests for cops that need filesystem metadata (e.g. `Lint/ScriptPermission`).
pub fn check_source_with_cop_config_version_and_path(
    source: &str,
    filename: &str,
    cop_name: &str,
    config: &Config,
    target_ruby_version: f64,
    file_path: Option<&Path>,
) -> Vec<Offense> {
    // Respect per-cop Exclude patterns (used by some fixture tests)
    if config.is_excluded_for_cop(Path::new(filename), cop_name) {
        return vec![];
    }
    let cop = build_single_cop(cop_name, config);
    match cop {
        Some(c) => {
            let result = parse(source.as_bytes());
            cops::run_cops_full(
                &[c],
                &result,
                source,
                filename,
                target_ruby_version,
                file_path,
            )
        }
        None => vec![],
    }
}
/// Build a single cop with the given configuration
pub fn build_single_cop(cop_name: &str, config: &Config) -> Option<Box<dyn cops::Cop>> {
    cops::registry::build_one(cop_name, config)
}

/// Find unsupported cops in the configuration
pub fn find_unsupported_cops(config: &Config) -> Vec<String> {
    let mut unsupported = Vec::new();

    for cop_name in config.cops.keys() {
        // Skip department-level configs like "Style" or "Lint"
        if !cop_name.contains('/') {
            continue;
        }

        // Check if it's enabled but not supported
        if config.is_cop_enabled(cop_name) && !config::is_supported_cop(cop_name) {
            unsupported.push(cop_name.clone());
        }
    }

    unsupported.sort();
    unsupported
}
