//! Layout/SpaceAroundEqualsInParameterDefault cop
//!
//! Ported from: https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/layout/space_around_equals_in_parameter_default.rb

use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Offense, Severity};

const MSG_MISSING: &str = "Surrounding space missing in default value assignment.";
const MSG_DETECTED: &str = "Surrounding space detected in default value assignment.";

#[derive(Clone, Copy, Debug)]
pub enum SpaceAroundEqualsStyle {
    Space,
    NoSpace,
}

pub struct SpaceAroundEqualsInParameterDefault {
    style: SpaceAroundEqualsStyle,
}

impl SpaceAroundEqualsInParameterDefault {
    pub fn new(style: SpaceAroundEqualsStyle) -> Self {
        Self { style }
    }
}

impl Default for SpaceAroundEqualsInParameterDefault {
    fn default() -> Self {
        Self::new(SpaceAroundEqualsStyle::Space)
    }
}

impl Cop for SpaceAroundEqualsInParameterDefault {
    fn name(&self) -> &'static str {
        "Layout/SpaceAroundEqualsInParameterDefault"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_def(&self, node: &ruby_prism::DefNode, ctx: &CheckContext) -> Vec<Offense> {
        let Some(params) = node.parameters() else {
            return vec![];
        };
        let mut out = Vec::new();
        for opt in params.optionals().iter() {
            if let Some(o) = opt.as_optional_parameter_node() {
                self.check_optarg(&o, ctx, &mut out);
            }
        }
        out
    }
}

impl SpaceAroundEqualsInParameterDefault {
    fn check_optarg(
        &self,
        node: &ruby_prism::OptionalParameterNode,
        ctx: &CheckContext,
        out: &mut Vec<Offense>,
    ) {
        let bytes = ctx.source.as_bytes();
        let arg_end = node.name_loc().end_offset();
        let equals_begin = node.operator_loc().start_offset();
        let equals_end = node.operator_loc().end_offset();
        let value_begin = node.value().location().start_offset();

        let arg_space_after = bytes.get(arg_end).is_some_and(|&b| b == b' ' || b == b'\t');
        let equals_space_after = bytes.get(equals_end).is_some_and(|&b| b == b' ' || b == b'\t');

        let space_both = arg_space_after && equals_space_after;
        let no_space = !arg_space_after && !equals_space_after;

        let correct_style = match self.style {
            SpaceAroundEqualsStyle::Space => space_both,
            SpaceAroundEqualsStyle::NoSpace => no_space,
        };

        if correct_style {
            return;
        }

        let range_start = arg_end;
        let range_end = value_begin;
        let message = match self.style {
            SpaceAroundEqualsStyle::Space => MSG_MISSING,
            SpaceAroundEqualsStyle::NoSpace => MSG_DETECTED,
        };

        // Autocorrect: replace range_source up to equals with replacement, preserve value.
        // RuboCop: m = range.source.match(/=\s*(\S+)/); rest = m.captures[0]; replacement = style==space ? ' = ' : '='
        // corrector.replace(range, replacement + rest)
        let range_src = &ctx.source[range_start..range_end];
        // Find `=` in range_src then capture the first non-space after it
        let rest = extract_rest_after_eq(range_src);
        let replacement = match self.style {
            SpaceAroundEqualsStyle::Space => format!(" = {rest}"),
            SpaceAroundEqualsStyle::NoSpace => format!("={rest}"),
        };

        let _ = equals_begin; // not needed separately

        let offense = ctx
            .offense_with_range(self.name(), message, Severity::Convention, range_start, range_end)
            .with_correction(Correction::replace(range_start, range_end, &replacement));
        out.push(offense);
    }
}

fn extract_rest_after_eq(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] != b'=' {
        i += 1;
    }
    if i >= bytes.len() {
        return String::new();
    }
    i += 1; // past `=`
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    // capture non-whitespace prefix
    let start = i;
    while i < bytes.len() && !matches!(bytes[i], b' ' | b'\t') {
        i += 1;
    }
    s[start..i].to_string()
}
