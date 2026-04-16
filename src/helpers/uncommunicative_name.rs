//! Shared logic for Naming/MethodParameterName and Naming/BlockParameterName
//! (mirrors RuboCop's `RuboCop::Cop::UncommunicativeName` mixin).

use crate::cops::CheckContext;
use crate::offense::{Offense, Severity};

/// Configuration for UncommunicativeName-style checks.
#[derive(Debug, Clone)]
pub struct UncommunicativeConfig {
    pub min_name_length: usize,
    pub allow_names_ending_in_numbers: bool,
    pub allowed_names: Vec<String>,
    pub forbidden_names: Vec<String>,
}

impl UncommunicativeConfig {
    pub fn new(
        min_name_length: usize,
        allow_names_ending_in_numbers: bool,
        allowed_names: Vec<String>,
        forbidden_names: Vec<String>,
    ) -> Self {
        Self {
            min_name_length,
            allow_names_ending_in_numbers,
            allowed_names,
            forbidden_names,
        }
    }
}

/// Kind of parameter for the range-width rule (mirrors `restarg_type?` / `kwrestarg_type?`).
#[derive(Debug, Clone, Copy)]
pub enum ParamKind {
    Normal, // required, optional, required_kw, optional_kw, block
    Rest,   // `*args`   — range width += 1
    KwRest, // `**kwargs` — range width += 2
}

/// A single parameter extracted for checking.
pub struct ParamInfo {
    /// The full parameter name including any leading underscores (but excluding
    /// the `*`/`**`/`&` sigil for rest/kwrest/block).
    pub full_name: String,
    /// Byte offset of the parameter start (where `*`, `**`, `&`, or name begins).
    pub begin_pos: usize,
    pub kind: ParamKind,
}

