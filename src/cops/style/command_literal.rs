//! Style/CommandLiteral — Enforces backtick vs %x for shell commands.
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/style/command_literal.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};
use ruby_prism::Visit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcedStyle {
    Backticks,
    PercentX,
    Mixed,
}

impl Default for EnforcedStyle {
    fn default() -> Self {
        EnforcedStyle::Backticks
    }
}

const MSG_BACKTICKS: &str = "Use backticks around command string.";
const MSG_PERCENT_X: &str = "Use `%x` around command string.";

pub struct CommandLiteral {
    style: EnforcedStyle,
    allow_inner_backticks: bool,
    preferred_delimiters: String, // `()` by default
}

impl Default for CommandLiteral {
    fn default() -> Self {
        Self {
            style: EnforcedStyle::Backticks,
            allow_inner_backticks: false,
            preferred_delimiters: "()".to_string(),
        }
    }
}

impl CommandLiteral {
    pub fn new(style: EnforcedStyle, allow_inner_backticks: bool, preferred_delimiters: String) -> Self {
        Self { style, allow_inner_backticks, preferred_delimiters }
    }

    fn open_delim(&self) -> char {
        self.preferred_delimiters.chars().next().unwrap_or('(')
    }

    fn close_delim(&self) -> char {
        self.preferred_delimiters.chars().nth(1).unwrap_or(')')
    }
}

impl Cop for CommandLiteral {
    fn name(&self) -> &'static str {
        "Style/CommandLiteral"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut visitor = CommandLiteralVisitor {
            ctx,
            style: self.style,
            allow_inner_backticks: self.allow_inner_backticks,
            open_delim: self.open_delim(),
            close_delim: self.close_delim(),
            offenses: Vec::new(),
        };
        visitor.visit(&node.as_node());
        visitor.offenses
    }
}

struct CommandLiteralVisitor<'a> {
    ctx: &'a CheckContext<'a>,
    style: EnforcedStyle,
    allow_inner_backticks: bool,
    open_delim: char,
    close_delim: char,
    offenses: Vec<Offense>,
}

impl<'a> CommandLiteralVisitor<'a> {
    /// Returns true if the body text contains a backtick character
    fn contains_backtick(body: &str) -> bool {
        body.contains('`')
    }

    /// Build %x replacement from backtick content
    fn backtick_to_percent_x(&self, body: &str) -> String {
        format!("%x{}{}{}", self.open_delim, body, self.close_delim)
    }

    /// Build backtick replacement from %x content
    fn percent_x_to_backtick(body: &str) -> String {
        format!("`{body}`")
    }

    fn check_x_string(&mut self, start: usize, end: usize) {
        // %x string: source[start..end] e.g. `%x(ls)` or `%x[ls]`
        let src = &self.ctx.source[start..end];

        // Find the opening delimiter position
        // src starts with %x followed by delimiter
        let prefix_len = 2; // "%x"
        if src.len() <= prefix_len { return; }
        let open_char = src.as_bytes()[prefix_len] as char;
        // closing is last char
        let body_start = start + prefix_len + 1;
        let body_end = end - 1;
        if body_end <= body_start { return; }
        let body = &self.ctx.source[body_start..body_end];

        let has_inner_backtick = Self::contains_backtick(body);

        match self.style {
            EnforcedStyle::Backticks => {
                if has_inner_backtick {
                    if self.allow_inner_backticks {
                        // allow_inner_backticks=true: still flag (use backticks), but no correction
                        self.offenses.push(
                            self.ctx.offense_with_range(
                                "Style/CommandLiteral", MSG_BACKTICKS,
                                Severity::Convention, start, end,
                            )
                        );
                    }
                    // allow_inner_backticks=false: %x with inner backtick → accepted (no offense)
                } else {
                    // No inner backtick → flag, correct to backtick
                    let correction_src = Self::percent_x_to_backtick(body);
                    let offense = self.ctx.offense_with_range(
                        "Style/CommandLiteral", MSG_BACKTICKS,
                        Severity::Convention, start, end,
                    ).with_correction(crate::offense::Correction::replace(start, end, correction_src));
                    self.offenses.push(offense);
                }
            }
            EnforcedStyle::PercentX => {
                // x strings are already %x → accepted
            }
            EnforcedStyle::Mixed => {
                // single-line %x without backticks → flag, correct to backtick; multiline accepted
                let is_multiline = body.contains('\n');
                if !is_multiline {
                    if has_inner_backtick {
                        if self.allow_inner_backticks {
                            // flag without correction
                            self.offenses.push(
                                self.ctx.offense_with_range(
                                    "Style/CommandLiteral", MSG_BACKTICKS,
                                    Severity::Convention, start, end,
                                )
                            );
                        }
                        // allow_inner_backticks=false: accepted
                    } else {
                        let correction_src = Self::percent_x_to_backtick(body);
                        let offense = self.ctx.offense_with_range(
                            "Style/CommandLiteral", MSG_BACKTICKS,
                            Severity::Convention, start, end,
                        ).with_correction(crate::offense::Correction::replace(start, end, correction_src));
                        self.offenses.push(offense);
                    }
                }
                // multiline %x accepted
            }
        }
    }

