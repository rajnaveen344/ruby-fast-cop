use crate::cops::{CheckContext, Cop};
use crate::offense::{Correction, Edit, Location, Offense, Severity};

const MSG: &str = "More than one disable comment on one line.";

#[derive(Default)]
pub struct DoubleCopDisableDirective;

impl DoubleCopDisableDirective {
    pub fn new() -> Self {
        Self
    }
}

impl Cop for DoubleCopDisableDirective {
    fn name(&self) -> &'static str {
        "Style/DoubleCopDisableDirective"
    }

    fn severity(&self) -> Severity {
        Severity::Convention
    }

    fn check_program(&self, _node: &ruby_prism::ProgramNode, ctx: &CheckContext) -> Vec<Offense> {
        let mut offenses = Vec::new();
        let source = ctx.source;

        let mut line_byte_offset = 0;
        for line in source.lines() {
            // Count occurrences of `# rubocop:disable` or `# rubocop:todo`
            let disable_count = line.matches("# rubocop:disable").count();
            let todo_count = line.matches("# rubocop:todo").count();
            let count = disable_count + todo_count;
            if count > 1 {
                // Find the first # rubocop: on this line
                let comment_offset = line.find("# rubocop:").unwrap_or(0);
                let start = line_byte_offset + comment_offset;
                let end = line_byte_offset + line.len();
                let loc = Location::from_offsets(source, start, end);

                // Build correction: merge all disable/todo directives on the line
                let correction = build_merge_correction(line, start, end, disable_count, todo_count);

                let offense = Offense::new(self.name(), MSG, self.severity(), loc, ctx.filename);
                offenses.push(if let Some(c) = correction { offense.with_correction(c) } else { offense });
            }
            line_byte_offset += line.len() + 1; // +1 for \n
        }
        offenses
    }
}

fn build_merge_correction(line: &str, start: usize, end: usize, disable_count: usize, todo_count: usize) -> Option<Correction> {
    // Determine type: all disable or all todo
    let directive = if disable_count >= todo_count { "rubocop:disable" } else { "rubocop:todo" };

    // Collect all cop names from all directives on this line
    let mut cops: Vec<&str> = Vec::new();
    let mut rest = line;
    while let Some(pos) = rest.find("# rubocop:") {
        rest = &rest[pos + 2..]; // skip "# "
        // Skip "rubocop:disable " or "rubocop:todo "
        if let Some(after) = rest.strip_prefix("rubocop:disable ").or_else(|| rest.strip_prefix("rubocop:todo ")) {
            rest = after;
            // Collect cop names until end of string or next "# rubocop:"
            let next_directive = rest.find("# rubocop:").unwrap_or(rest.len());
            let cop_list = rest[..next_directive].trim();
            for cop in cop_list.split(',') {
                let cop = cop.trim();
                if !cop.is_empty() && !cop.starts_with('#') {
                    cops.push(cop);
                }
            }
            rest = &rest[next_directive..];
        } else {
            break;
        }
    }

    if cops.is_empty() {
        return None;
    }

    let merged = format!("# {} {}", directive, cops.join(", "));
    Some(Correction {
        edits: vec![Edit { start_offset: start, end_offset: end, replacement: merged }],
    })
}

crate::register_cop!("Style/DoubleCopDisableDirective", |_cfg| {
    Some(Box::new(DoubleCopDisableDirective::new()))
});
