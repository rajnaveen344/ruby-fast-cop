pub mod config;
pub mod cops;
pub mod offense;

pub use config::Config;
pub use offense::{Location, Offense, Severity};

use ruby_prism::parse;
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
    let mut offenses =
        cops::run_cops_with_version(&cops, &result, &source, &filename, target_ruby_version);

    // Filter out offenses for cops that have this file excluded
    offenses.retain(|offense| !config.is_excluded_for_cop(path, &offense.cop_name));

    Ok(offenses)
}

/// Build cops based on configuration
pub fn build_cops_from_config(config: &Config) -> Vec<Box<dyn cops::Cop>> {
    let mut result: Vec<Box<dyn cops::Cop>> = Vec::new();

    // Lint/Debugger
    if config.is_cop_enabled("Lint/Debugger") {
        result.push(Box::new(cops::lint::Debugger::new()));
    }

    // Lint/AssignmentInCondition
    if config.is_cop_enabled("Lint/AssignmentInCondition") {
        let allow_safe = config
            .get_cop_config("Lint/AssignmentInCondition")
            .and_then(|c| c.allow_safe_assignment)
            .unwrap_or(true);
        result.push(Box::new(cops::lint::AssignmentInCondition::new(allow_safe)));
    }

    // Layout/LineLength
    if config.is_cop_enabled("Layout/LineLength") {
        let max = config
            .get_cop_config("Layout/LineLength")
            .and_then(|c| c.max)
            .unwrap_or(120);
        result.push(Box::new(cops::layout::LineLength::new(max)));
    }

    // Metrics/BlockLength
    if config.is_cop_enabled("Metrics/BlockLength") {
        let max = config
            .get_cop_config("Metrics/BlockLength")
            .and_then(|c| c.max)
            .unwrap_or(25);
        result.push(Box::new(cops::metrics::BlockLength::new(max)));
    }

    // Style/AutoResourceCleanup
    if config.is_cop_enabled("Style/AutoResourceCleanup") {
        result.push(Box::new(cops::style::AutoResourceCleanup::new()));
    }

    // Style/FormatStringToken
    if config.is_cop_enabled("Style/FormatStringToken") {
        let style = config
            .get_cop_config("Style/FormatStringToken")
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "template" => cops::style::FormatStringTokenStyle::Template,
                "unannotated" => cops::style::FormatStringTokenStyle::Unannotated,
                _ => cops::style::FormatStringTokenStyle::Annotated,
            })
            .unwrap_or(cops::style::FormatStringTokenStyle::Annotated);
        result.push(Box::new(cops::style::FormatStringToken::new(style)));
    }

    // Style/HashSyntax
    if config.is_cop_enabled("Style/HashSyntax") {
        let style = config
            .get_cop_config("Style/HashSyntax")
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "hash_rockets" => cops::style::HashSyntaxStyle::HashRockets,
                "no_mixed_keys" => cops::style::HashSyntaxStyle::NoMixedKeys,
                "ruby19_no_mixed_keys" => cops::style::HashSyntaxStyle::Ruby19NoMixedKeys,
                _ => cops::style::HashSyntaxStyle::Ruby19,
            })
            .unwrap_or(cops::style::HashSyntaxStyle::Ruby19);
        result.push(Box::new(cops::style::HashSyntax::new(style)));
    }

    // Style/MethodCalledOnDoEndBlock
    if config.is_cop_enabled("Style/MethodCalledOnDoEndBlock") {
        result.push(Box::new(cops::style::MethodCalledOnDoEndBlock::new()));
    }

    // Style/RaiseArgs
    if config.is_cop_enabled("Style/RaiseArgs") {
        let style = config
            .get_cop_config("Style/RaiseArgs")
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "compact" => cops::style::RaiseArgsStyle::Compact,
                _ => cops::style::RaiseArgsStyle::Explode,
            })
            .unwrap_or(cops::style::RaiseArgsStyle::Explode);
        result.push(Box::new(cops::style::RaiseArgs::new(style)));
    }

    // Style/RescueStandardError
    if config.is_cop_enabled("Style/RescueStandardError") {
        let style = config
            .get_cop_config("Style/RescueStandardError")
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "implicit" => cops::style::RescueStandardErrorStyle::Implicit,
                _ => cops::style::RescueStandardErrorStyle::Explicit,
            })
            .unwrap_or(cops::style::RescueStandardErrorStyle::Explicit);
        result.push(Box::new(cops::style::RescueStandardError::new(style)));
    }

    // Style/StringMethods
    if config.is_cop_enabled("Style/StringMethods") {
        result.push(Box::new(cops::style::StringMethods::new()));
    }

    result
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
    let cop = build_single_cop(cop_name, config);
    match cop {
        Some(c) => {
            let result = parse(source.as_bytes());
            cops::run_cops_with_version(&[c], &result, source, filename, target_ruby_version)
        }
        None => vec![],
    }
}

