use crate::cops::{CheckContext, Cop};
use crate::offense::{Location, Offense, Severity};

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

        for line in source.lines() {
            // Count occurrences of `# rubocop:disable` or `# rubocop:todo`
            let count = line.matches("# rubocop:disable").count()
                + line.matches("# rubocop:todo").count();
            if count <= 1 {
                continue;
            }
            // Find the line's byte offset
            let line_start = source
                .find(line)
                .unwrap_or(0);
            // Find the first # rubocop: on this line
            let comment_offset = line.find("# rubocop:").unwrap_or(0);
            let start = line_start + comment_offset;
            let end = line_start + line.len();
            let loc = Location::from_offsets(source, start, end);
            offenses.push(Offense::new(
                self.name(),
                MSG,
                self.severity(),
                loc,
                ctx.filename,
            ));
        }
        offenses
    }
}

crate::register_cop!("Style/DoubleCopDisableDirective", |_cfg| {
    Some(Box::new(DoubleCopDisableDirective::new()))
});
