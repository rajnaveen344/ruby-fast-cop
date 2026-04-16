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
    let mut offenses =
        cops::run_cops_with_version(&cops, &result, &source, &filename, target_ruby_version);

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
    let mut result: Vec<Box<dyn cops::Cop>> = Vec::new();

    // Lint/AssignmentInCondition
    if config.is_cop_enabled("Lint/AssignmentInCondition") {
        let allow_safe = config
            .get_cop_config("Lint/AssignmentInCondition")
            .and_then(|c| c.allow_safe_assignment)
            .unwrap_or(true);
        result.push(Box::new(cops::lint::AssignmentInCondition::new(allow_safe)));
    }

    // Lint/Debugger
    if config.is_cop_enabled("Lint/Debugger") {
        result.push(Box::new(cops::lint::Debugger::new()));
    }

    // Lint/DuplicateMethods
    if config.is_cop_enabled("Lint/DuplicateMethods") {
        result.push(Box::new(cops::lint::DuplicateMethods::new()));
    }

    // Lint/EmptyConditionalBody
    if config.is_cop_enabled("Lint/EmptyConditionalBody") {
        if let Some(cop) = build_single_cop("Lint/EmptyConditionalBody", config) {
            result.push(cop);
        }
    }

    // Lint/LiteralInInterpolation
    if config.is_cop_enabled("Lint/LiteralInInterpolation") {
        result.push(Box::new(cops::lint::LiteralInInterpolation::new()));
    }

    // Lint/FormatParameterMismatch
    if config.is_cop_enabled("Lint/FormatParameterMismatch") {
        result.push(Box::new(cops::lint::FormatParameterMismatch::new()));
    }

    // Lint/OutOfRangeRegexpRef
    if config.is_cop_enabled("Lint/OutOfRangeRegexpRef") {
        result.push(Box::new(cops::lint::OutOfRangeRegexpRef::new()));
    }

    // Lint/RedundantSplatExpansion
    if config.is_cop_enabled("Lint/RedundantSplatExpansion") {
        result.push(Box::new(cops::lint::RedundantSplatExpansion::new(true)));
    }

    // Lint/RedundantSafeNavigation
    if config.is_cop_enabled("Lint/RedundantSafeNavigation") {
        if let Some(cop) = build_single_cop("Lint/RedundantSafeNavigation", config) {
            result.push(cop);
        }
    }

    // Lint/RedundantTypeConversion
    if config.is_cop_enabled("Lint/RedundantTypeConversion") {
        result.push(Box::new(cops::lint::RedundantTypeConversion::new()));
    }

    // Lint/RescueType
    if config.is_cop_enabled("Lint/RescueType") {
        result.push(Box::new(cops::lint::RescueType::new()));
    }

    // Lint/SelfAssignment
    if config.is_cop_enabled("Lint/SelfAssignment") {
        let cop_config = config.get_cop_config("Lint/SelfAssignment");
        let allow_rbs = cop_config
            .and_then(|c| c.raw.get("AllowRBSInlineAnnotation"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        result.push(Box::new(cops::lint::LintSelfAssignment::new(allow_rbs)));
    }

    // Lint/SafeNavigationChain
    if config.is_cop_enabled("Lint/SafeNavigationChain") {
        let cop_config = config.get_cop_config("Lint/SafeNavigationChain");
        let allowed = cop_config
            .and_then(|c| c.raw.get("AllowedMethods"))
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        result.push(Box::new(cops::lint::SafeNavigationChain::with_allowed_methods(allowed)));
    }

    // Lint/SafeNavigationConsistency
    if config.is_cop_enabled("Lint/SafeNavigationConsistency") {
        let cop_config = config.get_cop_config("Lint/SafeNavigationConsistency");
        let allowed = cop_config
            .and_then(|c| c.raw.get("AllowedMethods"))
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_else(|| vec!["present?".into(), "blank?".into(), "try".into(), "presence".into()]);
        result.push(Box::new(cops::lint::SafeNavigationConsistency::with_config(allowed)));
    }

    // Lint/UnreachableCode
    if config.is_cop_enabled("Lint/UnreachableCode") {
        result.push(Box::new(cops::lint::UnreachableCode::new()));
    }

    // Lint/UselessAccessModifier
    if config.is_cop_enabled("Lint/UselessAccessModifier") {
        let cop_config = config.get_cop_config("Lint/UselessAccessModifier");
        let context_creating = cop_config
            .and_then(|c| c.raw.get("ContextCreatingMethods"))
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let method_creating = cop_config
            .and_then(|c| c.raw.get("MethodCreatingMethods"))
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        result.push(Box::new(cops::lint::UselessAccessModifier::with_config(
            context_creating,
            method_creating,
        )));
    }

    // Lint/UselessAssignment
    if config.is_cop_enabled("Lint/UselessAssignment") {
        result.push(Box::new(cops::lint::UselessAssignment::new()));
    }

    // Lint/Void
    if config.is_cop_enabled("Lint/Void") {
        result.push(Box::new(cops::lint::Void::new(false)));
    }

    // Lint/AmbiguousBlockAssociation
    if config.is_cop_enabled("Lint/AmbiguousBlockAssociation") {
        if let Some(cop) = build_single_cop("Lint/AmbiguousBlockAssociation", config) {
            result.push(cop);
        }
    }

    // Lint/NestedMethodDefinition
    if config.is_cop_enabled("Lint/NestedMethodDefinition") {
        if let Some(cop) = build_single_cop("Lint/NestedMethodDefinition", config) {
            result.push(cop);
        }
    }

    // Lint/ShadowedException
    if config.is_cop_enabled("Lint/ShadowedException") {
        result.push(Box::new(cops::lint::ShadowedException::new()));
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

    // Style/AccessModifierDeclarations
    if config.is_cop_enabled("Style/AccessModifierDeclarations") {
        let cop_config = config.get_cop_config("Style/AccessModifierDeclarations");
        let style = cop_config
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "inline" => cops::style::AccessModifierDeclarationsStyle::Inline,
                _ => cops::style::AccessModifierDeclarationsStyle::Group,
            })
            .unwrap_or(cops::style::AccessModifierDeclarationsStyle::Group);
        let allow_symbols = cop_config
            .and_then(|c| c.raw.get("AllowModifiersOnSymbols"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let allow_attrs = cop_config
            .and_then(|c| c.raw.get("AllowModifiersOnAttrs"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let allow_alias = cop_config
            .and_then(|c| c.raw.get("AllowModifiersOnAliasMethod"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        result.push(Box::new(cops::style::AccessModifierDeclarations::with_config(
            style,
            allow_symbols,
            allow_attrs,
            allow_alias,
        )));
    }

    // Style/ArrayIntersect
    if config.is_cop_enabled("Style/ArrayIntersect") {
        if let Some(cop) = build_single_cop("Style/ArrayIntersect", config) {
            result.push(cop);
        }
    }

    // Style/AndOr
    if config.is_cop_enabled("Style/AndOr") {
        if let Some(cop) = build_single_cop("Style/AndOr", config) {
            result.push(cop);
        }
    }

    // Style/AutoResourceCleanup
    if config.is_cop_enabled("Style/AutoResourceCleanup") {
        result.push(Box::new(cops::style::AutoResourceCleanup::new()));
    }

    // Style/BlockDelimiters
    if config.is_cop_enabled("Style/BlockDelimiters") {
        if let Some(cop) = build_single_cop("Style/BlockDelimiters", config) {
            result.push(cop);
        }
    }

    // Style/CommentedKeyword
    if config.is_cop_enabled("Style/CommentedKeyword") {
        result.push(Box::new(cops::style::CommentedKeyword::new()));
    }

    // Style/EmptyElse
    if config.is_cop_enabled("Style/EmptyElse") {
        if let Some(cop) = build_single_cop("Style/EmptyElse", config) {
            result.push(cop);
        }
    }

    // Style/ConditionalAssignment
    if config.is_cop_enabled("Style/ConditionalAssignment") {
        let cop_config = config.get_cop_config("Style/ConditionalAssignment");
        let style = cop_config
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "assign_to_condition" => cops::style::ConditionalAssignmentStyle::AssignToCondition,
                _ => cops::style::ConditionalAssignmentStyle::AssignInsideCondition,
            })
            .unwrap_or(cops::style::ConditionalAssignmentStyle::AssignInsideCondition);
        let include_ternary = cop_config
            .and_then(|c| c.raw.get("IncludeTernaryExpressions"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let single_line_only = cop_config
            .and_then(|c| c.raw.get("SingleLineConditionsOnly"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        result.push(Box::new(cops::style::ConditionalAssignment::with_config(
            style,
            include_ternary,
            single_line_only,
        )));
    }

    // Style/Documentation
    if config.is_cop_enabled("Style/Documentation") {
        if let Some(cop) = build_single_cop("Style/Documentation", config) {
            result.push(cop);
        }
    }

    // Style/EmptyLiteral
    if config.is_cop_enabled("Style/EmptyLiteral") {
        if let Some(cop) = build_single_cop("Style/EmptyLiteral", config) {
            result.push(cop);
        }
    }

    // Style/FormatStringToken
    if config.is_cop_enabled("Style/FormatStringToken") {
        let cop_config = config.get_cop_config("Style/FormatStringToken");
        let style = cop_config
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "template" => cops::style::FormatStringTokenStyle::Template,
                "unannotated" => cops::style::FormatStringTokenStyle::Unannotated,
                _ => cops::style::FormatStringTokenStyle::Annotated,
            })
            .unwrap_or(cops::style::FormatStringTokenStyle::Annotated);
        let max_unannotated = cop_config
            .and_then(|c| c.raw.get("MaxUnannotatedPlaceholdersAllowed"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        let conservative = cop_config
            .and_then(|c| c.raw.get("Mode"))
            .and_then(|v| v.as_str())
            .map(|s| s == "conservative")
            .unwrap_or(false);
        let allowed_methods = cop_config
            .and_then(|c| c.raw.get("AllowedMethods"))
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let allowed_patterns = cop_config
            .and_then(|c| c.raw.get("AllowedPatterns"))
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        result.push(Box::new(cops::style::FormatStringToken::with_config(
            style,
            max_unannotated,
            conservative,
            allowed_methods,
            allowed_patterns,
        )));
    }

    // Style/HashEachMethods
    if config.is_cop_enabled("Style/HashEachMethods") {
        if let Some(cop) = build_single_cop("Style/HashEachMethods", config) {
            result.push(cop);
        }
    }

    // Style/GlobalVars
    if config.is_cop_enabled("Style/GlobalVars") {
        if let Some(cop) = build_single_cop("Style/GlobalVars", config) {
            result.push(cop);
        }
    }

    // Style/GuardClause
    if config.is_cop_enabled("Style/GuardClause") {
        if let Some(cop) = build_single_cop("Style/GuardClause", config) {
            result.push(cop);
        }
    }

    // Style/HashSyntax
    if config.is_cop_enabled("Style/HashSyntax") {
        let cop_config = config.get_cop_config("Style/HashSyntax");
        let style = cop_config
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "hash_rockets" => cops::style::HashSyntaxStyle::HashRockets,
                "no_mixed_keys" => cops::style::HashSyntaxStyle::NoMixedKeys,
                "ruby19_no_mixed_keys" => cops::style::HashSyntaxStyle::Ruby19NoMixedKeys,
                _ => cops::style::HashSyntaxStyle::Ruby19,
            })
            .unwrap_or(cops::style::HashSyntaxStyle::Ruby19);
        let shorthand = cop_config
            .and_then(|c| c.raw.get("EnforcedShorthandSyntax"))
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "always" => cops::style::HashSyntaxShorthandStyle::Always,
                "never" => cops::style::HashSyntaxShorthandStyle::Never,
                "consistent" => cops::style::HashSyntaxShorthandStyle::Consistent,
                "either_consistent" => cops::style::HashSyntaxShorthandStyle::EitherConsistent,
                _ => cops::style::HashSyntaxShorthandStyle::Either,
            })
            .unwrap_or(cops::style::HashSyntaxShorthandStyle::Either);
        let use_rockets_with_symbols = cop_config
            .and_then(|c| c.raw.get("UseHashRocketsWithSymbolValues"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let prefer_rockets_non_alnum = cop_config
            .and_then(|c| c.raw.get("PreferHashRocketsForNonAlnumEndingSymbols"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        result.push(Box::new(cops::style::HashSyntax::with_config(
            style,
            shorthand,
            use_rockets_with_symbols,
            prefer_rockets_non_alnum,
        )));
    }

    // Style/IfUnlessModifier
    if config.is_cop_enabled("Style/IfUnlessModifier") {
        let ll_config = config.get_cop_config("Layout/LineLength");
        let ll_enabled = config.is_cop_enabled("Layout/LineLength");
        let max_ll = ll_config.and_then(|c| c.max).unwrap_or(80) as usize;
        let allow_uri = ll_config
            .and_then(|c| c.raw.get("AllowURI"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let allow_cop_directives = ll_config
            .and_then(|c| c.raw.get("AllowCopDirectives"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let tab_width = config.get_cop_config("Layout/IndentationStyle")
            .and_then(|c| c.raw.get("IndentationWidth"))
            .and_then(|v| v.as_i64())
            .or_else(|| config.get_cop_config("Layout/IndentationWidth")
                .and_then(|c| c.raw.get("Width"))
                .and_then(|v| v.as_i64()))
            .map(|v| v as usize);
        result.push(Box::new(cops::style::IfUnlessModifier::with_config(
            max_ll, ll_enabled, allow_uri, allow_cop_directives, tab_width,
        )));
    }

    // Style/IdenticalConditionalBranches
    if config.is_cop_enabled("Style/IdenticalConditionalBranches") {
        result.push(Box::new(cops::style::IdenticalConditionalBranches::new()));
    }

    // Style/InverseMethods
    if config.is_cop_enabled("Style/InverseMethods") {
        if let Some(cop) = build_single_cop("Style/InverseMethods", config) {
            result.push(cop);
        }
    }

    // Style/MethodCalledOnDoEndBlock
    if config.is_cop_enabled("Style/MethodCalledOnDoEndBlock") {
        result.push(Box::new(cops::style::MethodCalledOnDoEndBlock::new()));
    }

    // Style/MethodDefParentheses
    if config.is_cop_enabled("Style/MethodDefParentheses") {
        if let Some(cop) = build_single_cop("Style/MethodDefParentheses", config) {
            result.push(cop);
        }
    }

    // Style/OneLineConditional
    if config.is_cop_enabled("Style/OneLineConditional") {
        if let Some(cop) = build_single_cop("Style/OneLineConditional", config) {
            result.push(cop);
        }
    }

    // Style/MutableConstant
    if config.is_cop_enabled("Style/MutableConstant") {
        let cop_config = config.get_cop_config("Style/MutableConstant");
        let style = cop_config
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "strict" => cops::style::MutableConstantStyle::Strict,
                _ => cops::style::MutableConstantStyle::Literals,
            })
            .unwrap_or(cops::style::MutableConstantStyle::Literals);
        result.push(Box::new(cops::style::MutableConstant::new(style)));
    }

    // Style/NegativeArrayIndex
    if config.is_cop_enabled("Style/NegativeArrayIndex") {
        result.push(Box::new(cops::style::NegativeArrayIndex::new()));
    }

    // Style/Next
    if config.is_cop_enabled("Style/Next") {
        if let Some(cop) = build_single_cop("Style/Next", config) {
            result.push(cop);
        }
    }

    // Style/RedundantCondition
    if config.is_cop_enabled("Style/RedundantCondition") {
        if let Some(cop) = build_single_cop("Style/RedundantCondition", config) {
            result.push(cop);
        }
    }

    // Style/SymbolProc
    if config.is_cop_enabled("Style/SymbolProc") {
        if let Some(cop) = build_single_cop("Style/SymbolProc", config) {
            result.push(cop);
        }
    }

    // Style/PercentLiteralDelimiters
    if config.is_cop_enabled("Style/PercentLiteralDelimiters") {
        let cop_config = config.get_cop_config("Style/PercentLiteralDelimiters");
        let preferred = cop_config
            .and_then(|c| c.raw.get("PreferredDelimiters"))
            .and_then(|v| v.as_mapping())
            .map(|m| {
                let mut map = std::collections::HashMap::new();
                for (k, v) in m.iter() {
                    if let (Some(key), Some(val)) = (k.as_str(), v.as_str()) {
                        map.insert(key.to_string(), val.to_string());
                    }
                }
                map
            })
            .unwrap_or_else(|| {
                let mut m = std::collections::HashMap::new();
                m.insert("default".to_string(), "()".to_string());
                m
            });
        result.push(Box::new(cops::style::PercentLiteralDelimiters::with_config(preferred)));
    }

    // Style/RedundantBegin
    if config.is_cop_enabled("Style/RedundantBegin") {
        result.push(Box::new(cops::style::RedundantBegin::new()));
    }

    // Style/RedundantReturn
    if config.is_cop_enabled("Style/RedundantReturn") {
        if let Some(cop) = build_single_cop("Style/RedundantReturn", config) {
            result.push(cop);
        }
    }

    // Style/Lambda
    if config.is_cop_enabled("Style/Lambda") {
        if let Some(cop) = build_single_cop("Style/Lambda", config) {
            result.push(cop);
        }
    }

    // Style/TrivialAccessors
    if config.is_cop_enabled("Style/TrivialAccessors") {
        if let Some(cop) = build_single_cop("Style/TrivialAccessors", config) {
            result.push(cop);
        }
    }

    // Style/CaseLikeIf
    if config.is_cop_enabled("Style/CaseLikeIf") {
        if let Some(cop) = build_single_cop("Style/CaseLikeIf", config) {
            result.push(cop);
        }
    }

    // Style/SoleNestedConditional
    if config.is_cop_enabled("Style/SoleNestedConditional") {
        if let Some(cop) = build_single_cop("Style/SoleNestedConditional", config) {
            result.push(cop);
        }
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

    // Style/RedundantParentheses
    if config.is_cop_enabled("Style/RedundantParentheses") {
        let (ternary_req, allow_multiline) = read_redundant_parens_cross_cop_config(config);
        result.push(Box::new(cops::style::RedundantParentheses::with_config(
            ternary_req,
            allow_multiline,
        )));
    }

    // Style/RedundantFreeze
    if config.is_cop_enabled("Style/RedundantFreeze") {
        result.push(Box::new(cops::style::RedundantFreeze::new()));
    }

    // Style/RedundantSelf
    if config.is_cop_enabled("Style/RedundantSelf") {
        result.push(Box::new(cops::style::RedundantSelf::new()));
    }

    // Style/RedundantRegexpCharacterClass
    if config.is_cop_enabled("Style/RedundantRegexpCharacterClass") {
        result.push(Box::new(cops::style::RedundantRegexpCharacterClass::new()));
    }

    // Style/RedundantRegexpEscape
    if config.is_cop_enabled("Style/RedundantRegexpEscape") {
        result.push(Box::new(cops::style::RedundantRegexpEscape::new()));
    }

    // Style/RedundantStringEscape
    if config.is_cop_enabled("Style/RedundantStringEscape") {
        result.push(Box::new(cops::style::RedundantStringEscape::new()));
    }

    // Style/RedundantSort
    if config.is_cop_enabled("Style/RedundantSort") {
        result.push(Box::new(cops::style::RedundantSort::new()));
    }

    // Style/HashTransformKeys
    if config.is_cop_enabled("Style/HashTransformKeys") {
        result.push(Box::new(cops::style::HashTransformKeys::new()));
    }

    // Style/HashTransformValues
    if config.is_cop_enabled("Style/HashTransformValues") {
        result.push(Box::new(cops::style::HashTransformValues::new()));
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

    // Style/SafeNavigation
    if config.is_cop_enabled("Style/SafeNavigation") {
        result.push(Box::new(cops::style::SafeNavigation::new()));
    }

    // Style/Sample
    if config.is_cop_enabled("Style/Sample") {
        result.push(Box::new(cops::style::Sample::new()));
    }

    // Style/SelectByRegexp
    if config.is_cop_enabled("Style/SelectByRegexp") {
        result.push(Box::new(cops::style::SelectByRegexp::new()));
    }

    // Style/SelfAssignment
    if config.is_cop_enabled("Style/SelfAssignment") {
        result.push(Box::new(cops::style::SelfAssignment::new()));
    }

    // Style/StringMethods
    if config.is_cop_enabled("Style/StringMethods") {
        result.push(Box::new(cops::style::StringMethods::new()));
    }

    // Style/TernaryParentheses
    if config.is_cop_enabled("Style/TernaryParentheses") {
        if let Some(cop) = build_single_cop("Style/TernaryParentheses", config) {
            result.push(cop);
        }
    }

    // Style/YodaCondition
    if config.is_cop_enabled("Style/YodaCondition") {
        let cop_config = config.get_cop_config("Style/YodaCondition");
        let style = match cop_config.and_then(|c| c.raw.get("EnforcedStyle")).and_then(|v| v.as_str()) {
            Some("forbid_for_equality_operators_only") => cops::style::YodaConditionStyle::ForbidForEqualityOperatorsOnly,
            Some("require_for_all_comparison_operators") => cops::style::YodaConditionStyle::RequireForAllComparisonOperators,
            Some("require_for_equality_operators_only") => cops::style::YodaConditionStyle::RequireForEqualityOperatorsOnly,
            _ => cops::style::YodaConditionStyle::ForbidForAllComparisonOperators,
        };
        result.push(Box::new(cops::style::YodaCondition::new(style)));
    }

    // Style/ZeroLengthPredicate
    if config.is_cop_enabled("Style/ZeroLengthPredicate") {
        result.push(Box::new(cops::style::ZeroLengthPredicate::new()));
    }

    // Style/TrailingUnderscoreVariable
    if config.is_cop_enabled("Style/TrailingUnderscoreVariable") {
        if let Some(cop) = build_single_cop("Style/TrailingUnderscoreVariable", config) {
            result.push(cop);
        }
    }

    // Style/TrailingCommaInArguments
    if config.is_cop_enabled("Style/TrailingCommaInArguments") {
        let cop_config = config.get_cop_config("Style/TrailingCommaInArguments");
        let style = cop_config
            .and_then(|c| c.raw.get("EnforcedStyleForMultiline"))
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "comma" => cops::style::TrailingCommaInArgumentsStyle::Comma,
                "consistent_comma" => cops::style::TrailingCommaInArgumentsStyle::ConsistentComma,
                "diff_comma" => cops::style::TrailingCommaInArgumentsStyle::DiffComma,
                _ => cops::style::TrailingCommaInArgumentsStyle::NoComma,
            })
            .unwrap_or(cops::style::TrailingCommaInArgumentsStyle::NoComma);
        result.push(Box::new(cops::style::TrailingCommaInArguments::new(style)));
    }

    // Style/TrailingCommaInArrayLiteral
    if config.is_cop_enabled("Style/TrailingCommaInArrayLiteral") {
        let cop_config = config.get_cop_config("Style/TrailingCommaInArrayLiteral");
        let style = cop_config
            .and_then(|c| c.raw.get("EnforcedStyleForMultiline"))
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "comma" => cops::style::TrailingCommaInArrayLiteralStyle::Comma,
                "consistent_comma" => cops::style::TrailingCommaInArrayLiteralStyle::ConsistentComma,
                "diff_comma" => cops::style::TrailingCommaInArrayLiteralStyle::DiffComma,
                _ => cops::style::TrailingCommaInArrayLiteralStyle::NoComma,
            })
            .unwrap_or(cops::style::TrailingCommaInArrayLiteralStyle::NoComma);
        result.push(Box::new(cops::style::TrailingCommaInArrayLiteral::new(style)));
    }

    // Style/TrailingCommaInHashLiteral
    if config.is_cop_enabled("Style/TrailingCommaInHashLiteral") {
        let cop_config = config.get_cop_config("Style/TrailingCommaInHashLiteral");
        let style = cop_config
            .and_then(|c| c.raw.get("EnforcedStyleForMultiline"))
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "comma" => cops::style::TrailingCommaInHashLiteralStyle::Comma,
                "consistent_comma" => cops::style::TrailingCommaInHashLiteralStyle::ConsistentComma,
                "diff_comma" => cops::style::TrailingCommaInHashLiteralStyle::DiffComma,
                _ => cops::style::TrailingCommaInHashLiteralStyle::NoComma,
            })
            .unwrap_or(cops::style::TrailingCommaInHashLiteralStyle::NoComma);
        result.push(Box::new(cops::style::TrailingCommaInHashLiteral::new(style)));
    }

    // Layout/TrailingWhitespace
    if config.is_cop_enabled("Layout/TrailingWhitespace") {
        let cop_config = config.get_cop_config("Layout/TrailingWhitespace");
        let allow_in_heredoc = cop_config
            .and_then(|c| c.raw.get("AllowInHeredoc"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        result.push(Box::new(cops::layout::TrailingWhitespace::with_config(
            allow_in_heredoc,
        )));
    }

    // Layout/TrailingEmptyLines
    if config.is_cop_enabled("Layout/TrailingEmptyLines") {
        let cop_config = config.get_cop_config("Layout/TrailingEmptyLines");
        let style = cop_config
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "final_blank_line" => cops::layout::TrailingEmptyLinesStyle::FinalBlankLine,
                _ => cops::layout::TrailingEmptyLinesStyle::FinalNewline,
            })
            .unwrap_or(cops::layout::TrailingEmptyLinesStyle::FinalNewline);
        result.push(Box::new(cops::layout::TrailingEmptyLines::new(style)));
    }

    // Layout/BlockAlignment
    if config.is_cop_enabled("Layout/BlockAlignment") {
        if let Some(cop) = build_single_cop("Layout/BlockAlignment", config) {
            result.push(cop);
        }
    }

    // Layout/CaseIndentation
    if config.is_cop_enabled("Layout/CaseIndentation") {
        if let Some(cop) = build_single_cop("Layout/CaseIndentation", config) {
            result.push(cop);
        }
    }

    // Layout/ElseAlignment
    if config.is_cop_enabled("Layout/ElseAlignment") {
        if let Some(cop) = build_single_cop("Layout/ElseAlignment", config) {
            result.push(cop);
        }
    }

    // Layout/BeginEndAlignment
    if config.is_cop_enabled("Layout/BeginEndAlignment") {
        let style = config.get_cop_config("Layout/BeginEndAlignment")
            .and_then(|c| c.raw.get("EnforcedStyleAlignWith"))
            .and_then(|v| v.as_str())
            .unwrap_or("start_of_line");
        let align_style = match style {
            "begin" => cops::layout::BeginEndAlignmentStyle::Begin,
            _ => cops::layout::BeginEndAlignmentStyle::StartOfLine,
        };
        result.push(Box::new(cops::layout::BeginEndAlignment::new(align_style)));
    }

    // Layout/DefEndAlignment
    if config.is_cop_enabled("Layout/DefEndAlignment") {
        let style = config.get_cop_config("Layout/DefEndAlignment")
            .and_then(|c| c.raw.get("EnforcedStyleAlignWith"))
            .and_then(|v| v.as_str())
            .unwrap_or("start_of_line");
        let align_style = match style {
            "def" => cops::layout::DefEndAlignmentStyle::Def,
            _ => cops::layout::DefEndAlignmentStyle::StartOfLine,
        };
        result.push(Box::new(cops::layout::DefEndAlignment::new(align_style)));
    }

    // Layout/EmptyLineAfterGuardClause
    if config.is_cop_enabled("Layout/EmptyLineAfterGuardClause") {
        result.push(Box::new(cops::layout::EmptyLineAfterGuardClause::new()));
    }

    // Layout/EmptyLineBetweenDefs
    if config.is_cop_enabled("Layout/EmptyLineBetweenDefs") {
        if let Some(cop) = build_single_cop("Layout/EmptyLineBetweenDefs", config) {
            result.push(cop);
        }
    }

    // Layout/EmptyLinesAroundAccessModifier
    if config.is_cop_enabled("Layout/EmptyLinesAroundAccessModifier") {
        let cop_config = config.get_cop_config("Layout/EmptyLinesAroundAccessModifier");
        let style = cop_config
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "only_before" => cops::layout::EmptyLinesAroundAccessModifierStyle::OnlyBefore,
                _ => cops::layout::EmptyLinesAroundAccessModifierStyle::Around,
            })
            .unwrap_or(cops::layout::EmptyLinesAroundAccessModifierStyle::Around);
        result.push(Box::new(cops::layout::EmptyLinesAroundAccessModifier::new(style)));
    }

    if config.is_cop_enabled("Layout/EmptyLinesAroundClassBody") {
        let cop_config = config.get_cop_config("Layout/EmptyLinesAroundClassBody");
        let style = cop_config
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| cops::layout::EmptyLinesAroundClassBodyStyle::parse(s))
            .unwrap_or(cops::layout::EmptyLinesAroundClassBodyStyle::NoEmptyLines);
        result.push(Box::new(cops::layout::EmptyLinesAroundClassBody::new(style)));
    }

    if config.is_cop_enabled("Layout/EmptyLinesAroundModuleBody") {
        let cop_config = config.get_cop_config("Layout/EmptyLinesAroundModuleBody");
        let style = cop_config
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| cops::layout::EmptyLinesAroundModuleBodyStyle::parse(s))
            .unwrap_or(cops::layout::EmptyLinesAroundModuleBodyStyle::NoEmptyLines);
        result.push(Box::new(cops::layout::EmptyLinesAroundModuleBody::new(style)));
    }

    // Layout/HeredocIndentation
    if config.is_cop_enabled("Layout/HeredocIndentation") {
        let cop_config = config.get_cop_config("Layout/HeredocIndentation");
        let active_support = cop_config
            .and_then(|c| c.raw.get("ActiveSupportExtensionsEnabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        result.push(Box::new(cops::layout::HeredocIndentation::with_config(
            2, active_support, None, true,
        )));
    }

    // Layout/IndentationWidth
    if config.is_cop_enabled("Layout/IndentationWidth") {
        result.push(build_indentation_width_cop(config));
    }

    // Layout/HashAlignment
    if config.is_cop_enabled("Layout/HashAlignment") {
        if let Some(cop) = build_single_cop("Layout/HashAlignment", config) {
            result.push(cop);
        }
    }

    // Layout/FirstArgumentIndentation
    if config.is_cop_enabled("Layout/FirstArgumentIndentation") {
        let cop_config = config.get_cop_config("Layout/FirstArgumentIndentation");
        let style = cop_config
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "consistent" => cops::layout::FirstArgumentIndentationStyle::Consistent,
                "consistent_relative_to_receiver" => cops::layout::FirstArgumentIndentationStyle::ConsistentRelativeToReceiver,
                "special_for_inner_method_call" => cops::layout::FirstArgumentIndentationStyle::SpecialForInnerMethodCall,
                _ => cops::layout::FirstArgumentIndentationStyle::SpecialForInnerMethodCallInParentheses,
            })
            .unwrap_or(cops::layout::FirstArgumentIndentationStyle::SpecialForInnerMethodCallInParentheses);
        let width = cop_config
            .and_then(|c| c.raw.get("IndentationWidth"))
            .and_then(|v| v.as_i64())
            .map(|v| v as usize);
        result.push(Box::new(cops::layout::FirstArgumentIndentation::new(style, width)));
    }

    // Layout/FirstHashElementIndentation
    if config.is_cop_enabled("Layout/FirstHashElementIndentation") {
        if let Some(cop) = build_single_cop("Layout/FirstHashElementIndentation", config) {
            result.push(cop);
        }
    }

    // Layout/FirstArrayElementIndentation
    if config.is_cop_enabled("Layout/FirstArrayElementIndentation") {
        if let Some(cop) = build_single_cop("Layout/FirstArrayElementIndentation", config) {
            result.push(cop);
        }
    }

    // Layout/EndAlignment
    if config.is_cop_enabled("Layout/EndAlignment") {
        let style = config.get_cop_config("Layout/EndAlignment")
            .and_then(|c| c.raw.get("EnforcedStyleAlignWith"))
            .and_then(|v| v.as_str())
            .unwrap_or("keyword");
        let align_style = match style {
            "variable" => cops::layout::EndAlignmentStyle::Variable,
            "start_of_line" => cops::layout::EndAlignmentStyle::StartOfLine,
            _ => cops::layout::EndAlignmentStyle::Keyword,
        };
        result.push(Box::new(cops::layout::EndAlignment::new(align_style)));
    }

    // Layout/RescueEnsureAlignment
    if config.is_cop_enabled("Layout/RescueEnsureAlignment") {
        let begin_end_style = config.get_cop_config("Layout/BeginEndAlignment")
            .and_then(|c| {
                let enabled = c.raw.get("Enabled").and_then(|v| v.as_bool()).unwrap_or(true);
                if enabled {
                    c.raw.get("EnforcedStyleAlignWith").and_then(|v| v.as_str().map(|s| s.to_string()))
                } else {
                    None
                }
            });
        result.push(Box::new(cops::layout::RescueEnsureAlignment::with_begin_end_style(begin_end_style)));
    }

    // Layout/LeadingCommentSpace
    if config.is_cop_enabled("Layout/LeadingCommentSpace") {
        result.push(Box::new(cops::layout::LeadingCommentSpace::new()));
    }

    // Layout/SpaceAfterComma
    if config.is_cop_enabled("Layout/SpaceAfterComma") {
        let space_inside_braces_is_space = config
            .get_cop_config("Layout/SpaceInsideHashLiteralBraces")
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| s == "space")
            .unwrap_or(false);
        result.push(Box::new(cops::layout::SpaceAfterComma::with_config(
            space_inside_braces_is_space,
        )));
    }

    // Layout/SpaceAroundKeyword
    if config.is_cop_enabled("Layout/SpaceAroundKeyword") {
        result.push(Box::new(cops::layout::SpaceAroundKeyword::new()));
    }

    // Layout/SpaceAroundMethodCallOperator
    if config.is_cop_enabled("Layout/SpaceAroundMethodCallOperator") {
        result.push(Box::new(cops::layout::SpaceAroundMethodCallOperator::new()));
    }

    // Layout/SpaceAroundOperators
    if config.is_cop_enabled("Layout/SpaceAroundOperators") {
        let c = config.get_cop_config("Layout/SpaceAroundOperators");
        let allow_for_alignment = c.and_then(|c| c.raw.get("AllowForAlignment")).and_then(|v| v.as_bool()).unwrap_or(true);
        let exp = c.and_then(|c| c.raw.get("EnforcedStyleForExponentOperator")).and_then(|v| v.as_str()).map(|s| s == "space").unwrap_or(false);
        let sl = c.and_then(|c| c.raw.get("EnforcedStyleForRationalLiterals")).and_then(|v| v.as_str()).map(|s| s == "space").unwrap_or(false);
        let hash_table_style = config
            .get_cop_config("Layout/HashAlignment")
            .and_then(|c| c.raw.get("EnforcedHashRocketStyle"))
            .and_then(|v| v.as_str())
            .map(|s| s == "table")
            .unwrap_or(false);
        result.push(Box::new(cops::layout::SpaceAroundOperators::with_config(allow_for_alignment, exp, sl, hash_table_style)));
    }

    // Layout/SpaceAroundBlockParameters
    if config.is_cop_enabled("Layout/SpaceAroundBlockParameters") {
        let style = config
            .get_cop_config("Layout/SpaceAroundBlockParameters")
            .and_then(|c| c.raw.get("EnforcedStyleInsidePipes"))
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "space" => cops::layout::SpaceAroundBlockParametersStyle::Space,
                _ => cops::layout::SpaceAroundBlockParametersStyle::NoSpace,
            })
            .unwrap_or(cops::layout::SpaceAroundBlockParametersStyle::NoSpace);
        result.push(Box::new(cops::layout::SpaceAroundBlockParameters::new(style)));
    }

    // Layout/MultilineArrayBraceLayout
    if config.is_cop_enabled("Layout/MultilineArrayBraceLayout") {
        if let Some(cop) = build_single_cop("Layout/MultilineArrayBraceLayout", config) {
            result.push(cop);
        }
    }

    // Layout/MultilineHashBraceLayout
    if config.is_cop_enabled("Layout/MultilineHashBraceLayout") {
        if let Some(cop) = build_single_cop("Layout/MultilineHashBraceLayout", config) {
            result.push(cop);
        }
    }

    // Layout/MultilineMethodCallBraceLayout
    if config.is_cop_enabled("Layout/MultilineMethodCallBraceLayout") {
        if let Some(cop) = build_single_cop("Layout/MultilineMethodCallBraceLayout", config) {
            result.push(cop);
        }
    }

    // Layout/MultilineMethodCallIndentation
    if config.is_cop_enabled("Layout/MultilineMethodCallIndentation") {
        let cop_config = config.get_cop_config("Layout/MultilineMethodCallIndentation");
        let style = cop_config
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "indented" => cops::layout::MultilineMethodCallIndentationStyle::Indented,
                "indented_relative_to_receiver" => cops::layout::MultilineMethodCallIndentationStyle::IndentedRelativeToReceiver,
                _ => cops::layout::MultilineMethodCallIndentationStyle::Aligned,
            })
            .unwrap_or(cops::layout::MultilineMethodCallIndentationStyle::Aligned);
        let width = cop_config
            .and_then(|c| c.raw.get("IndentationWidth"))
            .and_then(|v| v.as_i64())
            .map(|v| v as usize);
        result.push(Box::new(cops::layout::MultilineMethodCallIndentation::new(style, width)));
    }

    // Layout/MultilineOperationIndentation
    if config.is_cop_enabled("Layout/MultilineOperationIndentation") {
        let cop_config = config.get_cop_config("Layout/MultilineOperationIndentation");
        let style = cop_config
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "indented" => cops::layout::MultilineOperationIndentationStyle::Indented,
                _ => cops::layout::MultilineOperationIndentationStyle::Aligned,
            })
            .unwrap_or(cops::layout::MultilineOperationIndentationStyle::Aligned);
        let width = cop_config
            .and_then(|c| c.raw.get("IndentationWidth"))
            .and_then(|v| v.as_i64())
            .map(|v| v as usize);
        result.push(Box::new(cops::layout::MultilineOperationIndentation::new(style, width)));
    }

    // Layout/SpaceInsideArrayLiteralBrackets
    if config.is_cop_enabled("Layout/SpaceInsideArrayLiteralBrackets") {
        if let Some(cop) = build_single_cop("Layout/SpaceInsideArrayLiteralBrackets", config) {
            result.push(cop);
        }
    }

    // Layout/SpaceInsideArrayPercentLiteral
    if config.is_cop_enabled("Layout/SpaceInsideArrayPercentLiteral") {
        result.push(Box::new(cops::layout::SpaceInsideArrayPercentLiteral::new()));
    }

    // Layout/SpaceInsideBlockBraces
    if config.is_cop_enabled("Layout/SpaceInsideBlockBraces") {
        if let Some(cop) = build_single_cop("Layout/SpaceInsideBlockBraces", config) {
            result.push(cop);
        }
    }

    // Layout/SpaceInsideHashLiteralBraces
    if config.is_cop_enabled("Layout/SpaceInsideHashLiteralBraces") {
        if let Some(cop) = build_single_cop("Layout/SpaceInsideHashLiteralBraces", config) {
            result.push(cop);
        }
    }

    // Layout/SpaceInsidePercentLiteralDelimiters
    if config.is_cop_enabled("Layout/SpaceInsidePercentLiteralDelimiters") {
        result.push(Box::new(cops::layout::SpaceInsidePercentLiteralDelimiters::new()));
    }

    // Layout/SpaceInsideReferenceBrackets
    if config.is_cop_enabled("Layout/SpaceInsideReferenceBrackets") {
        if let Some(cop) = build_single_cop("Layout/SpaceInsideReferenceBrackets", config) {
            result.push(cop);
        }
    }

    // Style/FrozenStringLiteralComment
    if config.is_cop_enabled("Style/FrozenStringLiteralComment") {
        let cop_config = config.get_cop_config("Style/FrozenStringLiteralComment");
        let style = cop_config
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "never" => cops::style::FrozenStringLiteralCommentStyle::Never,
                "always_true" => cops::style::FrozenStringLiteralCommentStyle::AlwaysTrue,
                _ => cops::style::FrozenStringLiteralCommentStyle::Always,
            })
            .unwrap_or(cops::style::FrozenStringLiteralCommentStyle::Always);
        result.push(Box::new(cops::style::FrozenStringLiteralComment::new(
            style,
        )));
    }

    // Style/NumericPredicate
    if config.is_cop_enabled("Style/NumericPredicate") {
        let cop_config = config.get_cop_config("Style/NumericPredicate");
        let style = match cop_config.and_then(|c| c.raw.get("EnforcedStyle")).and_then(|v| v.as_str()) {
            Some("comparison") => cops::style::NumericPredicateStyle::Comparison,
            _ => cops::style::NumericPredicateStyle::Predicate,
        };
        let allowed_methods = cop_config
            .and_then(|c| c.raw.get("AllowedMethods"))
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let allowed_patterns = cop_config
            .and_then(|c| c.raw.get("AllowedPatterns"))
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        result.push(Box::new(cops::style::NumericPredicate::with_config(style, allowed_methods, allowed_patterns)));
    }

    // Style/DoubleNegation
    if config.is_cop_enabled("Style/DoubleNegation") {
        let cop_config = config.get_cop_config("Style/DoubleNegation");
        let style = match cop_config.and_then(|c| c.raw.get("EnforcedStyle")).and_then(|v| v.as_str()) {
            Some("forbidden") => cops::style::DoubleNegationStyle::Forbidden,
            _ => cops::style::DoubleNegationStyle::AllowedInReturns,
        };
        result.push(Box::new(cops::style::DoubleNegation::new(style)));
    }

    // Style/WordArray
    if config.is_cop_enabled("Style/WordArray") {
        let cop_config = config.get_cop_config("Style/WordArray");
        let style = match cop_config.and_then(|c| c.raw.get("EnforcedStyle")).and_then(|v| v.as_str()) {
            Some("brackets") => cops::style::WordArrayStyle::Brackets,
            _ => cops::style::WordArrayStyle::Percent,
        };
        let min_size = cop_config
            .and_then(|c| c.raw.get("MinSize"))
            .and_then(|v| v.as_u64())
            .unwrap_or(2) as usize;
        let word_regex = cop_config
            .and_then(|c| c.raw.get("WordRegex"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| r"\A(?:\w|\w-\w|\n|\t)+\z".into());
        let word_regex = normalize_ruby_regex(&word_regex);
        result.push(Box::new(cops::style::WordArray::with_config(style, min_size, word_regex)));
    }

    // Style/Semicolon
    if config.is_cop_enabled("Style/Semicolon") {
        let cop_config = config.get_cop_config("Style/Semicolon");
        let allow = cop_config
            .and_then(|c| c.raw.get("AllowAsExpressionSeparator"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        result.push(Box::new(cops::style::Semicolon::new(allow)));
    }

    // Style/StringLiterals
    if config.is_cop_enabled("Style/StringLiterals") {
        let cop_config = config.get_cop_config("Style/StringLiterals");
        let style = cop_config
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "double_quotes" => cops::style::StringLiteralsStyle::DoubleQuotes,
                _ => cops::style::StringLiteralsStyle::SingleQuotes,
            })
            .unwrap_or(cops::style::StringLiteralsStyle::SingleQuotes);
        let consistent = cop_config
            .and_then(|c| c.raw.get("ConsistentQuotesInMultiline"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        result.push(Box::new(cops::style::StringLiterals::with_config(
            style, consistent,
        )));
    }

    // Style/NumericLiterals
    if config.is_cop_enabled("Style/NumericLiterals") {
        let cop_config = config.get_cop_config("Style/NumericLiterals");
        let min_digits = cop_config
            .and_then(|c| c.raw.get("MinDigits"))
            .and_then(|v| v.as_u64())
            .unwrap_or(6) as usize;
        let strict = cop_config
            .and_then(|c| c.raw.get("Strict"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let allowed_numbers = cop_config
            .and_then(|c| c.raw.get("AllowedNumbers"))
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                    seq.iter()
                        .filter_map(|v| {
                            v.as_i64().or_else(|| {
                                v.as_str().and_then(|s| s.parse::<i64>().ok())
                            })
                        })
                        .collect()
                })
            .unwrap_or_default();
        let allowed_patterns = cop_config
            .and_then(|c| c.raw.get("AllowedPatterns"))
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        result.push(Box::new(cops::style::NumericLiterals::with_config(
            min_digits,
            strict,
            allowed_numbers,
            allowed_patterns,
        )));
    }

    // Metrics/MethodLength
    if config.is_cop_enabled("Metrics/MethodLength") {
        let cop_config = config.get_cop_config("Metrics/MethodLength");
        let max = cop_config.and_then(|c| c.max).unwrap_or(10);
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
        result.push(Box::new(cops::metrics::MethodLength::with_config(
            max,
            count_comments,
            count_as_one,
            allowed_methods,
            allowed_patterns,
        )));
    }

    // Metrics/ClassLength
    if config.is_cop_enabled("Metrics/ClassLength") {
        let cop_config = config.get_cop_config("Metrics/ClassLength");
        let max = cop_config.and_then(|c| c.max).unwrap_or(100);
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
        result.push(Box::new(cops::metrics::ClassLength::with_config(
            max,
            count_comments,
            count_as_one,
        )));
    }

    // Naming/FileName
    if config.is_cop_enabled("Naming/FileName") {
        let cop_config = config.get_cop_config("Naming/FileName");
        let ignore_executable_scripts = cop_config
            .and_then(|c| c.raw.get("IgnoreExecutableScripts"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let expect_matching_definition = cop_config
            .and_then(|c| c.raw.get("ExpectMatchingDefinition"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let check_definition_path_hierarchy = cop_config
            .and_then(|c| c.raw.get("CheckDefinitionPathHierarchy"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let check_definition_path_hierarchy_roots = cop_config
            .and_then(|c| c.raw.get("CheckDefinitionPathHierarchyRoots"))
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_else(|| vec!["lib".into(), "spec".into(), "test".into(), "src".into()]);
        let regex = cop_config
            .and_then(|c| c.raw.get("Regex"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from);
        let allowed_acronyms = cop_config
            .and_then(|c| c.raw.get("AllowedAcronyms"))
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let include_patterns = cop_config
            .and_then(|c| c.raw.get("Include"))
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .or_else(|| config.all_cops_include())
            .unwrap_or_default();
        result.push(Box::new(cops::naming::FileName::with_full_config(
            ignore_executable_scripts,
            expect_matching_definition,
            check_definition_path_hierarchy,
            check_definition_path_hierarchy_roots,
            regex,
            allowed_acronyms,
            include_patterns,
        )));
    }

    // Naming/MemoizedInstanceVariableName
    if config.is_cop_enabled("Naming/MemoizedInstanceVariableName") {
        let cop_config = config.get_cop_config("Naming/MemoizedInstanceVariableName");
        let style = cop_config
            .and_then(|c| c.raw.get("EnforcedStyleForLeadingUnderscores"))
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "required" => cops::naming::LeadingUnderscoreStyle::Required,
                "optional" => cops::naming::LeadingUnderscoreStyle::Optional,
                _ => cops::naming::LeadingUnderscoreStyle::Disallowed,
            })
            .unwrap_or(cops::naming::LeadingUnderscoreStyle::Disallowed);
        result.push(Box::new(cops::naming::MemoizedInstanceVariableName::with_style(style)));
    }

    // Naming/MethodName
    if config.is_cop_enabled("Naming/MethodName") {
        let cop_config = config.get_cop_config("Naming/MethodName");
        let style = cop_config
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "camelCase" => cops::naming::MethodNameStyle::CamelCase,
                _ => cops::naming::MethodNameStyle::SnakeCase,
            })
            .unwrap_or(cops::naming::MethodNameStyle::SnakeCase);
        let allowed_patterns = cop_config
            .and_then(|c| c.raw.get("AllowedPatterns"))
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let forbidden_identifiers = cop_config
            .and_then(|c| c.raw.get("ForbiddenIdentifiers"))
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_else(|| vec!["__id__".to_string(), "__send__".to_string()]);
        let forbidden_patterns = cop_config
            .and_then(|c| c.raw.get("ForbiddenPatterns"))
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        result.push(Box::new(cops::naming::MethodName::with_config(
            style, allowed_patterns, forbidden_identifiers, forbidden_patterns,
        )));
    }

    // Naming/PredicateMethod
    if config.is_cop_enabled("Naming/PredicateMethod") {
        let cop_config = config.get_cop_config("Naming/PredicateMethod");
        let mode = cop_config
            .and_then(|c| c.raw.get("Mode"))
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "aggressive" => cops::naming::PredicateMethodMode::Aggressive,
                _ => cops::naming::PredicateMethodMode::Conservative,
            })
            .unwrap_or(cops::naming::PredicateMethodMode::Conservative);
        let allow_bang = cop_config
            .and_then(|c| c.raw.get("AllowBangMethods"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let allowed_methods = cop_config
            .and_then(|c| c.raw.get("AllowedMethods"))
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let allowed_patterns = cop_config
            .and_then(|c| c.raw.get("AllowedPatterns"))
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let wayward_predicates = cop_config
            .and_then(|c| c.raw.get("WaywardPredicates"))
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        result.push(Box::new(cops::naming::PredicateMethod::with_config(
            mode,
            allow_bang,
            allowed_methods,
            allowed_patterns,
            wayward_predicates,
        )));
    }

    // Naming/VariableName
    if config.is_cop_enabled("Naming/VariableName") {
        let cop_config = config.get_cop_config("Naming/VariableName");
        let style = cop_config
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "camelCase" => cops::naming::VariableNameStyle::CamelCase,
                _ => cops::naming::VariableNameStyle::SnakeCase,
            })
            .unwrap_or(cops::naming::VariableNameStyle::SnakeCase);
        let allowed_identifiers = cop_config
            .and_then(|c| c.raw.get("AllowedIdentifiers"))
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let allowed_patterns = cop_config
            .and_then(|c| c.raw.get("AllowedPatterns"))
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let forbidden_identifiers = cop_config
            .and_then(|c| c.raw.get("ForbiddenIdentifiers"))
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let forbidden_patterns = cop_config
            .and_then(|c| c.raw.get("ForbiddenPatterns"))
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        result.push(Box::new(cops::naming::VariableName::with_config(
            style, allowed_identifiers, allowed_patterns, forbidden_identifiers, forbidden_patterns,
        )));
    }

    // Naming/VariableNumber
    if config.is_cop_enabled("Naming/VariableNumber") {
        let cop_config = config.get_cop_config("Naming/VariableNumber");
        let style = cop_config
            .and_then(|c| c.enforced_style.as_ref())
            .map(|s| match s.as_str() {
                "snake_case" => cops::naming::VariableNumberStyle::SnakeCase,
                "non_integer" => cops::naming::VariableNumberStyle::NonInteger,
                _ => cops::naming::VariableNumberStyle::NormalCase,
            })
            .unwrap_or(cops::naming::VariableNumberStyle::NormalCase);
        let check_method_names = cop_config
            .and_then(|c| c.raw.get("CheckMethodNames"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let check_symbols = cop_config
            .and_then(|c| c.raw.get("CheckSymbols"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let allowed_identifiers = cop_config
            .and_then(|c| c.raw.get("AllowedIdentifiers"))
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let allowed_patterns = cop_config
            .and_then(|c| c.raw.get("AllowedPatterns"))
            .and_then(|v| v.as_sequence())
            .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        result.push(Box::new(cops::naming::VariableNumber::with_config(
            style, check_method_names, check_symbols, allowed_identifiers, allowed_patterns,
        )));
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
        "Lint/Debugger" => {
            let cop_config = config.get_cop_config("Lint/Debugger");
            let methods = cop_config
                .and_then(|c| c.raw.get("DebuggerMethods"))
                .and_then(parse_debugger_list)
                .unwrap_or_else(cops::lint::Debugger::default_methods);
            let requires = cop_config
                .and_then(|c| c.raw.get("DebuggerRequires"))
                .and_then(parse_debugger_list)
                .unwrap_or_else(cops::lint::Debugger::default_requires);
            Some(Box::new(cops::lint::Debugger::with_config(
                methods, requires,
            )))
        }

        "Lint/DuplicateMethods" => {
            let cop_config = config.get_cop_config("Lint/DuplicateMethods");
            let active_support = cop_config
                .and_then(|c| c.raw.get("ActiveSupportExtensionsEnabled"))
                .and_then(|v| v.as_bool())
                .or_else(|| cop_config
                    .and_then(|c| c.raw.get("AllCopsActiveSupportExtensionsEnabled"))
                    .and_then(|v| v.as_bool()))
                .unwrap_or(false);
            Some(Box::new(cops::lint::DuplicateMethods::with_config(active_support)))
        }

        "Lint/LiteralAsCondition" => {
            Some(Box::new(cops::lint::LiteralAsCondition::new()))
        }

        "Lint/EmptyConditionalBody" => {
            let cop_config = config.get_cop_config("Lint/EmptyConditionalBody");
            let allow_comments = cop_config
                .and_then(|c| c.raw.get("AllowComments"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            Some(Box::new(cops::lint::EmptyConditionalBody::new(allow_comments)))
        }

        "Lint/FormatParameterMismatch" => {
            Some(Box::new(cops::lint::FormatParameterMismatch::new()))
        }

        "Lint/LiteralInInterpolation" => {
            Some(Box::new(cops::lint::LiteralInInterpolation::new()))
        }

        "Lint/OutOfRangeRegexpRef" => {
            Some(Box::new(cops::lint::OutOfRangeRegexpRef::new()))
        }

        "Lint/RedundantSafeNavigation" => {
            let cop_config = config.get_cop_config("Lint/RedundantSafeNavigation");
            let allowed_methods = cop_config
                .and_then(|c| c.raw.get("AllowedMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_else(|| vec!["respond_to?".to_string()]);
            let infer = cop_config
                .and_then(|c| c.raw.get("InferNonNilReceiver"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let additional = cop_config
                .and_then(|c| c.raw.get("AdditionalNilMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            Some(Box::new(cops::lint::RedundantSafeNavigation::with_config(
                allowed_methods, infer, additional,
            )))
        }

        "Lint/RedundantSplatExpansion" => {
            let cop_config = config.get_cop_config("Lint/RedundantSplatExpansion");
            let allow_percent = cop_config
                .and_then(|c| c.raw.get("AllowPercentLiteralArrayArgument"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            Some(Box::new(cops::lint::RedundantSplatExpansion::new(allow_percent)))
        }

        "Lint/RedundantTypeConversion" => {
            Some(Box::new(cops::lint::RedundantTypeConversion::new()))
        }

        "Lint/RescueType" => {
            Some(Box::new(cops::lint::RescueType::new()))
        }

        "Lint/SafeNavigationChain" => {
            let cop_config = config.get_cop_config("Lint/SafeNavigationChain");
            let allowed = cop_config
                .and_then(|c| c.raw.get("AllowedMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            Some(Box::new(cops::lint::SafeNavigationChain::with_allowed_methods(allowed)))
        }

        "Lint/SafeNavigationConsistency" => {
            let cop_config = config.get_cop_config("Lint/SafeNavigationConsistency");
            let allowed = cop_config
                .and_then(|c| c.raw.get("AllowedMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_else(|| vec!["present?".into(), "blank?".into(), "try".into(), "presence".into()]);
            Some(Box::new(cops::lint::SafeNavigationConsistency::with_config(allowed)))
        }

        "Lint/SelfAssignment" => {
            let cop_config = config.get_cop_config("Lint/SelfAssignment");
            let allow_rbs = cop_config
                .and_then(|c| c.raw.get("AllowRBSInlineAnnotation"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Some(Box::new(cops::lint::LintSelfAssignment::new(allow_rbs)))
        }

        "Lint/ShadowedArgument" => {
            let cop_config = config.get_cop_config("Lint/ShadowedArgument");
            let ignore_implicit = cop_config
                .and_then(|c| c.raw.get("IgnoreImplicitReferences"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Some(Box::new(cops::lint::ShadowedArgument::with_config(ignore_implicit)))
        }

        "Lint/UnreachableCode" => {
            Some(Box::new(cops::lint::UnreachableCode::new()))
        }

        "Lint/UnusedBlockArgument" => {
            let cop_config = config.get_cop_config("Lint/UnusedBlockArgument");
            let allow_keyword = cop_config
                .and_then(|c| c.raw.get("AllowUnusedKeywordArguments"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let ignore_empty = cop_config
                .and_then(|c| c.raw.get("IgnoreEmptyBlocks"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            Some(Box::new(cops::lint::UnusedBlockArgument::with_config(
                allow_keyword, ignore_empty,
            )))
        }

        "Lint/UnusedMethodArgument" => {
            let cop_config = config.get_cop_config("Lint/UnusedMethodArgument");
            let allow_keyword = cop_config
                .and_then(|c| c.raw.get("AllowUnusedKeywordArguments"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let ignore_empty = cop_config
                .and_then(|c| c.raw.get("IgnoreEmptyMethods"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let ignore_not_impl = cop_config
                .and_then(|c| c.raw.get("IgnoreNotImplementedMethods"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let exceptions = cop_config
                .and_then(|c| c.raw.get("NotImplementedExceptions"))
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_else(|| vec!["NotImplementedError".to_string()]);
            Some(Box::new(cops::lint::UnusedMethodArgument::with_config(
                allow_keyword, ignore_empty, ignore_not_impl, exceptions,
            )))
        }

        "Lint/UselessAccessModifier" => {
            let cop_config = config.get_cop_config("Lint/UselessAccessModifier");
            let active_support = cop_config
                .and_then(|c| c.raw.get("AllCopsActiveSupportExtensionsEnabled"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mut context_creating: Vec<String> = cop_config
                .and_then(|c| c.raw.get("ContextCreatingMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            // When ActiveSupport is enabled, 'included' is a context-creating method
            if active_support && !context_creating.contains(&"included".to_string()) {
                context_creating.push("included".to_string());
            }
            let method_creating = cop_config
                .and_then(|c| c.raw.get("MethodCreatingMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            Some(Box::new(cops::lint::UselessAccessModifier::with_config(
                context_creating,
                method_creating,
            )))
        }

        "Lint/UselessAssignment" => {
            Some(Box::new(cops::lint::UselessAssignment::new()))
        }

        "Lint/Void" => {
            let cop_config = config.get_cop_config("Lint/Void");
            let check_methods = cop_config
                .and_then(|c| c.raw.get("CheckForMethodsWithNoSideEffects"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Some(Box::new(cops::lint::Void::new(check_methods)))
        }

        "Lint/AssignmentInCondition" => {
            let allow_safe = config
                .get_cop_config("Lint/AssignmentInCondition")
                .and_then(|c| c.allow_safe_assignment)
                .unwrap_or(true);
            Some(Box::new(cops::lint::AssignmentInCondition::new(allow_safe)))
        }

        "Lint/AmbiguousBlockAssociation" => {
            let cop_config = config.get_cop_config("Lint/AmbiguousBlockAssociation");
            let allowed_methods = cop_config
                .and_then(|c| c.raw.get("AllowedMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            let allowed_patterns = cop_config
                .and_then(|c| c.raw.get("AllowedPatterns"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            Some(Box::new(cops::lint::AmbiguousBlockAssociation::with_config(
                allowed_methods, allowed_patterns,
            )))
        }

        "Lint/NestedMethodDefinition" => {
            let cop_config = config.get_cop_config("Lint/NestedMethodDefinition");
            let allowed_methods = cop_config
                .and_then(|c| c.raw.get("AllowedMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            let allowed_patterns = cop_config
                .and_then(|c| c.raw.get("AllowedPatterns"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            Some(Box::new(cops::lint::NestedMethodDefinition::with_config(
                allowed_methods, allowed_patterns,
            )))
        }

        "Lint/ShadowedException" => {
            Some(Box::new(cops::lint::ShadowedException::new()))
        }

        "Layout/LineLength" => {
            let cop_config = config.get_cop_config("Layout/LineLength");
            let max = cop_config.and_then(|c| c.max).unwrap_or(120);
            let allow_uri = cop_config
                .and_then(|c| c.raw.get("AllowURI"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            // AllowHeredoc can be bool or array of delimiter strings
            let allow_heredoc = cop_config
                .and_then(|c| c.raw.get("AllowHeredoc"))
                .map(|v| {
                    if let Some(b) = v.as_bool() {
                        if b {
                            cops::layout::AllowHeredoc::All
                        } else {
                            cops::layout::AllowHeredoc::Disabled
                        }
                    } else if let Some(seq) = v.as_sequence() {
                        let delimiters: Vec<String> = seq
                            .iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect();
                        cops::layout::AllowHeredoc::Specific(delimiters)
                    } else {
                        cops::layout::AllowHeredoc::Disabled
                    }
                })
                .unwrap_or(cops::layout::AllowHeredoc::Disabled);
            let allow_qualified_name = cop_config
                .and_then(|c| c.raw.get("AllowQualifiedName"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let allow_cop_directives = cop_config
                .and_then(|c| c.raw.get("AllowCopDirectives"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let allow_rbs_inline_annotation = cop_config
                .and_then(|c| c.raw.get("AllowRBSInlineAnnotation"))
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
            let split_strings = cop_config
                .and_then(|c| c.raw.get("SplitStrings"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Some(Box::new(cops::layout::LineLength::with_config(
                max,
                allow_uri,
                allow_heredoc,
                allow_qualified_name,
                allow_cop_directives,
                allow_rbs_inline_annotation,
                uri_schemes,
                allowed_patterns,
                tab_width,
                split_strings,
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

        "Style/AccessModifierDeclarations" => {
            let cop_config = config.get_cop_config("Style/AccessModifierDeclarations");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "inline" => cops::style::AccessModifierDeclarationsStyle::Inline,
                    _ => cops::style::AccessModifierDeclarationsStyle::Group,
                })
                .unwrap_or(cops::style::AccessModifierDeclarationsStyle::Group);
            let allow_symbols = cop_config
                .and_then(|c| c.raw.get("AllowModifiersOnSymbols"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let allow_attrs = cop_config
                .and_then(|c| c.raw.get("AllowModifiersOnAttrs"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let allow_alias = cop_config
                .and_then(|c| c.raw.get("AllowModifiersOnAliasMethod"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            Some(Box::new(cops::style::AccessModifierDeclarations::with_config(
                style,
                allow_symbols,
                allow_attrs,
                allow_alias,
            )))
        }

        "Style/AndOr" => {
            let cop_config = config.get_cop_config("Style/AndOr");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "always" => cops::style::AndOrStyle::Always,
                    _ => cops::style::AndOrStyle::Conditionals,
                })
                .unwrap_or(cops::style::AndOrStyle::Conditionals);
            Some(Box::new(cops::style::AndOr::new(style)))
        }

        "Style/ArrayIntersect" => {
            let cop_config = config.get_cop_config("Style/ArrayIntersect");
            let active_support = cop_config
                .and_then(|c| c.raw.get("ActiveSupportExtensionsEnabled"))
                .and_then(|v| v.as_bool())
                .or_else(|| cop_config
                    .and_then(|c| c.raw.get("AllCopsActiveSupportExtensionsEnabled"))
                    .and_then(|v| v.as_bool()))
                .unwrap_or(false);
            Some(Box::new(cops::style::ArrayIntersect::with_config(active_support)))
        }

        "Style/AutoResourceCleanup" => Some(Box::new(cops::style::AutoResourceCleanup::new())),

        "Style/CommentedKeyword" => {
            Some(Box::new(cops::style::CommentedKeyword::new()))
        }

        "Style/BlockDelimiters" => {
            let cop_config = config.get_cop_config("Style/BlockDelimiters");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "semantic" => cops::style::BlockDelimitersStyle::Semantic,
                    "braces_for_chaining" => cops::style::BlockDelimitersStyle::BracesForChaining,
                    "always_braces" => cops::style::BlockDelimitersStyle::AlwaysBraces,
                    _ => cops::style::BlockDelimitersStyle::LineCountBased,
                })
                .unwrap_or(cops::style::BlockDelimitersStyle::LineCountBased);

            let allow_braces = cop_config
                .and_then(|c| c.raw.get("AllowBracesOnProceduralOneLiners"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let braces_required = cop_config
                .and_then(|c| c.raw.get("BracesRequiredMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();

            let functional = cop_config
                .and_then(|c| c.raw.get("FunctionalMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();

            let procedural = cop_config
                .and_then(|c| c.raw.get("ProceduralMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();

            let allowed_methods = cop_config
                .and_then(|c| c.raw.get("AllowedMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_else(|| vec!["lambda".to_string(), "proc".to_string(), "it".to_string()]);

            let allowed_patterns = cop_config
                .and_then(|c| c.raw.get("AllowedPatterns"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();

            Some(Box::new(cops::style::BlockDelimiters::with_config(
                style, allow_braces, braces_required, functional, procedural, allowed_methods, allowed_patterns,
            )))
        }

        "Style/EmptyElse" => {
            let cop_config = config.get_cop_config("Style/EmptyElse");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "empty" => cops::style::EmptyElseStyle::Empty,
                    "nil" => cops::style::EmptyElseStyle::Nil,
                    _ => cops::style::EmptyElseStyle::Both,
                })
                .unwrap_or(cops::style::EmptyElseStyle::Both);
            let allow_comments = cop_config
                .and_then(|c| c.raw.get("AllowComments"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Some(Box::new(cops::style::EmptyElse::new(style, allow_comments)))
        }

        "Style/Documentation" => {
            let cop_config = config.get_cop_config("Style/Documentation");
            let allowed: Vec<String> = cop_config
                .and_then(|c| c.raw.get("AllowedConstants"))
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            Some(Box::new(cops::style::Documentation::with_allowed_constants(allowed)))
        }

        "Style/EmptyLiteral" => {
            // Cross-cop: check Style/StringLiterals EnforcedStyle for quote preference
            let prefer_double = config.get_cop_config("Style/EmptyLiteral")
                .and_then(|c| c.enforced_style.as_ref())
                .or_else(|| config.get_cop_config("Style/StringLiterals")
                    .and_then(|c| c.enforced_style.as_ref()))
                .map(|s| s == "double_quotes")
                .unwrap_or(false);
            // Cross-cop: check if Style/FrozenStringLiteralComment is enabled
            let frozen_cop_enabled = config.get_cop_config("Style/FrozenStringLiteralComment")
                .and_then(|c| c.enabled)
                .unwrap_or(false);
            Some(Box::new(cops::style::EmptyLiteral::with_full_config(prefer_double, frozen_cop_enabled)))
        }

        "Style/ConditionalAssignment" => {
            let cop_config = config.get_cop_config("Style/ConditionalAssignment");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "assign_to_condition" => cops::style::ConditionalAssignmentStyle::AssignToCondition,
                    _ => cops::style::ConditionalAssignmentStyle::AssignInsideCondition,
                })
                .unwrap_or(cops::style::ConditionalAssignmentStyle::AssignInsideCondition);
            let include_ternary = cop_config
                .and_then(|c| c.raw.get("IncludeTernaryExpressions"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let single_line_only = cop_config
                .and_then(|c| c.raw.get("SingleLineConditionsOnly"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            Some(Box::new(cops::style::ConditionalAssignment::with_config(
                style,
                include_ternary,
                single_line_only,
            )))
        }

        "Style/FormatStringToken" => {
            let cop_config = config.get_cop_config("Style/FormatStringToken");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "template" => cops::style::FormatStringTokenStyle::Template,
                    "unannotated" => cops::style::FormatStringTokenStyle::Unannotated,
                    _ => cops::style::FormatStringTokenStyle::Annotated,
                })
                .unwrap_or(cops::style::FormatStringTokenStyle::Annotated);
            let max_unannotated = cop_config
                .and_then(|c| c.raw.get("MaxUnannotatedPlaceholdersAllowed"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            let conservative = cop_config
                .and_then(|c| c.raw.get("Mode"))
                .and_then(|v| v.as_str())
                .map(|s| s == "conservative")
                .unwrap_or(false);
            let allowed_methods = cop_config
                .and_then(|c| c.raw.get("AllowedMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let allowed_patterns = cop_config
                .and_then(|c| c.raw.get("AllowedPatterns"))
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            Some(Box::new(cops::style::FormatStringToken::with_config(
                style,
                max_unannotated,
                conservative,
                allowed_methods,
                allowed_patterns,
            )))
        }

        "Style/GlobalVars" => {
            let cop_config = config.get_cop_config("Style/GlobalVars");
            let allowed: Vec<String> = cop_config
                .and_then(|c| c.raw.get("AllowedVariables"))
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            Some(Box::new(cops::style::GlobalVars::with_allowed_variables(allowed)))
        }

        "Style/GuardClause" => {
            let cop_config = config.get_cop_config("Style/GuardClause");
            let min_body_length = cop_config
                .and_then(|c| c.raw.get("MinBodyLength"))
                .and_then(|v| v.as_i64())
                .unwrap_or(1);
            let allow_consecutive = cop_config
                .and_then(|c| c.raw.get("AllowConsecutiveConditionals"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            // Only enforce too-long-for-single-line when Layout/LineLength is enabled.
            // The test fixtures set Max=80 but Enabled=false in some cases — reading Max
            // unconditionally would falsely trigger the multi-statement form.
            let max_line_length = if config.is_cop_enabled("Layout/LineLength") {
                config.get_cop_config("Layout/LineLength")
                    .and_then(|c| c.max)
                    .map(|m| m as usize)
            } else {
                None
            };
            Some(Box::new(cops::style::GuardClause::with_config(
                min_body_length, allow_consecutive, max_line_length,
            )))
        }

        "Style/IfUnlessModifier" => {
            let ll_config = config.get_cop_config("Layout/LineLength");
            let ll_enabled = config.is_cop_enabled("Layout/LineLength");
            let max_ll = ll_config.and_then(|c| c.max).unwrap_or(80) as usize;
            let allow_uri = ll_config
                .and_then(|c| c.raw.get("AllowURI"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let allow_cop_directives = ll_config
                .and_then(|c| c.raw.get("AllowCopDirectives"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let tab_width = config.get_cop_config("Layout/IndentationStyle")
                .and_then(|c| c.raw.get("IndentationWidth"))
                .and_then(|v| v.as_i64())
                .or_else(|| config.get_cop_config("Layout/IndentationWidth")
                    .and_then(|c| c.raw.get("Width"))
                    .and_then(|v| v.as_i64()))
                .map(|v| v as usize);
            Some(Box::new(cops::style::IfUnlessModifier::with_config(
                max_ll, ll_enabled, allow_uri, allow_cop_directives, tab_width,
            )))
        }

        "Style/InverseMethods" => {
            let cop_config = config.get_cop_config("Style/InverseMethods");
            let inverse_methods = cop_config
                .and_then(|c| c.raw.get("InverseMethods"))
                .and_then(|v| v.as_mapping())
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| {
                            Some((k.as_str()?.to_string(), v.as_str()?.to_string()))
                        })
                        .collect::<std::collections::HashMap<_, _>>()
                });
            let inverse_blocks = cop_config
                .and_then(|c| c.raw.get("InverseBlocks"))
                .and_then(|v| v.as_mapping())
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| {
                            Some((k.as_str()?.to_string(), v.as_str()?.to_string()))
                        })
                        .collect::<std::collections::HashMap<_, _>>()
                });
            match (inverse_methods, inverse_blocks) {
                (Some(im), Some(ib)) => Some(Box::new(cops::style::InverseMethods::with_config(im, ib))),
                (Some(im), None) => Some(Box::new(cops::style::InverseMethods::with_config(
                    im,
                    std::collections::HashMap::new(),
                ))),
                _ => Some(Box::new(cops::style::InverseMethods::new())),
            }
        }

        "Style/HashSyntax" => {
            let cop_config = config.get_cop_config("Style/HashSyntax");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "hash_rockets" => cops::style::HashSyntaxStyle::HashRockets,
                    "no_mixed_keys" => cops::style::HashSyntaxStyle::NoMixedKeys,
                    "ruby19_no_mixed_keys" => cops::style::HashSyntaxStyle::Ruby19NoMixedKeys,
                    _ => cops::style::HashSyntaxStyle::Ruby19,
                })
                .unwrap_or(cops::style::HashSyntaxStyle::Ruby19);
            let shorthand = cop_config
                .and_then(|c| c.raw.get("EnforcedShorthandSyntax"))
                .and_then(|v| v.as_str())
                .map(|s| match s {
                    "always" => cops::style::HashSyntaxShorthandStyle::Always,
                    "never" => cops::style::HashSyntaxShorthandStyle::Never,
                    "consistent" => cops::style::HashSyntaxShorthandStyle::Consistent,
                    "either_consistent" => cops::style::HashSyntaxShorthandStyle::EitherConsistent,
                    _ => cops::style::HashSyntaxShorthandStyle::Either,
                })
                .unwrap_or(cops::style::HashSyntaxShorthandStyle::Either);
            let use_rockets_with_symbols = cop_config
                .and_then(|c| c.raw.get("UseHashRocketsWithSymbolValues"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let prefer_rockets_non_alnum = cop_config
                .and_then(|c| c.raw.get("PreferHashRocketsForNonAlnumEndingSymbols"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            Some(Box::new(cops::style::HashSyntax::with_config(
                style,
                shorthand,
                use_rockets_with_symbols,
                prefer_rockets_non_alnum,
            )))
        }

        "Style/HashEachMethods" => {
            let cop_config = config.get_cop_config("Style/HashEachMethods");
            let allowed_receivers: Vec<String> = cop_config
                .and_then(|c| c.raw.get("AllowedReceivers"))
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            Some(Box::new(cops::style::HashEachMethods::with_config(allowed_receivers)))
        }

        "Style/IdenticalConditionalBranches" => {
            Some(Box::new(cops::style::IdenticalConditionalBranches::new()))
        }

        "Style/MethodCalledOnDoEndBlock" => {
            Some(Box::new(cops::style::MethodCalledOnDoEndBlock::new()))
        }

        "Style/MethodDefParentheses" => {
            let cop_config = config.get_cop_config("Style/MethodDefParentheses");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "require_no_parentheses" => cops::style::MethodDefParenthesesStyle::RequireNoParentheses,
                    "require_no_parentheses_except_multiline" => cops::style::MethodDefParenthesesStyle::RequireNoParenthesesExceptMultiline,
                    _ => cops::style::MethodDefParenthesesStyle::RequireParentheses,
                })
                .unwrap_or(cops::style::MethodDefParenthesesStyle::RequireParentheses);
            Some(Box::new(cops::style::MethodDefParentheses::new(style)))
        }

        "Style/MutableConstant" => {
            let cop_config = config.get_cop_config("Style/MutableConstant");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "strict" => cops::style::MutableConstantStyle::Strict,
                    _ => cops::style::MutableConstantStyle::Literals,
                })
                .unwrap_or(cops::style::MutableConstantStyle::Literals);
            Some(Box::new(cops::style::MutableConstant::new(style)))
        }

        "Style/NegativeArrayIndex" => Some(Box::new(cops::style::NegativeArrayIndex::new())),

        "Style/OneLineConditional" => {
            let cop_config = config.get_cop_config("Style/OneLineConditional");
            let always_multiline = cop_config
                .and_then(|c| c.raw.get("AlwaysCorrectToMultiline"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Some(Box::new(cops::style::OneLineConditional::with_config(always_multiline)))
        }

        "Style/Next" => {
            let cop_config = config.get_cop_config("Style/Next");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "always" => cops::style::NextStyle::Always,
                    _ => cops::style::NextStyle::SkipModifierIfs,
                })
                .unwrap_or(cops::style::NextStyle::SkipModifierIfs);
            let min_body_length = cop_config
                .and_then(|c| c.raw.get("MinBodyLength"))
                .and_then(|v| v.as_i64())
                .unwrap_or(1);
            let allow_consecutive = cop_config
                .and_then(|c| c.raw.get("AllowConsecutiveConditionals"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Some(Box::new(cops::style::Next::with_config(
                style, min_body_length, allow_consecutive,
            )))
        }

        "Style/RedundantCondition" => {
            let cop_config = config.get_cop_config("Style/RedundantCondition");
            let allowed_methods = cop_config
                .and_then(|c| c.raw.get("AllowedMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_else(|| vec!["infinite?".to_string(), "nonzero?".to_string()]);
            Some(Box::new(cops::style::RedundantCondition::with_config(allowed_methods)))
        }

        "Style/SymbolProc" => {
            let cop_config = config.get_cop_config("Style/SymbolProc");
            let allowed_methods = cop_config
                .and_then(|c| c.raw.get("AllowedMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_else(|| vec!["define_method".to_string()]);
            let allowed_patterns = cop_config
                .and_then(|c| c.raw.get("AllowedPatterns"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let allow_methods_with_arguments = cop_config
                .and_then(|c| c.raw.get("AllowMethodsWithArguments"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let allow_comments = cop_config
                .and_then(|c| c.raw.get("AllowComments"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let active_support = cop_config
                .and_then(|c| c.raw.get("ActiveSupportExtensionsEnabled"))
                .and_then(|v| v.as_bool())
                .or_else(|| cop_config
                    .and_then(|c| c.raw.get("AllCopsActiveSupportExtensionsEnabled"))
                    .and_then(|v| v.as_bool()))
                .unwrap_or(false);
            Some(Box::new(cops::style::SymbolProc::with_config(
                allowed_methods, allowed_patterns, allow_methods_with_arguments,
                allow_comments, active_support,
            )))
        }

        "Style/PercentLiteralDelimiters" => {
            let cop_config = config.get_cop_config("Style/PercentLiteralDelimiters");
            let preferred = cop_config
                .and_then(|c| c.raw.get("PreferredDelimiters"))
                .and_then(|v| v.as_mapping())
                .map(|m| {
                    let mut map = std::collections::HashMap::new();
                    for (k, v) in m.iter() {
                        if let (Some(key), Some(val)) = (k.as_str(), v.as_str()) {
                            map.insert(key.to_string(), val.to_string());
                        }
                    }
                    map
                })
                .unwrap_or_else(|| {
                    let mut m = std::collections::HashMap::new();
                    m.insert("default".to_string(), "()".to_string());
                    m
                });
            Some(Box::new(cops::style::PercentLiteralDelimiters::with_config(preferred)))
        }

        "Style/RedundantBegin" => Some(Box::new(cops::style::RedundantBegin::new())),

        "Style/RedundantReturn" => {
            let cop_config = config.get_cop_config("Style/RedundantReturn");
            let allow_multi = cop_config
                .and_then(|c| c.raw.get("AllowMultipleReturnValues"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Some(Box::new(cops::style::RedundantReturn::with_config(allow_multi)))
        }

        "Style/Lambda" => {
            let cop_config = config.get_cop_config("Style/Lambda");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "lambda" => cops::style::LambdaStyle::Lambda,
                    "literal" => cops::style::LambdaStyle::Literal,
                    _ => cops::style::LambdaStyle::LineCountDependent,
                })
                .unwrap_or(cops::style::LambdaStyle::LineCountDependent);
            Some(Box::new(cops::style::Lambda::with_style(style)))
        }

        "Style/TrivialAccessors" => {
            let cop_config = config.get_cop_config("Style/TrivialAccessors");
            let default_methods: Vec<String> = vec![
                "to_ary", "to_a", "to_c", "to_enum", "to_h", "to_hash", "to_i", "to_int", "to_io",
                "to_open", "to_path", "to_proc", "to_r", "to_regexp", "to_str", "to_s", "to_sym",
            ].into_iter().map(String::from).collect();
            let allowed_methods = cop_config
                .and_then(|c| c.raw.get("AllowedMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>())
                .unwrap_or(default_methods);
            let exact = cop_config.and_then(|c| c.raw.get("ExactNameMatch")).and_then(|v| v.as_bool()).unwrap_or(true);
            let allow_pred = cop_config.and_then(|c| c.raw.get("AllowPredicates")).and_then(|v| v.as_bool()).unwrap_or(true);
            let allow_dsl = cop_config.and_then(|c| c.raw.get("AllowDSLWriters")).and_then(|v| v.as_bool()).unwrap_or(true);
            let ignore_class = cop_config.and_then(|c| c.raw.get("IgnoreClassMethods")).and_then(|v| v.as_bool()).unwrap_or(false);
            Some(Box::new(cops::style::TrivialAccessors::with_config(
                allowed_methods, exact, allow_pred, allow_dsl, ignore_class,
            )))
        }

        "Style/CaseLikeIf" => {
            let cop_config = config.get_cop_config("Style/CaseLikeIf");
            let min_branches = cop_config
                .and_then(|c| c.raw.get("MinBranchesCount"))
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .unwrap_or(3);
            Some(Box::new(cops::style::CaseLikeIf::with_config(min_branches)))
        }

        "Style/SoleNestedConditional" => {
            let cop_config = config.get_cop_config("Style/SoleNestedConditional");
            let allow_modifier = cop_config
                .and_then(|c| c.raw.get("AllowModifier"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Some(Box::new(cops::style::SoleNestedConditional::with_config(allow_modifier)))
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

            Some(Box::new(
                cops::style::RaiseArgs::with_allowed_compact_types(style, allowed_compact_types),
            ))
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

        "Style/SafeNavigation" => {
            let cop_config = config.get_cop_config("Style/SafeNavigation");
            let allowed_methods = cop_config
                .and_then(|c| c.raw.get("AllowedMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_else(|| {
                    vec![
                        "present?".to_string(),
                        "blank?".to_string(),
                        "presence".to_string(),
                        "try".to_string(),
                        "try!".to_string(),
                    ]
                });
            let convert_nil = cop_config
                .and_then(|c| c.raw.get("ConvertCodeThatCanStartToReturnNil"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let max_chain = cop_config
                .and_then(|c| c.raw.get("MaxChainLength"))
                .and_then(|v| v.as_u64())
                .unwrap_or(2) as usize;
            // Read from cop's own config or from Lint/SafeNavigationChain cross-cop config
            let safe_nav_chain_enabled = cop_config
                .and_then(|c| c.raw.get("SafeNavigationChainEnabled"))
                .and_then(|v| v.as_bool())
                .or_else(|| config.get_cop_config("Lint/SafeNavigationChain")
                    .and_then(|c| c.enabled))
                .unwrap_or(true);
            Some(Box::new(cops::style::SafeNavigation::with_full_config(
                allowed_methods,
                convert_nil,
                max_chain,
                safe_nav_chain_enabled,
            )))
        }

        "Style/RedundantParentheses" => {
            let (ternary_req, allow_multiline) = read_redundant_parens_cross_cop_config(config);
            Some(Box::new(cops::style::RedundantParentheses::with_config(
                ternary_req,
                allow_multiline,
            )))
        }

        "Style/RedundantFreeze" => {
            // AllCops/StringLiteralsFrozenByDefault is extracted as a flat key
            // `AllCopsStringLiteralsFrozenByDefault` in cop test config.
            let frozen_by_default = config.get_cop_config("Style/RedundantFreeze")
                .and_then(|c| c.raw.get("AllCopsStringLiteralsFrozenByDefault"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Some(Box::new(cops::style::RedundantFreeze::with_config(frozen_by_default)))
        }
        "Style/RedundantSelf" => Some(Box::new(cops::style::RedundantSelf::new())),
        "Style/RedundantRegexpCharacterClass" => {
            Some(Box::new(cops::style::RedundantRegexpCharacterClass::new()))
        }

        "Style/RedundantRegexpEscape" => Some(Box::new(cops::style::RedundantRegexpEscape::new())),
        "Style/RedundantStringEscape" => Some(Box::new(cops::style::RedundantStringEscape::new())),

        "Style/Sample" => Some(Box::new(cops::style::Sample::new())),

        "Style/RedundantSort" => Some(Box::new(cops::style::RedundantSort::new())),

        "Style/HashTransformKeys" => Some(Box::new(cops::style::HashTransformKeys::new())),

        "Style/HashTransformValues" => Some(Box::new(cops::style::HashTransformValues::new())),

        "Style/SelectByRegexp" => Some(Box::new(cops::style::SelectByRegexp::new())),

        "Style/SelfAssignment" => Some(Box::new(cops::style::SelfAssignment::new())),

        "Style/StringMethods" => Some(Box::new(cops::style::StringMethods::new())),

        "Style/TernaryParentheses" => {
            let cop_config = config.get_cop_config("Style/TernaryParentheses");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "require_parentheses" => cops::style::TernaryParenthesesStyle::RequireParentheses,
                    "require_parentheses_when_complex" => cops::style::TernaryParenthesesStyle::RequireParenthesesWhenComplex,
                    _ => cops::style::TernaryParenthesesStyle::RequireNoParentheses,
                })
                .unwrap_or(cops::style::TernaryParenthesesStyle::RequireNoParentheses);
            let allow_safe = cop_config
                .and_then(|c| c.raw.get("AllowSafeAssignment"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            Some(Box::new(cops::style::TernaryParentheses::new(style, allow_safe)))
        }

        "Style/YodaCondition" => {
            let cop_config = config.get_cop_config("Style/YodaCondition");
            let style = match cop_config.and_then(|c| c.raw.get("EnforcedStyle")).and_then(|v| v.as_str()) {
                Some("forbid_for_equality_operators_only") => cops::style::YodaConditionStyle::ForbidForEqualityOperatorsOnly,
                Some("require_for_all_comparison_operators") => cops::style::YodaConditionStyle::RequireForAllComparisonOperators,
                Some("require_for_equality_operators_only") => cops::style::YodaConditionStyle::RequireForEqualityOperatorsOnly,
                _ => cops::style::YodaConditionStyle::ForbidForAllComparisonOperators,
            };
            Some(Box::new(cops::style::YodaCondition::new(style)))
        }
        "Style/ZeroLengthPredicate" => Some(Box::new(cops::style::ZeroLengthPredicate::new())),

        "Style/TrailingUnderscoreVariable" => {
            let cop_config = config.get_cop_config("Style/TrailingUnderscoreVariable");
            let allow_named = cop_config
                .and_then(|c| c.raw.get("AllowNamedUnderscoreVariables"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            Some(Box::new(cops::style::TrailingUnderscoreVariable::new(allow_named)))
        }

        "Style/TrailingCommaInArguments" => {
            let cop_config = config.get_cop_config("Style/TrailingCommaInArguments");
            let style = cop_config
                .and_then(|c| c.raw.get("EnforcedStyleForMultiline"))
                .and_then(|v| v.as_str())
                .map(|s| match s {
                    "comma" => cops::style::TrailingCommaInArgumentsStyle::Comma,
                    "consistent_comma" => cops::style::TrailingCommaInArgumentsStyle::ConsistentComma,
                    "diff_comma" => cops::style::TrailingCommaInArgumentsStyle::DiffComma,
                    _ => cops::style::TrailingCommaInArgumentsStyle::NoComma,
                })
                .unwrap_or(cops::style::TrailingCommaInArgumentsStyle::NoComma);
            Some(Box::new(cops::style::TrailingCommaInArguments::new(style)))
        }

        "Style/TrailingCommaInArrayLiteral" => {
            let cop_config = config.get_cop_config("Style/TrailingCommaInArrayLiteral");
            let style = cop_config
                .and_then(|c| c.raw.get("EnforcedStyleForMultiline"))
                .and_then(|v| v.as_str())
                .map(|s| match s {
                    "comma" => cops::style::TrailingCommaInArrayLiteralStyle::Comma,
                    "consistent_comma" => cops::style::TrailingCommaInArrayLiteralStyle::ConsistentComma,
                    "diff_comma" => cops::style::TrailingCommaInArrayLiteralStyle::DiffComma,
                    _ => cops::style::TrailingCommaInArrayLiteralStyle::NoComma,
                })
                .unwrap_or(cops::style::TrailingCommaInArrayLiteralStyle::NoComma);
            Some(Box::new(cops::style::TrailingCommaInArrayLiteral::new(style)))
        }

        "Style/TrailingCommaInHashLiteral" => {
            let cop_config = config.get_cop_config("Style/TrailingCommaInHashLiteral");
            let style = cop_config
                .and_then(|c| c.raw.get("EnforcedStyleForMultiline"))
                .and_then(|v| v.as_str())
                .map(|s| match s {
                    "comma" => cops::style::TrailingCommaInHashLiteralStyle::Comma,
                    "consistent_comma" => cops::style::TrailingCommaInHashLiteralStyle::ConsistentComma,
                    "diff_comma" => cops::style::TrailingCommaInHashLiteralStyle::DiffComma,
                    _ => cops::style::TrailingCommaInHashLiteralStyle::NoComma,
                })
                .unwrap_or(cops::style::TrailingCommaInHashLiteralStyle::NoComma);
            Some(Box::new(cops::style::TrailingCommaInHashLiteral::new(style)))
        }

        "Layout/TrailingWhitespace" => {
            let cop_config = config.get_cop_config("Layout/TrailingWhitespace");
            let allow_in_heredoc = cop_config
                .and_then(|c| c.raw.get("AllowInHeredoc"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Some(Box::new(cops::layout::TrailingWhitespace::with_config(
                allow_in_heredoc,
            )))
        }

        "Layout/TrailingEmptyLines" => {
            let cop_config = config.get_cop_config("Layout/TrailingEmptyLines");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "final_blank_line" => cops::layout::TrailingEmptyLinesStyle::FinalBlankLine,
                    _ => cops::layout::TrailingEmptyLinesStyle::FinalNewline,
                })
                .unwrap_or(cops::layout::TrailingEmptyLinesStyle::FinalNewline);
            Some(Box::new(cops::layout::TrailingEmptyLines::new(style)))
        }

        "Layout/BeginEndAlignment" => {
            let style = config.get_cop_config("Layout/BeginEndAlignment")
                .and_then(|c| c.raw.get("EnforcedStyleAlignWith"))
                .and_then(|v| v.as_str())
                .unwrap_or("start_of_line");
            let align_style = match style {
                "begin" => cops::layout::BeginEndAlignmentStyle::Begin,
                _ => cops::layout::BeginEndAlignmentStyle::StartOfLine,
            };
            Some(Box::new(cops::layout::BeginEndAlignment::new(align_style)))
        }

        "Layout/DefEndAlignment" => {
            let style = config.get_cop_config("Layout/DefEndAlignment")
                .and_then(|c| c.raw.get("EnforcedStyleAlignWith"))
                .and_then(|v| v.as_str())
                .unwrap_or("start_of_line");
            let align_style = match style {
                "def" => cops::layout::DefEndAlignmentStyle::Def,
                _ => cops::layout::DefEndAlignmentStyle::StartOfLine,
            };
            Some(Box::new(cops::layout::DefEndAlignment::new(align_style)))
        }

        "Layout/EmptyLineBetweenDefs" => {
            let cop_config = config.get_cop_config("Layout/EmptyLineBetweenDefs");
            let mut cop = cops::layout::EmptyLineBetweenDefs::new();
            if let Some(c) = cop_config {
                if let Some(v) = c.raw.get("AllowAdjacentOneLineDefs").and_then(|v| v.as_bool()) {
                    cop.allow_adjacent_one_line_defs = v;
                }
                if let Some(v) = c.raw.get("EmptyLineBetweenMethodDefs").and_then(|v| v.as_bool()) {
                    cop.empty_line_between_method_defs = v;
                }
                if let Some(v) = c.raw.get("EmptyLineBetweenClassDefs").and_then(|v| v.as_bool()) {
                    cop.empty_line_between_class_defs = v;
                }
                if let Some(v) = c.raw.get("EmptyLineBetweenModuleDefs").and_then(|v| v.as_bool()) {
                    cop.empty_line_between_module_defs = v;
                }
                if let Some(v) = c.raw.get("DefLikeMacros").and_then(|v| v.as_sequence()) {
                    cop.def_like_macros = v.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect();
                }
                if let Some(v) = c.raw.get("NumberOfEmptyLines") {
                    if let Some(n) = v.as_u64() {
                        cop.number_of_empty_lines_min = n as u32;
                        cop.number_of_empty_lines_max = n as u32;
                    } else if let Some(seq) = v.as_sequence() {
                        let nums: Vec<u32> = seq.iter()
                            .filter_map(|x| x.as_u64().map(|n| n as u32))
                            .collect();
                        if let (Some(&min), Some(&max)) = (nums.first(), nums.last()) {
                            cop.number_of_empty_lines_min = min;
                            cop.number_of_empty_lines_max = max;
                        }
                    }
                }
            }
            Some(Box::new(cop))
        }

        "Layout/EmptyLineAfterGuardClause" => {
            Some(Box::new(cops::layout::EmptyLineAfterGuardClause::new()))
        }

        "Layout/EmptyLinesAroundAccessModifier" => {
            let cop_config = config.get_cop_config("Layout/EmptyLinesAroundAccessModifier");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "only_before" => cops::layout::EmptyLinesAroundAccessModifierStyle::OnlyBefore,
                    _ => cops::layout::EmptyLinesAroundAccessModifierStyle::Around,
                })
                .unwrap_or(cops::layout::EmptyLinesAroundAccessModifierStyle::Around);
            Some(Box::new(cops::layout::EmptyLinesAroundAccessModifier::new(style)))
        }

        "Layout/EmptyLinesAroundClassBody" => {
            let cop_config = config.get_cop_config("Layout/EmptyLinesAroundClassBody");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| cops::layout::EmptyLinesAroundClassBodyStyle::parse(s))
                .unwrap_or(cops::layout::EmptyLinesAroundClassBodyStyle::NoEmptyLines);
            Some(Box::new(cops::layout::EmptyLinesAroundClassBody::new(style)))
        }

        "Layout/EmptyLinesAroundModuleBody" => {
            let cop_config = config.get_cop_config("Layout/EmptyLinesAroundModuleBody");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| cops::layout::EmptyLinesAroundModuleBodyStyle::parse(s))
                .unwrap_or(cops::layout::EmptyLinesAroundModuleBodyStyle::NoEmptyLines);
            Some(Box::new(cops::layout::EmptyLinesAroundModuleBody::new(style)))
        }

        "Layout/HeredocIndentation" => {
            let cop_config = config.get_cop_config("Layout/HeredocIndentation");
            let active_support = cop_config
                .and_then(|c| c.raw.get("ActiveSupportExtensionsEnabled"))
                .and_then(|v| v.as_bool())
                .or_else(|| cop_config
                    .and_then(|c| c.raw.get("AllCopsActiveSupportExtensionsEnabled"))
                    .and_then(|v| v.as_bool()))
                .unwrap_or(false);
            // Read MaxLineLength/AllowHeredoc from Layout/LineLength cross-cop config
            let ll_config = config.get_cop_config("Layout/LineLength");
            let max_line_length = cop_config
                .and_then(|c| c.raw.get("MaxLineLength"))
                .and_then(|v| v.as_i64())
                .or_else(|| ll_config.and_then(|c| c.max).map(|v| v as i64))
                .map(|v| v as usize);
            let allow_heredoc = cop_config
                .and_then(|c| c.raw.get("AllowHeredoc"))
                .and_then(|v| v.as_bool())
                .or_else(|| ll_config
                    .and_then(|c| c.raw.get("AllowHeredoc"))
                    .and_then(|v| v.as_bool()))
                .unwrap_or(true);
            Some(Box::new(cops::layout::HeredocIndentation::with_config(
                2, active_support, max_line_length, allow_heredoc,
            )))
        }

        "Layout/IndentationWidth" => {
            Some(build_indentation_width_cop(config))
        }

        "Layout/HashAlignment" => {
            let cop_config = config.get_cop_config("Layout/HashAlignment");

            let parse_styles = |key: &str, default: &str| -> Vec<cops::layout::HashAlignmentStyle> {
                let raw = cop_config.and_then(|c| c.raw.get(key));
                let strings: Vec<String> = if let Some(val) = raw {
                    if let Some(s) = val.as_str() {
                        vec![s.to_string()]
                    } else if let Some(seq) = val.as_sequence() {
                        seq.iter().filter_map(|v| v.as_str().map(String::from)).collect()
                    } else {
                        vec![default.to_string()]
                    }
                } else {
                    vec![default.to_string()]
                };
                strings.iter().filter_map(|s| match s.as_str() {
                    "key" => Some(cops::layout::HashAlignmentStyle::Key),
                    "separator" => Some(cops::layout::HashAlignmentStyle::Separator),
                    "table" => Some(cops::layout::HashAlignmentStyle::Table),
                    _ => None,
                }).collect()
            };

            let rocket_styles = parse_styles("EnforcedHashRocketStyle", "key");
            let colon_styles = parse_styles("EnforcedColonStyle", "key");

            let last_arg_style = cop_config
                .and_then(|c| c.raw.get("EnforcedLastArgumentHashStyle"))
                .and_then(|v| v.as_str())
                .map(|s| match s {
                    "always_ignore" => cops::layout::HashAlignmentLastArgStyle::AlwaysIgnore,
                    "ignore_implicit" => cops::layout::HashAlignmentLastArgStyle::IgnoreImplicit,
                    "ignore_explicit" => cops::layout::HashAlignmentLastArgStyle::IgnoreExplicit,
                    _ => cops::layout::HashAlignmentLastArgStyle::AlwaysInspect,
                })
                .unwrap_or(cops::layout::HashAlignmentLastArgStyle::AlwaysInspect);

            if rocket_styles.is_empty() || colon_styles.is_empty() {
                None
            } else {
                let arg_align_config = config.get_cop_config("Layout/ArgumentAlignment");
                let arg_align_fixed = arg_align_config
                    .and_then(|c| c.enforced_style.as_ref())
                    .map(|s| s == "with_fixed_indentation")
                    .unwrap_or(false);
                Some(Box::new(cops::layout::HashAlignment::new(
                    rocket_styles,
                    colon_styles,
                    last_arg_style,
                ).with_argument_alignment_fixed(arg_align_fixed)))
            }
        }

        "Layout/FirstArgumentIndentation" => {
            let cop_config = config.get_cop_config("Layout/FirstArgumentIndentation");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "consistent" => cops::layout::FirstArgumentIndentationStyle::Consistent,
                    "consistent_relative_to_receiver" => cops::layout::FirstArgumentIndentationStyle::ConsistentRelativeToReceiver,
                    "special_for_inner_method_call" => cops::layout::FirstArgumentIndentationStyle::SpecialForInnerMethodCall,
                    _ => cops::layout::FirstArgumentIndentationStyle::SpecialForInnerMethodCallInParentheses,
                })
                .unwrap_or(cops::layout::FirstArgumentIndentationStyle::SpecialForInnerMethodCallInParentheses);
            let width = cop_config
                .and_then(|c| c.raw.get("IndentationWidth"))
                .and_then(|v| v.as_i64())
                .map(|v| v as usize);
            Some(Box::new(cops::layout::FirstArgumentIndentation::new(style, width)))
        }

        "Layout/FirstHashElementIndentation" => {
            let cop_config = config.get_cop_config("Layout/FirstHashElementIndentation");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "consistent" => cops::layout::FirstHashElementIndentationStyle::Consistent,
                    "align_braces" => cops::layout::FirstHashElementIndentationStyle::AlignBraces,
                    _ => cops::layout::FirstHashElementIndentationStyle::SpecialInsideParentheses,
                })
                .unwrap_or(cops::layout::FirstHashElementIndentationStyle::SpecialInsideParentheses);
            let width = cop_config
                .and_then(|c| c.raw.get("IndentationWidth"))
                .and_then(|v| v.as_i64())
                .map(|v| v as usize);
            let ha = config.get_cop_config("Layout/HashAlignment");
            let colon_sep = ha
                .and_then(|c| c.raw.get("EnforcedColonStyle"))
                .and_then(|v| v.as_str())
                .map(|s| s == "separator")
                .unwrap_or(false);
            let rocket_sep = ha
                .and_then(|c| c.raw.get("EnforcedHashRocketStyle"))
                .and_then(|v| v.as_str())
                .map(|s| s == "separator")
                .unwrap_or(false);
            Some(Box::new(cops::layout::FirstHashElementIndentation::new(
                style, width, colon_sep, rocket_sep,
            )))
        }

        "Layout/FirstArrayElementIndentation" => {
            let cop_config = config.get_cop_config("Layout/FirstArrayElementIndentation");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "consistent" => cops::layout::FirstArrayElementIndentationStyle::Consistent,
                    "align_brackets" => cops::layout::FirstArrayElementIndentationStyle::AlignBrackets,
                    _ => cops::layout::FirstArrayElementIndentationStyle::SpecialInsideParentheses,
                })
                .unwrap_or(cops::layout::FirstArrayElementIndentationStyle::SpecialInsideParentheses);
            let width = cop_config
                .and_then(|c| c.raw.get("IndentationWidth"))
                .and_then(|v| v.as_i64())
                .map(|v| v as usize);
            Some(Box::new(cops::layout::FirstArrayElementIndentation::new(style, width)))
        }

        "Layout/EndAlignment" => {
            let style = config.get_cop_config("Layout/EndAlignment")
                .and_then(|c| c.raw.get("EnforcedStyleAlignWith"))
                .and_then(|v| v.as_str())
                .unwrap_or("keyword");
            let align_style = match style {
                "variable" => cops::layout::EndAlignmentStyle::Variable,
                "start_of_line" => cops::layout::EndAlignmentStyle::StartOfLine,
                _ => cops::layout::EndAlignmentStyle::Keyword,
            };
            Some(Box::new(cops::layout::EndAlignment::new(align_style)))
        }

        "Layout/BlockAlignment" => {
            let style = config.get_cop_config("Layout/BlockAlignment")
                .and_then(|c| c.raw.get("EnforcedStyleAlignWith"))
                .and_then(|v| v.as_str())
                .unwrap_or("either");
            let align_style = match style {
                "start_of_block" => cops::layout::BlockAlignmentStyle::StartOfBlock,
                "start_of_line" => cops::layout::BlockAlignmentStyle::StartOfLine,
                _ => cops::layout::BlockAlignmentStyle::Either,
            };
            Some(Box::new(cops::layout::BlockAlignment::new(align_style)))
        }

        "Layout/CaseIndentation" => {
            let cop_config = config.get_cop_config("Layout/CaseIndentation");
            let style = cop_config
                .and_then(|c| c.raw.get("EnforcedStyle"))
                .and_then(|v| v.as_str())
                .unwrap_or("case")
                .to_string();
            let indent_one_step = cop_config
                .and_then(|c| c.raw.get("IndentOneStep"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            // cop's own IndentationWidth override — may be empty string or integer
            let indent_width = cop_config
                .and_then(|c| c.raw.get("IndentationWidth"))
                .and_then(|v| v.as_i64())
                .map(|v| v as usize);
            let layout_iw = config
                .get_cop_config("Layout/IndentationWidth")
                .and_then(|c| c.raw.get("Width"))
                .and_then(|v| v.as_i64())
                .map(|v| v as usize)
                .unwrap_or(2);
            Some(Box::new(cops::layout::CaseIndentation::with_config(
                style, indent_one_step, indent_width, layout_iw,
            )))
        }

        "Layout/ElseAlignment" => {
            let end_style = config.get_cop_config("Layout/EndAlignment")
                .and_then(|c| c.raw.get("EnforcedStyleAlignWith"))
                .and_then(|v| v.as_str())
                .unwrap_or("keyword")
                .to_string();
            Some(Box::new(cops::layout::ElseAlignment::with_end_align_style(end_style)))
        }

        "Layout/RescueEnsureAlignment" => {
            let begin_end_style = config.get_cop_config("Layout/BeginEndAlignment")
                .and_then(|c| {
                    let enabled = c.raw.get("Enabled").and_then(|v| v.as_bool()).unwrap_or(true);
                    if enabled {
                        c.raw.get("EnforcedStyleAlignWith").and_then(|v| v.as_str().map(|s| s.to_string()))
                    } else {
                        None
                    }
                });
            Some(Box::new(cops::layout::RescueEnsureAlignment::with_begin_end_style(begin_end_style)))
        }

        "Layout/LeadingCommentSpace" => {
            let cop_config = config.get_cop_config("Layout/LeadingCommentSpace");
            let allow_doxygen = cop_config
                .and_then(|c| c.raw.get("AllowDoxygenCommentStyle"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let allow_gemfile_ruby = cop_config
                .and_then(|c| c.raw.get("AllowGemfileRubyComment"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let allow_rbs = cop_config
                .and_then(|c| c.raw.get("AllowRBSInlineAnnotation"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let allow_steep = cop_config
                .and_then(|c| c.raw.get("AllowSteepAnnotation"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Some(Box::new(cops::layout::LeadingCommentSpace::with_config(
                allow_doxygen,
                allow_gemfile_ruby,
                allow_rbs,
                allow_steep,
            )))
        }

        "Layout/SpaceAfterComma" => {
            let space_inside_braces_is_space = config
                .get_cop_config("Layout/SpaceInsideHashLiteralBraces")
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| s == "space")
                .unwrap_or(false);
            Some(Box::new(cops::layout::SpaceAfterComma::with_config(
                space_inside_braces_is_space,
            )))
        }

        "Layout/SpaceAroundKeyword" => {
            Some(Box::new(cops::layout::SpaceAroundKeyword::new()))
        }
        "Layout/SpaceAroundOperators" => {
            let c = config.get_cop_config("Layout/SpaceAroundOperators");
            let allow_for_alignment = c.and_then(|c| c.raw.get("AllowForAlignment")).and_then(|v| v.as_bool()).unwrap_or(true);
            let exp = c.and_then(|c| c.raw.get("EnforcedStyleForExponentOperator")).and_then(|v| v.as_str()).map(|s| s == "space").unwrap_or(false);
            let sl = c.and_then(|c| c.raw.get("EnforcedStyleForRationalLiterals")).and_then(|v| v.as_str()).map(|s| s == "space").unwrap_or(false);
            let hash_table_style = config
                .get_cop_config("Layout/HashAlignment")
                .and_then(|c| c.raw.get("EnforcedHashRocketStyle"))
                .and_then(|v| v.as_str())
                .map(|s| s == "table")
                .unwrap_or(false);
            Some(Box::new(cops::layout::SpaceAroundOperators::with_config(allow_for_alignment, exp, sl, hash_table_style)))
        }
        "Layout/SpaceAroundBlockParameters" => {
            let style = config
                .get_cop_config("Layout/SpaceAroundBlockParameters")
                .and_then(|c| c.raw.get("EnforcedStyleInsidePipes"))
                .and_then(|v| v.as_str())
                .map(|s| match s {
                    "space" => cops::layout::SpaceAroundBlockParametersStyle::Space,
                    _ => cops::layout::SpaceAroundBlockParametersStyle::NoSpace,
                })
                .unwrap_or(cops::layout::SpaceAroundBlockParametersStyle::NoSpace);
            Some(Box::new(cops::layout::SpaceAroundBlockParameters::new(style)))
        }
        "Layout/SpaceAroundMethodCallOperator" => {
            Some(Box::new(cops::layout::SpaceAroundMethodCallOperator::new()))
        }

        "Layout/MultilineArrayBraceLayout" => {
            let style = config
                .get_cop_config("Layout/MultilineArrayBraceLayout")
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| cops::layout::MultilineBraceLayoutStyle::from_str(s))
                .unwrap_or(cops::layout::MultilineBraceLayoutStyle::Symmetrical);
            Some(Box::new(cops::layout::MultilineArrayBraceLayout::new(style)))
        }

        "Layout/MultilineHashBraceLayout" => {
            let style = config
                .get_cop_config("Layout/MultilineHashBraceLayout")
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| cops::layout::MultilineBraceLayoutStyle::from_str(s))
                .unwrap_or(cops::layout::MultilineBraceLayoutStyle::Symmetrical);
            Some(Box::new(cops::layout::MultilineHashBraceLayout::new(style)))
        }

        "Layout/MultilineMethodCallBraceLayout" => {
            let style = config
                .get_cop_config("Layout/MultilineMethodCallBraceLayout")
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| cops::layout::MultilineBraceLayoutStyle::from_str(s))
                .unwrap_or(cops::layout::MultilineBraceLayoutStyle::Symmetrical);
            Some(Box::new(cops::layout::MultilineMethodCallBraceLayout::new(style)))
        }

        "Layout/MultilineMethodCallIndentation" => {
            let cop_config = config.get_cop_config("Layout/MultilineMethodCallIndentation");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "indented" => cops::layout::MultilineMethodCallIndentationStyle::Indented,
                    "indented_relative_to_receiver" => cops::layout::MultilineMethodCallIndentationStyle::IndentedRelativeToReceiver,
                    _ => cops::layout::MultilineMethodCallIndentationStyle::Aligned,
                })
                .unwrap_or(cops::layout::MultilineMethodCallIndentationStyle::Aligned);
            let width = cop_config
                .and_then(|c| c.raw.get("IndentationWidth"))
                .and_then(|v| v.as_i64())
                .map(|v| v as usize);
            Some(Box::new(cops::layout::MultilineMethodCallIndentation::new(style, width)))
        }

        "Layout/MultilineOperationIndentation" => {
            let cop_config = config.get_cop_config("Layout/MultilineOperationIndentation");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "indented" => cops::layout::MultilineOperationIndentationStyle::Indented,
                    _ => cops::layout::MultilineOperationIndentationStyle::Aligned,
                })
                .unwrap_or(cops::layout::MultilineOperationIndentationStyle::Aligned);
            let width = cop_config
                .and_then(|c| c.raw.get("IndentationWidth"))
                .and_then(|v| v.as_i64())
                .map(|v| v as usize);
            Some(Box::new(cops::layout::MultilineOperationIndentation::new(style, width)))
        }

        "Layout/SpaceInsideArrayLiteralBrackets" => {
            let cop_config = config.get_cop_config("Layout/SpaceInsideArrayLiteralBrackets");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "space" => cops::layout::SpaceInsideArrayLiteralBracketsStyle::Space,
                    "compact" => cops::layout::SpaceInsideArrayLiteralBracketsStyle::Compact,
                    _ => cops::layout::SpaceInsideArrayLiteralBracketsStyle::NoSpace,
                })
                .unwrap_or(cops::layout::SpaceInsideArrayLiteralBracketsStyle::NoSpace);
            let empty_style = cop_config
                .and_then(|c| c.raw.get("EnforcedStyleForEmptyBrackets"))
                .and_then(|v| v.as_str())
                .map(|s| match s {
                    "space" => cops::layout::SpaceInsideArrayLiteralBracketsEmptyStyle::Space,
                    _ => cops::layout::SpaceInsideArrayLiteralBracketsEmptyStyle::NoSpace,
                })
                .unwrap_or(cops::layout::SpaceInsideArrayLiteralBracketsEmptyStyle::NoSpace);
            Some(Box::new(cops::layout::SpaceInsideArrayLiteralBrackets::new(
                style, empty_style,
            )))
        }

        "Layout/SpaceInsideArrayPercentLiteral" => {
            Some(Box::new(cops::layout::SpaceInsideArrayPercentLiteral::new()))
        }

        "Layout/SpaceInsideBlockBraces" => {
            let cop_config = config.get_cop_config("Layout/SpaceInsideBlockBraces");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "no_space" => cops::layout::SpaceInsideBlockBracesStyle::NoSpace,
                    _ => cops::layout::SpaceInsideBlockBracesStyle::Space,
                })
                .unwrap_or(cops::layout::SpaceInsideBlockBracesStyle::Space);
            // Mirror RuboCop: invalid EnforcedStyleForEmptyBraces value raises an error
            // (which the parity test "fails_with_an_error" exercises). We model that
            // by disabling the cop instead of registering it.
            let raw_empty = cop_config
                .and_then(|c| c.raw.get("EnforcedStyleForEmptyBraces"))
                .and_then(|v| v.as_str());
            let empty_style = match raw_empty {
                Some("space") => cops::layout::SpaceInsideBlockBracesEmptyStyle::Space,
                Some("no_space") | None => cops::layout::SpaceInsideBlockBracesEmptyStyle::NoSpace,
                Some(_) => return None,
            };
            let space_before_params = cop_config
                .and_then(|c| c.raw.get("SpaceBeforeBlockParameters"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            Some(Box::new(cops::layout::SpaceInsideBlockBraces::new(
                style, empty_style, space_before_params,
            )))
        }

        "Layout/SpaceInsideHashLiteralBraces" => {
            let cop_config = config.get_cop_config("Layout/SpaceInsideHashLiteralBraces");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "no_space" => cops::layout::SpaceInsideHashLiteralBracesStyle::NoSpace,
                    "compact" => cops::layout::SpaceInsideHashLiteralBracesStyle::Compact,
                    _ => cops::layout::SpaceInsideHashLiteralBracesStyle::Space,
                })
                .unwrap_or(cops::layout::SpaceInsideHashLiteralBracesStyle::Space);
            let empty_style = cop_config
                .and_then(|c| c.raw.get("EnforcedStyleForEmptyBraces"))
                .and_then(|v| v.as_str())
                .map(|s| match s {
                    "space" => cops::layout::SpaceInsideHashLiteralBracesEmptyStyle::Space,
                    _ => cops::layout::SpaceInsideHashLiteralBracesEmptyStyle::NoSpace,
                })
                .unwrap_or(cops::layout::SpaceInsideHashLiteralBracesEmptyStyle::NoSpace);
            Some(Box::new(cops::layout::SpaceInsideHashLiteralBraces::new(
                style, empty_style,
            )))
        }

        "Layout/SpaceInsidePercentLiteralDelimiters" => {
            Some(Box::new(cops::layout::SpaceInsidePercentLiteralDelimiters::new()))
        }

        "Layout/SpaceInsideReferenceBrackets" => {
            let cop_config = config.get_cop_config("Layout/SpaceInsideReferenceBrackets");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "space" => cops::layout::SpaceInsideReferenceBracketsStyle::Space,
                    _ => cops::layout::SpaceInsideReferenceBracketsStyle::NoSpace,
                })
                .unwrap_or(cops::layout::SpaceInsideReferenceBracketsStyle::NoSpace);
            let empty_style = cop_config
                .and_then(|c| c.raw.get("EnforcedStyleForEmptyBrackets"))
                .and_then(|v| v.as_str())
                .map(|s| match s {
                    "space" => cops::layout::SpaceInsideReferenceBracketsEmptyStyle::Space,
                    _ => cops::layout::SpaceInsideReferenceBracketsEmptyStyle::NoSpace,
                })
                .unwrap_or(cops::layout::SpaceInsideReferenceBracketsEmptyStyle::NoSpace);
            Some(Box::new(cops::layout::SpaceInsideReferenceBrackets::new(
                style, empty_style,
            )))
        }

        "Style/FrozenStringLiteralComment" => {
            let cop_config = config.get_cop_config("Style/FrozenStringLiteralComment");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "never" => cops::style::FrozenStringLiteralCommentStyle::Never,
                    "always_true" => cops::style::FrozenStringLiteralCommentStyle::AlwaysTrue,
                    _ => cops::style::FrozenStringLiteralCommentStyle::Always,
                })
                .unwrap_or(cops::style::FrozenStringLiteralCommentStyle::Always);
            Some(Box::new(cops::style::FrozenStringLiteralComment::new(
                style,
            )))
        }

        "Style/Semicolon" => {
            let cop_config = config.get_cop_config("Style/Semicolon");
            let allow = cop_config
                .and_then(|c| c.raw.get("AllowAsExpressionSeparator"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Some(Box::new(cops::style::Semicolon::new(allow)))
        }

        "Style/DoubleNegation" => {
            let cop_config = config.get_cop_config("Style/DoubleNegation");
            let style = match cop_config.and_then(|c| c.raw.get("EnforcedStyle")).and_then(|v| v.as_str()) {
                Some("forbidden") => cops::style::DoubleNegationStyle::Forbidden,
                _ => cops::style::DoubleNegationStyle::AllowedInReturns,
            };
            Some(Box::new(cops::style::DoubleNegation::new(style)))
        }

        "Style/NumericPredicate" => {
            let cop_config = config.get_cop_config("Style/NumericPredicate");
            let style = match cop_config.and_then(|c| c.raw.get("EnforcedStyle")).and_then(|v| v.as_str()) {
                Some("comparison") => cops::style::NumericPredicateStyle::Comparison,
                _ => cops::style::NumericPredicateStyle::Predicate,
            };
            let allowed_methods = cop_config
                .and_then(|c| c.raw.get("AllowedMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let allowed_patterns = cop_config
                .and_then(|c| c.raw.get("AllowedPatterns"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            Some(Box::new(cops::style::NumericPredicate::with_config(style, allowed_methods, allowed_patterns)))
        }

        "Style/WordArray" => {
            let cop_config = config.get_cop_config("Style/WordArray");
            let style = match cop_config.and_then(|c| c.raw.get("EnforcedStyle")).and_then(|v| v.as_str()) {
                Some("brackets") => cops::style::WordArrayStyle::Brackets,
                _ => cops::style::WordArrayStyle::Percent,
            };
            let min_size = cop_config
                .and_then(|c| c.raw.get("MinSize"))
                .and_then(|v| v.as_u64())
                .unwrap_or(2) as usize;
            let word_regex = cop_config
                .and_then(|c| c.raw.get("WordRegex"))
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| r"\A(?:\w|\w-\w|\n|\t)+\z".into());
            let word_regex = normalize_ruby_regex(&word_regex);
            Some(Box::new(cops::style::WordArray::with_config(style, min_size, word_regex)))
        }

        "Style/StringLiterals" => {
            let cop_config = config.get_cop_config("Style/StringLiterals");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .and_then(|s| match s.as_str() {
                    "single_quotes" => Some(cops::style::StringLiteralsStyle::SingleQuotes),
                    "double_quotes" => Some(cops::style::StringLiteralsStyle::DoubleQuotes),
                    _ => None, // Invalid config value - skip cop
                });
            let style = match style {
                Some(s) => s,
                None => {
                    // Unknown EnforcedStyle with explicit config - don't run cop
                    if cop_config.and_then(|c| c.enforced_style.as_ref()).is_some() {
                        return None;
                    }
                    cops::style::StringLiteralsStyle::SingleQuotes
                }
            };
            let consistent = cop_config
                .and_then(|c| c.raw.get("ConsistentQuotesInMultiline"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Some(Box::new(cops::style::StringLiterals::with_config(
                style, consistent,
            )))
        }

        "Style/NumericLiterals" => {
            let cop_config = config.get_cop_config("Style/NumericLiterals");
            let min_digits = cop_config
                .and_then(|c| c.raw.get("MinDigits"))
                .and_then(|v| v.as_u64())
                .unwrap_or(6) as usize;
            let strict = cop_config
                .and_then(|c| c.raw.get("Strict"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let allowed_numbers = cop_config
                .and_then(|c| c.raw.get("AllowedNumbers"))
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| {
                            v.as_i64().or_else(|| {
                                v.as_str().and_then(|s| s.parse::<i64>().ok())
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let allowed_patterns = cop_config
                .and_then(|c| c.raw.get("AllowedPatterns"))
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            Some(Box::new(cops::style::NumericLiterals::with_config(
                min_digits,
                strict,
                allowed_numbers,
                allowed_patterns,
            )))
        }

        "Metrics/MethodLength" => {
            let cop_config = config.get_cop_config("Metrics/MethodLength");
            let max = cop_config.and_then(|c| c.max).unwrap_or(10);
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
            Some(Box::new(cops::metrics::MethodLength::with_config(
                max,
                count_comments,
                count_as_one,
                allowed_methods,
                allowed_patterns,
            )))
        }

        "Metrics/ClassLength" => {
            let cop_config = config.get_cop_config("Metrics/ClassLength");
            let max = cop_config.and_then(|c| c.max).unwrap_or(100);
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
            Some(Box::new(cops::metrics::ClassLength::with_config(
                max,
                count_comments,
                count_as_one,
            )))
        }

        "Naming/FileName" => {
            let cop_config = config.get_cop_config("Naming/FileName");
            let ignore_executable_scripts = cop_config
                .and_then(|c| c.raw.get("IgnoreExecutableScripts"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let expect_matching_definition = cop_config
                .and_then(|c| c.raw.get("ExpectMatchingDefinition"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let check_definition_path_hierarchy = cop_config
                .and_then(|c| c.raw.get("CheckDefinitionPathHierarchy"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let check_definition_path_hierarchy_roots = cop_config
                .and_then(|c| c.raw.get("CheckDefinitionPathHierarchyRoots"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_else(|| vec!["lib".into(), "spec".into(), "test".into(), "src".into()]);
            let regex = cop_config
                .and_then(|c| c.raw.get("Regex"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from);
            let allowed_acronyms = cop_config
                .and_then(|c| c.raw.get("AllowedAcronyms"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let include_patterns = cop_config
                .and_then(|c| c.raw.get("Include"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .or_else(|| config.all_cops_include())
                .unwrap_or_default();
            Some(Box::new(cops::naming::FileName::with_full_config(
                ignore_executable_scripts,
                expect_matching_definition,
                check_definition_path_hierarchy,
                check_definition_path_hierarchy_roots,
                regex,
                allowed_acronyms,
                include_patterns,
            )))
        }

        "Naming/MemoizedInstanceVariableName" => {
            let cop_config = config.get_cop_config("Naming/MemoizedInstanceVariableName");
            let style = cop_config
                .and_then(|c| c.raw.get("EnforcedStyleForLeadingUnderscores"))
                .and_then(|v| v.as_str())
                .map(|s| match s {
                    "required" => cops::naming::LeadingUnderscoreStyle::Required,
                    "optional" => cops::naming::LeadingUnderscoreStyle::Optional,
                    _ => cops::naming::LeadingUnderscoreStyle::Disallowed,
                })
                .unwrap_or(cops::naming::LeadingUnderscoreStyle::Disallowed);
            Some(Box::new(cops::naming::MemoizedInstanceVariableName::with_style(style)))
        }

        "Naming/MethodName" => {
            let cop_config = config.get_cop_config("Naming/MethodName");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "camelCase" => cops::naming::MethodNameStyle::CamelCase,
                    _ => cops::naming::MethodNameStyle::SnakeCase,
                })
                .unwrap_or(cops::naming::MethodNameStyle::SnakeCase);
            let allowed_patterns = cop_config
                .and_then(|c| c.raw.get("AllowedPatterns"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let forbidden_identifiers = cop_config
                .and_then(|c| c.raw.get("ForbiddenIdentifiers"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_else(|| vec!["__id__".to_string(), "__send__".to_string()]);
            let forbidden_patterns = cop_config
                .and_then(|c| c.raw.get("ForbiddenPatterns"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            Some(Box::new(cops::naming::MethodName::with_config(
                style, allowed_patterns, forbidden_identifiers, forbidden_patterns,
            )))
        }

        "Naming/PredicateMethod" => {
            let cop_config = config.get_cop_config("Naming/PredicateMethod");
            let mode = cop_config
                .and_then(|c| c.raw.get("Mode"))
                .and_then(|v| v.as_str())
                .map(|s| match s {
                    "aggressive" => cops::naming::PredicateMethodMode::Aggressive,
                    _ => cops::naming::PredicateMethodMode::Conservative,
                })
                .unwrap_or(cops::naming::PredicateMethodMode::Conservative);
            let allow_bang = cop_config
                .and_then(|c| c.raw.get("AllowBangMethods"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let allowed_methods = cop_config
                .and_then(|c| c.raw.get("AllowedMethods"))
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let allowed_patterns = cop_config
                .and_then(|c| c.raw.get("AllowedPatterns"))
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let wayward_predicates = cop_config
                .and_then(|c| c.raw.get("WaywardPredicates"))
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            Some(Box::new(cops::naming::PredicateMethod::with_config(
                mode,
                allow_bang,
                allowed_methods,
                allowed_patterns,
                wayward_predicates,
            )))
        }

        "Naming/VariableName" => {
            let cop_config = config.get_cop_config("Naming/VariableName");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "camelCase" => cops::naming::VariableNameStyle::CamelCase,
                    _ => cops::naming::VariableNameStyle::SnakeCase,
                })
                .unwrap_or(cops::naming::VariableNameStyle::SnakeCase);
            let allowed_identifiers = cop_config
                .and_then(|c| c.raw.get("AllowedIdentifiers"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let allowed_patterns = cop_config
                .and_then(|c| c.raw.get("AllowedPatterns"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let forbidden_identifiers = cop_config
                .and_then(|c| c.raw.get("ForbiddenIdentifiers"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let forbidden_patterns = cop_config
                .and_then(|c| c.raw.get("ForbiddenPatterns"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            Some(Box::new(cops::naming::VariableName::with_config(
                style, allowed_identifiers, allowed_patterns, forbidden_identifiers, forbidden_patterns,
            )))
        }

        "Naming/VariableNumber" => {
            let cop_config = config.get_cop_config("Naming/VariableNumber");
            let style = cop_config
                .and_then(|c| c.enforced_style.as_ref())
                .map(|s| match s.as_str() {
                    "snake_case" => cops::naming::VariableNumberStyle::SnakeCase,
                    "non_integer" => cops::naming::VariableNumberStyle::NonInteger,
                    _ => cops::naming::VariableNumberStyle::NormalCase,
                })
                .unwrap_or(cops::naming::VariableNumberStyle::NormalCase);
            let check_method_names = cop_config
                .and_then(|c| c.raw.get("CheckMethodNames"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let check_symbols = cop_config
                .and_then(|c| c.raw.get("CheckSymbols"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let allowed_identifiers = cop_config
                .and_then(|c| c.raw.get("AllowedIdentifiers"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let allowed_patterns = cop_config
                .and_then(|c| c.raw.get("AllowedPatterns"))
                .and_then(|v| v.as_sequence())
                .map(|seq| seq.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            Some(Box::new(cops::naming::VariableNumber::with_config(
                style, check_method_names, check_symbols, allowed_identifiers, allowed_patterns,
            )))
        }

        _ => None,
    }
}

/// Parse a DebuggerMethods or DebuggerRequires config value.
/// Handles both array format (flat list) and hash format (grouped by category).
/// Read cross-cop config for Style/RedundantParentheses.
/// Returns (ternary_parentheses_required, allow_in_multiline_conditions).
fn read_redundant_parens_cross_cop_config(config: &Config) -> (bool, bool) {
    // Style/TernaryParentheses: if enabled and EnforcedStyle is require_parentheses
    // or require_parentheses_when_complex, ternary parens are required
    let ternary_req = config
        .get_cop_config("Style/TernaryParentheses")
        .map(|c| {
            let enabled = c.enabled.unwrap_or(true);
            let style = c.enforced_style.as_deref().unwrap_or("");
            enabled
                && (style == "require_parentheses"
                    || style == "require_parentheses_when_complex")
        })
        .unwrap_or(false);

    // Style/ParenthesesAroundCondition: if enabled and AllowInMultilineConditions is true
    let allow_multiline = config
        .get_cop_config("Style/ParenthesesAroundCondition")
        .map(|c| {
            let enabled = c.enabled.unwrap_or(true);
            let allow = c
                .raw
                .get("AllowInMultilineConditions")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            enabled && allow
        })
        .unwrap_or(false);

    (ternary_req, allow_multiline)
}

/// Build IndentationWidth cop from config, reading cross-cop settings
fn build_indentation_width_cop(config: &Config) -> Box<dyn cops::Cop> {
    let cop_config = config.get_cop_config("Layout/IndentationWidth");
    let width = cop_config
        .and_then(|c| c.raw.get("Width"))
        .and_then(|v| v.as_i64())
        .unwrap_or(2) as usize;
    let align_with = cop_config
        .and_then(|c| c.raw.get("EnforcedStyleAlignWith"))
        .and_then(|v| v.as_str())
        .unwrap_or("start_of_line");
    let align_style = match align_with {
        "relative_to_receiver" => cops::layout::IndentationWidthAlignWithStyle::RelativeToReceiver,
        _ => cops::layout::IndentationWidthAlignWithStyle::StartOfLine,
    };
    let consistency = config.get_cop_config("Layout/IndentationConsistency")
        .and_then(|c| c.raw.get("EnforcedStyle").and_then(|v| v.as_str().map(|s| s.to_string()))
            .or_else(|| c.enforced_style.clone()))
        .map(|s| match s.as_str() {
            "indented_internal_methods" => cops::layout::IndentationWidthConsistencyStyle::IndentedInternalMethods,
            _ => cops::layout::IndentationWidthConsistencyStyle::Normal,
        })
        .unwrap_or(cops::layout::IndentationWidthConsistencyStyle::Normal);

    // Cross-cop: Layout/EndAlignment
    let end_align = config.get_cop_config("Layout/EndAlignment")
        .and_then(|c| c.raw.get("EnforcedStyleAlignWith").and_then(|v| v.as_str().map(|s| s.to_string())))
        .map(|s| match s.as_str() {
            "variable" => cops::layout::IndentationWidthEndAlignStyle::Variable,
            "start_of_line" => cops::layout::IndentationWidthEndAlignStyle::StartOfLine,
            _ => cops::layout::IndentationWidthEndAlignStyle::Keyword,
        })
        .unwrap_or(cops::layout::IndentationWidthEndAlignStyle::Keyword);

    // Cross-cop: Layout/DefEndAlignment
    let def_end_align = config.get_cop_config("Layout/DefEndAlignment")
        .and_then(|c| c.raw.get("EnforcedStyleAlignWith").and_then(|v| v.as_str().map(|s| s.to_string())))
        .map(|s| match s.as_str() {
            "def" => cops::layout::IndentationWidthDefEndAlignStyle::Def,
            _ => cops::layout::IndentationWidthDefEndAlignStyle::StartOfLine,
        })
        .unwrap_or(cops::layout::IndentationWidthDefEndAlignStyle::StartOfLine);

    // Cross-cop: Layout/IndentationStyle
    let indent_style = config.get_cop_config("Layout/IndentationStyle")
        .and_then(|c| c.raw.get("EnforcedStyle").and_then(|v| v.as_str().map(|s| s.to_string()))
            .or_else(|| c.enforced_style.clone()))
        .map(|s| match s.as_str() {
            "tabs" => cops::layout::IndentationWidthIndentStyle::Tabs,
            _ => cops::layout::IndentationWidthIndentStyle::Spaces,
        })
        .unwrap_or(cops::layout::IndentationWidthIndentStyle::Spaces);

    // Cross-cop: Layout/AccessModifierIndentation
    let access_mod_style = config.get_cop_config("Layout/AccessModifierIndentation")
        .and_then(|c| c.raw.get("EnforcedStyle").and_then(|v| v.as_str().map(|s| s.to_string()))
            .or_else(|| c.enforced_style.clone()))
        .map(|s| match s.as_str() {
            "outdent" => cops::layout::IndentationWidthAccessModifierStyle::Outdent,
            _ => cops::layout::IndentationWidthAccessModifierStyle::Indent,
        })
        .unwrap_or(cops::layout::IndentationWidthAccessModifierStyle::Indent);

    // AllowedPatterns
    let allowed_patterns = cop_config
        .and_then(|c| c.raw.get("AllowedPatterns"))
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    Box::new(cops::layout::IndentationWidth::with_full_config(
        width,
        align_style,
        consistency,
        end_align,
        def_end_align,
        indent_style,
        access_mod_style,
        allowed_patterns,
    ))
}

/// For hash format, values that are empty strings, false, or null are skipped (disabled groups).
/// Returns None if the value is null/missing so the caller can use defaults.
fn parse_debugger_list(value: &serde_yaml::Value) -> Option<Vec<String>> {
    if value.is_null() {
        return None;
    }
    // Array format: ["method1", "method2"]
    if let Some(seq) = value.as_sequence() {
        return Some(
            seq.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
        );
    }
    // Hash format: { GroupName: ["method1", "method2"], DisabledGroup: null }
    if let Some(map) = value.as_mapping() {
        let mut result = Vec::new();
        for (_key, val) in map {
            // Skip disabled groups: null, false, empty string
            if val.is_null() {
                continue;
            }
            if let Some(b) = val.as_bool() {
                if !b {
                    continue;
                }
            }
            if let Some(s) = val.as_str() {
                if s.is_empty() {
                    continue;
                }
                // Single string value
                result.push(s.to_string());
                continue;
            }
            // Array of strings
            if let Some(seq) = val.as_sequence() {
                for item in seq {
                    if let Some(s) = item.as_str() {
                        result.push(s.to_string());
                    }
                }
            }
        }
        return Some(result);
    }
    None
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