/// Build a single cop with the given configuration
pub fn build_single_cop(cop_name: &str, config: &Config) -> Option<Box<dyn cops::Cop>> {
    match cop_name {
        "Lint/Debugger" => Some(Box::new(cops::lint::Debugger::new())),

        "Lint/AssignmentInCondition" => {
            let allow_safe = config
                .get_cop_config("Lint/AssignmentInCondition")
                .and_then(|c| c.allow_safe_assignment)
                .unwrap_or(true);
            Some(Box::new(cops::lint::AssignmentInCondition::new(allow_safe)))
        }

        "Layout/LineLength" => {
            let cop_config = config.get_cop_config("Layout/LineLength");
            let max = cop_config.and_then(|c| c.max).unwrap_or(120);
            let allow_uri = cop_config
                .and_then(|c| c.raw.get("AllowURI"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let allow_heredoc = cop_config
                .and_then(|c| c.raw.get("AllowHeredoc"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let allow_qualified_name = cop_config
                .and_then(|c| c.raw.get("AllowQualifiedName"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let uri_schemes = cop_config
                .and_then(|c| c.raw.get("URISchemes"))
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_else(|| vec!["http".to_string(), "https".to_string()]);
            let allowed_patterns = cop_config
                .and_then(|c| c.raw.get("AllowedPatterns"))
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let tab_width = cop_config
                .and_then(|c| c.raw.get("TabWidth"))
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .unwrap_or(2);
            Some(Box::new(cops::layout::LineLength::with_config(
                max,
                allow_uri,
                allow_heredoc,
                allow_qualified_name,
                uri_schemes,
                allowed_patterns,
                tab_width,
            )))
        }

        "Metrics/BlockLength" => {
            let cop_config = config.get_cop_config("Metrics/BlockLength");
            let max = cop_config.and_then(|c| c.max).unwrap_or(25);
            let count_comments = cop_config
                .and_then(|c| c.raw.get("CountComments"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let count_as_one = cop_config
                .and_then(|c| c.raw.get("CountAsOne"))
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            // Merge AllowedMethods + IgnoredMethods + ExcludedMethods (legacy names)
            let mut allowed_methods: Vec<String> = Vec::new();
            for key in &["AllowedMethods", "IgnoredMethods", "ExcludedMethods"] {
                if let Some(seq) = cop_config
                    .and_then(|c| c.raw.get(*key))
                    .and_then(|v| v.as_sequence())
                {
                    for v in seq {
                        if let Some(s) = v.as_str() {
                            allowed_methods.push(s.to_string());
                        }
                    }
                }
            }

            let mut allowed_patterns: Vec<String> = Vec::new();
            for key in &["AllowedPatterns", "IgnoredPatterns"] {
                if let Some(seq) = cop_config
                    .and_then(|c| c.raw.get(*key))
                    .and_then(|v| v.as_sequence())
                {
                    for v in seq {
                        if let Some(s) = v.as_str() {
                            allowed_patterns.push(s.to_string());
                        }
                    }
                }
            }

            Some(Box::new(cops::metrics::BlockLength::with_config(
                max,
                count_comments,
                count_as_one,
                allowed_methods,
                allowed_patterns,
            )))
        }

        "Style/AutoResourceCleanup" => Some(Box::new(cops::style::AutoResourceCleanup::new())),

        "Style/FormatStringToken" => {
            let style = config
                .get_cop_config("Style/FormatStringToken")
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "template" => cops::style::FormatStringTokenStyle::Template,
                    "unannotated" => cops::style::FormatStringTokenStyle::Unannotated,
                    _ => cops::style::FormatStringTokenStyle::Annotated,
                })
                .unwrap_or(cops::style::FormatStringTokenStyle::Annotated);
            Some(Box::new(cops::style::FormatStringToken::new(style)))
        }

        "Style/HashSyntax" => {
            let style = config
                .get_cop_config("Style/HashSyntax")
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "hash_rockets" => cops::style::HashSyntaxStyle::HashRockets,
                    "no_mixed_keys" => cops::style::HashSyntaxStyle::NoMixedKeys,
                    "ruby19_no_mixed_keys" => cops::style::HashSyntaxStyle::Ruby19NoMixedKeys,
                    _ => cops::style::HashSyntaxStyle::Ruby19,
                })
                .unwrap_or(cops::style::HashSyntaxStyle::Ruby19);
            Some(Box::new(cops::style::HashSyntax::new(style)))
        }

        "Style/MethodCalledOnDoEndBlock" => {
            Some(Box::new(cops::style::MethodCalledOnDoEndBlock::new()))
        }

        "Style/RaiseArgs" => {
            let cop_config = config.get_cop_config("Style/RaiseArgs");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "compact" => cops::style::RaiseArgsStyle::Compact,
                    _ => cops::style::RaiseArgsStyle::Explode,
                })
                .unwrap_or(cops::style::RaiseArgsStyle::Explode);

            // Parse AllowedCompactTypes from raw config
            let allowed_compact_types = cop_config
                .and_then(|c| c.raw.get("AllowedCompactTypes"))
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            Some(Box::new(cops::style::RaiseArgs::with_allowed_compact_types(
                style,
                allowed_compact_types,
            )))
        }

        "Style/RescueStandardError" => {
            let style = config
                .get_cop_config("Style/RescueStandardError")
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "implicit" => cops::style::RescueStandardErrorStyle::Implicit,
                    _ => cops::style::RescueStandardErrorStyle::Explicit,
                })
                .unwrap_or(cops::style::RescueStandardErrorStyle::Explicit);
            Some(Box::new(cops::style::RescueStandardError::new(style)))
        }

        "Style/StringMethods" => Some(Box::new(cops::style::StringMethods::new())),

        _ => None,
    }
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
