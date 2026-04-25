//! Lint/CopDirectiveSyntax — validate `# rubocop:...` comment formatting.
//! Ports `RuboCop::Cop::Lint::CopDirectiveSyntax`.

use crate::cops::{CheckContext, Cop};
use crate::offense::{Offense, Severity};
use regex::Regex;

#[derive(Default)]
pub struct CopDirectiveSyntax;

impl CopDirectiveSyntax {
    pub fn new() -> Self { Self }
}

const COMMON_MSG: &str = "Malformed directive comment detected.";
const MISSING_MODE_NAME_MSG: &str = "The mode name is missing.";
const INVALID_MODE_NAME_MSG: &str =
    "The mode name must be one of `enable`, `disable`, `todo`, `push`, or `pop`.";
const MISSING_COP_NAME_MSG: &str = "The cop name is missing.";
const MALFORMED_COP_NAMES_MSG: &str =
    "Cop names must be separated by commas. Comment in the directive must start with `--`.";

const AVAILABLE_MODES: &[&str] = &["disable", "enable", "todo", "push", "pop"];

fn marker_re() -> Regex {
    // `#\s*rubocop\s*:\s*`
    Regex::new(r"^#\s*rubocop\s*:\s*").unwrap()
}

fn directive_re() -> Regex {
    // Full directive regex (header + optional cops/push-pop part).
    let cop_name = r"(?:[A-Za-z]\w+/)*[A-Za-z]\w+";
    let cop_names = format!(r"(?:{cn}\s*,\s*)*{cn}", cn = cop_name);
    let cops_pattern = format!(r"(all|{})", cop_names);
    let push_pop = format!(r"([+\-]{cn}(?:\s+[+\-]{cn})*)", cn = cop_name);
    let modes = AVAILABLE_MODES.join("|");
    let header = format!(r"#\s*rubocop\s*:\s*((?:{}))\b", modes);
    let full = format!(r"{}(?:\s+{}|\s+{})?", header, cops_pattern, push_pop);
    Regex::new(&full).unwrap()
}

fn missing_cop_name_re() -> Regex {
    let modes = AVAILABLE_MODES.join("|");
    Regex::new(&format!(r"\A#\s*rubocop\s*:\s*((?:{}))\s*\z", modes)).unwrap()
}

impl Cop for CopDirectiveSyntax {
    fn name(&self) -> &'static str { "Lint/CopDirectiveSyntax" }
    fn severity(&self) -> Severity { Severity::Warning }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let result = ruby_prism::parse(ctx.source.as_bytes());
        let mut out = Vec::new();
        let marker = marker_re();
        let dir_re = directive_re();
        let miss_cop_re = missing_cop_name_re();

        for comment in result.comments() {
            let loc = comment.location();
            let s = loc.start_offset();
            let e = loc.end_offset();
            let text = &ctx.source[s..e];

            // start_with_marker? — text matches marker regex at offset 0.
            if !marker.is_match(text) {
                continue;
            }

            // Try full directive match.
            let m = dir_re.find(text);
            // pre_match is text[..m.start()]. If m exists and pre_match matches `\A#\s*\z` exactly,
            // RuboCop keeps match_data; otherwise still keeps if pre_match empty (m.start()==0).
            // Actually: `match_data&.pre_match&.match?(/\A#\s*\z/) ? nil : match_data`.
            // So nil if pre_match looks like `# `; else keep. Wait inverted: nil if pre_match matches.
            // For start at 0, pre_match = "" — `\A#\s*\z` requires `#`. "" doesn't match. So keep. ✓
            let match_data = match m {
                Some(mm) => {
                    let pre = &text[..mm.start()];
                    let pre_re = Regex::new(r"\A#\s*\z").unwrap();
                    if pre_re.is_match(pre) {
                        None
                    } else {
                        Some(mm)
                    }
                }
                None => None,
            };

            // Determine if malformed.
            let mut missing_cop_name = false;
            let mut malformed = false;

            if match_data.is_none() {
                malformed = true;
            } else {
                let mm = match_data.as_ref().unwrap();
                // missing_cop_name? — only if not push/pop and matches MALFORMED_DIRECTIVE_WITHOUT_COP_NAME.
                // Determine mode via captures.
                let caps = dir_re.captures(text).unwrap();
                let mode = caps.get(1).map(|c| c.as_str()).unwrap_or("");
                let is_push_pop = mode == "push" || mode == "pop";
                if !is_push_pop && miss_cop_re.is_match(text) {
                    missing_cop_name = true;
                    malformed = true;
                }

                if !malformed {
                    // tail = post_match.lstrip
                    let tail = text[mm.end()..].trim_start();
                    if !(tail.is_empty() || tail.starts_with("--")) {
                        malformed = true;
                    }
                }
            }

            if !malformed {
                continue;
            }

            // Build the additional message.
            let after_marker = marker.replace(text, "").into_owned();
            let mode_opt: Option<&str> = after_marker
                .split(' ')
                .next()
                .filter(|s| !s.is_empty());
            let extra: &str = if mode_opt.is_none() {
                MISSING_MODE_NAME_MSG
            } else {
                let mode = mode_opt.unwrap();
                if !AVAILABLE_MODES.contains(&mode) {
                    INVALID_MODE_NAME_MSG
                } else if missing_cop_name {
                    MISSING_COP_NAME_MSG
                } else {
                    MALFORMED_COP_NAMES_MSG
                }
            };
            let msg = format!("{} {}", COMMON_MSG, extra);

            out.push(ctx.offense_with_range(
                "Lint/CopDirectiveSyntax",
                &msg,
                Severity::Warning,
                s,
                e,
            ));
        }
        out
    }
}

crate::register_cop!("Lint/CopDirectiveSyntax", |_cfg| Some(Box::new(CopDirectiveSyntax::new())));