    fn check_backtick_string(&mut self, start: usize, end: usize) {
        // XString (backtick): source[start..end], e.g. `` `ls` ``
        let src = &self.ctx.source[start..end];
        // body is between first and last char (backticks)
        if src.len() < 2 { return; }
        let body = &self.ctx.source[start + 1..end - 1];
        let has_inner_backtick = body.contains("\\`");
        let is_multiline = body.contains('\n');

        match self.style {
            EnforcedStyle::Backticks => {
                // backtick with escaped backticks → offense without correction
                if has_inner_backtick {
                    if !self.allow_inner_backticks {
                        self.offenses.push(
                            self.ctx.offense_with_range(
                                "Style/CommandLiteral", MSG_PERCENT_X,
                                Severity::Convention, start, end,
                            )
                        );
                    }
                }
                // Otherwise accepted
            }
            EnforcedStyle::PercentX => {
                // backtick with escaped backticks → offense without correction
                if has_inner_backtick {
                    self.offenses.push(
                        self.ctx.offense_with_range(
                            "Style/CommandLiteral", MSG_PERCENT_X,
                            Severity::Convention, start, end,
                        )
                    );
                } else {
                    // Correct to %x
                    let correction_src = self.backtick_to_percent_x(body);
                    let offense = self.ctx.offense_with_range(
                        "Style/CommandLiteral", MSG_PERCENT_X,
                        Severity::Convention, start, end,
                    ).with_correction(crate::offense::Correction::replace(start, end, correction_src));
                    self.offenses.push(offense);
                }
            }
            EnforcedStyle::Mixed => {
                // multiline backtick → flag, correct to %x
                if is_multiline {
                    if has_inner_backtick {
                        // no correction
                        self.offenses.push(
                            self.ctx.offense_with_range(
                                "Style/CommandLiteral", MSG_PERCENT_X,
                                Severity::Convention, start, end,
                            )
                        );
                    } else {
                        let correction_src = self.backtick_to_percent_x(body);
                        let offense = self.ctx.offense_with_range(
                            "Style/CommandLiteral", MSG_PERCENT_X,
                            Severity::Convention, start, end,
                        ).with_correction(crate::offense::Correction::replace(start, end, correction_src));
                        self.offenses.push(offense);
                    }
                } else {
                    // single-line backtick with escaped backtick
                    if has_inner_backtick && !self.allow_inner_backticks {
                        self.offenses.push(
                            self.ctx.offense_with_range(
                                "Style/CommandLiteral", MSG_PERCENT_X,
                                Severity::Convention, start, end,
                            )
                        );
                    }
                    // single-line without inner backtick → accepted
                }
            }
        }
    }
}

impl<'a> Visit<'_> for CommandLiteralVisitor<'a> {
    fn visit_x_string_node(&mut self, node: &ruby_prism::XStringNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let opening = node.opening_loc();
        let open_src = &self.ctx.source[opening.start_offset()..opening.end_offset()];
        if open_src.starts_with("<<") {
            // heredoc — skip
        } else if open_src.starts_with("%x") || open_src.starts_with("%X") {
            // %x string
            self.check_x_string(start, end);
        } else {
            // backtick string
            self.check_backtick_string(start, end);
        }
        ruby_prism::visit_x_string_node(self, node);
    }

    fn visit_interpolated_x_string_node(&mut self, node: &ruby_prism::InterpolatedXStringNode) {
        let start = node.location().start_offset();
        let end = node.location().end_offset();
        let opening = node.opening_loc();
        let open_src = &self.ctx.source[opening.start_offset()..opening.end_offset()];
        if open_src.starts_with("<<") {
            // heredoc — skip
            ruby_prism::visit_interpolated_x_string_node(self, node);
            return;
        }
        if open_src.starts_with("%x") || open_src.starts_with("%X") {
            // %x string
            self.check_x_string(start, end);
        } else {
            // backtick with interpolation
            self.check_backtick_string(start, end);
        }
        ruby_prism::visit_interpolated_x_string_node(self, node);
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Cfg {
    enforced_style: String,
    allow_inner_backticks: bool,
}

crate::register_cop!("Style/CommandLiteral", |cfg| {
    let c: Cfg = cfg.typed("Style/CommandLiteral");

    // Read preferred delimiters from Style/PercentLiteralDelimiters cross-cop config
    let preferred_delimiters = {
        let pld = cfg.get_cop_config("Style/PercentLiteralDelimiters");
        let delims = pld.and_then(|c| c.raw.get("PreferredDelimiters"));
        let xdelim = delims.and_then(|d| {
            // Try %x key first, then default
            d.get("%x").or_else(|| d.get("default"))
        });
        xdelim.and_then(|v| v.as_str()).unwrap_or("()").to_string()
    };

    let style = match c.enforced_style.as_str() {
        "percent_x" => EnforcedStyle::PercentX,
        "mixed" => EnforcedStyle::Mixed,
        _ => EnforcedStyle::Backticks,
    };
    Some(Box::new(CommandLiteral::new(style, c.allow_inner_backticks, preferred_delimiters)))
});
