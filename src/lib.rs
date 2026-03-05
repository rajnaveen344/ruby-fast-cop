pub mod config;
pub mod cops;
pub mod correction;
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

    // Lint/LiteralInInterpolation
    if config.is_cop_enabled("Lint/LiteralInInterpolation") {
        result.push(Box::new(cops::lint::LiteralInInterpolation::new()));
    }

    // Lint/RedundantTypeConversion
    if config.is_cop_enabled("Lint/RedundantTypeConversion") {
        result.push(Box::new(cops::lint::RedundantTypeConversion::new()));
    }

    // Lint/UnreachableCode
    if config.is_cop_enabled("Lint/UnreachableCode") {
        result.push(Box::new(cops::lint::UnreachableCode::new()));
    }

    // Lint/Void
    if config.is_cop_enabled("Lint/Void") {
        result.push(Box::new(cops::lint::Void::new(false)));
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

    // Style/AutoResourceCleanup
    if config.is_cop_enabled("Style/AutoResourceCleanup") {
        result.push(Box::new(cops::style::AutoResourceCleanup::new()));
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

    // Style/MethodCalledOnDoEndBlock
    if config.is_cop_enabled("Style/MethodCalledOnDoEndBlock") {
        result.push(Box::new(cops::style::MethodCalledOnDoEndBlock::new()));
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

    // Style/RedundantRegexpEscape
    if config.is_cop_enabled("Style/RedundantRegexpEscape") {
        result.push(Box::new(cops::style::RedundantRegexpEscape::new()));
    }

    // Style/RedundantStringEscape
    if config.is_cop_enabled("Style/RedundantStringEscape") {
        result.push(Box::new(cops::style::RedundantStringEscape::new()));
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

    // Style/SelectByRegexp
    if config.is_cop_enabled("Style/SelectByRegexp") {
        result.push(Box::new(cops::style::SelectByRegexp::new()));
    }

    // Style/StringMethods
    if config.is_cop_enabled("Style/StringMethods") {
        result.push(Box::new(cops::style::StringMethods::new()));
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

    // Layout/SpaceInsidePercentLiteralDelimiters
    if config.is_cop_enabled("Layout/SpaceInsidePercentLiteralDelimiters") {
        result.push(Box::new(cops::layout::SpaceInsidePercentLiteralDelimiters::new()));
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
            let active_support = config
                .get_cop_config("Lint/DuplicateMethods")
                .and_then(|c| c.raw.get("ActiveSupportExtensionsEnabled"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            Some(Box::new(cops::lint::DuplicateMethods::with_config(active_support)))
        }

        "Lint/LiteralAsCondition" => {
            Some(Box::new(cops::lint::LiteralAsCondition::new()))
        }

        "Lint/LiteralInInterpolation" => {
            Some(Box::new(cops::lint::LiteralInInterpolation::new()))
        }

        "Lint/RedundantTypeConversion" => {
            Some(Box::new(cops::lint::RedundantTypeConversion::new()))
        }

        "Lint/UnreachableCode" => {
            Some(Box::new(cops::lint::UnreachableCode::new()))
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

        "Style/AutoResourceCleanup" => Some(Box::new(cops::style::AutoResourceCleanup::new())),

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

        "Style/MethodCalledOnDoEndBlock" => {
            Some(Box::new(cops::style::MethodCalledOnDoEndBlock::new()))
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
            let safe_nav_chain_enabled = cop_config
                .and_then(|c| c.raw.get("SafeNavigationChainEnabled"))
                .and_then(|v| v.as_bool())
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

        "Style/RedundantRegexpEscape" => Some(Box::new(cops::style::RedundantRegexpEscape::new())),
        "Style/RedundantStringEscape" => Some(Box::new(cops::style::RedundantStringEscape::new())),

        "Style/SelectByRegexp" => Some(Box::new(cops::style::SelectByRegexp::new())),

        "Style/StringMethods" => Some(Box::new(cops::style::StringMethods::new())),

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

        "Layout/SpaceInsidePercentLiteralDelimiters" => {
            Some(Box::new(cops::layout::SpaceInsidePercentLiteralDelimiters::new()))
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