/// Runs the UncommunicativeName check. `name_type` is "method parameter" or
/// "block parameter".
pub fn check_params(
    params: &[ParamInfo],
    name_type: &str,
    cop_name: &'static str,
    config: &UncommunicativeConfig,
    ctx: &CheckContext,
) -> Vec<Offense> {
    let mut offenses = Vec::new();

    for param in params {
        let full_name = param.full_name.as_str();
        if full_name.is_empty() || full_name == "_" {
            continue;
        }

        // Strip *leading* underscores (RuboCop: `gsub(/\A(_+)/, '')`).
        let bare = full_name.trim_start_matches('_');
        if config.allowed_names.iter().any(|n| n == bare) {
            continue;
        }

        let length = full_name.len()
            + match param.kind {
                ParamKind::Normal => 0,
                ParamKind::Rest => 1,
                ParamKind::KwRest => 2,
            };
        let start = param.begin_pos;
        let end = start + length;

        // issue_offenses: order matches RuboCop.
        if config.forbidden_names.iter().any(|n| n == bare) {
            offenses.push(ctx.offense_with_range(
                cop_name,
                &format!("Do not use {} as a name for a {}.", bare, name_type),
                Severity::Convention,
                start,
                end,
            ));
        }
        if bare.chars().any(|c| c.is_uppercase()) {
            offenses.push(ctx.offense_with_range(
                cop_name,
                &format!("Only use lowercase characters for {}.", name_type),
                Severity::Convention,
                start,
                end,
            ));
        }
        if bare.chars().count() < config.min_name_length {
            let cap = capitalize(name_type);
            offenses.push(ctx.offense_with_range(
                cop_name,
                &format!(
                    "{} must be at least {} characters long.",
                    cap, config.min_name_length
                ),
                Severity::Convention,
                start,
                end,
            ));
        }
        if !config.allow_names_ending_in_numbers
            && bare.chars().last().map_or(false, |c| c.is_ascii_digit())
        {
            offenses.push(ctx.offense_with_range(
                cop_name,
                &format!("Do not end {} with a number.", name_type),
                Severity::Convention,
                start,
                end,
            ));
        }
    }

    offenses
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Extract parameters from a `ParametersNode` in source order.
///
/// `source` is the program source text so we can pull the parameter name as a
/// `String` directly from its `name_loc()` — avoids lifetime issues with
/// Prism's internal constant pool.
pub fn extract_params(
    source: &str,
    params_node: &ruby_prism::ParametersNode,
) -> Vec<ParamInfo> {
    let mut out = Vec::new();

    let name_from_loc = |loc: &ruby_prism::Location| -> String {
        let start = loc.start_offset();
        let end = loc.end_offset();
        source.get(start..end).unwrap_or("").to_string()
    };

    // Required positional
    for n in params_node.requireds().iter() {
        if let Some(req) = n.as_required_parameter_node() {
            // RequiredParameterNode's location IS the name location.
            let loc = req.location();
            out.push(ParamInfo {
                full_name: name_from_loc(&loc),
                begin_pos: loc.start_offset(),
                kind: ParamKind::Normal,
            });
        }
        // Destructured params (`def foo((a, b))`) — RuboCop's children.first on
        // a MultiTargetNode returns an array, which `.to_s` doesn't produce a
        // simple name; skip, matches our fixture scope.
    }

    // Optional positional
    for n in params_node.optionals().iter() {
        if let Some(opt) = n.as_optional_parameter_node() {
            let name_loc = opt.name_loc();
            out.push(ParamInfo {
                full_name: name_from_loc(&name_loc),
                begin_pos: name_loc.start_offset(),
                kind: ParamKind::Normal,
            });
        }
    }

    // Rest
    if let Some(rest_node) = params_node.rest() {
        if let Some(rest) = rest_node.as_rest_parameter_node() {
            if let Some(name_loc) = rest.name_loc() {
                out.push(ParamInfo {
                    full_name: name_from_loc(&name_loc),
                    begin_pos: rest.location().start_offset(),
                    kind: ParamKind::Rest,
                });
            }
        }
    }

    // Post-rest required
    for n in params_node.posts().iter() {
        if let Some(req) = n.as_required_parameter_node() {
            let loc = req.location();
            out.push(ParamInfo {
                full_name: name_from_loc(&loc),
                begin_pos: loc.start_offset(),
                kind: ParamKind::Normal,
            });
        }
    }

    // Keyword params — name_loc includes the trailing `:`, trim it.
    for n in params_node.keywords().iter() {
        if let Some(req_kw) = n.as_required_keyword_parameter_node() {
            let name_loc = req_kw.name_loc();
            let raw = name_from_loc(&name_loc);
            let name = raw.trim_end_matches(':').to_string();
            out.push(ParamInfo {
                full_name: name,
                begin_pos: name_loc.start_offset(),
                kind: ParamKind::Normal,
            });
        } else if let Some(opt_kw) = n.as_optional_keyword_parameter_node() {
            let name_loc = opt_kw.name_loc();
            let raw = name_from_loc(&name_loc);
            let name = raw.trim_end_matches(':').to_string();
            out.push(ParamInfo {
                full_name: name,
                begin_pos: name_loc.start_offset(),
                kind: ParamKind::Normal,
            });
        }
    }

    // Keyword rest
    if let Some(kwrest_node) = params_node.keyword_rest() {
        if let Some(kwrest) = kwrest_node.as_keyword_rest_parameter_node() {
            if let Some(name_loc) = kwrest.name_loc() {
                out.push(ParamInfo {
                    full_name: name_from_loc(&name_loc),
                    begin_pos: kwrest.location().start_offset(),
                    kind: ParamKind::KwRest,
                });
            }
        }
        // `**nil` is a NoKeywordsParameterNode — skip.
    }

    // Block param
    if let Some(block) = params_node.block() {
        if let Some(name_loc) = block.name_loc() {
            out.push(ParamInfo {
                full_name: name_from_loc(&name_loc),
                begin_pos: block.location().start_offset(),
                kind: ParamKind::Normal,
            });
        }
    }

    out
}
